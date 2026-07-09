//! NCL — Niao Column Library (pandas + numpy, native speed).
#![allow(dead_code)]

mod algos;
mod bridge;
pub mod column;
mod common;
mod dtypes;
mod frame;
mod groupby;
pub(crate) mod handles;
mod io;
mod join;
mod linalg;
mod math;
mod ndarray;
mod null;
mod parallel;
mod reshape;
mod rolling;
mod series;
mod stats;

use crate::{NativeFn, NiaoResult, RuntimeError, Value, ValueRef};
use column::Column;
use common::*;
use frame::DataFrame;
use handles::{alloc_handle, with_handle, with_handle_mut, NclHandle};
use ndarray::NDArray;
use niao_ast::Span;
use niao_errors::codes;
use series::Series;
use std::collections::HashMap;
use std::rc::Rc;

pub const MODULE_NAME: &str = "ncl";
pub const MODULE_PATHS: &[&str] = &["ncl", "std/ncl"];

// ---------------------------------------------------------------------------
// Array creation
// ---------------------------------------------------------------------------

fn ncl_zeros(args: &[ValueRef], span: Span) -> NiaoResult<ValueRef> {
    arity_range(args, 1, 2, "ncl_zeros", span)?;
    let n = int_arg(args, 0, "ncl_zeros", span)? as usize;
    let as_float = args.len() == 2 && bool_arg(args, 1, "ncl_zeros", span)?;
    Ok(ok_value(math::make_zeros(n, as_float)))
}

fn ncl_ones(args: &[ValueRef], span: Span) -> NiaoResult<ValueRef> {
    arity_range(args, 1, 2, "ncl_ones", span)?;
    let n = int_arg(args, 0, "ncl_ones", span)? as usize;
    let as_float = args.len() == 2 && bool_arg(args, 1, "ncl_ones", span)?;
    Ok(ok_value(math::make_ones(n, as_float)))
}

fn ncl_arange(args: &[ValueRef], span: Span) -> NiaoResult<ValueRef> {
    arity_range(args, 2, 3, "ncl_arange", span)?;
    let start = int_arg(args, 0, "ncl_arange", span)?;
    let stop = int_arg(args, 1, "ncl_arange", span)?;
    let step = if args.len() == 3 {
        int_arg(args, 2, "ncl_arange", span)?
    } else {
        1
    };
    math::arange(start, stop, step)
        .map(ok_value)
        .map_err(|e| RuntimeError::at(span, codes::E1961_NCL_ERROR, e))
}

fn ncl_linspace(args: &[ValueRef], span: Span) -> NiaoResult<ValueRef> {
    arity(args, 3, "ncl_linspace", span)?;
    let start = float_arg(args, 0, "ncl_linspace", span)?;
    let stop = float_arg(args, 1, "ncl_linspace", span)?;
    let n = int_arg(args, 2, "ncl_linspace", span)? as usize;
    Ok(ok_value(math::linspace(start, stop, n)))
}

fn ncl_array(args: &[ValueRef], span: Span) -> NiaoResult<ValueRef> {
    arity(args, 1, "ncl_array", span)?;
    let val = Column::coerce_packed_array(&*args[0].borrow()).ok_or_else(|| {
        RuntimeError::at(
            span,
            codes::E1964_NCL_TYPE,
            format!(
                "ncl_array() expects homogeneous numeric array, got {}",
                args[0].borrow().type_name()
            ),
        )
    })?;
    Ok(ok_value(val))
}

fn ncl_slice(args: &[ValueRef], span: Span) -> NiaoResult<ValueRef> {
    arity(args, 3, "ncl_slice", span)?;
    let v = array_arg(args, 0, "ncl_slice", span)?;
    let start = int_arg(args, 1, "ncl_slice", span)? as usize;
    let end = int_arg(args, 2, "ncl_slice", span)? as usize;
    match v {
        Value::IntArray(x) => Ok(ok_value(Value::IntArray(x[start..end.min(x.len())].to_vec()))),
        Value::FloatArray(x) => Ok(ok_value(Value::FloatArray(x[start..end.min(x.len())].to_vec()))),
        Value::BoolArray(x) => Ok(ok_value(Value::BoolArray(x[start..end.min(x.len())].to_vec()))),
        _ => Err(RuntimeError::at(span, codes::E1964_NCL_TYPE, "ncl_slice() requires packed array")),
    }
}

// ---------------------------------------------------------------------------
// Vectorized math
// ---------------------------------------------------------------------------

fn ncl_add(args: &[ValueRef], span: Span) -> NiaoResult<ValueRef> {
    arity(args, 2, "ncl_add", span)?;
    math::array_add(&*args[0].borrow(), &*args[1].borrow())
        .map(ok_value)
        .map_err(|e| RuntimeError::at(span, codes::E1961_NCL_ERROR, e))
}

fn ncl_sub(args: &[ValueRef], span: Span) -> NiaoResult<ValueRef> {
    arity(args, 2, "ncl_sub", span)?;
    math::array_sub(&*args[0].borrow(), &*args[1].borrow())
        .map(ok_value)
        .map_err(|e| RuntimeError::at(span, codes::E1961_NCL_ERROR, e))
}

fn ncl_mul(args: &[ValueRef], span: Span) -> NiaoResult<ValueRef> {
    arity(args, 2, "ncl_mul", span)?;
    math::array_mul(&*args[0].borrow(), &*args[1].borrow())
        .map(ok_value)
        .map_err(|e| RuntimeError::at(span, codes::E1961_NCL_ERROR, e))
}

fn ncl_div(args: &[ValueRef], span: Span) -> NiaoResult<ValueRef> {
    arity(args, 2, "ncl_div", span)?;
    math::array_div(&*args[0].borrow(), &*args[1].borrow())
        .map(ok_value)
        .map_err(|e| RuntimeError::at(span, codes::E1961_NCL_ERROR, e))
}

fn ncl_abs(args: &[ValueRef], span: Span) -> NiaoResult<ValueRef> {
    arity(args, 1, "ncl_abs", span)?;
    math::array_abs(&*args[0].borrow())
        .map(ok_value)
        .map_err(|e| RuntimeError::at(span, codes::E1961_NCL_ERROR, e))
}

fn ncl_sqrt(args: &[ValueRef], span: Span) -> NiaoResult<ValueRef> {
    arity(args, 1, "ncl_sqrt", span)?;
    math::array_sqrt(&*args[0].borrow())
        .map(ok_value)
        .map_err(|e| RuntimeError::at(span, codes::E1961_NCL_ERROR, e))
}

fn ncl_exp(args: &[ValueRef], span: Span) -> NiaoResult<ValueRef> {
    arity(args, 1, "ncl_exp", span)?;
    math::array_exp(&*args[0].borrow())
        .map(ok_value)
        .map_err(|e| RuntimeError::at(span, codes::E1961_NCL_ERROR, e))
}

