//! Hyperparameter search and early stopping.

use crate::trainer::{Trainer, TrainMetrics, ValMetrics};
use crate::dataloader::DataLoader;
use crate::loss::LossKind;
use crate::model::Sequential;
use crate::optimizer::OptimizerKind;
use niao_tensor::Device;
use rand::Rng;
use std::collections::HashMap;

#[derive(Debug, Clone)]
pub struct SearchResult {
    pub params: HashMap<String, f32>,
    pub train_loss: f32,
    pub val_loss: f32,
}

pub struct GridSearch {
    pub grid: HashMap<String, Vec<f32>>,
}

impl GridSearch {
    pub fn new(grid: HashMap<String, Vec<f32>>) -> Self {
        Self { grid }
    }

    pub fn run<F>(
        &self,
        mut build: F,
        mut loader: DataLoader,
        mut val_loader: DataLoader,
        epochs: usize,
    ) -> Vec<SearchResult>
    where
        F: FnMut(&HashMap<String, f32>) -> Trainer,
    {
        let keys: Vec<String> = self.grid.keys().cloned().collect();
        let mut results = Vec::new();
        let combos = cartesian(&self.grid, &keys, 0, &mut HashMap::new());
        for params in combos {
            let mut trainer = build(&params);
            let mut last_train = TrainMetrics::default();
            let mut last_val = ValMetrics::default();
            for _ in 0..epochs {
                last_train = trainer.train_epoch(&mut loader).unwrap_or_default();
                last_val = trainer.validate(&mut val_loader).unwrap_or_default();
            }
            results.push(SearchResult {
                params: params.clone(),
                train_loss: last_train.loss,
                val_loss: last_val.loss,
            });
        }
        results.sort_by(|a, b| a.val_loss.partial_cmp(&b.val_loss).unwrap());
        results
    }
}

pub struct RandomSearch {
    pub space: HashMap<String, (f32, f32)>,
    pub n_trials: usize,
}

impl RandomSearch {
    pub fn new(space: HashMap<String, (f32, f32)>, n_trials: usize) -> Self {
        Self { space, n_trials }
    }

    pub fn run<F>(
        &self,
        mut build: F,
        mut loader: DataLoader,
        mut val_loader: DataLoader,
        epochs: usize,
    ) -> Vec<SearchResult>
    where
        F: FnMut(&HashMap<String, f32>) -> Trainer,
    {
        let mut rng = rand::thread_rng();
        let mut results = Vec::new();
        for _ in 0..self.n_trials {
            let mut params = HashMap::new();
            for (k, (lo, hi)) in &self.space {
                params.insert(k.clone(), rng.gen_range(*lo..*hi));
            }
            let mut trainer = build(&params);
            let mut last_train = TrainMetrics::default();
            let mut last_val = ValMetrics::default();
            for _ in 0..epochs {
                last_train = trainer.train_epoch(&mut loader).unwrap_or_default();
                last_val = trainer.validate(&mut val_loader).unwrap_or_default();
            }
            results.push(SearchResult {
                params,
                train_loss: last_train.loss,
                val_loss: last_val.loss,
            });
        }
        results.sort_by(|a, b| a.val_loss.partial_cmp(&b.val_loss).unwrap());
        results
    }
}

pub struct EarlyStopping {
    pub patience: usize,
    pub min_delta: f32,
    best_loss: f32,
    wait: usize,
    pub stopped_epoch: Option<usize>,
}

impl EarlyStopping {
    pub fn new(patience: usize, min_delta: f32) -> Self {
        Self {
            patience,
            min_delta,
            best_loss: f32::INFINITY,
            wait: 0,
            stopped_epoch: None,
        }
    }

    pub fn step(&mut self, epoch: usize, val_loss: f32) -> bool {
        if val_loss < self.best_loss - self.min_delta {
            self.best_loss = val_loss;
            self.wait = 0;
            return false;
        }
        self.wait += 1;
        if self.wait >= self.patience {
            self.stopped_epoch = Some(epoch);
            true
        } else {
            false
        }
    }
}

fn cartesian(
    grid: &HashMap<String, Vec<f32>>,
    keys: &[String],
    idx: usize,
    current: &mut HashMap<String, f32>,
) -> Vec<HashMap<String, f32>> {
    if idx >= keys.len() {
        return vec![current.clone()];
    }
    let key = &keys[idx];
    let mut out = Vec::new();
    for &v in &grid[key] {
        current.insert(key.clone(), v);
        out.extend(cartesian(grid, keys, idx + 1, current));
    }
    current.remove(key);
    out
}

pub fn trainer_from_params(
    model: Sequential,
    params: &HashMap<String, f32>,
    loss_fn: LossKind,
    device: Device,
) -> Trainer {
    let lr = params.get("lr").copied().unwrap_or(0.001);
    Trainer::new(model, OptimizerKind::adam(lr), loss_fn, device)
}
