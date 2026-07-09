//! Training loop.

use crate::dataloader::DataLoader;
use crate::backward::ParamGrad;
use crate::loss::{self, LossKind};
use crate::model::Sequential;
use crate::optimizer::{self, OptimizerKind, OptimizerState};
use niao_tensor::{Device, Tensor, TensorResult};
use std::path::Path;

#[derive(Debug, Clone, Default)]
pub struct TrainMetrics {
    pub loss: f32,
    pub batches: usize,
}

#[derive(Debug, Clone, Default)]
pub struct ValMetrics {
    pub loss: f32,
    pub accuracy: f32,
}

#[derive(Debug, Clone)]
pub struct Trainer {
    pub model: Sequential,
    pub optimizer: OptimizerKind,
    pub loss_fn: LossKind,
    pub device: Device,
    pub grad_clip: Option<f32>,
    pub opt_state: OptimizerState,
    pub loss_history: Vec<f32>,
}

impl Trainer {
    pub fn new(
        model: Sequential,
        optimizer: OptimizerKind,
        loss_fn: LossKind,
        device: Device,
    ) -> Self {
        Self {
            model,
            optimizer,
            loss_fn,
            device,
            grad_clip: Some(1.0),
            opt_state: OptimizerState::default(),
            loss_history: Vec::new(),
        }
    }

    pub fn train_epoch(&mut self, loader: &mut DataLoader) -> TensorResult<TrainMetrics> {
        self.model.train();
        loader.reset();
        let mut total_loss = 0.0f32;
        let mut batches = 0usize;
        while let Some((x, y)) = loader.next_batch() {
            let pred = self.model.forward(&x)?;
            let loss_t = loss::compute_loss(self.loss_fn, &pred, &y)?;
            let loss_val = loss_t.to_cpu()?[0];
            total_loss += loss_val;
            batches += 1;
            self.backward_and_step(&pred, &y)?;
        }
        let avg = if batches > 0 {
            total_loss / batches as f32
        } else {
            0.0
        };
        self.loss_history.push(avg);
        Ok(TrainMetrics {
            loss: avg,
            batches,
        })
    }

    pub fn train_batch(&mut self, x: &Tensor, y: &Tensor) -> TensorResult<f32> {
        self.model.train();
        let pred = self.model.forward(x)?;
        let loss_t = loss::compute_loss(self.loss_fn, &pred, y)?;
        let loss_val = loss_t.to_cpu()?[0];
        self.backward_and_step(&pred, y)?;
        Ok(loss_val)
    }

    pub fn validate(&self, loader: &mut DataLoader) -> TensorResult<ValMetrics> {
        let mut eval_model = self.model.layers.clone();
        for l in &mut eval_model {
            l.training = false;
        }
        let mut eval_seq = Sequential::new(eval_model, self.device);
        loader.reset();
        let mut total_loss = 0.0f32;
        let mut correct = 0usize;
        let mut total = 0usize;
        let mut batch_count = 0usize;
        while let Some((x, y)) = loader.next_batch() {
            let pred = eval_seq.forward(&x)?;
            let loss_t = loss::compute_loss(self.loss_fn, &pred, &y)?;
            total_loss += loss_t.to_cpu()?[0];
            batch_count += 1;
            if self.loss_fn == LossKind::CrossEntropy && pred.shape.len() == 2 {
                let p = pred.to_cpu()?;
                let labels = y.to_cpu()?;
                let cols = pred.shape[1];
                for r in 0..pred.shape[0] {
                    let row = &p[r * cols..(r + 1) * cols];
                    let pred_cls = row
                        .iter()
                        .enumerate()
                        .max_by(|(_, a), (_, b)| a.partial_cmp(b).unwrap())
                        .map(|(i, _)| i)
                        .unwrap_or(0);
                    if pred_cls == labels[r] as usize {
                        correct += 1;
                    }
                    total += 1;
                }
            }
        }
        let n_batches = batch_count.max(1);
        Ok(ValMetrics {
            loss: total_loss / n_batches as f32,
            accuracy: if total > 0 {
                correct as f32 / total as f32
            } else {
                0.0
            },
        })
    }

    pub fn backward_step(&mut self, pred: &Tensor, target: &Tensor) -> TensorResult<()> {
        self.backward_and_step(pred, target)
    }

