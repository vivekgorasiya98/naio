//! High-level query helpers and row mapping.

use super::handles::ConnHandle;
use super::types::{bound_to_sql_params, pg_to_neko, rewrite_placeholders, row_column_names, sql_param_refs};
use crate::Value;
use postgres::Row;
use std::collections::HashMap;
use std::io::Write;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum RowFormat {
    Object,
    Array,
}

pub fn parse_row_format(s: &str) -> Result<RowFormat, String> {
    match s {
        "object" => Ok(RowFormat::Object),
        "array" => Ok(RowFormat::Array),
        other => Err(format!("unknown row format \"{other}\" (use \"object\" or \"array\")")),
    }
}

fn row_to_object(row: &Row, cols: &[String]) -> Value {
    let mut map = HashMap::with_capacity(cols.len());
    for (i, name) in cols.iter().enumerate() {
        map.insert(name.clone(), pg_to_neko(row, i).ref_cell());
    }
    Value::Object(map)
}

fn row_to_array(row: &Row, col_count: usize) -> Value {
    let mut items = Vec::with_capacity(col_count);
    for i in 0..col_count {
        items.push(pg_to_neko(row, i).ref_cell());
    }
    Value::Array(items)
}

pub fn collect_rows(rows: Vec<Row>, format: RowFormat) -> Result<Value, String> {
    if rows.is_empty() {
        return match format {
            RowFormat::Object => Ok(Value::Array(Vec::new())),
            RowFormat::Array => {
                let mut map = HashMap::new();
                map.insert("columns".to_string(), Value::Array(Vec::new()).ref_cell());
                map.insert("rows".to_string(), Value::Array(Vec::new()).ref_cell());
                Ok(Value::Object(map))
            }
        };
    }
    let cols = row_column_names(&rows[0]);
    let col_count = cols.len();
    match format {
        RowFormat::Object => {
            let mut out = Vec::with_capacity(rows.len());
            for row in &rows {
                out.push(row_to_object(row, &cols).ref_cell());
            }
            Ok(Value::Array(out))
        }
        RowFormat::Array => {
            let mut data = Vec::with_capacity(rows.len());
            for row in &rows {
                data.push(row_to_array(row, col_count).ref_cell());
            }
            let mut map = HashMap::new();
            map.insert(
                "columns".to_string(),
                Value::Array(cols.into_iter().map(|c| Value::String(c).ref_cell()).collect()).ref_cell(),
            );
            map.insert("rows".to_string(), Value::Array(data).ref_cell());
            Ok(Value::Object(map))
        }
    }
}

pub fn query_on_conn(
    conn: &mut ConnHandle,
    sql: &str,
    params: &[super::types::BoundValue],
    format: RowFormat,
) -> Result<Value, String> {
    let sql = rewrite_placeholders(sql);
    let boxes = bound_to_sql_params(params);
    let refs = sql_param_refs(&boxes);
    let rows = conn
        .client_mut()
        .query(sql.as_str(), &refs)
        .map_err(|e| e.to_string())?;
    collect_rows(rows, format)
}

pub fn query_row_on_conn(
    conn: &mut ConnHandle,
    sql: &str,
    params: &[super::types::BoundValue],
) -> Result<Value, String> {
    let sql = rewrite_placeholders(sql);
    let boxes = bound_to_sql_params(params);
    let refs = sql_param_refs(&boxes);
    let rows = conn
        .client_mut()
        .query(sql.as_str(), &refs)
        .map_err(|e| e.to_string())?;
    if let Some(row) = rows.first() {
        let cols = row_column_names(row);
        Ok(row_to_object(row, &cols))
    } else {
        Ok(Value::Nil)
    }
}

pub fn query_value_on_conn(
    conn: &mut ConnHandle,
    sql: &str,
    params: &[super::types::BoundValue],
) -> Result<Value, String> {
    let sql = rewrite_placeholders(sql);
    let boxes = bound_to_sql_params(params);
    let refs = sql_param_refs(&boxes);
    let rows = conn
        .client_mut()
        .query(sql.as_str(), &refs)
        .map_err(|e| e.to_string())?;
    if let Some(row) = rows.first() {
        Ok(pg_to_neko(row, 0))
    } else {
        Ok(Value::Nil)
    }
}

