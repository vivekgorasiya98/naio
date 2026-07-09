//! Layer-wise reverse-mode backprop with forward caches.

use crate::layer::{Layer, LayerCache, LayerKind};
use niao_tensor::cpu;
use niao_tensor::{Device, Tensor, TensorResult};

#[derive(Debug, Clone, Default)]
pub struct ParamGrad {
    pub weight: Option<Vec<f32>>,
    pub bias: Option<Vec<f32>>,
    pub gamma: Option<Vec<f32>>,
    pub beta: Option<Vec<f32>>,
}

pub fn clip_grads(grads: &mut [Vec<f32>], max_norm: f32) {
    let total: f32 = grads.iter().map(|g| g.iter().map(|x| x * x).sum::<f32>()).sum();
    let norm = total.sqrt();
    if norm > max_norm && norm > 0.0 {
        let scale = max_norm / norm;
        for g in grads.iter_mut() {
            for v in g.iter_mut() {
                *v *= scale;
            }
        }
    }
}

impl Layer {
    pub fn backward(&self, grad_output: &[f32]) -> TensorResult<(Vec<f32>, ParamGrad)> {
        let cache = self
            .cache
            .as_ref()
            .ok_or_else(|| niao_tensor::TensorError::Op("backward without forward cache".into()))?;
        match &self.kind {
            LayerKind::Linear {
                in_features,
                out_features,
            } => backward_linear(
                cache,
                grad_output,
                self.weight.as_ref().unwrap(),
                *in_features,
                *out_features,
            ),
            LayerKind::ReLU => backward_relu(cache, grad_output),
            LayerKind::Sigmoid => backward_sigmoid(cache, grad_output),
            LayerKind::Tanh => backward_tanh(cache, grad_output),
            LayerKind::Softmax => backward_softmax(cache, grad_output),
            LayerKind::Flatten | LayerKind::Reshape { .. } => {
                let inp = cache.input.as_ref().unwrap();
                let total: usize = inp.shape.iter().product();
                if grad_output.len() == total {
                    Ok((grad_output.to_vec(), ParamGrad::default()))
                } else if inp.shape.len() == 2 && grad_output.len() == inp.shape[1] {
                    Ok((grad_output.to_vec(), ParamGrad::default()))
                } else {
                    Ok((grad_output.to_vec(), ParamGrad::default()))
                }
            }
            LayerKind::Dropout { rate } => backward_dropout(cache, grad_output, *rate),
            LayerKind::Conv2d {
                in_channels,
                out_channels,
                kernel,
                stride,
                padding,
            } => backward_conv2d(
                cache,
                grad_output,
                self.weight.as_ref().unwrap(),
                self.bias.as_ref(),
                *in_channels,
                *out_channels,
                kernel.0,
                kernel.1,
                *stride,
                *padding,
            ),
            LayerKind::BatchNorm2d { channels } => backward_batch_norm(
                cache,
                grad_output,
                self.gamma.as_ref().unwrap(),
                *channels,
            ),
        }
    }
}

fn backward_linear(
    cache: &LayerCache,
    grad_out: &[f32],
    weight: &Tensor,
    in_f: usize,
    out_f: usize,
) -> TensorResult<(Vec<f32>, ParamGrad)> {
    let input = cache.input.as_ref().unwrap();
    let rows = input.shape[0];
    let x = input.to_cpu()?;
    let w = weight.to_cpu()?;
    let go = grad_out;

    // dW = grad_out^T @ x  => [out_f, in_f]
    let got = transpose2d(go, rows, out_f);
    let grad_w = cpu::matmul_f32(&got, &x, out_f, rows, in_f);
    // db = sum over batch
    let mut grad_b = vec![0.0f32; out_f];
    for r in 0..rows {
        for c in 0..out_f {
            grad_b[c] += go[r * out_f + c];
        }
    }
    // dX = grad_out @ W  => [rows, in_f]
    let grad_x = cpu::matmul_f32(go, &w, rows, out_f, in_f);

    Ok((
        grad_x,
        ParamGrad {
            weight: Some(grad_w),
            bias: Some(grad_b),
            gamma: None,
            beta: None,
        },
    ))
}

fn backward_relu(cache: &LayerCache, grad_out: &[f32]) -> TensorResult<(Vec<f32>, ParamGrad)> {
    let fwd = cache.input.as_ref().unwrap().to_cpu()?;
    let mut gx = vec![0.0; fwd.len()];
    cpu::relu_grad_f32(&fwd, grad_out, &mut gx);
    Ok((gx, ParamGrad::default()))
}

fn backward_sigmoid(cache: &LayerCache, grad_out: &[f32]) -> TensorResult<(Vec<f32>, ParamGrad)> {
    let out = cache.output.as_ref().unwrap().to_cpu()?;
    let gx: Vec<f32> = out
        .iter()
        .zip(grad_out.iter())
        .map(|(&s, &g)| g * s * (1.0 - s))
        .collect();
    Ok((gx, ParamGrad::default()))
}

fn backward_tanh(cache: &LayerCache, grad_out: &[f32]) -> TensorResult<(Vec<f32>, ParamGrad)> {
    let out = cache.output.as_ref().unwrap().to_cpu()?;
    let gx: Vec<f32> = out
        .iter()
        .zip(grad_out.iter())
        .map(|(&t, &g)| g * (1.0 - t * t))
        .collect();
    Ok((gx, ParamGrad::default()))
}

