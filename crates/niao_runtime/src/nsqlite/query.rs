//! High-level query helpers and row mapping.

use super::handles::ConnHandle;
use super::types::{apply_params, sql_to_niao};
use crate::Value;
use rusqlite::Row;
use std::collections::HashMap;

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

fn row_to_object(row: &Row<'_>, cols: &[String]) -> Value {
    let mut map = HashMap::with_capacity(cols.len());
    for (i, name) in cols.iter().enumerate() {
        let val = row.get::<_, rusqlite::types::Value>(i).unwrap_or(rusqlite::types::Value::Null);
        map.insert(name.clone(), sql_to_niao(val).ref_cell());
    }
    Value::Object(map)
}

fn row_to_array(row: &Row<'_>, col_count: usize) -> Value {
    let mut items = Vec::with_capacity(col_count);
    for i in 0..col_count {
        let val = row.get::<_, rusqlite::types::Value>(i).unwrap_or(rusqlite::types::Value::Null);
        items.push(sql_to_niao(val).ref_cell());
    }
    Value::Array(items)
}

pub fn collect_rows(
    mut stmt: rusqlite::Statement<'_>,
    format: RowFormat,
) -> Result<Value, String> {
    let col_count = stmt.column_count() as usize;
    let cols: Vec<String> = (0..col_count)
        .map(|i| stmt.column_name(i).unwrap_or("").to_string())
        .collect();

    let mut rows = stmt.raw_query();
    match format {
        RowFormat::Object => {
            let mut out = Vec::new();
            while let Some(row) = rows.next().map_err(|e| e.to_string())? {
                out.push(row_to_object(row, &cols).ref_cell());
            }
            Ok(Value::Array(out))
        }
        RowFormat::Array => {
            let mut data = Vec::new();
            while let Some(row) = rows.next().map_err(|e| e.to_string())? {
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
    let mut stmt = conn.conn.prepare(sql).map_err(|e| e.to_string())?;
    apply_params(&mut stmt, params)?;
    collect_rows(stmt, format)
}

pub fn query_row_on_conn(
    conn: &mut ConnHandle,
    sql: &str,
    params: &[super::types::BoundValue],
) -> Result<Value, String> {
    let mut stmt = conn.conn.prepare(sql).map_err(|e| e.to_string())?;
    apply_params(&mut stmt, params)?;
    let col_count = stmt.column_count() as usize;
    let cols: Vec<String> = (0..col_count)
        .map(|i| stmt.column_name(i).unwrap_or("").to_string())
        .collect();
    let mut rows = stmt.raw_query();
    if let Some(row) = rows.next().map_err(|e| e.to_string())? {
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
    let mut stmt = conn.conn.prepare(sql).map_err(|e| e.to_string())?;
    apply_params(&mut stmt, params)?;
    let mut rows = stmt.raw_query();
    if let Some(row) = rows.next().map_err(|e| e.to_string())? {
        let val = row.get::<_, rusqlite::types::Value>(0).unwrap_or(rusqlite::types::Value::Null);
        Ok(sql_to_niao(val))
    } else {
        Ok(Value::Nil)
    }
}

pub fn query_column_on_conn(
    conn: &mut ConnHandle,
    sql: &str,
    params: &[super::types::BoundValue],
) -> Result<Value, String> {
    let mut stmt = conn.conn.prepare(sql).map_err(|e| e.to_string())?;
    apply_params(&mut stmt, params)?;
    let mut rows = stmt.raw_query();
    let mut out = Vec::new();
    while let Some(row) = rows.next().map_err(|e| e.to_string())? {
        let val = row.get::<_, rusqlite::types::Value>(0).unwrap_or(rusqlite::types::Value::Null);
        out.push(sql_to_niao(val).ref_cell());
    }
    Ok(Value::Array(out))
}

pub fn exec_on_conn(conn: &mut ConnHandle, sql: &str, params: &[super::types::BoundValue]) -> Result<(), String> {
    if params.is_empty() {
        conn.conn.execute(sql, []).map_err(|e| e.to_string())?;
    } else {
        let mut stmt = conn.conn.prepare(sql).map_err(|e| e.to_string())?;
        apply_params(&mut stmt, params)?;
        stmt.raw_execute().map_err(|e| e.to_string())?;
    }
    Ok(())
}

pub fn batch_on_conn(
    conn: &mut ConnHandle,
    sql: &str,
    rows: &[Vec<super::types::BoundValue>],
) -> Result<i64, String> {
    let tx = conn.conn.transaction().map_err(|e| e.to_string())?;
    let mut total = 0i64;
    {
        for row in rows {
            let mut stmt = tx.prepare(sql).map_err(|e| e.to_string())?;
            apply_params(&mut stmt, row)?;
            total += stmt.raw_execute().map_err(|e| e.to_string())? as i64;
        }
    }
    tx.commit().map_err(|e| e.to_string())?;
    Ok(total)
}