fn ncl_log(args: &[ValueRef], span: Span) -> NiaoResult<ValueRef> {
    arity(args, 1, "ncl_log", span)?;
    math::array_log(&*args[0].borrow())
        .map(ok_value)
        .map_err(|e| RuntimeError::at(span, codes::E1961_NCL_ERROR, e))
}

fn ncl_sin(args: &[ValueRef], span: Span) -> NiaoResult<ValueRef> {
    arity(args, 1, "ncl_sin", span)?;
    math::array_sin(&*args[0].borrow())
        .map(ok_value)
        .map_err(|e| RuntimeError::at(span, codes::E1961_NCL_ERROR, e))
}

fn ncl_cos(args: &[ValueRef], span: Span) -> NiaoResult<ValueRef> {
    arity(args, 1, "ncl_cos", span)?;
    math::array_cos(&*args[0].borrow())
        .map(ok_value)
        .map_err(|e| RuntimeError::at(span, codes::E1961_NCL_ERROR, e))
}

// ---------------------------------------------------------------------------
// Stats
// ---------------------------------------------------------------------------

fn ncl_sum(args: &[ValueRef], span: Span) -> NiaoResult<ValueRef> {
    arity(args, 1, "ncl_sum", span)?;
    stats::sum_value(&*args[0].borrow())
        .map(ok_value)
        .map_err(|e| RuntimeError::at(span, codes::E1961_NCL_ERROR, e))
}

fn ncl_mean(args: &[ValueRef], span: Span) -> NiaoResult<ValueRef> {
    arity(args, 1, "ncl_mean", span)?;
    stats::mean_value(&*args[0].borrow())
        .map(ok_value)
        .map_err(|e| RuntimeError::at(span, codes::E1961_NCL_ERROR, e))
}

fn ncl_min(args: &[ValueRef], span: Span) -> NiaoResult<ValueRef> {
    arity(args, 1, "ncl_min", span)?;
    stats::min_value(&*args[0].borrow())
        .map(ok_value)
        .map_err(|e| RuntimeError::at(span, codes::E1961_NCL_ERROR, e))
}

fn ncl_max(args: &[ValueRef], span: Span) -> NiaoResult<ValueRef> {
    arity(args, 1, "ncl_max", span)?;
    stats::max_value(&*args[0].borrow())
        .map(ok_value)
        .map_err(|e| RuntimeError::at(span, codes::E1961_NCL_ERROR, e))
}

fn ncl_std(args: &[ValueRef], span: Span) -> NiaoResult<ValueRef> {
    arity(args, 1, "ncl_std", span)?;
    stats::std_value(&*args[0].borrow())
        .map(ok_value)
        .map_err(|e| RuntimeError::at(span, codes::E1961_NCL_ERROR, e))
}

fn ncl_var(args: &[ValueRef], span: Span) -> NiaoResult<ValueRef> {
    arity(args, 1, "ncl_var", span)?;
    stats::var_value(&*args[0].borrow())
        .map(ok_value)
        .map_err(|e| RuntimeError::at(span, codes::E1961_NCL_ERROR, e))
}

fn ncl_median(args: &[ValueRef], span: Span) -> NiaoResult<ValueRef> {
    arity(args, 1, "ncl_median", span)?;
    stats::median_value(&*args[0].borrow())
        .map(ok_value)
        .map_err(|e| RuntimeError::at(span, codes::E1961_NCL_ERROR, e))
}

fn ncl_corr(args: &[ValueRef], span: Span) -> NiaoResult<ValueRef> {
    arity(args, 2, "ncl_corr", span)?;
    stats::corr_arrays(&*args[0].borrow(), &*args[1].borrow())
        .map(ok_value)
        .map_err(|e| RuntimeError::at(span, codes::E1961_NCL_ERROR, e))
}

fn ncl_dot(args: &[ValueRef], span: Span) -> NiaoResult<ValueRef> {
    arity(args, 2, "ncl_dot", span)?;
    linalg::dot_values(&*args[0].borrow(), &*args[1].borrow())
        .map(ok_value)
        .map_err(|e| RuntimeError::at(span, codes::E1961_NCL_ERROR, e))
}

fn ncl_parallel_sum(args: &[ValueRef], span: Span) -> NiaoResult<ValueRef> {
    arity(args, 1, "ncl_parallel_sum", span)?;
    parallel::parallel_sum(&*args[0].borrow())
        .map(ok_value)
        .map_err(|e| RuntimeError::at(span, codes::E1961_NCL_ERROR, e))
}

// ---------------------------------------------------------------------------
// Series / DataFrame
// ---------------------------------------------------------------------------

fn ncl_series(args: &[ValueRef], span: Span) -> NiaoResult<ValueRef> {
    arity_range(args, 1, 2, "ncl_series", span)?;
    let name = if args.len() == 2 {
        string_arg(args, 1, "ncl_series", span)?
    } else {
        String::new()
    };
    let series = Series::from_value(name, &*args[0].borrow()).ok_or_else(|| {
        RuntimeError::at(span, codes::E1964_NCL_TYPE, "ncl_series() expects array data")
    })?;
    Ok(ok_handle(alloc_handle(NclHandle::Series(series))))
}

fn ncl_dataframe(args: &[ValueRef], span: Span) -> NiaoResult<ValueRef> {
    arity(args, 1, "ncl_dataframe", span)?;
    match &*args[0].borrow() {
        Value::Object(map) => {
            let mut cols = Vec::new();
            for (name, val) in map {
                let col = Column::from_value_array(&*val.borrow()).ok_or_else(|| {
                    RuntimeError::at(
                        span,
                        codes::E1964_NCL_TYPE,
                        format!("column '{name}' must be array"),
                    )
                })?;
                cols.push(Series::new(name.clone(), col));
            }
            let df = DataFrame::new(cols).map_err(|e| RuntimeError::at(span, codes::E1961_NCL_ERROR, e))?;
            Ok(ok_handle(alloc_handle(NclHandle::DataFrame(df))))
        }
        other => Err(RuntimeError::at(
            span,
            codes::E1964_NCL_TYPE,
            format!("ncl_dataframe() expects object, got {}", other.type_name()),
        )),
    }
}

fn ncl_df_get(args: &[ValueRef], span: Span) -> NiaoResult<ValueRef> {
    arity(args, 2, "ncl_df_get", span)?;
    let id = ncl_handle_arg(args, 0, "ncl_df_get", span)?;
    let col = string_arg(args, 1, "ncl_df_get", span)?;
    with_handle(id, "ncl_df_get", span, |h| {
        match h {
            NclHandle::DataFrame(df) => {
                let s = df
                    .get_column(&col)
                    .ok_or_else(|| format!("column '{col}' not found"))?
                    .clone();
                Ok(alloc_handle(NclHandle::Series(s)))
            }
            _ => Err("expected DataFrame handle".into()),
        }
    })
    .map(ok_handle)
}

