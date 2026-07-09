//! Value <-> SQLite type mapping.

use crate::Value;
use niao_ast::Span;
use niao_errors::codes;
use rusqlite::types::Value as SqlValue;

#[derive(Clone)]
pub enum BoundValue {
    Null,
    Int(i64),
    Float(f64),
    Text(String),
    Blob(Vec<u8>),
}

pub fn niao_to_bound(val: &Value, span: Span) -> Result<BoundValue, crate::RuntimeError> {
    match val {
        Value::Nil => Ok(BoundValue::Null),
        Value::Int(n) => Ok(BoundValue::Int(*n)),
        Value::Float(f) => Ok(BoundValue::Float(*f)),
        Value::Bool(b) => Ok(BoundValue::Int(if *b { 1 } else { 0 })),
        Value::String(s) => Ok(BoundValue::Text(s.clone())),
        Value::ByteArray(b) => Ok(BoundValue::Blob(b.clone())),
        other => Err(crate::RuntimeError::at(
            span,
            codes::E1706_NSQLITE_BIND,
            format!(
                "cannot bind value of type {} to SQLite parameter",
                other.type_name()
            ),
        )),
    }
}

pub fn sql_to_niao(val: SqlValue) -> Value {
    match val {
        SqlValue::Null => Value::Nil,
        SqlValue::Integer(n) => Value::Int(n),
        SqlValue::Real(f) => Value::Float(f),
        SqlValue::Text(s) => Value::String(s),
        SqlValue::Blob(b) => Value::ByteArray(b),
    }
}

pub fn bind_positional(
    stmt: &mut rusqlite::Statement<'_>,
    index: i32,
    value: &BoundValue,
) -> Result<(), String> {
    let idx = index as usize;
    match value {
        BoundValue::Null => stmt.raw_bind_parameter(idx, rusqlite::types::Null),
        BoundValue::Int(n) => stmt.raw_bind_parameter(idx, *n),
        BoundValue::Float(f) => stmt.raw_bind_parameter(idx, *f),
        BoundValue::Text(s) => stmt.raw_bind_parameter(idx, s.as_str()),
        BoundValue::Blob(b) => stmt.raw_bind_parameter(idx, b.as_slice()),
    }
    .map_err(|e| e.to_string())
}

pub fn bind_named(
    stmt: &mut rusqlite::Statement<'_>,
    name: &str,
    value: &BoundValue,
) -> Result<(), String> {
    let owned;
    let key: &str = if name.starts_with(':') || name.starts_with('@') || name.starts_with('$') {
        name
    } else {
        owned = format!(":{name}");
        &owned
    };
    let idx = stmt
        .parameter_index(key)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("unknown parameter name \"{key}\""))?;
    match value {
        BoundValue::Null => stmt.raw_bind_parameter(idx, rusqlite::types::Null),
        BoundValue::Int(n) => stmt.raw_bind_parameter(idx, *n),
        BoundValue::Float(f) => stmt.raw_bind_parameter(idx, *f),
        BoundValue::Text(s) => stmt.raw_bind_parameter(idx, s.as_str()),
        BoundValue::Blob(b) => stmt.raw_bind_parameter(idx, b.as_slice()),
    }
    .map_err(|e| e.to_string())
}

pub fn apply_params(stmt: &mut rusqlite::Statement<'_>, params: &[BoundValue]) -> Result<(), String> {
    for (i, p) in params.iter().enumerate() {
        bind_positional(stmt, (i + 1) as i32, p)?;
    }
    Ok(())
}

pub fn apply_stmt_bindings(stmt: &mut rusqlite::Statement<'_>, handle: &super::handles::StmtHandle) -> Result<(), String> {
    for (idx, val) in &handle.params {
        bind_positional(stmt, *idx, val)?;
    }
    for (name, val) in &handle.named_params {
        bind_named(stmt, name, val)?;
    }
    Ok(())
}

pub fn value_to_async(val: &Value) -> crate::async_tasks::AsyncValue {
    use crate::async_tasks::AsyncValue;
    match val {
        Value::Nil => AsyncValue::nil(),
        Value::Int(n) => AsyncValue::int(*n),
        Value::Bool(b) => AsyncValue::Bool(*b),
        Value::Float(f) => AsyncValue::Float(*f),
        Value::String(s) => AsyncValue::String(s.clone()),
        Value::IntArray(v) => AsyncValue::IntArray(v.clone()),
        Value::ByteArray(v) => AsyncValue::ByteArray(v.clone()),
        Value::Array(items) => AsyncValue::Array(items.iter().map(|v| value_to_async(&*v.borrow())).collect()),
        Value::Object(map) => {
            let mut out = std::collections::HashMap::with_capacity(map.len());
            for (k, v) in map {
                out.insert(k.clone(), value_to_async(&*v.borrow()));
            }
            AsyncValue::Object(out)
        }
        other => AsyncValue::String(other.type_name().to_string()),
    }
}
