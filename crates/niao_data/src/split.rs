//! Train/validation split utilities.

use niao_tensor::{Tensor, TensorResult};
use rand::seq::SliceRandom;
use rand::SeedableRng;

#[derive(Debug, Clone)]
pub struct SplitResult {
    pub x_train: Tensor,
    pub y_train: Tensor,
    pub x_val: Tensor,
    pub y_val: Tensor,
}

pub fn train_val_split(x: &Tensor, y: &Tensor, ratio: f32, seed: u64) -> TensorResult<SplitResult> {
    train_test_split(x, y, ratio, seed)
}

pub fn train_test_split(x: &Tensor, y: &Tensor, train_ratio: f32, seed: u64) -> TensorResult<SplitResult> {
    let n = x.shape[0];
    let mut indices: Vec<usize> = (0..n).collect();
    let mut rng = rand::rngs::StdRng::seed_from_u64(seed);
    indices.shuffle(&mut rng);
    let train_n = ((n as f32) * train_ratio).round() as usize;
    let train_n = train_n.clamp(1, n.saturating_sub(1).max(1));

    let x_data = x.to_cpu()?;
    let y_data = y.to_cpu()?;
    let feat_dim = x.len() / n;
    let label_dim = y.len() / n.max(1);

    let mut x_train = Vec::with_capacity(train_n * feat_dim);
    let mut y_train = Vec::with_capacity(train_n * label_dim);
    let mut x_val = Vec::with_capacity((n - train_n) * feat_dim);
    let mut y_val = Vec::with_capacity((n - train_n) * label_dim);

    for (i, &idx) in indices.iter().enumerate() {
        let xs = idx * feat_dim;
        let ys = idx * label_dim;
        if i < train_n {
            x_train.extend_from_slice(&x_data[xs..xs + feat_dim]);
            y_train.extend_from_slice(&y_data[ys..ys + label_dim]);
        } else {
            x_val.extend_from_slice(&x_data[xs..xs + feat_dim]);
            y_val.extend_from_slice(&y_data[ys..ys + label_dim]);
        }
    }

    let x_shape: Vec<usize> = if x.shape.len() == 2 {
        vec![train_n, x.shape[1]]
    } else {
        vec![train_n, feat_dim]
    };
    let x_val_shape: Vec<usize> = if x.shape.len() == 2 {
        vec![n - train_n, x.shape[1]]
    } else {
        vec![n - train_n, feat_dim]
    };
    let y_shape = if y.shape.len() == 2 {
        vec![train_n, y.shape[1]]
    } else {
        vec![train_n]
    };
    let y_val_shape = if y.shape.len() == 2 {
        vec![n - train_n, y.shape[1]]
    } else {
        vec![n - train_n]
    };

    Ok(SplitResult {
        x_train: Tensor::from_cpu_data(&x_shape, x_train, x.device)?,
        y_train: Tensor::from_cpu_data(&y_shape, y_train, y.device)?,
        x_val: Tensor::from_cpu_data(&x_val_shape, x_val, x.device)?,
        y_val: Tensor::from_cpu_data(&y_val_shape, y_val, y.device)?,
    })
}