fn ncl_df_set(args: &[ValueRef], span: Span) -> NiaoResult<ValueRef> {
    arity(args, 3, "ncl_df_set", span)?;
    let id = ncl_handle_arg(args, 0, "ncl_df_set", span)?;
    let col = string_arg(args, 1, "ncl_df_set", span)?;
    let series = Series::from_value(col.clone(), &*args[2].borrow()).ok_or_else(|| {
        RuntimeError::at(span, codes::E1964_NCL_TYPE, "ncl_df_set() expects array column")
    })?;
    with_handle_mut(id, "ncl_df_set", span, |h| {
        match h {
            NclHandle::DataFrame(df) => {
                df.set_column(series).map(|_| 0i64)
            }
            _ => Err("expected DataFrame handle".into()),
        }
    })
    .map(ok_int)
}

fn ncl_df_columns(args: &[ValueRef], span: Span) -> NiaoResult<ValueRef> {
    arity(args, 1, "ncl_df_columns", span)?;
    let id = ncl_handle_arg(args, 0, "ncl_df_columns", span)?;
    with_handle(id, "ncl_df_columns", span, |h| match h {
        NclHandle::DataFrame(df) => {
            let names = df.column_names();
            let items: Vec<ValueRef> = names.into_iter().map(|s| Value::String(s).ref_cell()).collect();
            Ok(Value::Array(items))
        }
        _ => Err("expected DataFrame handle".into()),
    })
    .map(ok_value)
}

fn ncl_df_shape(args: &[ValueRef], span: Span) -> NiaoResult<ValueRef> {
    arity(args, 1, "ncl_df_shape", span)?;
    let id = ncl_handle_arg(args, 0, "ncl_df_shape", span)?;
    with_handle(id, "ncl_df_shape", span, |h| match h {
        NclHandle::DataFrame(df) => Ok((df.len() as i64, df.column_count() as i64)),
        NclHandle::Series(s) => Ok((s.len() as i64, 1)),
        _ => Err("expected DataFrame or Series handle".into()),
    })
    .map(|(rows, cols)| {
        ok_value(Value::Array(vec![Value::Int(rows).ref_cell(), Value::Int(cols).ref_cell()]))
    })
}

fn ncl_series_values(args: &[ValueRef], span: Span) -> NiaoResult<ValueRef> {
    arity(args, 1, "ncl_series_values", span)?;
    let id = ncl_handle_arg(args, 0, "ncl_series_values", span)?;
    with_handle(id, "ncl_series_values", span, |h| match h {
        NclHandle::Series(s) => Ok(s.to_value_array()),
        _ => Err("expected Series handle".into()),
    })
    .map(ok_value)
}

fn ncl_series_name(args: &[ValueRef], span: Span) -> NiaoResult<ValueRef> {
    arity(args, 1, "ncl_series_name", span)?;
    let id = ncl_handle_arg(args, 0, "ncl_series_name", span)?;
    with_handle(id, "ncl_series_name", span, |h| match h {
        NclHandle::Series(s) => Ok(s.name.clone()),
        _ => Err("expected Series handle".into()),
    })
    .map(|s| Value::String(s).ref_cell())
}

fn ncl_head(args: &[ValueRef], span: Span) -> NiaoResult<ValueRef> {
    arity_range(args, 1, 2, "ncl_head", span)?;
    let n = if args.len() == 2 {
        int_arg(args, 1, "ncl_head", span)? as usize
    } else {
        5
    };
    let id = ncl_handle_arg(args, 0, "ncl_head", span)?;
    with_handle(id, "ncl_head", span, |h| match h {
        NclHandle::DataFrame(df) => {
            let end = n.min(df.len());
            let indices: Vec<usize> = (0..end).collect();
            let sub = df.select_rows(&indices)?;
            Ok(alloc_handle(NclHandle::DataFrame(sub)))
        }
        NclHandle::Series(s) => {
            let sub = s.slice(0, n.min(s.len()));
            Ok(alloc_handle(NclHandle::Series(sub)))
        }
        _ => Err("expected DataFrame or Series".into()),
    })
    .map(ok_handle)
}

fn ncl_tail(args: &[ValueRef], span: Span) -> NiaoResult<ValueRef> {
    arity_range(args, 1, 2, "ncl_tail", span)?;
    let n = if args.len() == 2 {
        int_arg(args, 1, "ncl_tail", span)? as usize
    } else {
        5
    };
    let id = ncl_handle_arg(args, 0, "ncl_tail", span)?;
    with_handle(id, "ncl_tail", span, |h| match h {
        NclHandle::DataFrame(df) => {
            let start = df.len().saturating_sub(n);
            let indices: Vec<usize> = (start..df.len()).collect();
            let sub = df.select_rows(&indices)?;
            Ok(alloc_handle(NclHandle::DataFrame(sub)))
        }
        NclHandle::Series(s) => {
            let start = s.len().saturating_sub(n);
            let sub = s.slice(start, s.len());
            Ok(alloc_handle(NclHandle::Series(sub)))
        }
        _ => Err("expected DataFrame or Series".into()),
    })
    .map(ok_handle)
}

fn ncl_filter(args: &[ValueRef], span: Span) -> NiaoResult<ValueRef> {
    arity(args, 2, "ncl_filter", span)?;
    let id = ncl_handle_arg(args, 0, "ncl_filter", span)?;
    let mask_val = array_arg(args, 1, "ncl_filter", span)?;
    let mask = &mask_val;
    let indices: Vec<usize> = match mask {
        Value::BoolArray(b) => b.iter().enumerate().filter(|(_, &v)| v != 0).map(|(i, _)| i).collect(),
        Value::IntArray(b) => b.iter().enumerate().filter(|(_, &v)| v != 0).map(|(i, _)| i).collect(),
        _ => {
            return Err(RuntimeError::at(
                span,
                codes::E1964_NCL_TYPE,
                "ncl_filter() mask must be bool/int array",
            ));
        }
    };
    with_handle(id, "ncl_filter", span, |h| match h {
        NclHandle::DataFrame(df) => {
            let sub = df.select_rows(&indices)?;
            Ok(alloc_handle(NclHandle::DataFrame(sub)))
        }
        NclHandle::Series(s) => {
            let data = match &s.data {
                Column::Int(v) => Column::Int(indices.iter().map(|&i| v[i]).collect()),
                Column::Float(v) => Column::Float(indices.iter().map(|&i| v[i]).collect()),
                Column::Bool(v) => Column::Bool(indices.iter().map(|&i| v[i]).collect()),
                Column::String(sa) => {
                    let dense: Vec<String> = indices.iter().map(|&i| sa.get(i).unwrap_or_default()).collect();
                    Column::String(crate::StringArray::dense(dense))
                }
                Column::Any(v) => Column::Any(indices.iter().map(|&i| v[i].clone()).collect()),
            };
            Ok(alloc_handle(NclHandle::Series(Series::new(s.name.clone(), data))))
        }
        _ => Err("expected DataFrame or Series".into()),
    })
    .map(ok_handle)
}