fn backward_softmax(cache: &LayerCache, grad_out: &[f32]) -> TensorResult<(Vec<f32>, ParamGrad)> {
    // For softmax output layer with CE loss, grad is pre-computed; passthrough
    Ok((grad_out.to_vec(), ParamGrad::default()))
}

fn backward_dropout(
    cache: &LayerCache,
    grad_out: &[f32],
    rate: f32,
) -> TensorResult<(Vec<f32>, ParamGrad)> {
    let mask = cache.dropout_mask.as_ref().unwrap();
    let keep = 1.0 - rate;
    let scale = 1.0 / keep;
    let gx: Vec<f32> = grad_out
        .iter()
        .zip(mask.iter())
        .map(|(&g, &m)| if m != 0 { g * scale } else { 0.0 })
        .collect();
    Ok((gx, ParamGrad::default()))
}

fn backward_conv2d(
    cache: &LayerCache,
    grad_out: &[f32],
    weight: &Tensor,
    bias: Option<&Tensor>,
    in_c: usize,
    out_c: usize,
    k_h: usize,
    k_w: usize,
    stride: usize,
    padding: usize,
) -> TensorResult<(Vec<f32>, ParamGrad)> {
    let input = cache.input.as_ref().unwrap();
    let (batch, _, in_h, in_w) = (
        input.shape[0],
        input.shape[1],
        input.shape[2],
        input.shape[3],
    );
    let out_h = cache.conv_out_h.unwrap();
    let out_w = cache.conv_out_w.unwrap();
    let x = input.to_cpu()?;
    let w = weight.to_cpu()?;
    let (grad_input, grad_weight) = cpu::conv2d_backward(
        &x,
        &w,
        grad_out,
        batch,
        in_c,
        in_h,
        in_w,
        out_c,
        k_h,
        k_w,
        stride,
        padding,
        out_h,
        out_w,
    );
    let grad_bias = if bias.is_some() {
        let mut gb = vec![0.0f32; out_c];
        for b in 0..batch {
            for oc in 0..out_c {
                for oh in 0..out_h {
                    for ow in 0..out_w {
                        let idx = ((b * out_c + oc) * out_h + oh) * out_w + ow;
                        gb[oc] += grad_out[idx];
                    }
                }
            }
        }
        Some(gb)
    } else {
        None
    };
    Ok((
        grad_input,
        ParamGrad {
            weight: Some(grad_weight),
            bias: grad_bias,
            gamma: None,
            beta: None,
        },
    ))
}

fn backward_batch_norm(
    cache: &LayerCache,
    grad_out: &[f32],
    gamma: &Tensor,
    channels: usize,
) -> TensorResult<(Vec<f32>, ParamGrad)> {
    let mean = cache.bn_mean.as_ref().unwrap();
    let var = cache.bn_var.as_ref().unwrap();
    let input = cache.input.as_ref().unwrap();
    let batch = input.shape[0];
    let spatial = input.shape[2] * input.shape[3];
    let n = (batch * spatial) as f32;
    let eps = 1e-5;
    let x = input.to_cpu()?;
    let g = gamma.to_cpu()?;
    let mut grad_input = vec![0.0f32; x.len()];
    let mut grad_gamma = vec![0.0f32; channels];
    let mut grad_beta = vec![0.0f32; channels];

    for c in 0..channels {
        let inv_std = 1.0 / (var[c] + eps).sqrt();
        let mut sum_dgamma = 0.0f32;
        let mut sum_dbeta = 0.0f32;
        for b in 0..batch {
            for s in 0..spatial {
                let idx = (b * channels + c) * spatial + s;
                sum_dbeta += grad_out[idx];
                let xhat = (x[idx] - mean[c]) * inv_std;
                sum_dgamma += grad_out[idx] * xhat;
            }
        }
        grad_gamma[c] = sum_dgamma;
        grad_beta[c] = sum_dbeta;
        for b in 0..batch {
            for s in 0..spatial {
                let idx = (b * channels + c) * spatial + s;
                let xhat = (x[idx] - mean[c]) * inv_std;
                let dxhat = grad_out[idx] * g[c];
                let dvar: f32 = 0.0; // simplified
                let _ = dvar;
                grad_input[idx] = dxhat * inv_std / n;
                let _ = xhat;
            }
        }
        // Simplified BN backward per channel
        let mut sum1 = 0.0f32;
        let mut sum2 = 0.0f32;
        for b in 0..batch {
            for s in 0..spatial {
                let idx = (b * channels + c) * spatial + s;
                let xhat = (x[idx] - mean[c]) * inv_std;
                sum1 += grad_out[idx] * g[c];
                sum2 += grad_out[idx] * g[c] * xhat;
            }
        }
        for b in 0..batch {
            for s in 0..spatial {
                let idx = (b * channels + c) * spatial + s;
                let xhat = (x[idx] - mean[c]) * inv_std;
                grad_input[idx] =
                    (grad_out[idx] * g[c] - sum1 / n - xhat * sum2 / n) * inv_std;
            }
        }
    }
    Ok((
        grad_input,
        ParamGrad {
            weight: None,
            bias: None,
            gamma: Some(grad_gamma),
            beta: Some(grad_beta),
        },
    ))
}

fn transpose2d(data: &[f32], rows: usize, cols: usize) -> Vec<f32> {
    let mut out = vec![0.0; rows * cols];
    for r in 0..rows {
        for c in 0..cols {
            out[c * rows + r] = data[r * cols + c];
        }
    }
    out
}
