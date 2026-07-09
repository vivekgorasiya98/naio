//! Neural network layers.

use niao_tensor::cpu;
use niao_tensor::{Device, Tensor, TensorResult};
use rand::Rng;
use rand_distr::{Distribution, Normal};

#[derive(Debug, Clone)]
pub enum LayerKind {
    Linear { in_features: usize, out_features: usize },
    ReLU,
    Sigmoid,
    Tanh,
    Softmax,
    Flatten,
    Reshape { shape: Vec<usize> },
    Dropout { rate: f32 },
    Conv2d {
        in_channels: usize,
        out_channels: usize,
        kernel: (usize, usize),
        stride: usize,
        padding: usize,
    },
    BatchNorm2d { channels: usize },
}

/// Forward-pass cache for backprop.
#[derive(Debug, Clone, Default)]
pub struct LayerCache {
    pub input: Option<Tensor>,
    pub output: Option<Tensor>,
    pub dropout_mask: Option<Vec<u8>>,
    pub conv_out_h: Option<usize>,
    pub conv_out_w: Option<usize>,
    pub bn_mean: Option<Vec<f32>>,
    pub bn_var: Option<Vec<f32>>,
}

#[derive(Debug, Clone)]
pub struct Layer {
    pub kind: LayerKind,
    pub weight: Option<Tensor>,
    pub bias: Option<Tensor>,
    pub gamma: Option<Tensor>,
    pub beta: Option<Tensor>,
    pub training: bool,
    pub cache: Option<LayerCache>,
}

impl Layer {
    pub fn linear(in_features: usize, out_features: usize, device: Device) -> TensorResult<Self> {
        let scale = (2.0 / in_features as f32).sqrt();
        let mut rng = rand::thread_rng();
        let normal = Normal::new(0.0, scale as f64).unwrap();
        let w_data: Vec<f32> = (0..in_features * out_features)
            .map(|_| normal.sample(&mut rng) as f32)
            .collect();
        let b_data = vec![0.0f32; out_features];
        Ok(Self {
            kind: LayerKind::Linear {
                in_features,
                out_features,
            },
            weight: Some(Tensor::from_cpu_data(
                &[out_features, in_features],
                w_data,
                device,
            )?),
            bias: Some(Tensor::from_cpu_data(&[out_features], b_data, device)?),
            gamma: None,
            beta: None,
            training: true,
            cache: None,
        })
    }

    pub fn relu() -> Self {
        Self {
            kind: LayerKind::ReLU,
            weight: None,
            bias: None,
            gamma: None,
            beta: None,
            training: true,
            cache: None,
        }
    }

    pub fn sigmoid() -> Self {
        Self {
            kind: LayerKind::Sigmoid,
            weight: None,
            bias: None,
            gamma: None,
            beta: None,
            training: true,
            cache: None,
        }
    }

    pub fn tanh() -> Self {
        Self {
            kind: LayerKind::Tanh,
            weight: None,
            bias: None,
            gamma: None,
            beta: None,
            training: true,
            cache: None,
        }
    }

    pub fn softmax() -> Self {
        Self {
            kind: LayerKind::Softmax,
            weight: None,
            bias: None,
            gamma: None,
            beta: None,
            training: true,
            cache: None,
        }
    }

    pub fn flatten() -> Self {
        Self {
            kind: LayerKind::Flatten,
            weight: None,
            bias: None,
            gamma: None,
            beta: None,
            training: true,
            cache: None,
        }
    }

    pub fn dropout(rate: f32) -> Self {
        Self {
            kind: LayerKind::Dropout { rate },
            weight: None,
            bias: None,
            gamma: None,
            beta: None,
            training: true,
            cache: None,
        }
    }

