//! Normalization utilities.

use niao_tensor::{Tensor, TensorResult};

#[derive(Debug, Clone)]
pub struct Normalizer {
    pub mean: Vec<f32>,
    pub std: Vec<f32>,
    pub min: Vec<f32>,
    pub max: Vec<f32>,
    pub mode: NormalizerMode,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NormalizerMode {
    ZScore,
    MinMax,
}

pub fn standardize_fit_transform(x: &Tensor) -> TensorResult<(Normalizer, Tensor)> {
    fit_transform(x, NormalizerMode::ZScore)
}

pub fn minmax_fit_transform(x: &Tensor) -> TensorResult<(Normalizer, Tensor)> {
    fit_transform(x, NormalizerMode::MinMax)
}

pub fn normalize_fit_transform(x: &Tensor) -> TensorResult<(Normalizer, Tensor)> {
    minmax_fit_transform(x)
}

fn fit_transform(x: &Tensor, mode: NormalizerMode) -> TensorResult<(Normalizer, Tensor)> {
    let data = x.to_cpu()?;
    let n = x.shape[0];
    let feat = x.len() / n.max(1);
    let mut mean = vec![0.0f32; feat];
    let mut min = vec![f32::INFINITY; feat];
    let mut max = vec![f32::NEG_INFINITY; feat];

    for row in 0..n {
        for f in 0..feat {
            let v = data[row * feat + f];
            mean[f] += v;
            min[f] = min[f].min(v);
            max[f] = max[f].max(v);
        }
    }
    for f in 0..feat {
        mean[f] /= n.max(1) as f32;
    }
    let mut std = vec![0.0f32; feat];
    for row in 0..n {
        for f in 0..feat {
            let d = data[row * feat + f] - mean[f];
            std[f] += d * d;
        }
    }
    for f in 0..feat {
        std[f] = (std[f] / n.max(1) as f32).sqrt().max(1e-8);
    }

    let mut out = data.clone();
    for row in 0..n {
        for f in 0..feat {
            let idx = row * feat + f;
            out[idx] = match mode {
                NormalizerMode::ZScore => (data[idx] - mean[f]) / std[f],
                NormalizerMode::MinMax => {
                    let range = (max[f] - min[f]).max(1e-8);
                    (data[idx] - min[f]) / range
                }
            };
        }
    }

    let norm = Normalizer {
        mean,
        std,
        min,
        max,
        mode,
    };
    let shape = x.shape.clone();
    Ok((norm, Tensor::from_cpu_data(&shape, out, x.device)?))
}

impl Normalizer {
    pub fn transform(&self, x: &Tensor) -> TensorResult<Tensor> {
        let data = x.to_cpu()?;
        let n = x.shape[0];
        let feat = self.mean.len();
        let mut out = data.clone();
        for row in 0..n {
            for f in 0..feat {
                let idx = row * feat + f;
                out[idx] = match self.mode {
                    NormalizerMode::ZScore => (data[idx] - self.mean[f]) / self.std[f],
                    NormalizerMode::MinMax => {
                        let range = (self.max[f] - self.min[f]).max(1e-8);
                        (data[idx] - self.min[f]) / range
                    }
                };
            }
        }
        Tensor::from_cpu_data(&x.shape, out, x.device)
    }
}
