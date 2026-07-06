//! Data pipeline builtins (neko_data bridge).

use super::common::*;
use super::handles::{alloc_handle, with_handle_mut, NmlHandle};
use crate::ncl::column::Column;
use crate::ncl::handles::{alloc_handle as ncl_alloc, with_handle as ncl_with, NclHandle};
use crate::{NativeFn, NekoResult, RuntimeError, Value, ValueRef};
use neko_ast::Span;
use neko_data::{
    minmax_fit_transform as normalize_fit_transform, one_hot_encode, standardize_fit_transform,
    train_test_split,
};
use neko_errors::codes;
use std::collections::HashMap;
use std::rc::Rc;

fn series_to_f32(col: &Column) -> Result<Vec<f32>, String> {
    match col {
        Column::Float(v) => Ok(v.iter().map(|&x| x as f32).collect()),
        Column::Int(v) => Ok(v.iter().map(|&x| x as f32).collect()),
        _ => Err("column must be numeric".into()),
    }
}

pub fn nml_from_dataframe(args: &[ValueRef], span: Span) -> NekoResult<ValueRef> {
    arity(args, 3, "nml_from_dataframe", span)?;
    let df_id = ncl_handle_from_arg(args, 0, "nml_from_dataframe", span)?;
    let feat_names = string_array_arg(args, 1, "nml_from_dataframe", span)?;
    let label_name = string_arg(args, 2, "nml_from_dataframe", span)?;
    ncl_with(df_id, "nml_from_dataframe", span, |h| {
        let NclHandle::DataFrame(df) = h else {
            return Err("expected DataFrame".into());
        };
        let mut feat_cols = Vec::new();
        for name in &feat_names {
            let s = df.get_column(name).ok_or_else(|| format!("column '{name}' not found"))?;
            feat_cols.push(series_to_f32(&s.data)?);
        }
        let label_s = df
            .get_column(&label_name)
            .ok_or_else(|| format!("column '{label_name}' not found"))?;
        let labels = series_to_f32(&label_s.data)?;
        let (x, y) = neko_data::dataframe_columns_to_tensors(&feat_cols, &labels)
            .map_err(|e| e.to_string())?;
        let x_id = alloc_handle(NmlHandle::Tensor(x));
        let y_id = alloc_handle(NmlHandle::Tensor(y));
        Ok((x_id, y_id))
    })
    .map(|(x_id, y_id)| {
        let mut map = HashMap::new();
        map.insert("x".to_string(), Value::NmlHandle(x_id).ref_cell());
        map.insert("y".to_string(), Value::NmlHandle(y_id).ref_cell());
        Value::Object(map).ref_cell()
    })
}

pub fn nml_train_test_split(args: &[ValueRef], span: Span) -> NekoResult<ValueRef> {
    arity_range(args, 3, 4, "nml_train_test_split", span)?;
    let x_id = nml_handle_arg(args, 0, "nml_train_test_split", span)?;
    let y_id = nml_handle_arg(args, 1, "nml_train_test_split", span)?;
    let ratio = float_arg(args, 2, "nml_train_test_split", span)? as f32;
    let seed = if args.len() == 4 {
        int_arg(args, 3, "nml_train_test_split", span)? as u64
    } else {
        42
    };
    let x = super::tensor_from_handle(x_id, "nml_train_test_split", span)?;
    let y = super::tensor_from_handle(y_id, "nml_train_test_split", span)?;
    let split = train_test_split(&x, &y, ratio, seed)
        .map_err(|e| RuntimeError::at(span, codes::E1971_NML_ERROR, e.to_string()))?;
    let mut map = HashMap::new();
    map.insert(
        "x_train".to_string(),
        Value::NmlHandle(alloc_handle(NmlHandle::Tensor(split.x_train))).ref_cell(),
    );
    map.insert(
        "y_train".to_string(),
        Value::NmlHandle(alloc_handle(NmlHandle::Tensor(split.y_train))).ref_cell(),
    );
    map.insert(
        "x_val".to_string(),
        Value::NmlHandle(alloc_handle(NmlHandle::Tensor(split.x_val))).ref_cell(),
    );
    map.insert(
        "y_val".to_string(),
        Value::NmlHandle(alloc_handle(NmlHandle::Tensor(split.y_val))).ref_cell(),
    );
    Ok(Value::Object(map).ref_cell())
}

