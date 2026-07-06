//! nsqlite → packed DataFrame bridge.

use super::column::Column;
use super::frame::DataFrame;
use super::series::Series;
use crate::nsqlite::handles;
use crate::nsqlite::types::{apply_params, neko_to_bound, BoundValue};
use crate::StringArray;
use neko_ast::Span;
use rusqlite::types::Value as SqlValue;

pub fn from_sqlite(conn_id: u64, sql: &str, params: &[crate::ValueRef], span: Span) -> Result<DataFrame, String> {
    handles::with_conn_mut(conn_id, "ncl_from_sqlite", span, |handle| {
        let bound: Result<Vec<BoundValue>, _> = params
            .iter()
            .map(|p| neko_to_bound(&p.borrow(), span))
            .collect();
        let bound = bound.map_err(|e| e.message())?;
        let mut stmt = handle.conn.prepare(sql).map_err(|e| e.to_string())?;
        apply_params(&mut stmt, &bound)?;
        let col_count = stmt.column_count() as usize;
        let names: Vec<String> = (0..col_count)
            .map(|i| stmt.column_name(i).unwrap_or("").to_string())
            .collect();

        let mut col_bufs: Vec<ColumnBuilder> = (0..col_count)
            .map(|_| ColumnBuilder::new())
            .collect();

        let mut rows = stmt.raw_query();
        while let Some(row) = rows.next().map_err(|e| e.to_string())? {
            for i in 0..col_count {
                let val = row.get::<_, SqlValue>(i).unwrap_or(SqlValue::Null);
                col_bufs[i].push_sql(val);
            }
        }

        let series: Vec<Series> = names
            .into_iter()
            .zip(col_bufs)
            .map(|(name, b)| Series::new(name, b.finish()))
            .collect();
        DataFrame::new(series).map_err(|e| e.to_string())
    })
    .map_err(|e| e.message())
}

enum ColumnBuilder {
    Int(Vec<i64>),
    Float(Vec<f64>),
    String(Vec<String>),
    Any(Vec<crate::ValueRef>),
}

impl ColumnBuilder {
    fn new() -> Self {
        ColumnBuilder::Any(Vec::new())
    }

    fn new_typed(col_type: Option<rusqlite::types::Type>) -> Self {
        match col_type {
            Some(rusqlite::types::Type::Integer) => ColumnBuilder::Int(Vec::new()),
            Some(rusqlite::types::Type::Real) => ColumnBuilder::Float(Vec::new()),
            Some(rusqlite::types::Type::Text) => ColumnBuilder::String(Vec::new()),
            _ => ColumnBuilder::Any(Vec::new()),
        }
    }

    fn push_sql(&mut self, val: SqlValue) {
        if let ColumnBuilder::Any(v) = self {
            if v.is_empty() {
                *self = match val {
                    SqlValue::Integer(_) => ColumnBuilder::Int(Vec::new()),
                    SqlValue::Real(_) => ColumnBuilder::Float(Vec::new()),
                    SqlValue::Text(_) => ColumnBuilder::String(Vec::new()),
                    SqlValue::Null => ColumnBuilder::Int(Vec::new()),
                    _ => ColumnBuilder::Any(Vec::new()),
                };
            }
        }
        match self {
            ColumnBuilder::Int(v) => {
                let n = match val {
                    SqlValue::Integer(i) => i,
                    SqlValue::Real(f) => f as i64,
                    SqlValue::Null => i64::MIN,
                    _ => {
                        *self = ColumnBuilder::Any(Vec::new());
                        return self.push_sql(val);
                    }
                };
                v.push(n);
            }
            ColumnBuilder::Float(v) => {
                let f = match val {
                    SqlValue::Integer(i) => i as f64,
                    SqlValue::Real(f) => f,
                    SqlValue::Null => f64::NAN,
                    _ => {
                        *self = ColumnBuilder::Any(Vec::new());
                        return self.push_sql(val);
                    }
                };
                v.push(f);
            }
            ColumnBuilder::String(v) => {
                let s = match val {
                    SqlValue::Text(t) => t,
                    SqlValue::Integer(i) => i.to_string(),
                    SqlValue::Real(f) => f.to_string(),
                    SqlValue::Null => String::new(),
                    SqlValue::Blob(b) => String::from_utf8_lossy(&b).into_owned(),
                };
                v.push(s);
            }
            ColumnBuilder::Any(v) => {
                let nv = crate::nsqlite::types::sql_to_neko(val);
                v.push(nv.ref_cell());
            }
        }
    }