fn ncl_sort_values(args: &[ValueRef], span: Span) -> NiaoResult<ValueRef> {
    arity_range(args, 2, 3, "ncl_sort_values", span)?;
    let id = ncl_handle_arg(args, 0, "ncl_sort_values", span)?;
    let col = string_arg(args, 1, "ncl_sort_values", span)?;
    let desc = args.len() == 3 && bool_arg(args, 2, "ncl_sort_values", span)?;
    with_handle(id, "ncl_sort_values", span, |h| {
        let NclHandle::DataFrame(df) = h else {
            return Err("expected DataFrame".into());
        };
        let key = df
            .get_column(&col)
            .ok_or_else(|| format!("column '{col}' not found"))?;
        let mut indices: Vec<usize> = (0..df.len()).collect();
        match &key.data {
            Column::Int(v) => {
                if desc {
                    indices.sort_by(|&a, &b| v[b].cmp(&v[a]));
                } else {
                    indices.sort_by(|&a, &b| v[a].cmp(&v[b]));
                }
            }
            Column::Float(v) => {
                if desc {
                    indices.sort_by(|&a, &b| v[b].partial_cmp(&v[a]).unwrap());
                } else {
                    indices.sort_by(|&a, &b| v[a].partial_cmp(&v[b]).unwrap());
                }
            }
            _ => return Err("sort key must be numeric".into()),
        }
        let sub = df.select_rows(&indices)?;
        Ok(alloc_handle(NclHandle::DataFrame(sub)))
    })
    .map(ok_handle)
}

// ---------------------------------------------------------------------------
// Groupby / join / reshape
// ---------------------------------------------------------------------------

fn ncl_groupby(args: &[ValueRef], span: Span) -> NiaoResult<ValueRef> {
    arity(args, 2, "ncl_groupby", span)?;
    let id = ncl_handle_arg(args, 0, "ncl_groupby", span)?;
    let key = string_arg(args, 1, "ncl_groupby", span)?;
    with_handle(id, "ncl_groupby", span, |h| match h {
        NclHandle::DataFrame(df) => {
            let g = groupby::GroupBy::new(df, &key)?;
            Ok(alloc_handle(NclHandle::GroupBy(g)))
        }
        _ => Err("expected DataFrame".into()),
    })
    .map(ok_handle)
}

fn ncl_agg(args: &[ValueRef], span: Span) -> NiaoResult<ValueRef> {
    arity(args, 2, "ncl_agg", span)?;
    let id = ncl_handle_arg(args, 0, "ncl_agg", span)?;
    let spec = match &*args[1].borrow() {
        Value::Object(map) => map.clone(),
        other => {
            return Err(RuntimeError::at(
                span,
                codes::E1964_NCL_TYPE,
                format!("ncl_agg() expects object, got {}", other.type_name()),
            ));
        }
    };
    with_handle(id, "ncl_agg", span, |h| {
        let NclHandle::GroupBy(g) = h else {
            return Err("expected GroupBy handle".into());
        };
        let mut cols = Vec::new();
        for (col_name, op_val) in &spec {
            let op = match &*op_val.borrow() {
                Value::String(s) => s.clone(),
                _ => return Err("agg op must be string".into()),
            };
            let s = match op.as_str() {
                "sum" => g.agg_sum(col_name)?,
                "mean" => g.agg_mean(col_name)?,
                "count" => g.agg_count(),
                _ => return Err(format!("unknown agg op '{op}'")),
            };
            cols.push(s);
        }
        let df = DataFrame::new(cols)?;
        Ok(alloc_handle(NclHandle::DataFrame(df)))
    })
    .map(ok_handle)
}

fn ncl_merge(args: &[ValueRef], span: Span) -> NiaoResult<ValueRef> {
    arity(args, 3, "ncl_merge", span)?;
    let left = ncl_handle_arg(args, 0, "ncl_merge", span)?;
    let right = ncl_handle_arg(args, 1, "ncl_merge", span)?;
    let on = string_arg(args, 2, "ncl_merge", span)?;
    let (l_df, r_df) = {
        let mut l = None;
        let mut r = None;
        with_handle(left, "ncl_merge", span, |h| {
            if let NclHandle::DataFrame(df) = h {
                l = Some(df.clone());
            }
            Ok(())
        })?;
        with_handle(right, "ncl_merge", span, |h| {
            if let NclHandle::DataFrame(df) = h {
                r = Some(df.clone());
            }
            Ok(())
        })?;
        (l.ok_or_else(|| RuntimeError::at(span, codes::E1962_NCL_INVALID_HANDLE, "left not DataFrame"))?,
         r.ok_or_else(|| RuntimeError::at(span, codes::E1962_NCL_INVALID_HANDLE, "right not DataFrame"))?)
    };
    let merged = join::merge_inner(&l_df, &r_df, &on)
        .map_err(|e| RuntimeError::at(span, codes::E1961_NCL_ERROR, e))?;
    Ok(ok_handle(alloc_handle(NclHandle::DataFrame(merged))))
}

fn ncl_concat(args: &[ValueRef], span: Span) -> NiaoResult<ValueRef> {
    arity(args, 2, "ncl_concat", span)?;
    let a_id = ncl_handle_arg(args, 0, "ncl_concat", span)?;
    let b_id = ncl_handle_arg(args, 1, "ncl_concat", span)?;
    let (a, b) = {
        let mut da = None;
        let mut db = None;
        with_handle(a_id, "ncl_concat", span, |h| {
            if let NclHandle::DataFrame(df) = h {
                da = Some(df.clone());
            }
            Ok(())
        })?;
        with_handle(b_id, "ncl_concat", span, |h| {
            if let NclHandle::DataFrame(df) = h {
                db = Some(df.clone());
            }
            Ok(())
        })?;
        (da.ok_or_else(|| RuntimeError::at(span, codes::E1962_NCL_INVALID_HANDLE, "a not DataFrame"))?,
         db.ok_or_else(|| RuntimeError::at(span, codes::E1962_NCL_INVALID_HANDLE, "b not DataFrame"))?)
    };
    let out = join::concat_vertical(&a, &b)
        .map_err(|e| RuntimeError::at(span, codes::E1961_NCL_ERROR, e))?;
    Ok(ok_handle(alloc_handle(NclHandle::DataFrame(out))))
}

