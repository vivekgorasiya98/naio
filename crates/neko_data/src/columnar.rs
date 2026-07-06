//! Chunked training from columnar data without full materialization.

use neko_tensor::{Device, Tensor, TensorResult};

#[derive(Debug, Clone)]
pub struct ColumnarEpoch {
    pub feature_cols: Vec<Vec<f32>>,
    pub label_col: Vec<f32>,
    pub batch_size: usize,
    pub cursor: usize,
}

impl ColumnarEpoch {
    pub fn new(feature_cols: Vec<Vec<f32>>, label_col: Vec<f32>, batch_size: usize) -> Self {
        Self {
            feature_cols,
            label_col,
            batch_size,
            cursor: 0,
        }
    }

    pub fn len(&self) -> usize {
        self.label_col.len()
    }

    pub fn reset(&mut self) {
        self.cursor = 0;
    }

    pub fn next_batch(&mut self) -> Option<(Tensor, Tensor)> {
        let n = self.label_col.len();
        if self.cursor >= n {
            return None;
        }
        let end = (self.cursor + self.batch_size).min(n);
        let batch = end - self.cursor;
        let feat_n = self.feature_cols.len();
        let mut x_data = vec![0.0f32; batch * feat_n];
        for (f, col) in self.feature_cols.iter().enumerate() {
            for (i, idx) in (self.cursor..end).enumerate() {
                x_data[i * feat_n + f] = col[idx];
            }
        }
        let y_data: Vec<f32> = self.label_col[self.cursor..end].to_vec();
        self.cursor = end;
        let x = Tensor::from_cpu_data(&[batch, feat_n], x_data, Device::Cpu).ok()?;
        let y = Tensor::from_cpu_data(&[batch], y_data, Device::Cpu).ok()?;
        Some((x, y))
    }
}

pub fn columnar_to_tensors(epoch: &ColumnarEpoch) -> TensorResult<(Tensor, Tensor)> {
    let n = epoch.len();
    let feat_n = epoch.feature_cols.len();
    let mut x_data = vec![0.0f32; n * feat_n];
    for (f, col) in epoch.feature_cols.iter().enumerate() {
        for (r, &v) in col.iter().enumerate() {
            x_data[r * feat_n + f] = v;
        }
    }
    let x = Tensor::from_cpu_data(&[n, feat_n], x_data, Device::Cpu)?;
    let y = Tensor::from_cpu_data(&[n], epoch.label_col.clone(), Device::Cpu)?;
    Ok((x, y))
}