    fn finish(self) -> Column {
        match self {
            ColumnBuilder::Int(v) => Column::Int(v),
            ColumnBuilder::Float(v) => Column::Float(v),
            ColumnBuilder::String(v) => Column::String(StringArray::dense(v)),
            ColumnBuilder::Any(v) => Column::Any(v),
        }
    }
}

pub fn from_pg(
    conn_id: u64,
    sql: &str,
    params: &[crate::ValueRef],
    span: Span,
) -> Result<DataFrame, String> {
    let value = crate::npg::query_table(conn_id, sql, params, span).map_err(|e| e.message())?;
    table_value_to_dataframe(&value)
}

fn table_value_to_dataframe(value: &crate::Value) -> Result<DataFrame, String> {
    let crate::Value::Object(map) = value else {
        return Err("expected table object".into());
    };
    let cols_ref = map.get("columns").ok_or("missing columns")?;
    let rows_ref = map.get("rows").ok_or("missing rows")?;
    let col_names: Vec<String> = match &*cols_ref.borrow() {
        crate::Value::Array(items) => items
            .iter()
            .map(|c| match &*c.borrow() {
                crate::Value::String(s) => Ok(s.clone()),
                other => Err(format!("expected column name string, got {}", other.type_name())),
            })
            .collect::<Result<_, _>>()?,
        other => return Err(format!("expected columns array, got {}", other.type_name())),
    };
    let col_count = col_names.len();
    let mut builders: Vec<ColumnBuilder> = (0..col_count).map(|_| ColumnBuilder::new()).collect();
    if let crate::Value::Array(rows) = &*rows_ref.borrow() {
        for row_ref in rows {
            let crate::Value::Array(cells) = &*row_ref.borrow() else {
                return Err("expected row array".into());
            };
            for (i, cell) in cells.iter().enumerate() {
                if i >= builders.len() {
                    break;
                }
                push_neko_value(&mut builders[i], &cell.borrow());
            }
        }
    }
    let series: Vec<Series> = col_names
        .into_iter()
        .zip(builders)
        .map(|(name, b)| Series::new(name, b.finish()))
        .collect();
    DataFrame::new(series).map_err(|e| e.to_string())
}

fn push_neko_value(builder: &mut ColumnBuilder, val: &crate::Value) {
    match val {
        crate::Value::Int(n) => {
            if let ColumnBuilder::Any(v) = builder {
                if v.is_empty() {
                    *builder = ColumnBuilder::Int(Vec::new());
                }
            }
            if let ColumnBuilder::Int(v) = builder {
                v.push(*n);
            }
        }
        crate::Value::Float(f) => {
            if let ColumnBuilder::Any(v) = builder {
                if v.is_empty() {
                    *builder = ColumnBuilder::Float(Vec::new());
                }
            }
            if let ColumnBuilder::Float(v) = builder {
                v.push(*f);
            }
        }
        crate::Value::String(s) => {
            if let ColumnBuilder::Any(v) = builder {
                if v.is_empty() {
                    *builder = ColumnBuilder::String(Vec::new());
                }
            }
            if let ColumnBuilder::String(v) = builder {
                v.push(s.clone());
            }
        }
        other => {
            if let ColumnBuilder::Any(v) = builder {
                v.push(other.clone().ref_cell());
            }
        }
    }
}

pub fn from_object_rows(rows: &[crate::ValueRef], columns: &[String]) -> Result<DataFrame, String> {
    let mut builders: Vec<ColumnBuilder> = (0..columns.len()).map(|_| ColumnBuilder::new()).collect();
    for row_ref in rows {
        let crate::Value::Object(map) = &*row_ref.borrow() else {
            return Err("expected object row".into());
        };
        for (i, name) in columns.iter().enumerate() {
            if let Some(cell) = map.get(name) {
                push_neko_value(&mut builders[i], &cell.borrow());
            }
        }
    }
    let series: Vec<Series> = columns
        .iter()
        .cloned()
        .zip(builders)
        .map(|(name, b)| Series::new(name, b.finish()))
        .collect();
    DataFrame::new(series).map_err(|e| e.to_string())
}