fn ncl_pivot(args: &[ValueRef], span: Span) -> NiaoResult<ValueRef> {
    arity(args, 4, "ncl_pivot", span)?;
    let id = ncl_handle_arg(args, 0, "ncl_pivot", span)?;
    let index = string_arg(args, 1, "ncl_pivot", span)?;
    let columns = string_arg(args, 2, "ncl_pivot", span)?;
    let values = string_arg(args, 3, "ncl_pivot", span)?;
    with_handle(id, "ncl_pivot", span, |h| match h {
        NclHandle::DataFrame(df) => {
            let out = reshape::pivot(df, &index, &columns, &values)?;
            Ok(alloc_handle(NclHandle::DataFrame(out)))
        }
        _ => Err("expected DataFrame".into()),
    })
    .map(ok_handle)
}

fn ncl_melt(args: &[ValueRef], span: Span) -> NiaoResult<ValueRef> {
    arity(args, 2, "ncl_melt", span)?;
    let id = ncl_handle_arg(args, 0, "ncl_melt", span)?;
    let id_vars = match &*args[1].borrow() {
        Value::Array(items) => {
            let mut names = Vec::new();
            for item in items {
                match &*item.borrow() {
                    Value::String(s) => names.push(s.clone()),
                    _ => return Err(RuntimeError::at(span, codes::E1964_NCL_TYPE, "id_vars must be strings")),
                }
            }
            names
        }
        other => {
            return Err(RuntimeError::at(
                span,
                codes::E1964_NCL_TYPE,
                format!("ncl_melt() expects array of id column names, got {}", other.type_name()),
            ));
        }
    };
    with_handle(id, "ncl_melt", span, |h| match h {
        NclHandle::DataFrame(df) => {
            let out = reshape::melt(df, &id_vars)?;
            Ok(alloc_handle(NclHandle::DataFrame(out)))
        }
        _ => Err("expected DataFrame".into()),
    })
    .map(ok_handle)
}

// ---------------------------------------------------------------------------
// NA / rolling
// ---------------------------------------------------------------------------

fn ncl_isna(args: &[ValueRef], span: Span) -> NiaoResult<ValueRef> {
    arity(args, 1, "ncl_isna", span)?;
    let id = ncl_handle_arg(args, 0, "ncl_isna", span)?;
    with_handle(id, "ncl_isna", span, |h| match h {
        NclHandle::Series(s) => {
            let out = null::isna(s);
            Ok(alloc_handle(NclHandle::Series(out)))
        }
        _ => Err("expected Series".into()),
    })
    .map(ok_handle)
}

fn ncl_fillna(args: &[ValueRef], span: Span) -> NiaoResult<ValueRef> {
    arity(args, 2, "ncl_fillna", span)?;
    let id = ncl_handle_arg(args, 0, "ncl_fillna", span)?;
    let fill = int_arg(args, 1, "ncl_fillna", span)?;
    with_handle(id, "ncl_fillna", span, |h| match h {
        NclHandle::Series(s) => {
            let out = null::fillna_int(s, fill);
            Ok(alloc_handle(NclHandle::Series(out)))
        }
        NclHandle::DataFrame(df) => {
            let cols: Vec<Series> = df
                .columns
                .iter()
                .map(|c| null::fillna_int(c, fill))
                .collect();
            let out = DataFrame::new(cols)?;
            Ok(alloc_handle(NclHandle::DataFrame(out)))
        }
        _ => Err("expected Series or DataFrame".into()),
    })
    .map(ok_handle)
}

fn ncl_dropna(args: &[ValueRef], span: Span) -> NiaoResult<ValueRef> {
    arity(args, 1, "ncl_dropna", span)?;
    let id = ncl_handle_arg(args, 0, "ncl_dropna", span)?;
    with_handle(id, "ncl_dropna", span, |h| match h {
        NclHandle::Series(s) => {
            let idx = null::dropna_indices(s);
            let data = match &s.data {
                Column::Int(v) => Column::Int(idx.iter().map(|&i| v[i]).collect()),
                Column::Float(v) => Column::Float(idx.iter().map(|&i| v[i]).collect()),
                Column::Bool(v) => Column::Bool(idx.iter().map(|&i| v[i]).collect()),
                Column::String(sa) => {
                    let dense: Vec<String> = idx.iter().map(|&i| sa.get(i).unwrap_or_default()).collect();
                    Column::String(crate::StringArray::dense(dense))
                }
                Column::Any(v) => Column::Any(idx.iter().map(|&i| v[i].clone()).collect()),
            };
            Ok(alloc_handle(NclHandle::Series(Series::new(s.name.clone(), data))))
        }
        _ => Err("expected Series".into()),
    })
    .map(ok_handle)
}

fn ncl_rolling_mean(args: &[ValueRef], span: Span) -> NiaoResult<ValueRef> {
    arity(args, 2, "ncl_rolling_mean", span)?;
    let id = ncl_handle_arg(args, 0, "ncl_rolling_mean", span)?;
    let window = int_arg(args, 1, "ncl_rolling_mean", span)? as usize;
    with_handle(id, "ncl_rolling_mean", span, |h| match h {
        NclHandle::Series(s) => {
            let out = rolling::rolling_mean(s, window)?;
            Ok(alloc_handle(NclHandle::Series(out)))
        }
        _ => Err("expected Series".into()),
    })
    .map(ok_handle)
}

fn ncl_rolling_sum(args: &[ValueRef], span: Span) -> NiaoResult<ValueRef> {
    arity(args, 2, "ncl_rolling_sum", span)?;
    let id = ncl_handle_arg(args, 0, "ncl_rolling_sum", span)?;
    let window = int_arg(args, 1, "ncl_rolling_sum", span)? as usize;
    with_handle(id, "ncl_rolling_sum", span, |h| match h {
        NclHandle::Series(s) => {
            let out = rolling::rolling_sum(s, window)?;
            Ok(alloc_handle(NclHandle::Series(out)))
        }
        _ => Err("expected Series".into()),
    })
    .map(ok_handle)
}

fn ncl_rolling_std(args: &[ValueRef], span: Span) -> NiaoResult<ValueRef> {
    arity(args, 2, "ncl_rolling_std", span)?;
    let id = ncl_handle_arg(args, 0, "ncl_rolling_std", span)?;
    let window = int_arg(args, 1, "ncl_rolling_std", span)? as usize;
    with_handle(id, "ncl_rolling_std", span, |h| match h {
        NclHandle::Series(s) => {
            let out = rolling::rolling_std(s, window)?;
            Ok(alloc_handle(NclHandle::Series(out)))
        }
        _ => Err("expected Series".into()),
    })
    .map(ok_handle)
}

