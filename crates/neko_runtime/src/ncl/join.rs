//! Join and concat operations.

use super::column::Column;
use super::frame::DataFrame;
use super::series::Series;
use ahash::RandomState;
use std::collections::HashMap;

pub fn concat_vertical(a: &DataFrame, b: &DataFrame) -> Result<DataFrame, String> {
    if a.column_names() != b.column_names() {
        return Err("concat: column names must match".into());
    }
    let mut cols = Vec::new();
    for (ca, cb) in a.columns.iter().zip(&b.columns) {
        let data = concat_columns(&ca.data, &cb.data)?;
        cols.push(Series {
            name: ca.name.clone(),
            data,
            validity: None,
            index: None,
        });
    }
    DataFrame::new(cols)
}

fn concat_columns(a: &Column, b: &Column) -> Result<Column, String> {
    match (a, b) {
        (Column::Int(x), Column::Int(y)) => Ok(Column::Int([x.as_slice(), y.as_slice()].concat())),
        (Column::Float(x), Column::Float(y)) => {
            Ok(Column::Float([x.as_slice(), y.as_slice()].concat()))
        }
        (Column::Bool(x), Column::Bool(y)) => Ok(Column::Bool([x.as_slice(), y.as_slice()].concat())),
        (Column::String(x), Column::String(y)) => {
            let mut dense = x.dense_vec();
            dense.extend(y.dense_vec());
            Ok(Column::String(crate::StringArray::dense(dense)))
        }
        (Column::Any(x), Column::Any(y)) => {
            let mut v = x.clone();
            v.extend(y.iter().cloned());
            Ok(Column::Any(v))
        }
        _ => Err("concat: incompatible column types".into()),
    }
}

pub fn merge_inner(left: &DataFrame, right: &DataFrame, on: &str) -> Result<DataFrame, String> {
    let lk = left
        .get_column(on)
        .ok_or_else(|| format!("left column '{on}' not found"))?;
    let rk = right
        .get_column(on)
        .ok_or_else(|| format!("right column '{on}' not found"))?;

    let (left_indices, right_indices) = match (&lk.data, &rk.data) {
        (Column::Int(lv), Column::Int(rv)) => {
            let mut map: HashMap<i64, Vec<usize>, RandomState> =
                HashMap::with_hasher(RandomState::new());
            for (i, &k) in rv.iter().enumerate() {
                map.entry(k).or_default().push(i);
            }
            let mut li = Vec::new();
            let mut ri = Vec::new();
            for (i, &k) in lv.iter().enumerate() {
                if let Some(rs) = map.get(&k) {
                    for &r in rs {
                        li.push(i);
                        ri.push(r);
                    }
                }
            }
            (li, ri)
        }
        _ => return Err("merge on key must be int columns".into()),
    };

    build_merged(left, right, on, &left_indices, &right_indices)
}

fn build_merged(
    left: &DataFrame,
    right: &DataFrame,
    on: &str,
    li: &[usize],
    ri: &[usize],
) -> Result<DataFrame, String> {
    let mut cols = Vec::new();
    for c in &left.columns {
        if c.name == on && right.get_column(on).is_some() {
            continue;
        }
        let data = pick_indices(&c.data, li)?;
        cols.push(Series::new(c.name.clone(), data));
    }
    for c in &right.columns {
        if c.name == on {
            if left.get_column(on).is_some() {
                continue;
            }
        }
        let suffix = if left.get_column(&c.name).is_some() {
            format!("{}_r", c.name)
        } else {
            c.name.clone()
        };
        let data = pick_indices(&c.data, ri)?;
        cols.push(Series::new(suffix, data));
    }
    if left.get_column(on).is_none() {
        if let Some(c) = left.get_column(on).or_else(|| right.get_column(on)) {
            let data = pick_indices(&c.data, li)?;
            cols.insert(0, Series::new(on.to_string(), data));
        }
    } else if let Some(c) = left.get_column(on) {
        let data = pick_indices(&c.data, li)?;
        cols.insert(0, Series::new(on.to_string(), data));
    }
    DataFrame::new(cols)
}

fn pick_indices(col: &Column, indices: &[usize]) -> Result<Column, String> {
    match col {
        Column::Int(v) => Ok(Column::Int(indices.iter().map(|&i| v[i]).collect())),
        Column::Float(v) => Ok(Column::Float(indices.iter().map(|&i| v[i]).collect())),
        Column::Bool(v) => Ok(Column::Bool(indices.iter().map(|&i| v[i]).collect())),
        Column::String(s) => {
            let dense: Vec<String> = indices
                .iter()
                .map(|&i| s.get(i).unwrap_or_default())
                .collect();
            Ok(Column::String(crate::StringArray::dense(dense)))
        }
        Column::Any(v) => Ok(Column::Any(indices.iter().map(|&i| v[i].clone()).collect())),
    }
}
