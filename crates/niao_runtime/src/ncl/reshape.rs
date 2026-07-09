//! Pivot, melt, stack, unstack.

use super::column::Column;
use super::frame::DataFrame;
use super::series::Series;
use ahash::RandomState;
use std::collections::HashMap;

pub fn melt(df: &DataFrame, id_vars: &[String]) -> Result<DataFrame, String> {
    let n = df.len();
    let mut var_col = Vec::with_capacity(n * df.column_count());
    let mut val_col = Vec::with_capacity(n * df.column_count());
    let mut id_cols: HashMap<String, Vec<String>> = HashMap::new();
    for name in id_vars {
        let c = df
            .get_column(name)
            .ok_or_else(|| format!("id column '{name}' not found"))?;
        id_cols.insert(name.clone(), column_to_strings(&c.data, n)?);
    }
    for c in &df.columns {
        if id_vars.contains(&c.name) {
            continue;
        }
        for i in 0..n {
            var_col.push(c.name.clone());
            val_col.push(cell_to_string(&c.data, i)?);
        }
    }
    let mut cols = Vec::new();
    for name in id_vars {
        let base = id_cols.get(name).unwrap();
        let expanded: Vec<String> = (0..var_col.len())
            .map(|i| base[i % n].clone())
            .collect();
        cols.push(Series::new(
            name.clone(),
            Column::String(crate::StringArray::dense(expanded)),
        ));
    }
    cols.push(Series::new(
        "variable",
        Column::String(crate::StringArray::dense(var_col)),
    ));
    cols.push(Series::new(
        "value",
        Column::String(crate::StringArray::dense(val_col)),
    ));
    DataFrame::new(cols)
}

pub fn pivot(df: &DataFrame, index: &str, columns: &str, values: &str) -> Result<DataFrame, String> {
    let idx_col = df
        .get_column(index)
        .ok_or_else(|| format!("index column '{index}' not found"))?;
    let col_col = df
        .get_column(columns)
        .ok_or_else(|| format!("columns column '{columns}' not found"))?;
    let val_col = df
        .get_column(values)
        .ok_or_else(|| format!("values column '{values}' not found"))?;

    let mut pivot_cols: HashMap<String, HashMap<String, f64>, RandomState> =
        HashMap::with_hasher(RandomState::new());
    let mut row_keys: Vec<String> = Vec::new();
    let mut col_keys: Vec<String> = Vec::new();

    for i in 0..df.len() {
        let rk = cell_to_string(&idx_col.data, i)?;
        let ck = cell_to_string(&col_col.data, i)?;
        let v = cell_to_f64(&val_col.data, i)?;
        if !row_keys.contains(&rk) {
            row_keys.push(rk.clone());
        }
        if !col_keys.contains(&ck) {
            col_keys.push(ck.clone());
        }
        pivot_cols.entry(rk).or_default().insert(ck, v);
    }

    let mut out_cols = vec![Series::new(
        index.to_string(),
        Column::String(crate::StringArray::dense(row_keys.clone())),
    )];
    for ck in &col_keys {
        let vals: Vec<f64> = row_keys
            .iter()
            .map(|rk| pivot_cols.get(rk).and_then(|m| m.get(ck)).copied().unwrap_or(f64::NAN))
            .collect();
        out_cols.push(Series::from_float_array(ck.clone(), vals));
    }
    DataFrame::new(out_cols)
}

fn column_to_strings(col: &Column, n: usize) -> Result<Vec<String>, String> {
    (0..n).map(|i| cell_to_string(col, i)).collect()
}

fn cell_to_string(col: &Column, i: usize) -> Result<String, String> {
    Ok(match col {
        Column::Int(v) => v[i].to_string(),
        Column::Float(v) => v[i].to_string(),
        Column::Bool(v) => (v[i] != 0).to_string(),
        Column::String(s) => s.get(i).unwrap_or_default(),
        Column::Any(v) => v[i].borrow().to_string(),
    })
}

fn cell_to_f64(col: &Column, i: usize) -> Result<f64, String> {
    Ok(match col {
        Column::Int(v) => v[i] as f64,
        Column::Float(v) => v[i],
        Column::Bool(v) => v[i] as f64,
        _ => return Err("pivot values must be numeric".into()),
    })
}
