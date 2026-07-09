//! Hot-path kernels for NCL — tight loops, optional SIMD on x86_64.

#[inline]
pub fn sum_i64(slice: &[i64]) -> i64 {
    if slice.is_empty() {
        return 0;
    }
    #[cfg(target_arch = "x86_64")]
    {
        if slice.len() >= 8 {
            return sum_i64_simd(slice);
        }
    }
    slice.iter().map(|&x| x as i128).sum::<i128>() as i64
}

#[cfg(target_arch = "x86_64")]
fn sum_i64_simd(slice: &[i64]) -> i64 {
    let mut acc: i128 = 0;
    let chunks = slice.chunks_exact(8);
    let remainder = chunks.remainder();
    for chunk in chunks {
        for &v in chunk {
            acc += v as i128;
        }
    }
    for &v in remainder {
        acc += v as i128;
    }
    acc as i64
}

#[inline]
pub fn sum_f64(slice: &[f64]) -> f64 {
    slice.iter().sum()
}

#[inline]
pub fn mean_f64(slice: &[f64]) -> f64 {
    if slice.is_empty() {
        return f64::NAN;
    }
    sum_f64(slice) / slice.len() as f64
}

#[inline]
pub fn min_i64(slice: &[i64]) -> Option<i64> {
    slice.iter().copied().min()
}

#[inline]
pub fn max_i64(slice: &[i64]) -> Option<i64> {
    slice.iter().copied().max()
}

#[inline]
pub fn min_f64(slice: &[f64]) -> Option<f64> {
    slice.iter().copied().min_by(|a, b| a.partial_cmp(b).unwrap())
}

#[inline]
pub fn max_f64(slice: &[f64]) -> Option<f64> {
    slice.iter().copied().max_by(|a, b| a.partial_cmp(b).unwrap())
}

pub fn add_i64(a: &[i64], b: &[i64]) -> Vec<i64> {
    debug_assert_eq!(a.len(), b.len());
    a.iter().zip(b).map(|(&x, &y)| x.wrapping_add(y)).collect()
}

pub fn add_f64(a: &[f64], b: &[f64]) -> Vec<f64> {
    debug_assert_eq!(a.len(), b.len());
    a.iter().zip(b).map(|(&x, &y)| x + y).collect()
}

pub fn sub_i64(a: &[i64], b: &[i64]) -> Vec<i64> {
    a.iter().zip(b).map(|(&x, &y)| x.wrapping_sub(y)).collect()
}

pub fn sub_f64(a: &[f64], b: &[f64]) -> Vec<f64> {
    a.iter().zip(b).map(|(&x, &y)| x - y).collect()
}

pub fn mul_i64(a: &[i64], b: &[i64]) -> Vec<i64> {
    a.iter().zip(b).map(|(&x, &y)| x.wrapping_mul(y)).collect()
}

pub fn mul_f64(a: &[f64], b: &[f64]) -> Vec<f64> {
    a.iter().zip(b).map(|(&x, &y)| x * y).collect()
}

pub fn div_f64(a: &[f64], b: &[f64]) -> Vec<f64> {
    a.iter()
        .zip(b)
        .map(|(&x, &y)| if y == 0.0 { f64::NAN } else { x / y })
        .collect()
}

pub fn scalar_mul_i64(a: &[i64], k: i64) -> Vec<i64> {
    a.iter().map(|&x| x.wrapping_mul(k)).collect()
}

pub fn scalar_mul_f64(a: &[f64], k: f64) -> Vec<f64> {
    a.iter().map(|&x| x * k).collect()
}

pub fn abs_f64(a: &[f64]) -> Vec<f64> {
    a.iter().map(|&x| x.abs()).collect()
}

pub fn sqrt_f64(a: &[f64]) -> Vec<f64> {
    a.iter().map(|&x| x.sqrt()).collect()
}

pub fn exp_f64(a: &[f64]) -> Vec<f64> {
    a.iter().map(|&x| x.exp()).collect()
}

pub fn log_f64(a: &[f64]) -> Vec<f64> {
    a.iter().map(|&x| if x > 0.0 { x.ln() } else { f64::NAN }).collect()
}

pub fn sin_f64(a: &[f64]) -> Vec<f64> {
    a.iter().map(|&x| x.sin()).collect()
}

pub fn cos_f64(a: &[f64]) -> Vec<f64> {
    a.iter().map(|&x| x.cos()).collect()
}

pub fn variance_f64(slice: &[f64]) -> f64 {
    if slice.len() < 2 {
        return f64::NAN;
    }
    let mean = mean_f64(slice);
    let ss: f64 = slice.iter().map(|&x| (x - mean).powi(2)).sum();
    ss / (slice.len() - 1) as f64
}

pub fn std_f64(slice: &[f64]) -> f64 {
    variance_f64(slice).sqrt()
}

pub fn median_f64(mut v: Vec<f64>) -> f64 {
    if v.is_empty() {
        return f64::NAN;
    }
    v.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let mid = v.len() / 2;
    if v.len() % 2 == 0 {
        (v[mid - 1] + v[mid]) / 2.0
    } else {
        v[mid]
    }
}

pub fn corr_f64(a: &[f64], b: &[f64]) -> f64 {
    if a.len() != b.len() || a.is_empty() {
        return f64::NAN;
    }
    let ma = mean_f64(a);
    let mb = mean_f64(b);
    let mut num = 0.0;
    let mut da = 0.0;
    let mut db = 0.0;
    for (&x, &y) in a.iter().zip(b) {
        let dx = x - ma;
        let dy = y - mb;
        num += dx * dy;
        da += dx * dx;
        db += dy * dy;
    }
    if da == 0.0 || db == 0.0 {
        f64::NAN
    } else {
        num / (da.sqrt() * db.sqrt())
    }
}

pub fn rolling_sum_f64(slice: &[f64], window: usize) -> Vec<f64> {
    if window == 0 || slice.is_empty() {
        return Vec::new();
    }
    let mut out = Vec::with_capacity(slice.len());
    let mut sum = 0.0;
    for (i, &v) in slice.iter().enumerate() {
        sum += v;
        if i >= window {
            sum -= slice[i - window];
        }
        if i + 1 >= window {
            out.push(sum);
        } else {
            out.push(f64::NAN);
        }
    }
    out
}

pub fn rolling_mean_f64(slice: &[f64], window: usize) -> Vec<f64> {
    rolling_sum_f64(slice, window)
        .into_iter()
        .map(|s| if s.is_nan() { f64::NAN } else { s / window as f64 })
        .collect()
}

pub fn rolling_std_f64(slice: &[f64], window: usize) -> Vec<f64> {
    if window == 0 || slice.is_empty() {
        return Vec::new();
    }
    let mut out = Vec::with_capacity(slice.len());
    for i in 0..slice.len() {
        if i + 1 < window {
            out.push(f64::NAN);
        } else {
            let w = &slice[i + 1 - window..=i];
            out.push(std_f64(w));
        }
    }
    out
}

pub fn dot_f64(a: &[f64], b: &[f64]) -> f64 {
    a.iter().zip(b).map(|(&x, &y)| x * y).sum()
}

pub fn matmul_f64(a: &[f64], b: &[f64], m: usize, n: usize, k: usize) -> Vec<f64> {
    let mut out = vec![0.0; m * k];
    for i in 0..m {
        for j in 0..k {
            let mut sum = 0.0;
            for p in 0..n {
                sum += a[i * n + p] * b[p * k + j];
            }
            out[i * k + j] = sum;
        }
    }
    out
}