    pub fn conv2d(
        in_channels: usize,
        out_channels: usize,
        kernel: (usize, usize),
        device: Device,
    ) -> TensorResult<Self> {
        let k_h = kernel.0;
        let k_w = kernel.1;
        let n = out_channels * in_channels * k_h * k_w;
        let mut rng = rand::thread_rng();
        let scale = (2.0 / (in_channels * k_h * k_w) as f32).sqrt();
        let normal = Normal::new(0.0, scale as f64).unwrap();
        let w: Vec<f32> = (0..n).map(|_| normal.sample(&mut rng) as f32).collect();
        let b = vec![0.0f32; out_channels];
        Ok(Self {
            kind: LayerKind::Conv2d {
                in_channels,
                out_channels,
                kernel,
                stride: 1,
                padding: 0,
            },
            weight: Some(Tensor::from_cpu_data(
                &[out_channels, in_channels, k_h, k_w],
                w,
                device,
            )?),
            bias: Some(Tensor::from_cpu_data(&[out_channels], b, device)?),
            gamma: None,
            beta: None,
            training: true,
            cache: None,
        })
    }

    pub fn batch_norm2d(channels: usize, device: Device) -> TensorResult<Self> {
        let ones = vec![1.0f32; channels];
        let zeros = vec![0.0f32; channels];
        Ok(Self {
            kind: LayerKind::BatchNorm2d { channels },
            weight: None,
            bias: None,
            gamma: Some(Tensor::from_cpu_data(&[channels], ones, device)?),
            beta: Some(Tensor::from_cpu_data(&[channels], zeros, device)?),
            training: true,
            cache: None,
        })
    }

    pub fn forward(&mut self, input: &Tensor) -> TensorResult<Tensor> {
        self.cache = None;
        if !self.training {
            return self.forward_impl(input, false);
        }
        self.forward_impl(input, true)
    }

