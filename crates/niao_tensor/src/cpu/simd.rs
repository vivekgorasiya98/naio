//! CPU SIMD and elementwise kernels (x86_64 AVX2 when available).

pub fn add_f32(a: &[f32], b: &[f32], out: &mut [f32]) {
    debug_assert_eq!(a.len(), b.len());
    debug_assert_eq!(a.len(), out.len());
    #[cfg(target_arch = "x86_64")]
    {
        if std::arch::is_x86_feature_detected!("avx2") {
            unsafe {
                simd_add_f32_avx2(a, b, out);
            }
            return;
        }
    }
    scalar_add_f32(a, b, out);
}

pub fn mul_f32(a: &[f32], b: &[f32], out: &mut [f32]) {
    debug_assert_eq!(a.len(), b.len());
    debug_assert_eq!(a.len(), out.len());
    #[cfg(target_arch = "x86_64")]
    {
        if std::arch::is_x86_feature_detected!("avx2") {
            unsafe {
                simd_mul_f32_avx2(a, b, out);
            }
            return;
        }
    }
    for i in 0..a.len() {
        out[i] = a[i] * b[i];
    }
}

pub fn sub_f32(a: &[f32], b: &[f32], out: &mut [f32]) {
    debug_assert_eq!(a.len(), b.len());
    for i in 0..a.len() {
        out[i] = a[i] - b[i];
    }
}

pub fn scalar_mul_f32(a: &[f32], k: f32, out: &mut [f32]) {
    for i in 0..a.len() {
        out[i] = a[i] * k;
    }
}

pub fn relu_f32(a: &[f32], out: &mut [f32]) {
    for i in 0..a.len() {
        out[i] = a[i].max(0.0);
    }
}

pub fn relu_grad_f32(fwd: &[f32], grad: &[f32], out: &mut [f32]) {
    debug_assert_eq!(fwd.len(), grad.len());
    debug_assert_eq!(fwd.len(), out.len());
    let n = fwd.len().min(grad.len()).min(out.len());
    for i in 0..n {
        out[i] = if fwd[i] > 0.0 { grad[i] } else { 0.0 };
    }
}

pub fn sigmoid_f32(a: &[f32], out: &mut [f32]) {
    for i in 0..a.len() {
        out[i] = 1.0 / (1.0 + (-a[i]).exp());
    }
}

pub fn tanh_f32(a: &[f32], out: &mut [f32]) {
    for i in 0..a.len() {
        out[i] = a[i].tanh();
    }
}

/// Add bias vector to each row of a rows×cols matrix.
pub fn add_bias_rows(data: &[f32], bias: &[f32], rows: usize, cols: usize, out: &mut [f32]) {
    debug_assert_eq!(data.len(), rows * cols);
    debug_assert_eq!(bias.len(), cols);
    debug_assert_eq!(out.len(), rows * cols);
    for r in 0..rows {
        let base = r * cols;
        for c in 0..cols {
            out[base + c] = data[base + c] + bias[c];
        }
    }
}

/// Row-wise softmax for a rows×cols matrix in row-major order.
pub fn softmax_2d(a: &[f32], rows: usize, cols: usize, out: &mut [f32]) {
    debug_assert_eq!(a.len(), rows * cols);
    debug_assert_eq!(out.len(), rows * cols);
    for r in 0..rows {
        let base = r * cols;
        let row = &a[base..base + cols];
        let max = row.iter().copied().fold(f32::NEG_INFINITY, f32::max);
        let mut sum = 0.0f32;
        for c in 0..cols {
            let e = (row[c] - max).exp();
            out[base + c] = e;
            sum += e;
        }
        for c in 0..cols {
            out[base + c] /= sum;
        }
    }
}

fn scalar_add_f32(a: &[f32], b: &[f32], out: &mut [f32]) {
    for i in 0..a.len() {
        out[i] = a[i] + b[i];
    }
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx2")]
unsafe fn simd_add_f32_avx2(a: &[f32], b: &[f32], out: &mut [f32]) {
    use std::arch::x86_64::*;
    let n = a.len();
    let chunks = n / 8;
    let mut i = 0;
    for _ in 0..chunks {
        let va = _mm256_loadu_ps(a.as_ptr().add(i));
        let vb = _mm256_loadu_ps(b.as_ptr().add(i));
        let vr = _mm256_add_ps(va, vb);
        _mm256_storeu_ps(out.as_mut_ptr().add(i), vr);
        i += 8;
    }
    while i < n {
        out[i] = a[i] + b[i];
        i += 1;
    }
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx2")]
unsafe fn simd_mul_f32_avx2(a: &[f32], b: &[f32], out: &mut [f32]) {
    use std::arch::x86_64::*;
    let n = a.len();
    let chunks = n / 8;
    let mut i = 0;
    for _ in 0..chunks {
        let va = _mm256_loadu_ps(a.as_ptr().add(i));
        let vb = _mm256_loadu_ps(b.as_ptr().add(i));
        let vr = _mm256_mul_ps(va, vb);
        _mm256_storeu_ps(out.as_mut_ptr().add(i), vr);
        i += 8;
    }
    while i < n {
        out[i] = a[i] * b[i];
        i += 1;
    }
}
