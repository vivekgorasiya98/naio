//! Sparse adjacency (COO) and graph ops.

use neko_tensor::{Device, Tensor, TensorResult};

#[derive(Debug, Clone)]
pub struct SparseAdj {
    pub rows: Vec<u32>,
    pub cols: Vec<u32>,
    pub vals: Vec<f32>,
    pub n: usize,
}

impl SparseAdj {
    pub fn nnz(&self) -> usize {
        self.rows.len()
    }
}

pub fn from_edge_list(edges: &[(u32, u32, f32)], n: usize) -> SparseAdj {
    let mut rows = Vec::with_capacity(edges.len());
    let mut cols = Vec::with_capacity(edges.len());
    let mut vals = Vec::with_capacity(edges.len());
    for &(r, c, v) in edges {
        rows.push(r);
        cols.push(c);
        vals.push(v);
    }
    SparseAdj { rows, cols, vals, n }
}

pub fn add_self_loops(adj: &SparseAdj) -> SparseAdj {
    let mut rows = adj.rows.clone();
    let mut cols = adj.cols.clone();
    let mut vals = adj.vals.clone();
    for i in 0..adj.n {
        rows.push(i as u32);
        cols.push(i as u32);
        vals.push(1.0);
    }
    SparseAdj {
        rows,
        cols,
        vals,
        n: adj.n,
    }
}

pub fn normalize_adj(adj: &SparseAdj) -> SparseAdj {
    let n = adj.n;
    let mut deg = vec![0.0f32; n];
    for i in 0..adj.nnz() {
        let r = adj.rows[i] as usize;
        let c = adj.cols[i] as usize;
        if r < n {
            deg[r] += adj.vals[i];
        }
        if c < n && r != c {
            deg[c] += adj.vals[i];
        }
    }
    let mut rows = Vec::new();
    let mut cols = Vec::new();
    let mut vals = Vec::new();
    for i in 0..adj.nnz() {
        let r = adj.rows[i] as usize;
        let c = adj.cols[i] as usize;
        if r >= n || c >= n {
            continue;
        }
        let d_r = deg[r].sqrt().max(1e-8);
        let d_c = deg[c].sqrt().max(1e-8);
        rows.push(r as u32);
        cols.push(c as u32);
        vals.push(adj.vals[i] / (d_r * d_c));
    }
    SparseAdj { rows, cols, vals, n }
}

/// COO sparse × dense feature matrix: [n, f].
pub fn sparse_matmul(adj: &SparseAdj, features: &[f32], n: usize, f: usize) -> Vec<f32> {
    let mut out = vec![0.0f32; n * f];
    for i in 0..adj.nnz() {
        let r = adj.rows[i] as usize;
        let c = adj.cols[i] as usize;
        let v = adj.vals[i];
        if r >= n || c >= n {
            continue;
        }
        for j in 0..f {
            out[r * f + j] += v * features[c * f + j];
        }
    }
    out
}

pub fn adj_to_dense(adj: &SparseAdj) -> TensorResult<Tensor> {
    let n = adj.n;
    let mut data = vec![0.0f32; n * n];
    for i in 0..adj.nnz() {
        let r = adj.rows[i] as usize;
        let c = adj.cols[i] as usize;
        if r < n && c < n {
            data[r * n + c] += adj.vals[i];
        }
    }
    Tensor::from_cpu_data(&[n, n], data, Device::Cpu)
}

/// DeepWalk-lite: random-walk structural embeddings.
pub fn structural_embed(adj: &SparseAdj, dim: usize, walks: usize, walk_len: usize, seed: u64) -> TensorResult<Tensor> {
    use rand::prelude::*;
    let mut rng = StdRng::seed_from_u64(seed);
    let n = adj.n;
    let mut neighbors: Vec<Vec<usize>> = vec![vec![]; n];
    for i in 0..adj.nnz() {
        let r = adj.rows[i] as usize;
        let c = adj.cols[i] as usize;
        if r < n && c < n && r != c {
            neighbors[r].push(c);
        }
    }
    let mut embed = vec![0.0f32; n * dim];
    for _ in 0..walks {
        let start = rng.gen_range(0..n);
        let mut node = start;
        for step in 0..walk_len {
            let slot = (node * dim + (step % dim)) % embed.len();
            embed[slot] += 1.0;
            if neighbors[node].is_empty() {
                break;
            }
            let idx = rng.gen_range(0..neighbors[node].len());
            node = neighbors[node][idx];
        }
    }
    for row in 0..n {
        let mut norm = 0.0f32;
        for d in 0..dim {
            norm += embed[row * dim + d] * embed[row * dim + d];
        }
        norm = norm.sqrt().max(1e-8);
        for d in 0..dim {
            embed[row * dim + d] /= norm;
        }
    }
    Tensor::from_cpu_data(&[n, dim], embed, Device::Cpu)
}
