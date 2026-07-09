//! Vectorized math ufuncs.

use super::algos::*;
use super::column::Column;
use crate::Value;

pub fn array_add(a: &Value, b: &Value) -> Result<Value, String> {
    match (a, b) {
        (Value::IntArray(x), Value::IntArray(y)) => {
            if x.len() != y.len() {
                return Err("array length mismatch".into());
            }
            Ok(Value::IntArray(add_i64(x, y)))
        }
        (Value::FloatArray(x), Value::FloatArray(y)) => {
            if x.len() != y.len() {
                return Err("array length mismatch".into());
            }
            Ok(Value::FloatArray(add_f64(x, y)))
        }
        (Value::IntArray(x), Value::Int(k)) => Ok(Value::IntArray(scalar_mul_i64(x, *k))),
        (Value::IntArray(x), Value::Float(k)) => {
            let xf: Vec<f64> = x.iter().map(|&n| n as f64).collect();
            Ok(Value::FloatArray(scalar_mul_f64(&xf, *k)))
        }
        (Value::FloatArray(x), Value::Float(k)) => Ok(Value::FloatArray(scalar_mul_f64(x, *k))),
        (Value::FloatArray(x), Value::Int(k)) => Ok(Value::FloatArray(scalar_mul_f64(x, *k as f64))),
        _ => Err("add: unsupported types".into()),
    }
}

pub fn array_sub(a: &Value, b: &Value) -> Result<Value, String> {
    match (a, b) {
        (Value::IntArray(x), Value::IntArray(y)) => Ok(Value::IntArray(sub_i64(x, y))),
        (Value::FloatArray(x), Value::FloatArray(y)) => Ok(Value::FloatArray(sub_f64(x, y))),
        _ => Err("sub: unsupported types".into()),
    }
}

pub fn array_mul(a: &Value, b: &Value) -> Result<Value, String> {
    match (a, b) {
        (Value::IntArray(x), Value::IntArray(y)) => Ok(Value::IntArray(mul_i64(x, y))),
        (Value::FloatArray(x), Value::FloatArray(y)) => Ok(Value::FloatArray(mul_f64(x, y))),
        (Value::IntArray(x), Value::Int(k)) => Ok(Value::IntArray(scalar_mul_i64(x, *k))),
        (Value::Int(k), Value::IntArray(x)) => Ok(Value::IntArray(scalar_mul_i64(x, *k))),
        (Value::FloatArray(x), Value::Float(k)) => Ok(Value::FloatArray(scalar_mul_f64(x, *k))),
        (Value::Float(k), Value::FloatArray(x)) => Ok(Value::FloatArray(scalar_mul_f64(x, *k))),
        _ => Err("mul: unsupported types".into()),
    }
}

pub fn array_div(a: &Value, b: &Value) -> Result<Value, String> {
    match (a, b) {
        (Value::FloatArray(x), Value::FloatArray(y)) => Ok(Value::FloatArray(div_f64(x, y))),
        (Value::IntArray(x), Value::IntArray(y)) => {
            let xf: Vec<f64> = x.iter().map(|&n| n as f64).collect();
            let yf: Vec<f64> = y.iter().map(|&n| n as f64).collect();
            Ok(Value::FloatArray(div_f64(&xf, &yf)))
        }
        _ => Err("div: unsupported types".into()),
    }
}

pub fn array_abs(v: &Value) -> Result<Value, String> {
    match v {
        Value::FloatArray(x) => Ok(Value::FloatArray(abs_f64(x))),
        Value::IntArray(x) => Ok(Value::IntArray(x.iter().map(|&n| n.abs()).collect())),
        _ => Err("abs: requires array".into()),
    }
}

pub fn array_sqrt(v: &Value) -> Result<Value, String> {
    match v {
        Value::FloatArray(x) => Ok(Value::FloatArray(sqrt_f64(x))),
        Value::IntArray(x) => {
            let f: Vec<f64> = x.iter().map(|&n| (n as f64).sqrt()).collect();
            Ok(Value::FloatArray(f))
        }
        _ => Err("sqrt: requires array".into()),
    }
}

pub fn array_exp(v: &Value) -> Result<Value, String> {
    match v {
        Value::FloatArray(x) => Ok(Value::FloatArray(exp_f64(x))),
        Value::IntArray(x) => {
            let f: Vec<f64> = x.iter().map(|&n| (n as f64).exp()).collect();
            Ok(Value::FloatArray(f))
        }
        _ => Err("exp: requires array".into()),
    }
}

pub fn array_log(v: &Value) -> Result<Value, String> {
    match v {
        Value::FloatArray(x) => Ok(Value::FloatArray(log_f64(x))),
        _ => Err("log: requires float array".into()),
    }
}

pub fn array_sin(v: &Value) -> Result<Value, String> {
    match v {
        Value::FloatArray(x) => Ok(Value::FloatArray(sin_f64(x))),
        _ => Err("sin: requires float array".into()),
    }
}

pub fn array_cos(v: &Value) -> Result<Value, String> {
    match v {
        Value::FloatArray(x) => Ok(Value::FloatArray(cos_f64(x))),
        _ => Err("cos: requires float array".into()),
    }
}

pub fn make_zeros(n: usize, as_float: bool) -> Value {
    if as_float {
        Value::FloatArray(vec![0.0; n])
    } else {
        Value::IntArray(vec![0; n])
    }
}

pub fn make_ones(n: usize, as_float: bool) -> Value {
    if as_float {
        Value::FloatArray(vec![1.0; n])
    } else {
        Value::IntArray(vec![1; n])
    }
}

pub fn arange(start: i64, stop: i64, step: i64) -> Result<Value, String> {
    if step == 0 {
        return Err("step cannot be zero".into());
    }
    let mut v = Vec::new();
    if step > 0 {
        let mut i = start;
        while i < stop {
            v.push(i);
            i += step;
        }
    } else {
        let mut i = start;
        while i > stop {
            v.push(i);
            i += step;
        }
    }
    Ok(Value::IntArray(v))
}

pub fn linspace(start: f64, stop: f64, n: usize) -> Value {
    if n == 0 {
        return Value::FloatArray(Vec::new());
    }
    if n == 1 {
        return Value::FloatArray(vec![start]);
    }
    let step = (stop - start) / (n - 1) as f64;
    let v: Vec<f64> = (0..n).map(|i| start + step * i as f64).collect();
    Value::FloatArray(v)
}

pub fn value_from_column(col: &Column) -> Value {
    col.to_value_array()
}