fn ncl_describe(args: &[ValueRef], span: Span) -> NiaoResult<ValueRef> {
    arity(args, 1, "ncl_describe", span)?;
    let id = ncl_handle_arg(args, 0, "ncl_describe", span)?;
    with_handle(id, "ncl_describe", span, |h| match h {
        NclHandle::Series(s) => {
            let stats = stats::describe_series(s);
            let mut map = HashMap::new();
            for (k, v) in stats {
                map.insert(k, Value::Float(v).ref_cell());
            }
            Ok(Value::Object(map))
        }
        _ => Err("expected Series".into()),
    })
    .map(|obj| obj.ref_cell())
}

// ---------------------------------------------------------------------------
// I/O
// ---------------------------------------------------------------------------

fn ncl_read_csv(args: &[ValueRef], span: Span) -> NiaoResult<ValueRef> {
    arity(args, 1, "ncl_read_csv", span)?;
    let path = string_arg(args, 0, "ncl_read_csv", span)?;
    let df = io::read_csv(&path).map_err(|e| RuntimeError::at(span, codes::E1961_NCL_ERROR, e))?;
    Ok(ok_handle(alloc_handle(NclHandle::DataFrame(df))))
}

fn ncl_to_csv(args: &[ValueRef], span: Span) -> NiaoResult<ValueRef> {
    arity_range(args, 1, 2, "ncl_to_csv", span)?;
    let id = ncl_handle_arg(args, 0, "ncl_to_csv", span)?;
    if args.len() == 2 {
        let path = string_arg(args, 1, "ncl_to_csv", span)?;
        with_handle(id, "ncl_to_csv", span, |h| match h {
            NclHandle::DataFrame(df) => io::write_csv(&path, df).map(|_| Value::Nil),
            _ => Err("expected DataFrame".into()),
        })
        .map(|v| v.ref_cell())
    } else {
        with_handle(id, "ncl_to_csv", span, |h| match h {
            NclHandle::DataFrame(df) => Ok(Value::String(io::to_csv(df))),
            _ => Err("expected DataFrame".into()),
        })
        .map(|v| v.ref_cell())
    }
}

fn ncl_from_sqlite(args: &[ValueRef], span: Span) -> NiaoResult<ValueRef> {
    arity_range(args, 2, 3, "ncl_from_sqlite", span)?;
    let conn = match &*args[0].borrow() {
        Value::Int(id) if *id > 0 => *id as u64,
        other => {
            return Err(RuntimeError::at(
                span,
                codes::E1962_NCL_INVALID_HANDLE,
                format!("ncl_from_sqlite() expects connection handle, got {}", other.type_name()),
            ));
        }
    };
    let sql = string_arg(args, 1, "ncl_from_sqlite", span)?;
    let params: Vec<ValueRef> = if args.len() == 3 {
        match &*args[2].borrow() {
            Value::Array(items) => items.clone(),
            other => {
                return Err(RuntimeError::at(
                    span,
                    codes::E1964_NCL_TYPE,
                    format!("params must be array, got {}", other.type_name()),
                ));
            }
        }
    } else {
        Vec::new()
    };
    let df = bridge::from_sqlite(conn, &sql, &params, span)
        .map_err(|e| RuntimeError::at(span, codes::E1961_NCL_ERROR, e))?;
    Ok(ok_handle(alloc_handle(NclHandle::DataFrame(df))))
}

fn ncl_to_datetime(args: &[ValueRef], span: Span) -> NiaoResult<ValueRef> {
    arity_range(args, 1, 2, "ncl_to_datetime", span)?;
    let id = ncl_handle_arg(args, 0, "ncl_to_datetime", span)?;
    let fmt = if args.len() == 2 {
        string_arg(args, 1, "ncl_to_datetime", span)?
    } else {
        "%Y-%m-%d".into()
    };
    with_handle(id, "ncl_to_datetime", span, |h| match h {
        NclHandle::Series(s) => {
            let strings = match &s.data {
                Column::String(sa) => (0..sa.len()).map(|i| sa.get(i).unwrap_or_default()).collect::<Vec<_>>(),
                _ => return Err("to_datetime requires string column".into()),
            };
            let mut epochs = Vec::with_capacity(strings.len());
            for st in strings {
                let ms = if let Ok(nd) = chrono::NaiveDate::parse_from_str(&st, &fmt) {
                    nd.and_hms_opt(0, 0, 0)
                        .map(|dt| dt.and_utc().timestamp_millis())
                        .unwrap_or(i64::MIN)
                } else if let Ok(ndt) = chrono::NaiveDateTime::parse_from_str(&st, &fmt) {
                    ndt.and_utc().timestamp_millis()
                } else {
                    i64::MIN
                };
                epochs.push(ms);
            }
            Ok(alloc_handle(NclHandle::Series(Series::from_int_array(
                format!("{}_dt", s.name),
                epochs,
            ))))
        }
        _ => Err("expected Series".into()),
    })
    .map(ok_handle)
}

// ---------------------------------------------------------------------------
// NDArray
// ---------------------------------------------------------------------------

fn ncl_ndarray(args: &[ValueRef], span: Span) -> NiaoResult<ValueRef> {
    arity(args, 2, "ncl_ndarray", span)?;
    let shape_vals = match &*args[0].borrow() {
        Value::Array(items) => {
            let mut shape = Vec::new();
            for item in items {
                match &*item.borrow() {
                    Value::Int(n) => shape.push(*n as usize),
                    _ => {
                        return Err(RuntimeError::at(span, codes::E1964_NCL_TYPE, "shape must be int array"));
                    }
                }
            }
            shape
        }
        other => {
            return Err(RuntimeError::at(
                span,
                codes::E1964_NCL_TYPE,
                format!("shape must be array, got {}", other.type_name()),
            ));
        }
    };
    let data_val = Column::coerce_packed_array(&*args[1].borrow()).ok_or_else(|| {
        RuntimeError::at(
            span,
            codes::E1964_NCL_TYPE,
            format!(
                "ncl_ndarray() data must be a numeric array, got {}",
                args[1].borrow().type_name()
            ),
        )
    })?;
    match &data_val {
        Value::IntArray(data) => {
            let arr = NDArray::from_int(shape_vals, data.clone())
                .map_err(|e| RuntimeError::at(span, codes::E1961_NCL_ERROR, e))?;
            Ok(ok_handle(alloc_handle(NclHandle::NDArray(arr))))
        }
        Value::FloatArray(data) => {
            let arr = NDArray::from_float(shape_vals, data.clone())
                .map_err(|e| RuntimeError::at(span, codes::E1961_NCL_ERROR, e))?;
            Ok(ok_handle(alloc_handle(NclHandle::NDArray(arr))))
        }
        _ => Err(RuntimeError::at(
            span,
            codes::E1964_NCL_TYPE,
            "ncl_ndarray() data must be a numeric array",
        )),
    }
}