    fn forward_impl(&mut self, input: &Tensor, record: bool) -> TensorResult<Tensor> {
        match &self.kind {
            LayerKind::Linear {
                in_features,
                out_features,
            } => {
                let w = self.weight.as_ref().unwrap();
                let b = self.bias.as_ref().unwrap();
                let x = input.reshape(&[input.len() / in_features, *in_features])?;
                let w_cpu = w.to_cpu()?;
                let wt = transpose_rows_cols(&w_cpu, *out_features, *in_features);
                let y = x.matmul(&Tensor::from_cpu_data(&[*in_features, *out_features], wt, input.device)?)?;
                let y_data = y.to_cpu()?;
                let b_data = b.to_cpu()?;
                let rows = y.shape[0];
                let cols = *out_features;
                let mut out = vec![0.0; rows * cols];
                cpu::add_bias_rows(&y_data, &b_data, rows, cols, &mut out);
                let result = Tensor::from_cpu_data(&[rows, cols], out, input.device)?;
                if record {
                    self.cache = Some(LayerCache {
                        input: Some(x),
                        output: Some(result.clone()),
                        ..Default::default()
                    });
                }
                Ok(result)
            }
            LayerKind::ReLU => {
                let result = input.relu()?;
                if record {
                    self.cache = Some(LayerCache {
                        input: Some(input.clone()),
                        output: Some(result.clone()),
                        ..Default::default()
                    });
                }
                Ok(result)
            }
            LayerKind::Sigmoid => {
                let a = input.to_cpu()?;
                let mut out = vec![0.0; a.len()];
                cpu::sigmoid_f32(&a, &mut out);
                let result = Tensor::from_cpu_data(&input.shape, out, input.device)?;
                if record {
                    self.cache = Some(LayerCache {
                        input: Some(input.clone()),
                        output: Some(result.clone()),
                        ..Default::default()
                    });
                }
                Ok(result)
            }
            LayerKind::Tanh => {
                let a = input.to_cpu()?;
                let mut out = vec![0.0; a.len()];
                cpu::tanh_f32(&a, &mut out);
                let result = Tensor::from_cpu_data(&input.shape, out, input.device)?;
                if record {
                    self.cache = Some(LayerCache {
                        input: Some(input.clone()),
                        output: Some(result.clone()),
                        ..Default::default()
                    });
                }
                Ok(result)
            }
            LayerKind::Softmax => {
                let result = input.softmax(1)?;
                if record {
                    self.cache = Some(LayerCache {
                        input: Some(input.clone()),
                        output: Some(result.clone()),
                        ..Default::default()
                    });
                }
                Ok(result)
            }
            LayerKind::Flatten => {
                let n = input.len();
                let result = input.reshape(&[1, n])?;
                if record {
                    self.cache = Some(LayerCache {
                        input: Some(input.clone()),
                        ..Default::default()
                    });
                }
                Ok(result)
            }
            LayerKind::Reshape { shape } => {
                let result = input.reshape(shape)?;
                if record {
                    self.cache = Some(LayerCache {
                        input: Some(input.clone()),
                        ..Default::default()
                    });
                }
                Ok(result)
            }
            LayerKind::Dropout { rate } => {
                if !self.training || *rate <= 0.0 {
                    return Ok(input.clone());
                }
                let a = input.to_cpu()?;
                let mut rng = rand::thread_rng();
                let keep = 1.0 - rate;
                let scale = 1.0 / keep;
                let mut mask = Vec::with_capacity(a.len());
                let out: Vec<f32> = a
                    .iter()
                    .map(|&v| {
                        let kept = rng.gen::<f32>() < keep;
                        mask.push(if kept { 1 } else { 0 });
                        if kept { v * scale } else { 0.0 }
                    })
                    .collect();
                let result = Tensor::from_cpu_data(&input.shape, out, input.device)?;
                if record {
                    self.cache = Some(LayerCache {
                        input: Some(input.clone()),
                        dropout_mask: Some(mask),
                        ..Default::default()
                    });
                }
                Ok(result)
            }
            LayerKind::Conv2d {
                in_channels,
                out_channels,
                kernel,
                stride,
                padding,
            } => {
                if input.shape.len() != 4 {
                    return Err(niao_tensor::TensorError::Shape(
                        "conv2d expects NCHW 4D input".into(),
                    ));
                }
                let (batch, in_c, in_h, in_w) =
                    (input.shape[0], input.shape[1], input.shape[2], input.shape[3]);
                let x = input.to_cpu()?;
                let w = self.weight.as_ref().unwrap().to_cpu()?;
                let b = self.bias.as_ref().map(|t| t.to_cpu().unwrap());
                let (out_data, out_h, out_w) = cpu::conv2d_forward(
                    &x,
                    &w,
                    b.as_deref(),
                    batch,
                    in_c,
                    in_h,
                    in_w,
                    *out_channels,
                    kernel.0,
                    kernel.1,
                    *stride,
                    *padding,
                );
                let result = Tensor::from_cpu_data(
                    &[batch, *out_channels, out_h, out_w],
                    out_data,
                    input.device,
                )?;
                if record {
                    self.cache = Some(LayerCache {
                        input: Some(input.clone()),
                        conv_out_h: Some(out_h),
                        conv_out_w: Some(out_w),
                        ..Default::default()
                    });
                }
                Ok(result)
            }
            LayerKind::BatchNorm2d { channels } => {
                if input.shape.len() != 4 {
                    return Err(niao_tensor::TensorError::Shape(
                        "batch_norm2d expects NCHW".into(),
                    ));
                }
                let batch = input.shape[0];
                let spatial = input.shape[2] * input.shape[3];
                let x = input.to_cpu()?;
                let gamma = self.gamma.as_ref().unwrap().to_cpu()?;
                let beta = self.beta.as_ref().unwrap().to_cpu()?;
                let (out, mean, var) = cpu::batch_norm2d_forward(
                    &x,
                    &gamma,
                    &beta,
                    batch,
                    *channels,
                    spatial,
                    1e-5,
                );
                let result = Tensor::from_cpu_data(&input.shape, out, input.device)?;
                if record {
                    self.cache = Some(LayerCache {
                        input: Some(input.clone()),
                        bn_mean: Some(mean),
                        bn_var: Some(var),
                        ..Default::default()
                    });
                }
                Ok(result)
            }
        }
    }

    pub fn parameters(&self) -> Vec<&Tensor> {
        let mut p = Vec::new();
        if let Some(w) = &self.weight {
            p.push(w);
        }
        if let Some(b) = &self.bias {
            p.push(b);
        }
        if let Some(g) = &self.gamma {
            p.push(g);
        }
        if let Some(b) = &self.beta {
            p.push(b);
        }
        p
    }
}

fn transpose_rows_cols(data: &[f32], rows: usize, cols: usize) -> Vec<f32> {
    let mut out = vec![0.0; rows * cols];
    for r in 0..rows {
        for c in 0..cols {
            out[c * rows + r] = data[r * cols + c];
        }
    }
    out
}