pub fn query_column_on_conn(
    conn: &mut ConnHandle,
    sql: &str,
    params: &[super::types::BoundValue],
) -> Result<Value, String> {
    let sql = rewrite_placeholders(sql);
    let boxes = bound_to_sql_params(params);
    let refs = sql_param_refs(&boxes);
    let rows = conn
        .client_mut()
        .query(sql.as_str(), &refs)
        .map_err(|e| e.to_string())?;
    let mut out = Vec::with_capacity(rows.len());
    for row in &rows {
        out.push(pg_to_neko(row, 0).ref_cell());
    }
    Ok(Value::Array(out))
}

pub fn exec_on_conn(conn: &mut ConnHandle, sql: &str, params: &[super::types::BoundValue]) -> Result<u64, String> {
    let sql = rewrite_placeholders(sql);
    let boxes = bound_to_sql_params(params);
    let refs = sql_param_refs(&boxes);
    let n = conn
        .client_mut()
        .execute(sql.as_str(), &refs)
        .map_err(|e| e.to_string())?;
    Ok(n)
}

pub fn batch_on_conn(
    conn: &mut ConnHandle,
    sql: &str,
    rows: &[Vec<super::types::BoundValue>],
) -> Result<u64, String> {
    let sql = rewrite_placeholders(sql);
    let mut trans = conn.client_mut().transaction().map_err(|e| e.to_string())?;
    let mut total = 0u64;
    for row in rows {
        let boxes = bound_to_sql_params(row);
        let refs = sql_param_refs(&boxes);
        total += trans.execute(sql.as_str(), &refs).map_err(|e| e.to_string())?;
    }
    trans.commit().map_err(|e| e.to_string())?;
    Ok(total)
}

pub fn insert_on_conn(
    conn: &mut ConnHandle,
    schema: Option<&str>,
    table: &str,
    data: &HashMap<String, crate::ValueRef>,
    span: neko_ast::Span,
) -> Result<Value, String> {
    if data.is_empty() {
        return Err("insert data object is empty".into());
    }
    let table_ref = match schema {
        Some(s) => format!(
            "{}.{}",
            super::types::quote_ident(s),
            super::types::quote_ident(table)
        ),
        None => super::types::quote_ident(table),
    };
    let mut cols = Vec::new();
    let mut placeholders = Vec::new();
    let mut params = Vec::new();
    let mut n = 1;
    for (k, v) in data {
        cols.push(super::types::quote_ident(k));
        placeholders.push(format!("${n}"));
        n += 1;
        params.push(
            super::types::neko_to_bound(&*v.borrow(), span)
                .map_err(|e| format!("{e}"))?,
        );
    }
    let sql = format!(
        "INSERT INTO {table_ref} ({}) VALUES ({}) RETURNING *",
        cols.join(", "),
        placeholders.join(", ")
    );
    let boxes = bound_to_sql_params(&params);
    let refs = sql_param_refs(&boxes);
    let rows = conn.client_mut().query(&sql, &refs).map_err(|e| e.to_string())?;
    if let Some(row) = rows.first() {
        let cols = row_column_names(row);
        Ok(row_to_object(row, &cols))
    } else {
        Ok(Value::Nil)
    }
}

pub fn copy_from_on_conn(
    conn: &mut ConnHandle,
    schema: Option<&str>,
    table: &str,
    columns: &[String],
    rows: &[Vec<String>],
) -> Result<u64, String> {
    let table_ref = match schema {
        Some(s) => format!(
            "{}.{}",
            super::types::quote_ident(s),
            super::types::quote_ident(table)
        ),
        None => super::types::quote_ident(table),
    };
    let col_list = columns
        .iter()
        .map(|c| super::types::quote_ident(c))
        .collect::<Vec<_>>()
        .join(", ");
    let sql = format!("COPY {table_ref} ({col_list}) FROM STDIN WITH (FORMAT csv)");
    let mut writer = conn.client_mut().copy_in(&sql).map_err(|e| e.to_string())?;
    for row in rows {
        let line = row
            .iter()
            .map(|cell| cell.replace('"', "\"\""))
            .map(|cell| {
                if cell.contains(',') || cell.contains('"') || cell.contains('\n') {
                    format!("\"{cell}\"")
                } else {
                    cell
                }
            })
            .collect::<Vec<_>>()
            .join(",");
        writer
            .write(format!("{line}\n").as_bytes())
            .map_err(|e| e.to_string())?;
    }
    writer.finish().map_err(|e| e.to_string())
}