pub fn nml_normalize(args: &[ValueRef], span: Span) -> NekoResult<ValueRef> {
    arity(args, 1, "nml_normalize", span)?;
    let id = nml_handle_arg(args, 0, "nml_normalize", span)?;
    let t = super::tensor_from_handle(id, "nml_normalize", span)?;
    let (_norm, out) = normalize_fit_transform(&t)
        .map_err(|e| RuntimeError::at(span, codes::E1971_NML_ERROR, e.to_string()))?;
    Ok(ok_handle(alloc_handle(NmlHandle::Tensor(out))))
}

pub fn nml_standardize(args: &[ValueRef], span: Span) -> NekoResult<ValueRef> {
    arity(args, 1, "nml_standardize", span)?;
    let id = nml_handle_arg(args, 0, "nml_standardize", span)?;
    let t = super::tensor_from_handle(id, "nml_standardize", span)?;
    let (_norm, out) = standardize_fit_transform(&t)
        .map_err(|e| RuntimeError::at(span, codes::E1971_NML_ERROR, e.to_string()))?;
    Ok(ok_handle(alloc_handle(NmlHandle::Tensor(out))))
}

pub fn nml_one_hot(args: &[ValueRef], span: Span) -> NekoResult<ValueRef> {
    arity(args, 2, "nml_one_hot", span)?;
    let id = nml_handle_arg(args, 0, "nml_one_hot", span)?;
    let classes = int_arg(args, 1, "nml_one_hot", span)? as usize;
    let t = super::tensor_from_handle(id, "nml_one_hot", span)?;
    let labels: Vec<i64> = t.to_cpu().map_err(|e| RuntimeError::at(span, codes::E1971_NML_ERROR, e.to_string()))?
        .iter()
        .map(|&v| v as i64)
        .collect();
    let out = one_hot_encode(&labels, classes)
        .map_err(|e| RuntimeError::at(span, codes::E1971_NML_ERROR, e.to_string()))?;
    Ok(ok_handle(alloc_handle(NmlHandle::Tensor(out))))
}

pub fn nml_batch(args: &[ValueRef], span: Span) -> NekoResult<ValueRef> {
    if args.len() == 3 {
        let x_id = nml_handle_arg(args, 0, "nml_batch", span)?;
        let y_id = nml_handle_arg(args, 1, "nml_batch", span)?;
        let batch = int_arg(args, 2, "nml_batch", span)? as usize;
        let x = super::tensor_from_handle(x_id, "nml_batch", span)?;
        let y = super::tensor_from_handle(y_id, "nml_batch", span)?;
        let loader = neko_ml::DataLoader::new(x, y, batch).map_err(|e| {
            RuntimeError::at(span, codes::E1971_NML_ERROR, e.to_string())
        })?;
        return Ok(ok_handle(alloc_handle(NmlHandle::DataLoader(loader))));
    }

    arity(args, 1, "nml_batch", span)?;
    let loader_id = nml_handle_arg(args, 0, "nml_batch", span)?;
    with_handle_mut(loader_id, "nml_batch", span, |h| {
        let NmlHandle::DataLoader(loader) = h else {
            return Err("expected dataloader".into());
        };
        if let Some((x, y)) = loader.next_batch() {
            let x_id = alloc_handle(NmlHandle::Tensor(x));
            let y_id = alloc_handle(NmlHandle::Tensor(y));
            Ok((x_id, y_id))
        } else {
            Err("no more batches".into())
        }
    })
    .map(|(x_id, y_id)| {
        let mut map = HashMap::new();
        map.insert("x".to_string(), Value::NmlHandle(x_id).ref_cell());
        map.insert("y".to_string(), Value::NmlHandle(y_id).ref_cell());
        Value::Object(map).ref_cell()
    })
}

pub fn ncl_to_nml_matrix(args: &[ValueRef], span: Span) -> NekoResult<ValueRef> {
    arity(args, 1, "ncl_to_nml_matrix", span)?;
    let id = ncl_handle_from_arg(args, 0, "ncl_to_nml_matrix", span)?;
    ncl_with(id, "ncl_to_nml_matrix", span, |h| match h {
        NclHandle::Series(s) => {
            let col = series_to_f32(&s.data)?;
            let n = col.len();
            let t = neko_tensor::Tensor::from_cpu_data(&[n, 1], col, neko_tensor::Device::Cpu)
                .map_err(|e| e.to_string())?;
            Ok(alloc_handle(NmlHandle::Tensor(t)))
        }
        NclHandle::DataFrame(df) => {
            let mut cols = Vec::new();
            for c in &df.columns {
                cols.push(series_to_f32(&c.data)?);
            }
            let n = df.len();
            let feat_n = cols.len();
            let mut data = vec![0.0f32; n * feat_n];
            for (f, col) in cols.iter().enumerate() {
                for (r, &v) in col.iter().enumerate() {
                    data[r * feat_n + f] = v;
                }
            }
            let t = neko_tensor::Tensor::from_cpu_data(&[n, feat_n], data, neko_tensor::Device::Cpu)
                .map_err(|e| e.to_string())?;
            Ok(alloc_handle(NmlHandle::Tensor(t)))
        }
        _ => Err("expected Series or DataFrame".into()),
    })
    .map(ok_handle)
}

