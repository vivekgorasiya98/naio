//! Graph neural network layers.

use niao_graph::{sparse_matmul, SparseAdj};
use niao_tensor::cpu;
use niao_tensor::{Device, Tensor, TensorResult};
use rand::SeedableRng;
use rand::rngs::StdRng;
use rand_distr::{Distribution, Normal};

#[derive(Debug, Clone)]
pub struct GcnLayer {
    pub in_features: usize,
    pub out_features: usize,
    pub weight: Tensor,
    pub bias: Tensor,
    pub adj: Option<SparseAdj>,
}

impl GcnLayer {
    pub fn new(in_features: usize, out_features: usize, device: Device) -> TensorResult<Self> {
        let scale = (2.0 / in_features as f32).sqrt();
        let mut rng = StdRng::seed_from_u64(7);
        let normal = Normal::new(0.0, scale as f64).unwrap();
        let w: Vec<f32> = (0..in_features * out_features)
            .map(|_| normal.sample(&mut rng) as f32)
            .collect();
        let b = vec![0.0f32; out_features];
        Ok(Self {
            in_features,
            out_features,
            weight: Tensor::from_cpu_data(&[in_features, out_features], w, device)?,
            bias: Tensor::from_cpu_data(&[out_features], b, device)?,
            adj: None,
        })
    }

    pub fn forward(&self, features: &Tensor, adj: &SparseAdj) -> TensorResult<Tensor> {
        let n = adj.n;
        let f_in = self.in_features;
        let f_out = self.out_features;
        // `adj` is expected to be symmetric-normalized (see nml_graph_normalize).
        let x = features.cpu_data()?;
        let w = self.weight.cpu_data()?;
        let ax = sparse_matmul(adj, x, n, f_in);
        let mut h = cpu::matmul_f32(&ax, w, n, f_in, f_out);
        for v in h.iter_mut() {
            *v = v.max(0.0);
        }
        Tensor::from_cpu_data(&[n, f_out], h, features.device)
    }
}

#[derive(Debug, Clone)]
pub struct GraphSageLayer {
    pub in_features: usize,
    pub out_features: usize,
    pub weight: Tensor,
    pub bias: Tensor,
}

impl GraphSageLayer {
    pub fn new(in_features: usize, out_features: usize, device: Device) -> TensorResult<Self> {
        GcnLayer::new(in_features, out_features, device).map(|g| Self {
            in_features: g.in_features,
            out_features: g.out_features,
            weight: g.weight,
            bias: g.bias,
        })
    }

    pub fn forward(&self, features: &Tensor, adj: &SparseAdj) -> TensorResult<Tensor> {
        let n = adj.n;
        let f_in = self.in_features;
        let f_out = self.out_features;
        let x = features.cpu_data()?;
        let mut neighbor_sum = sparse_matmul(adj, x, n, f_in);
        let mut deg = vec![0.0f32; n];
        for i in 0..adj.nnz() {
            deg[adj.rows[i] as usize] += 1.0;
        }
        for i in 0..n {
            let d = deg[i].max(1.0);
            for j in 0..f_in {
                neighbor_sum[i * f_in + j] /= d;
            }
        }
        let mut combined = vec![0.0f32; n * f_in];
        for i in 0..n {
            for j in 0..f_in {
                combined[i * f_in + j] = (x[i * f_in + j] + neighbor_sum[i * f_in + j]) * 0.5;
            }
        }
        let w = self.weight.cpu_data()?;
        let mut h = cpu::matmul_f32(&combined, w, n, f_in, f_out);
        let b = self.bias.cpu_data()?;
        for i in 0..n {
            for j in 0..f_out {
                h[i * f_out + j] += b[j];
            }
        }
        for v in h.iter_mut() {
            *v = v.max(0.0);
        }
        Tensor::from_cpu_data(&[n, f_out], h, features.device)
    }
}

#[derive(Debug, Clone)]
pub struct GnnModel {
    pub layers: Vec<GcnLayer>,
    pub device: Device,
}

impl GnnModel {
    pub fn forward(&self, features: &Tensor, adj: &SparseAdj) -> TensorResult<Tensor> {
        let mut x = features.clone();
        for layer in &self.layers {
            x = layer.forward(&x, adj)?;
        }
        Ok(x)
    }
}
