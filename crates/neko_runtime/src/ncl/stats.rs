//! Statistical reductions and describe.

use super::algos::*;
use super::column::Column;
use super::series::Series;
use crate::Value;
use std::collections::HashMap;

pub fn sum_value(v: &Value) -> Result<Value, String> {
    match v {
        Value::IntArray(x) => Ok(Value::Int(sum_i64(x))),
        Value::FloatArray(x) => Ok(Value::Float(sum_f64(x))),
        _ => Err("sum requires int or float array".into()),
    }
}

pub fn mean_value(v: &Value) -> Result<Value, String> {
    match v {
        Value::IntArray(x) => {
            if x.is_empty() {
                return Ok(Value::Float(f64::NAN));
            }
            Ok(Value::Float(x.iter().map(|&n| n as f64).sum::<f64>() / x.len() as f64))
        }
        Value::FloatArray(x) => Ok(Value::Float(mean_f64(x))),
        _ => Err("mean requires array".into()),
    }
}

pub fn min_value(v: &Value) -> Result<Value, String> {
    match v {
        Value::IntArray(x) => min_i64(x).map(Value::Int).ok_or_else(|| "empty array".into()),
        Value::FloatArray(x) => min_f64(x).map(Value::Float).ok_or_else(|| "empty array".into()),
        _ => Err("min requires array".into()),
    }
}

pub fn max_value(v: &Value) -> Result<Value, String> {
    match v {
        Value::IntArray(x) => max_i64(x).map(Value::Int).ok_or_else(|| "empty array".into()),
        Value::FloatArray(x) => max_f64(x).map(Value::Float).ok_or_else(|| "empty array".into()),
        _ => Err("max requires array".into()),
    }
}

pub fn std_value(v: &Value) -> Result<Value, String> {
    match v {
        Value::FloatArray(x) => Ok(Value::Float(std_f64(x))),
        Value::IntArray(x) => {
            let f: Vec<f64> = x.iter().map(|&n| n as f64).collect();
            Ok(Value::Float(std_f64(&f)))
        }
        _ => Err("std requires array".into()),
    }
}

pub fn var_value(v: &Value) -> Result<Value, String> {
    match v {
        Value::FloatArray(x) => Ok(Value::Float(variance_f64(x))),
        Value::IntArray(x) => {
            let f: Vec<f64> = x.iter().map(|&n| n as f64).collect();
            Ok(Value::Float(variance_f64(&f)))
        }
        _ => Err("var requires array".into()),
    }
}

pub fn median_value(v: &Value) -> Result<Value, String> {
    match v {
        Value::FloatArray(x) => Ok(Value::Float(median_f64(x.clone()))),
        Value::IntArray(x) => {
            let f: Vec<f64> = x.iter().map(|&n| n as f64).collect();
            Ok(Value::Float(median_f64(f)))
        }
        _ => Err("median requires array".into()),
    }
}

pub fn corr_arrays(a: &Value, b: &Value) -> Result<Value, String> {
    match (a, b) {
        (Value::FloatArray(x), Value::FloatArray(y)) => Ok(Value::Float(corr_f64(x, y))),
        (Value::IntArray(x), Value::IntArray(y)) => {
            let xf: Vec<f64> = x.iter().map(|&n| n as f64).collect();
            let yf: Vec<f64> = y.iter().map(|&n| n as f64).collect();
            Ok(Value::Float(corr_f64(&xf, &yf)))
        }
        _ => Err("corr requires matching arrays".into()),
    }
}

pub fn describe_series(series: &Series) -> HashMap<String, f64> {
    let mut map = HashMap::new();
    match &series.data {
        Column::Int(v) => {
            if v.is_empty() {
                return map;
            }
            let f: Vec<f64> = v.iter().map(|&n| n as f64).collect();
            map.insert("count".into(), v.len() as f64);
            map.insert("mean".into(), mean_f64(&f));
            map.insert("std".into(), std_f64(&f));
            map.insert("min".into(), *v.iter().min().unwrap() as f64);
            map.insert("max".into(), *v.iter().max().unwrap() as f64);
        }
        Column::Float(v) => {
            if v.is_empty() {
                return map;
            }
            map.insert("count".into(), v.len() as f64);
            map.insert("mean".into(), mean_f64(v));
            map.insert("std".into(), std_f64(v));
            map.insert("min".into(), min_f64(v).unwrap_or(f64::NAN));
            map.insert("max".into(), max_f64(v).unwrap_or(f64::NAN));
        }
        _ => {}
    }
    map
}
