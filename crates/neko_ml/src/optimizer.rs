//! Optimizers.

use neko_tensor::Tensor;
use std::collections::HashMap;

#[derive(Debug, Clone)]
pub enum OptimizerKind {
    Sgd { lr: f32, momentum: f32 },
    Adam { lr: f32, beta1: f32, beta2: f32, eps: f32 },
    AdamW { lr: f32, beta1: f32, beta2: f32, eps: f32, weight_decay: f32 },
    Rmsprop { lr: f32, alpha: f32, eps: f32 },
}

impl OptimizerKind {
    pub fn sgd(lr: f32) -> Self {
        Self::Sgd { lr, momentum: 0.0 }
    }

    pub fn adam(lr: f32) -> Self {
        Self::Adam {
            lr,
            beta1: 0.9,
            beta2: 0.999,
            eps: 1e-8,
        }
    }

    pub fn adamw(lr: f32) -> Self {
        Self::AdamW {
            lr,
            beta1: 0.9,
            beta2: 0.999,
            eps: 1e-8,
            weight_decay: 0.01,
        }
    }
}

#[derive(Debug, Clone)]
pub struct OptimizerState {
    pub step: u64,
    pub velocity: HashMap<usize, Vec<f32>>,
    pub m: HashMap<usize, Vec<f32>>,
    pub v: HashMap<usize, Vec<f32>>,
}

impl Default for OptimizerState {
    fn default() -> Self {
        Self {
            step: 0,
            velocity: HashMap::new(),
            m: HashMap::new(),
            v: HashMap::new(),
        }
    }
}

pub fn step(
    kind: &OptimizerKind,
    state: &mut OptimizerState,
    param_id: usize,
    param: &mut Tensor,
    grad: &[f32],
) -> Result<(), String> {
    let mut data = param.to_cpu().map_err(|e| e.to_string())?;
    if data.len() != grad.len() {
        return Err("grad length mismatch".into());
    }
    match kind {
        OptimizerKind::Sgd { lr, momentum } => {
            let vel = state
                .velocity
                .entry(param_id)
                .or_insert_with(|| vec![0.0; data.len()]);
            for i in 0..data.len() {
                vel[i] = momentum * vel[i] + grad[i];
                data[i] -= lr * vel[i];
            }
        }
        OptimizerKind::Adam { lr, beta1, beta2, eps } | OptimizerKind::AdamW { lr, beta1, beta2, eps, weight_decay: _ } => {
            let m = state.m.entry(param_id).or_insert_with(|| vec![0.0; data.len()]);
            let v = state.v.entry(param_id).or_insert_with(|| vec![0.0; data.len()]);
            let t = state.step as f32;
            for i in 0..data.len() {
                m[i] = beta1 * m[i] + (1.0 - beta1) * grad[i];
                v[i] = beta2 * v[i] + (1.0 - beta2) * grad[i] * grad[i];
                let m_hat = m[i] / (1.0 - beta1.powf(t));
                let v_hat = v[i] / (1.0 - beta2.powf(t));
                data[i] -= lr * m_hat / (v_hat.sqrt() + eps);
            }
            if let OptimizerKind::AdamW { weight_decay, .. } = kind {
                for i in 0..data.len() {
                    data[i] -= lr * weight_decay * data[i];
                }
            }
        }
        OptimizerKind::Rmsprop { lr, alpha, eps } => {
            let v = state.v.entry(param_id).or_insert_with(|| vec![0.0; data.len()]);
            for i in 0..data.len() {
                v[i] = alpha * v[i] + (1.0 - alpha) * grad[i] * grad[i];
                data[i] -= lr * grad[i] / (v[i].sqrt() + eps);
            }
        }
    }
    *param = Tensor::from_cpu_data(&param.shape, data, param.device)
        .map_err(|e| e.to_string())?;
    Ok(())
}