fn ncl_shape(args: &[ValueRef], span: Span) -> NiaoResult<ValueRef> {
    arity(args, 1, "ncl_shape", span)?;
    let id = ncl_handle_arg(args, 0, "ncl_shape", span)?;
    with_handle(id, "ncl_shape", span, |h| match h {
        NclHandle::NDArray(a) => {
            let items: Vec<ValueRef> = a.shape.iter().map(|&n| Value::Int(n as i64).ref_cell()).collect();
            Ok(Value::Array(items))
        }
        _ => Err("expected NDArray".into()),
    })
    .map(ok_value)
}

fn ncl_dtype(args: &[ValueRef], span: Span) -> NiaoResult<ValueRef> {
    arity(args, 1, "ncl_dtype", span)?;
    let id = ncl_handle_arg(args, 0, "ncl_dtype", span)?;
    with_handle(id, "ncl_dtype", span, |h| match h {
        NclHandle::NDArray(a) => Ok(Value::String(a.dtype.name().into())),
        NclHandle::Series(s) => Ok(Value::String(s.dtype().name().into())),
        _ => Err("expected NDArray or Series".into()),
    })
    .map(|v| v.ref_cell())
}

fn ncl_reshape(args: &[ValueRef], span: Span) -> NiaoResult<ValueRef> {
    arity(args, 2, "ncl_reshape", span)?;
    let id = ncl_handle_arg(args, 0, "ncl_reshape", span)?;
    let new_shape = match &*args[1].borrow() {
        Value::Array(items) => {
            let mut shape = Vec::new();
            for item in items {
                match &*item.borrow() {
                    Value::Int(n) => shape.push(*n as usize),
                    _ => return Err(RuntimeError::at(span, codes::E1964_NCL_TYPE, "shape must be ints")),
                }
            }
            shape
        }
        other => {
            return Err(RuntimeError::at(
                span,
                codes::E1964_NCL_TYPE,
                format!("shape must be array, got {}", other.type_name()),
            ));
        }
    };
    with_handle(id, "ncl_reshape", span, |h| match h {
        NclHandle::NDArray(a) => {
            let out = a.reshape(new_shape)?;
            Ok(alloc_handle(NclHandle::NDArray(out)))
        }
        _ => Err("expected NDArray".into()),
    })
    .map(ok_handle)
}

fn ncl_flatten(args: &[ValueRef], span: Span) -> NiaoResult<ValueRef> {
    arity(args, 1, "ncl_flatten", span)?;
    let id = ncl_handle_arg(args, 0, "ncl_flatten", span)?;
    with_handle(id, "ncl_flatten", span, |h| match h {
        NclHandle::NDArray(a) => Ok(alloc_handle(NclHandle::NDArray(a.flatten()))),
        _ => Err("expected NDArray".into()),
    })
    .map(ok_handle)
}

fn ncl_kind(args: &[ValueRef], span: Span) -> NiaoResult<ValueRef> {
    arity(args, 1, "ncl_kind", span)?;
    let id = ncl_handle_arg(args, 0, "ncl_kind", span)?;
    with_handle(id, "ncl_kind", span, |h| Ok(Value::String(h.kind_name().into())))
        .map(|v| v.ref_cell())
}

fn ncl_len(args: &[ValueRef], span: Span) -> NiaoResult<ValueRef> {
    arity(args, 1, "ncl_len", span)?;
    if let Some(id) = handles::is_ncl_handle(&*args[0].borrow()) {
        return with_handle(id, "ncl_len", span, |h| Ok(h.len() as i64)).map(ok_int);
    }
    Err(RuntimeError::at(
        span,
        codes::E1962_NCL_INVALID_HANDLE,
        "ncl_len() expects NCL handle",
    ))
}

// ---------------------------------------------------------------------------
// Registration
// ---------------------------------------------------------------------------

fn all_builtins() -> Vec<(&'static str, NativeFn)> {
    vec![
        ("ncl_zeros", Rc::new(ncl_zeros)),
        ("ncl_ones", Rc::new(ncl_ones)),
        ("ncl_arange", Rc::new(ncl_arange)),
        ("ncl_linspace", Rc::new(ncl_linspace)),
        ("ncl_array", Rc::new(ncl_array)),
        ("ncl_slice", Rc::new(ncl_slice)),
        ("ncl_add", Rc::new(ncl_add)),
        ("ncl_sub", Rc::new(ncl_sub)),
        ("ncl_mul", Rc::new(ncl_mul)),
        ("ncl_div", Rc::new(ncl_div)),
        ("ncl_abs", Rc::new(ncl_abs)),
        ("ncl_sqrt", Rc::new(ncl_sqrt)),
        ("ncl_exp", Rc::new(ncl_exp)),
        ("ncl_log", Rc::new(ncl_log)),
        ("ncl_sin", Rc::new(ncl_sin)),
        ("ncl_cos", Rc::new(ncl_cos)),
        ("ncl_sum", Rc::new(ncl_sum)),
        ("ncl_mean", Rc::new(ncl_mean)),
        ("ncl_min", Rc::new(ncl_min)),
        ("ncl_max", Rc::new(ncl_max)),
        ("ncl_std", Rc::new(ncl_std)),
        ("ncl_var", Rc::new(ncl_var)),
        ("ncl_median", Rc::new(ncl_median)),
        ("ncl_corr", Rc::new(ncl_corr)),
        ("ncl_dot", Rc::new(ncl_dot)),
        ("ncl_parallel_sum", Rc::new(ncl_parallel_sum)),
        ("ncl_series", Rc::new(ncl_series)),
        ("ncl_dataframe", Rc::new(ncl_dataframe)),
        ("ncl_df_get", Rc::new(ncl_df_get)),
        ("ncl_df_set", Rc::new(ncl_df_set)),
        ("ncl_df_columns", Rc::new(ncl_df_columns)),
        ("ncl_df_shape", Rc::new(ncl_df_shape)),
        ("ncl_series_values", Rc::new(ncl_series_values)),
        ("ncl_series_name", Rc::new(ncl_series_name)),
        ("ncl_head", Rc::new(ncl_head)),
        ("ncl_tail", Rc::new(ncl_tail)),
        ("ncl_filter", Rc::new(ncl_filter)),
        ("ncl_sort_values", Rc::new(ncl_sort_values)),
        ("ncl_groupby", Rc::new(ncl_groupby)),
        ("ncl_agg", Rc::new(ncl_agg)),
        ("ncl_merge", Rc::new(ncl_merge)),
        ("ncl_concat", Rc::new(ncl_concat)),
        ("ncl_pivot", Rc::new(ncl_pivot)),
        ("ncl_melt", Rc::new(ncl_melt)),
        ("ncl_isna", Rc::new(ncl_isna)),
        ("ncl_fillna", Rc::new(ncl_fillna)),
        ("ncl_dropna", Rc::new(ncl_dropna)),
        ("ncl_rolling_mean", Rc::new(ncl_rolling_mean)),
        ("ncl_rolling_sum", Rc::new(ncl_rolling_sum)),
        ("ncl_rolling_std", Rc::new(ncl_rolling_std)),
        ("ncl_describe", Rc::new(ncl_describe)),
        ("ncl_read_csv", Rc::new(ncl_read_csv)),
        ("ncl_to_csv", Rc::new(ncl_to_csv)),
        ("ncl_from_sqlite", Rc::new(ncl_from_sqlite)),
        ("ncl_to_datetime", Rc::new(ncl_to_datetime)),
        ("ncl_ndarray", Rc::new(ncl_ndarray)),
        ("ncl_shape", Rc::new(ncl_shape)),
        ("ncl_dtype", Rc::new(ncl_dtype)),
        ("ncl_reshape", Rc::new(ncl_reshape)),
        ("ncl_flatten", Rc::new(ncl_flatten)),
        ("ncl_kind", Rc::new(ncl_kind)),
        ("ncl_len", Rc::new(ncl_len)),
    ]
}

