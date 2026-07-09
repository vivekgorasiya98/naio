//! Parallel chunked operations via rayon.

use super::algos::{add_f64, add_i64, sum_f64, sum_i64};
use crate::Value;

const PARALLEL_THRESHOLD: usize = 65_536;

pub fn parallel_sum(v: &Value) -> Result<Value, String> {
    match v {
        Value::IntArray(x) if x.len() >= PARALLEL_THRESHOLD => {
            use rayon::prelude::*;
            let s: i64 = x.par_chunks(4096).map(sum_i64).sum();
            Ok(Value::Int(s))
        }
        Value::IntArray(x) => Ok(Value::Int(sum_i64(x))),
        Value::FloatArray(x) if x.len() >= PARALLEL_THRESHOLD => {
            use rayon::prelude::*;
            let s: f64 = x.par_chunks(4096).map(sum_f64).sum();
            Ok(Value::Float(s))
        }
        Value::FloatArray(x) => Ok(Value::Float(sum_f64(x))),
        _ => Err("parallel_sum requires array".into()),
    }
}

pub fn parallel_add(a: &Value, b: &Value) -> Result<Value, String> {
    match (a, b) {
        (Value::IntArray(x), Value::IntArray(y)) if x.len() >= PARALLEL_THRESHOLD => {
            use rayon::prelude::*;
            let out: Vec<i64> = x
                .par_iter()
                .zip(y.par_iter())
                .map(|(&a, &b)| a.wrapping_add(b))
                .collect();
            Ok(Value::IntArray(out))
        }
        (Value::IntArray(x), Value::IntArray(y)) => Ok(Value::IntArray(add_i64(x, y))),
        (Value::FloatArray(x), Value::FloatArray(y)) => Ok(Value::FloatArray(add_f64(x, y))),
        _ => Err("parallel_add requires matching arrays".into()),
    }
}
