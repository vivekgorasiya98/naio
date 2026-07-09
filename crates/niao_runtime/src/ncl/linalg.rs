//! Basic linear algebra on float ndarrays.

use super::algos::{dot_f64, matmul_f64};
use super::ndarray::NDArray;
use crate::Value;

pub fn dot_values(a: &Value, b: &Value) -> Result<Value, String> {
    match (a, b) {
        (Value::FloatArray(x), Value::FloatArray(y)) => {
            if x.len() != y.len() {
                return Err("dot: length mismatch".into());
            }
            Ok(Value::Float(dot_f64(x, y)))
        }
        (Value::IntArray(x), Value::IntArray(y)) => {
            if x.len() != y.len() {
                return Err("dot: length mismatch".into());
            }
            let xf: Vec<f64> = x.iter().map(|&n| n as f64).collect();
            let yf: Vec<f64> = y.iter().map(|&n| n as f64).collect();
            Ok(Value::Float(dot_f64(&xf, &yf)))
        }
        _ => Err("dot requires arrays".into()),
    }
}

pub fn matmul_ndarray(a: &NDArray, b: &NDArray) -> Result<NDArray, String> {
    if a.shape.len() != 2 || b.shape.len() != 2 {
        return Err("matmul requires 2D ndarrays".into());
    }
    let (m, n) = (a.shape[0], a.shape[1]);
    let (n2, k) = (b.shape[0], b.shape[1]);
    if n != n2 {
        return Err("matmul inner dimension mismatch".into());
    }
    let af = a.data_float.as_ref().ok_or_else(|| "matmul requires float data".to_string())?;
    let bf = b.data_float.as_ref().ok_or_else(|| "matmul requires float data".to_string())?;
    let out = matmul_f64(af, bf, m, n, k);
    NDArray::from_float(vec![m, k], out)
}

pub fn inv_2x2(data: &[f64]) -> Result<Vec<f64>, String> {
    if data.len() != 4 {
        return Err("inv_2x2 requires 4 elements".into());
    }
    let det = data[0] * data[3] - data[1] * data[2];
    if det == 0.0 {
        return Err("singular matrix".into());
    }
    Ok(vec![
        data[3] / det,
        -data[1] / det,
        -data[2] / det,
        data[0] / det,
    ])
}