    fn clip_param_grads(param_grads: &mut [ParamGrad], max_norm: f32) {
        let total: f32 = param_grads
            .iter()
            .flat_map(|pg| {
                pg.weight
                    .iter()
                    .chain(pg.bias.iter())
                    .chain(pg.gamma.iter())
                    .chain(pg.beta.iter())
            })
            .flat_map(|g| g.iter())
            .map(|x| x * x)
            .sum();
        let norm = total.sqrt();
        if norm > max_norm && norm > 0.0 {
            let scale = max_norm / norm;
            for pg in param_grads.iter_mut() {
                if let Some(g) = pg.weight.as_mut() {
                    for v in g.iter_mut() {
                        *v *= scale;
                    }
                }
                if let Some(g) = pg.bias.as_mut() {
                    for v in g.iter_mut() {
                        *v *= scale;
                    }
                }
                if let Some(g) = pg.gamma.as_mut() {
                    for v in g.iter_mut() {
                        *v *= scale;
                    }
                }
                if let Some(g) = pg.beta.as_mut() {
                    for v in g.iter_mut() {
                        *v *= scale;
                    }
                }
            }
        }
    }

    fn backward_and_step(&mut self, pred: &Tensor, target: &Tensor) -> TensorResult<()> {
        let grad_out = loss::loss_grad(self.loss_fn, pred, target)?;
        let mut param_grads = self.model.backward(grad_out)?;
        self.model.zero_grad();

        if let Some(max_norm) = self.grad_clip {
            Self::clip_param_grads(&mut param_grads, max_norm);
        }

        self.opt_state.step += 1;
        let mut param_idx = 0usize;
        for (layer, pg) in self.model.layers.iter_mut().zip(param_grads.iter()) {
            if let (Some(w), Some(g)) = (&mut layer.weight, &pg.weight) {
                if g.len() != w.len() {
                    return Err(niao_tensor::TensorError::Op(format!(
                        "weight grad size mismatch: grad {} vs weight {}",
                        g.len(),
                        w.len()
                    )));
                }
                optimizer::step(
                    &self.optimizer,
                    &mut self.opt_state,
                    param_idx,
                    w,
                    g,
                )
                .map_err(|e| niao_tensor::TensorError::Op(e))?;
                param_idx += 1;
            }
            if let (Some(b), Some(g)) = (&mut layer.bias, &pg.bias) {
                if g.len() != b.len() {
                    return Err(niao_tensor::TensorError::Op(format!(
                        "bias grad size mismatch: grad {} vs bias {}",
                        g.len(),
                        b.len()
                    )));
                }
                optimizer::step(
                    &self.optimizer,
                    &mut self.opt_state,
                    param_idx,
                    b,
                    g,
                )
                .map_err(|e| niao_tensor::TensorError::Op(e))?;
                param_idx += 1;
            }
            if let (Some(gamma), Some(g)) = (&mut layer.gamma, &pg.gamma) {
                if g.len() != gamma.len() {
                    return Err(niao_tensor::TensorError::Op(format!(
                        "gamma grad size mismatch: grad {} vs gamma {}",
                        g.len(),
                        gamma.len()
                    )));
                }
                optimizer::step(
                    &self.optimizer,
                    &mut self.opt_state,
                    param_idx,
                    gamma,
                    g,
                )
                .map_err(|e| niao_tensor::TensorError::Op(e))?;
                param_idx += 1;
            }
            if let (Some(beta), Some(g)) = (&mut layer.beta, &pg.beta) {
                if g.len() != beta.len() {
                    return Err(niao_tensor::TensorError::Op(format!(
                        "beta grad size mismatch: grad {} vs beta {}",
                        g.len(),
                        beta.len()
                    )));
                }
                optimizer::step(
                    &self.optimizer,
                    &mut self.opt_state,
                    param_idx,
                    beta,
                    g,
                )
                .map_err(|e| niao_tensor::TensorError::Op(e))?;
                param_idx += 1;
            }
        }
        Ok(())
    }

    pub fn zero_grad(&mut self) {
        self.model.zero_grad();
    }

    pub fn save(&self, path: &Path) -> TensorResult<()> {
        crate::checkpoint::save_model(path, &self.model)
    }

    pub fn load(path: &Path, device: Device) -> TensorResult<Self> {
        let model = crate::checkpoint::load_model(path, device)?;
        Ok(Self::new(
            model,
            OptimizerKind::adam(0.001),
            LossKind::Mse,
            device,
        ))
    }
}
