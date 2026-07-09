//! Missing-value (NA) operations.

use super::column::Column;
use super::series::Series;

pub fn isna(series: &Series) -> Series {
    let mut bits = Vec::new();
    let len = series.len();
    match &series.data {
        Column::Int(v) => {
            for &x in v {
                bits.push(if x == i64::MIN { 1 } else { 0 });
            }
        }
        Column::Float(v) => {
            for &x in v {
                bits.push(if x.is_nan() { 1 } else { 0 });
            }
        }
        Column::Any(v) => {
            for slot in v {
                bits.push(if matches!(&*slot.borrow(), crate::Value::Nil) {
                    1
                } else {
                    0
                });
            }
        }
        _ => bits.resize(len, 0),
    }
    if let Some(ref valid) = series.validity {
        for i in 0..len {
            if !valid.is_valid(i) {
                bits[i] = 1;
            }
        }
    }
    Series::new(
        format!("{}_isna", series.name),
        Column::Bool(bits),
    )
}

pub fn fillna_int(series: &Series, fill: i64) -> Series {
    let data = match &series.data {
        Column::Int(v) => Column::Int(v.iter().map(|&x| if x == i64::MIN { fill } else { x }).collect()),
        Column::Float(v) => Column::Float(
            v.iter()
                .map(|&x| if x.is_nan() { fill as f64 } else { x })
                .collect(),
        ),
        other => other.clone(),
    };
    Series {
        name: series.name.clone(),
        data,
        validity: None,
        index: series.index.clone(),
    }
}

pub fn dropna_indices(series: &Series) -> Vec<usize> {
    let mut out = Vec::new();
    for i in 0..series.len() {
        let null = match &series.data {
            Column::Int(v) => v[i] == i64::MIN,
            Column::Float(v) => v[i].is_nan(),
            Column::Any(v) => matches!(&*v[i].borrow(), crate::Value::Nil),
            _ => false,
        };
        let invalid = series
            .validity
            .as_ref()
            .map(|v| !v.is_valid(i))
            .unwrap_or(false);
        if !null && !invalid {
            out.push(i);
        }
    }
    out
}

pub fn interpolate_linear_f64(v: &[f64]) -> Vec<f64> {
    let mut out = v.to_vec();
    let n = out.len();
    let mut i = 0;
    while i < n {
        if out[i].is_nan() {
            let start = if i > 0 { i - 1 } else { usize::MAX };
            let mut j = i + 1;
            while j < n && out[j].is_nan() {
                j += 1;
            }
            if start != usize::MAX && j < n {
                let left = out[start];
                let right = out[j];
                let gap = (j - start) as f64;
                for k in i..j {
                    let t = (k - start) as f64 / gap;
                    out[k] = left + t * (right - left);
                }
            }
            i = j;
        } else {
            i += 1;
        }
    }
    out
}
