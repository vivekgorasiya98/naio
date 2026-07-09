//! Batched data loading.

use niao_tensor::{Device, Tensor, TensorResult};

#[derive(Clone)]
pub struct DataLoader {
    pub features: Tensor,
    pub labels: Tensor,
    pub batch_size: usize,
    pub shuffle: bool,
    cursor: usize,
    order: Vec<usize>,
}

impl DataLoader {
    pub fn new(features: Tensor, labels: Tensor, batch_size: usize) -> TensorResult<Self> {
        if features.shape[0] != labels.shape[0] {
            return Err(niao_tensor::TensorError::Shape(
                "features/labels batch dim mismatch".into(),
            ));
        }
        let n = features.shape[0];
        Ok(Self {
            features,
            labels,
            batch_size,
            shuffle: true,
            cursor: 0,
            order: (0..n).collect(),
        })
    }

    pub fn len(&self) -> usize {
        (self.features.shape[0] + self.batch_size - 1) / self.batch_size
    }

    pub fn reset(&mut self) {
        self.cursor = 0;
        if self.shuffle {
            use rand::seq::SliceRandom;
            let mut rng = rand::thread_rng();
            self.order.shuffle(&mut rng);
        }
    }

    pub fn next_batch(&mut self) -> Option<(Tensor, Tensor)> {
        let n = self.features.shape[0];
        if self.cursor >= n {
            return None;
        }
        let start = self.cursor;
        let end = (start + self.batch_size).min(n);
        self.cursor = end;
        let indices: Vec<usize> = self.order[start..end].to_vec();
        self.gather_batch(&indices).ok()
    }

    fn gather_batch(&self, indices: &[usize]) -> TensorResult<(Tensor, Tensor)> {
        let feat_dim = self.features.len() / self.features.shape[0];
        let x_data = self.features.to_cpu()?;
        let y_data = self.labels.to_cpu()?;
        let batch = indices.len();
        let mut xb = vec![0.0f32; batch * feat_dim];
        let mut yb = vec![0.0f32; batch];
        for (bi, &idx) in indices.iter().enumerate() {
            for f in 0..feat_dim {
                xb[bi * feat_dim + f] = x_data[idx * feat_dim + f];
            }
            yb[bi] = y_data[idx];
        }
        let device = self.features.device;
        let x = Tensor::from_cpu_data(&[batch, feat_dim], xb, device)?;
        let y = Tensor::from_cpu_data(&[batch, 1], yb, device)?;
        Ok((x, y))
    }
}

pub fn make_dataloader(
    x: &[f32],
    y: &[f32],
    rows: usize,
    cols: usize,
    batch_size: usize,
    device: Device,
) -> TensorResult<DataLoader> {
    let features = Tensor::from_cpu_data(&[rows, cols], x.to_vec(), device)?;
    let labels = Tensor::from_cpu_data(&[rows, 1], y.to_vec(), device)?;
    DataLoader::new(features, labels, batch_size)
}
