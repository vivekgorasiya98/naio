//! CPU tensor kernels.

use super::simd::*;
use matrixmultiply::sgemm;
use rayon::prelude::*;

pub fn matmul_f32(a: &[f32], b: &[f32], m: usize, n: usize, k: usize) -> Vec<f32> {
    const MAX_DIM: usize = 1 << 20;
    if m > MAX_DIM || n > MAX_DIM || k > MAX_DIM {
        return Vec::new();
    }
    let a_need = m.saturating_mul(n);
    let b_need = n.saturating_mul(k);
    let len = m.saturating_mul(k);
    if a.len() < a_need || b.len() < b_need {
        return vec![0.0; len];
    }
    let mut c = vec![0.0f32; len];
    if m == 0 || n == 0 || k == 0 {
        return c;
    }
    // matrixmultiply sgemm: A is m×n, B is n×k, C is m×k (params m, k_inner, n_out)
    unsafe {
        sgemm(
            m,
            n,
            k,
            1.0,
            a.as_ptr(),
            n as isize,
            1,
            b.as_ptr(),
            k as isize,
            1,
            0.0,
            c.as_mut_ptr(),
            k as isize,
            1,
        );
    }
    c
}

/// Naive 2D convolution: NCHW layout, groups=1.
pub fn conv2d_forward(
    input: &[f32],
    weight: &[f32],
    bias: Option<&[f32]>,
    batch: usize,
    in_c: usize,
    in_h: usize,
    in_w: usize,
    out_c: usize,
    k_h: usize,
    k_w: usize,
    stride: usize,
    padding: usize,
) -> (Vec<f32>, usize, usize) {
    let out_h = (in_h + 2 * padding - k_h) / stride + 1;
    let out_w = (in_w + 2 * padding - k_w) / stride + 1;
    let out_len = batch * out_c * out_h * out_w;
    let mut out = vec![0.0f32; out_len];

    for b in 0..batch {
        for oc in 0..out_c {
            for oh in 0..out_h {
                for ow in 0..out_w {
                    let mut sum = bias.map(|bi| bi[oc]).unwrap_or(0.0);
                    for ic in 0..in_c {
                        for kh in 0..k_h {
                            for kw in 0..k_w {
                                let ih = oh * stride + kh;
                                let iw = ow * stride + kw;
                                if ih < padding || iw < padding {
                                    continue;
                                }
                                let ih = ih - padding;
                                let iw = iw - padding;
                                if ih >= in_h || iw >= in_w {
                                    continue;
                                }
                                let in_idx = ((b * in_c + ic) * in_h + ih) * in_w + iw;
                                let w_idx = ((oc * in_c + ic) * k_h + kh) * k_w + kw;
                                sum += input[in_idx] * weight[w_idx];
                            }
                        }
                    }
                    let out_idx = ((b * out_c + oc) * out_h + oh) * out_w + ow;
                    out[out_idx] = sum;
                }
            }
        }
    }
    (out, out_h, out_w)
}

/// Backward pass for conv2d (input and weight gradients).
pub fn conv2d_backward(
    input: &[f32],
    weight: &[f32],
    grad_out: &[f32],
    batch: usize,
    in_c: usize,
    in_h: usize,
    in_w: usize,
    out_c: usize,
    k_h: usize,
    k_w: usize,
    stride: usize,
    padding: usize,
    out_h: usize,
    out_w: usize,
) -> (Vec<f32>, Vec<f32>) {
    let mut grad_input = vec![0.0f32; batch * in_c * in_h * in_w];
    let mut grad_weight = vec![0.0f32; out_c * in_c * k_h * k_w];

    for b in 0..batch {
        for oc in 0..out_c {
            for oh in 0..out_h {
                for ow in 0..out_w {
                    let go = grad_out[((b * out_c + oc) * out_h + oh) * out_w + ow];
                    for ic in 0..in_c {
                        for kh in 0..k_h {
                            for kw in 0..k_w {
                                let ih = oh * stride + kh;
                                let iw = ow * stride + kw;
                                if ih < padding || iw < padding {
                                    continue;
                                }
                                let ih = ih - padding;
                                let iw = iw - padding;
                                if ih >= in_h || iw >= in_w {
                                    continue;
                                }
                                let in_idx = ((b * in_c + ic) * in_h + ih) * in_w + iw;
                                let w_idx = ((oc * in_c + ic) * k_h + kh) * k_w + kw;
                                grad_input[in_idx] += go * weight[w_idx];
                                grad_weight[w_idx] += go * input[in_idx];
                            }
                        }
                    }
                }
            }
        }
    }
    (grad_input, grad_weight)
}

pub fn batch_norm2d_forward(
    x: &[f32],
    gamma: &[f32],
    beta: &[f32],
    batch: usize,
    channels: usize,
    spatial: usize,
    eps: f32,
) -> (Vec<f32>, Vec<f32>, Vec<f32>) {
    let mut out = vec![0.0f32; x.len()];
    let mut mean = vec![0.0f32; channels];
    let mut var = vec![0.0f32; channels];
    let n = (batch * spatial) as f32;

    for c in 0..channels {
        let mut m = 0.0f32;
        for b in 0..batch {
            for s in 0..spatial {
                let idx = (b * channels + c) * spatial + s;
                m += x[idx];
            }
        }
        m /= n;
        mean[c] = m;
        let mut v = 0.0f32;
        for b in 0..batch {
            for s in 0..spatial {
                let idx = (b * channels + c) * spatial + s;
                let d = x[idx] - m;
                v += d * d;
            }
        }
        v /= n;
        var[c] = v;
        let inv_std = 1.0 / (v + eps).sqrt();
        for b in 0..batch {
            for s in 0..spatial {
                let idx = (b * channels + c) * spatial + s;
                out[idx] = gamma[c] * (x[idx] - m) * inv_std + beta[c];
            }
        }
    }
    (out, mean, var)
}

pub fn parallel_add_f32(a: &[f32], b: &[f32]) -> Vec<f32> {
    if a.len() < 65_536 {
        let mut out = vec![0.0; a.len()];
        add_f32(a, b, &mut out);
        return out;
    }
    a.par_chunks(4096)
        .zip(b.par_chunks(4096))
        .map(|(x, y)| {
            let mut chunk = vec![0.0; x.len()];
            add_f32(x, y, &mut chunk);
            chunk
        })
        .reduce(
            || Vec::new(),
            |mut a, b| {
                if a.is_empty() {
                    b
                } else {
                    for (i, v) in b.into_iter().enumerate() {
                        a[i] += v;
                    }
                    a
                }
            },
        )
}