pub fn npg_to_ncl(args: &[ValueRef], span: Span) -> NekoResult<ValueRef> {
    arity_range(args, 2, 3, "npg_to_ncl", span)?;
    let conn_id = int_arg(args, 0, "npg_to_ncl", span)? as u64;
    let sql = string_arg(args, 1, "npg_to_ncl", span)?;
    let params = if args.len() == 3 {
        match &*args[2].borrow() {
            Value::Array(items) => items.clone(),
            _ => Vec::new(),
        }
    } else {
        Vec::new()
    };
    let df = crate::ncl::dataframe_from_pg(conn_id, &sql, &params, span)
        .map_err(|e| RuntimeError::at(span, codes::E1971_NML_ERROR, e))?;
    Ok(Value::NclHandle(ncl_alloc(NclHandle::DataFrame(df))).ref_cell())
}

#[cfg(feature = "nmongo")]
pub fn nmongo_to_ncl(args: &[ValueRef], span: Span) -> NekoResult<ValueRef> {
    arity(args, 2, "nmongo_to_ncl", span)?;
    let cols = string_array_arg(args, 1, "nmongo_to_ncl", span)?;
    let rows = match &*args[0].borrow() {
        Value::Array(items) => items.clone(),
        other => {
            return Err(RuntimeError::at(
                span,
                codes::E1974_NML_TYPE,
                format!("nmongo_to_ncl() expects document array, got {}", other.type_name()),
            ));
        }
    };
    let df = crate::ncl::dataframe_from_objects(&rows, &cols)
        .map_err(|e| RuntimeError::at(span, codes::E1971_NML_ERROR, e))?;
    Ok(Value::NclHandle(ncl_alloc(NclHandle::DataFrame(df))).ref_cell())
}

#[cfg(not(feature = "nmongo"))]
pub fn nmongo_to_ncl(args: &[ValueRef], span: Span) -> NekoResult<ValueRef> {
    let _ = args;
    Err(RuntimeError::at(
        span,
        codes::E1971_NML_ERROR,
        "nmongo_to_ncl requires neko build with nmongo feature",
    ))
}

pub fn nml_node_features_from_ncl(args: &[ValueRef], span: Span) -> NekoResult<ValueRef> {
    arity(args, 3, "nml_node_features_from_ncl", span)?;
    let df_id = ncl_handle_from_arg(args, 0, "nml_node_features_from_ncl", span)?;
    let id_col = string_arg(args, 1, "nml_node_features_from_ncl", span)?;
    let feat_names = string_array_arg(args, 2, "nml_node_features_from_ncl", span)?;
    ncl_with(df_id, "nml_node_features_from_ncl", span, |h| {
        let NclHandle::DataFrame(df) = h else {
            return Err("expected DataFrame".into());
        };
        let id_series = df
            .get_column(&id_col)
            .ok_or_else(|| format!("column '{id_col}' not found"))?;
        let ids = series_to_f32(&id_series.data)?;
        let n = ids.len();
        let feat_n = feat_names.len();
        let max_id = ids.iter().cloned().fold(0.0f32, f32::max) as usize + 1;
        let mut cols = vec![vec![0.0f32; max_id]; feat_n];
        let mut row_feats: Vec<Vec<f32>> = Vec::new();
        for name in &feat_names {
            let s = df.get_column(name).ok_or_else(|| format!("column '{name}' not found"))?;
            row_feats.push(series_to_f32(&s.data)?);
        }
        for r in 0..n {
            let node = ids[r] as usize;
            if node < max_id {
                for (f, col) in row_feats.iter().enumerate() {
                    cols[f][node] = col[r];
                }
            }
        }
        let mut data = vec![0.0f32; max_id * feat_n];
        for (f, col) in cols.iter().enumerate() {
            for (node, &v) in col.iter().enumerate() {
                data[node * feat_n + f] = v;
            }
        }
        let t = neko_tensor::Tensor::from_cpu_data(&[max_id, feat_n], data, neko_tensor::Device::Cpu)
            .map_err(|e| e.to_string())?;
        Ok(alloc_handle(NmlHandle::Tensor(t)))
    })
    .map(ok_handle)
}

