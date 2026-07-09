//! Loss functions.

use niao_tensor::cpu;
use niao_tensor::{Tensor, TensorResult};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LossKind {
    Mse,
    CrossEntropy,
    BinaryCrossEntropy,
}

pub fn compute_loss(kind: LossKind, pred: &Tensor, target: &Tensor) -> TensorResult<Tensor> {
    match kind {
        LossKind::Mse => mse(pred, target),
        LossKind::CrossEntropy => cross_entropy(pred, target),
        LossKind::BinaryCrossEntropy => bce(pred, target),
    }
}

fn mse(pred: &Tensor, target: &Tensor) -> TensorResult<Tensor> {
    let p = pred.to_cpu()?;
    let t = target.to_cpu()?;
    if p.len() != t.len() {
        return Err(niao_tensor::TensorError::Shape("mse: length mismatch".into()));
    }
    let mut sum = 0.0f32;
    for i in 0..p.len() {
        let d = p[i] - t[i];
        sum += d * d;
    }
    let loss = sum / p.len() as f32;
    Tensor::from_cpu_data(&[1], vec![loss], pred.device)
}

fn cross_entropy(pred: &Tensor, target: &Tensor) -> TensorResult<Tensor> {
    if pred.shape.len() != 2 {
        return Err(niao_tensor::TensorError::Shape(
            "cross_entropy expects 2D logits".into(),
        ));
    }
    let rows = pred.shape[0];
    let cols = pred.shape[1];
    let logits = pred.to_cpu()?;
    let labels = target.to_cpu()?;
    let mut loss = 0.0f32;
    for r in 0..rows {
        let label = labels[r] as usize;
        if label >= cols {
            continue;
        }
        let row = &logits[r * cols..(r + 1) * cols];
        let max = row.iter().cloned().fold(f32::NEG_INFINITY, f32::max);
        let mut sum_exp = 0.0f32;
        for &v in row {
            sum_exp += (v - max).exp();
        }
        let log_sum = max + sum_exp.ln();
        loss -= logits[r * cols + label] - log_sum;
    }
    loss /= rows as f32;
    Tensor::from_cpu_data(&[1], vec![loss], pred.device)
}

fn bce(pred: &Tensor, target: &Tensor) -> TensorResult<Tensor> {
    let p = pred.to_cpu()?;
    let t = target.to_cpu()?;
    let eps = 1e-7;
    let mut loss = 0.0f32;
    for i in 0..p.len() {
        let pi = p[i].clamp(eps, 1.0 - eps);
        loss -= t[i] * pi.ln() + (1.0 - t[i]) * (1.0 - pi).ln();
    }
    loss /= p.len() as f32;
    Tensor::from_cpu_data(&[1], vec![loss], pred.device)
}

pub fn mse_grad(pred: &Tensor, target: &Tensor) -> TensorResult<Vec<f32>> {
    let p = pred.to_cpu()?;
    let t = target.to_cpu()?;
    let n = p.len() as f32;
    Ok(p.iter()
        .zip(t.iter())
        .map(|(&a, &b)| 2.0 * (a - b) / n)
        .collect())
}

pub fn loss_grad(kind: LossKind, pred: &Tensor, target: &Tensor) -> TensorResult<Vec<f32>> {
    match kind {
        LossKind::Mse => mse_grad(pred, target),
        LossKind::CrossEntropy => cross_entropy_grad(pred, target),
        LossKind::BinaryCrossEntropy => bce_grad(pred, target),
    }
}

pub fn cross_entropy_grad(pred: &Tensor, target: &Tensor) -> TensorResult<Vec<f32>> {
    if pred.shape.len() != 2 {
        return Err(niao_tensor::TensorError::Shape(
            "cross_entropy_grad expects 2D logits".into(),
        ));
    }
    let rows = pred.shape[0];
    let cols = pred.shape[1];
    let logits = pred.to_cpu()?;
    let labels = target.to_cpu()?;
    let mut softmax = vec![0.0f32; logits.len()];
    cpu::softmax_2d(&logits, rows, cols, &mut softmax);
    let mut grad = softmax;
    for r in 0..rows {
        let label = labels[r] as usize;
        if label < cols {
            grad[r * cols + label] -= 1.0;
        }
    }
    let scale = 1.0 / rows as f32;
    for v in grad.iter_mut() {
        *v *= scale;
    }
    Ok(grad)
}

pub fn bce_grad(pred: &Tensor, target: &Tensor) -> TensorResult<Vec<f32>> {
    let p = pred.to_cpu()?;
    let t = target.to_cpu()?;
    let n = p.len() as f32;
    let eps = 1e-7;
    Ok(p.iter()
        .zip(t.iter())
        .map(|(&pi, &ti)| {
            let p = pi.clamp(eps, 1.0 - eps);
            (-ti / p + (1.0 - ti) / (1.0 - p)) / n
        })
        .collect())
}
