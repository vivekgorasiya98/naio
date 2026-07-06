//! Rolling window operations.

use super::algos::{rolling_mean_f64, rolling_std_f64, rolling_sum_f64};
use super::column::Column;
use super::series::Series;

pub fn rolling_sum(series: &Series, window: usize) -> Result<Series, String> {
    let data = match &series.data {
        Column::Int(v) => {
            let f: Vec<f64> = v.iter().map(|&x| x as f64).collect();
            let r = rolling_sum_f64(&f, window);
            Column::Float(r)
        }
        Column::Float(v) => Column::Float(rolling_sum_f64(v, window)),
        _ => return Err("rolling sum requires numeric series".into()),
    };
    Ok(Series::new(format!("{}_rolling_sum", series.name), data))
}

pub fn rolling_mean(series: &Series, window: usize) -> Result<Series, String> {
    let data = match &series.data {
        Column::Int(v) => {
            let f: Vec<f64> = v.iter().map(|&x| x as f64).collect();
            Column::Float(rolling_mean_f64(&f, window))
        }
        Column::Float(v) => Column::Float(rolling_mean_f64(v, window)),
        _ => return Err("rolling mean requires numeric series".into()),
    };
    Ok(Series::new(format!("{}_rolling_mean", series.name), data))
}

pub fn rolling_std(series: &Series, window: usize) -> Result<Series, String> {
    let data = match &series.data {
        Column::Int(v) => {
            let f: Vec<f64> = v.iter().map(|&x| x as f64).collect();
            Column::Float(rolling_std_f64(&f, window))
        }
        Column::Float(v) => Column::Float(rolling_std_f64(v, window)),
        _ => return Err("rolling std requires numeric series".into()),
    };
    Ok(Series::new(format!("{}_rolling_std", series.name), data))
}

pub fn rolling_min(series: &Series, window: usize) -> Result<Series, String> {
    rolling_extrema(series, window, true)
}

pub fn rolling_max(series: &Series, window: usize) -> Result<Series, String> {
    rolling_extrema(series, window, false)
}

fn rolling_extrema(series: &Series, window: usize, min: bool) -> Result<Series, String> {
    if window == 0 {
        return Err("window must be > 0".into());
    }
    match &series.data {
        Column::Int(v) => {
            let mut out = Vec::with_capacity(v.len());
            for i in 0..v.len() {
                if i + 1 < window {
                    out.push(i64::MIN);
                } else {
                    let slice = &v[i + 1 - window..=i];
                    out.push(if min {
                        slice.iter().copied().min().unwrap()
                    } else {
                        slice.iter().copied().max().unwrap()
                    });
                }
            }
            Ok(Series::new(
                format!("{}_rolling_{}", series.name, if min { "min" } else { "max" }),
                Column::Int(out),
            ))
        }
        Column::Float(v) => {
            let mut out = Vec::with_capacity(v.len());
            for i in 0..v.len() {
                if i + 1 < window {
                    out.push(f64::NAN);
                } else {
                    let slice = &v[i + 1 - window..=i];
                    out.push(if min {
                        slice.iter().copied().min_by(|a, b| a.partial_cmp(b).unwrap()).unwrap()
                    } else {
                        slice.iter().copied().max_by(|a, b| a.partial_cmp(b).unwrap()).unwrap()
                    });
                }
            }
            Ok(Series::new(
                format!("{}_rolling_{}", series.name, if min { "min" } else { "max" }),
                Column::Float(out),
            ))
        }
        _ => Err("rolling min/max requires numeric series".into()),
    }
}