pub fn handle_count() -> usize {
    handles::handle_count()
}

pub fn builtins() -> Vec<(&'static str, NativeFn)> {
    all_builtins()
}

pub fn namespace() -> Value {
    let mut map = HashMap::new();
    let bind = |map: &mut HashMap<String, ValueRef>, name: &str, f: NativeFn| {
        map.insert(name.to_string(), Value::NativeFunction(f).ref_cell());
    };
    bind(&mut map, "zeros", Rc::new(ncl_zeros));
    bind(&mut map, "ones", Rc::new(ncl_ones));
    bind(&mut map, "arange", Rc::new(ncl_arange));
    bind(&mut map, "linspace", Rc::new(ncl_linspace));
    bind(&mut map, "array", Rc::new(ncl_array));
    bind(&mut map, "slice", Rc::new(ncl_slice));
    bind(&mut map, "add", Rc::new(ncl_add));
    bind(&mut map, "sub", Rc::new(ncl_sub));
    bind(&mut map, "mul", Rc::new(ncl_mul));
    bind(&mut map, "div", Rc::new(ncl_div));
    bind(&mut map, "abs", Rc::new(ncl_abs));
    bind(&mut map, "sqrt", Rc::new(ncl_sqrt));
    bind(&mut map, "exp", Rc::new(ncl_exp));
    bind(&mut map, "log", Rc::new(ncl_log));
    bind(&mut map, "sin", Rc::new(ncl_sin));
    bind(&mut map, "cos", Rc::new(ncl_cos));
    bind(&mut map, "sum", Rc::new(ncl_sum));
    bind(&mut map, "mean", Rc::new(ncl_mean));
    bind(&mut map, "min", Rc::new(ncl_min));
    bind(&mut map, "max", Rc::new(ncl_max));
    bind(&mut map, "std", Rc::new(ncl_std));
    bind(&mut map, "var", Rc::new(ncl_var));
    bind(&mut map, "median", Rc::new(ncl_median));
    bind(&mut map, "corr", Rc::new(ncl_corr));
    bind(&mut map, "dot", Rc::new(ncl_dot));
    bind(&mut map, "parallel_sum", Rc::new(ncl_parallel_sum));
    bind(&mut map, "series", Rc::new(ncl_series));
    bind(&mut map, "dataframe", Rc::new(ncl_dataframe));
    bind(&mut map, "df_get", Rc::new(ncl_df_get));
    bind(&mut map, "df_set", Rc::new(ncl_df_set));
    bind(&mut map, "df_columns", Rc::new(ncl_df_columns));
    bind(&mut map, "df_shape", Rc::new(ncl_df_shape));
    bind(&mut map, "series_values", Rc::new(ncl_series_values));
    bind(&mut map, "series_name", Rc::new(ncl_series_name));
    bind(&mut map, "head", Rc::new(ncl_head));
    bind(&mut map, "tail", Rc::new(ncl_tail));
    bind(&mut map, "filter", Rc::new(ncl_filter));
    bind(&mut map, "sort_values", Rc::new(ncl_sort_values));
    bind(&mut map, "groupby", Rc::new(ncl_groupby));
    bind(&mut map, "agg", Rc::new(ncl_agg));
    bind(&mut map, "merge", Rc::new(ncl_merge));
    bind(&mut map, "concat", Rc::new(ncl_concat));
    bind(&mut map, "pivot", Rc::new(ncl_pivot));
    bind(&mut map, "melt", Rc::new(ncl_melt));
    bind(&mut map, "isna", Rc::new(ncl_isna));
    bind(&mut map, "fillna", Rc::new(ncl_fillna));
    bind(&mut map, "dropna", Rc::new(ncl_dropna));
    bind(&mut map, "rolling_mean", Rc::new(ncl_rolling_mean));
    bind(&mut map, "rolling_sum", Rc::new(ncl_rolling_sum));
    bind(&mut map, "rolling_std", Rc::new(ncl_rolling_std));
    bind(&mut map, "describe", Rc::new(ncl_describe));
    bind(&mut map, "read_csv", Rc::new(ncl_read_csv));
    bind(&mut map, "to_csv", Rc::new(ncl_to_csv));
    bind(&mut map, "from_sqlite", Rc::new(ncl_from_sqlite));
    bind(&mut map, "to_datetime", Rc::new(ncl_to_datetime));
    bind(&mut map, "ndarray", Rc::new(ncl_ndarray));
    bind(&mut map, "shape", Rc::new(ncl_shape));
    bind(&mut map, "dtype", Rc::new(ncl_dtype));
    bind(&mut map, "reshape", Rc::new(ncl_reshape));
    bind(&mut map, "flatten", Rc::new(ncl_flatten));
    bind(&mut map, "kind", Rc::new(ncl_kind));
    bind(&mut map, "len", Rc::new(ncl_len));
    Value::Object(map)
}

/// PostgreSQL query → DataFrame (used by `npg_to_ncl` / NML pipelines).
pub fn dataframe_from_pg(
    conn_id: u64,
    sql: &str,
    params: &[ValueRef],
    span: Span,
) -> Result<frame::DataFrame, String> {
    bridge::from_pg(conn_id, sql, params, span)
}

/// MongoDB-style object rows → DataFrame.
pub fn dataframe_from_objects(rows: &[ValueRef], columns: &[String]) -> Result<frame::DataFrame, String> {
    bridge::from_object_rows(rows, columns)
}