pub fn nml_pipeline(args: &[ValueRef], span: Span) -> NekoResult<ValueRef> {
    arity(args, 3, "nml_pipeline", span)?;
    let spec_json = string_arg(args, 0, "nml_pipeline", span)?;
    let x_id = nml_handle_arg(args, 1, "nml_pipeline", span)?;
    let y_id = nml_handle_arg(args, 2, "nml_pipeline", span)?;
    let spec: neko_data::PipelineSpec = serde_json::from_str(&spec_json).map_err(|e| {
        RuntimeError::at(span, codes::E1971_NML_ERROR, format!("invalid pipeline spec: {e}"))
    })?;
    let x = super::tensor_from_handle(x_id, "nml_pipeline", span)?;
    let y = super::tensor_from_handle(y_id, "nml_pipeline", span)?;
    let mut pipe = neko_data::Pipeline::from_spec(&spec);
    let out = pipe.run(&x, &y).map_err(|e| {
        RuntimeError::at(span, codes::E1971_NML_ERROR, e.to_string())
    })?;
    let mut map = HashMap::new();
    map.insert(
        "x_train".to_string(),
        Value::NmlHandle(alloc_handle(NmlHandle::Tensor(out.x_train))).ref_cell(),
    );
    map.insert(
        "y_train".to_string(),
        Value::NmlHandle(alloc_handle(NmlHandle::Tensor(out.y_train))).ref_cell(),
    );
    map.insert(
        "x_val".to_string(),
        Value::NmlHandle(alloc_handle(NmlHandle::Tensor(out.x_val))).ref_cell(),
    );
    map.insert(
        "y_val".to_string(),
        Value::NmlHandle(alloc_handle(NmlHandle::Tensor(out.y_val))).ref_cell(),
    );
    Ok(Value::Object(map).ref_cell())
}

pub fn nml_columnar_epoch(args: &[ValueRef], span: Span) -> NekoResult<ValueRef> {
    arity_range(args, 4, 5, "nml_columnar_epoch", span)?;
    let trainer_id = nml_handle_arg(args, 0, "nml_columnar_epoch", span)?;
    let df_id = ncl_handle_from_arg(args, 1, "nml_columnar_epoch", span)?;
    let feat_names = string_array_arg(args, 2, "nml_columnar_epoch", span)?;
    let label_name = string_arg(args, 3, "nml_columnar_epoch", span)?;
    let batch = if args.len() == 5 {
        int_arg(args, 4, "nml_columnar_epoch", span)? as usize
    } else {
        32
    };
    let (feat_cols, label_col) = ncl_with(df_id, "nml_columnar_epoch", span, |h| {
        let NclHandle::DataFrame(df) = h else {
            return Err("expected DataFrame".into());
        };
        let mut feats = Vec::new();
        for name in &feat_names {
            let s = df.get_column(name).ok_or_else(|| format!("column '{name}' not found"))?;
            feats.push(series_to_f32(&s.data)?);
        }
        let label_s = df
            .get_column(&label_name)
            .ok_or_else(|| format!("column '{label_name}' not found"))?;
        Ok((feats, series_to_f32(&label_s.data)?))
    })?;
    let mut epoch = neko_data::ColumnarEpoch::new(feat_cols, label_col, batch);
    with_handle_mut(trainer_id, "nml_columnar_epoch", span, |h| {
        let NmlHandle::Trainer(t) = h else {
            return Err("expected trainer".into());
        };
        let loss = t.train_columnar_epoch(&mut epoch).map_err(|e| e.to_string())?;
        Ok(loss)
    })
    .map(ok_float)
}

pub fn data_builtins() -> Vec<(&'static str, NativeFn)> {
    vec![
        ("nml_from_dataframe", Rc::new(nml_from_dataframe)),
        ("nml_train_test_split", Rc::new(nml_train_test_split)),
        ("nml_normalize", Rc::new(nml_normalize)),
        ("nml_standardize", Rc::new(nml_standardize)),
        ("nml_one_hot", Rc::new(nml_one_hot)),
        ("nml_batch", Rc::new(nml_batch)),
        ("ncl_to_nml_matrix", Rc::new(ncl_to_nml_matrix)),
        ("npg_to_ncl", Rc::new(npg_to_ncl)),
        ("nmongo_to_ncl", Rc::new(nmongo_to_ncl)),
        ("nml_node_features_from_ncl", Rc::new(nml_node_features_from_ncl)),
        ("nml_pipeline", Rc::new(nml_pipeline)),
        ("nml_columnar_epoch", Rc::new(nml_columnar_epoch)),
    ]
}
