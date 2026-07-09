//! Value <-> PostgreSQL type mapping.

use crate::Value;
use niao_ast::Span;
use niao_errors::codes;
use postgres::types::ToSql;
use postgres::Row;
use serde_json::Value as JsonValue;
use std::collections::HashMap;

#[derive(Clone, Debug)]
pub enum BoundValue {
    Null,
    Int(i64),
    Float(f64),
    Bool(bool),
    Text(String),
    Blob(Vec<u8>),
    Json(JsonValue),
    Array(Vec<BoundValue>),
}

pub fn niao_to_bound(val: &Value, span: Span) -> Result<BoundValue, crate::RuntimeError> {
    match val {
        Value::Nil => Ok(BoundValue::Null),
        Value::Int(n) => Ok(BoundValue::Int(*n)),
        Value::Float(f) => Ok(BoundValue::Float(*f)),
        Value::Bool(b) => Ok(BoundValue::Bool(*b)),
        Value::String(s) => Ok(BoundValue::Text(s.clone())),
        Value::ByteArray(b) => Ok(BoundValue::Blob(b.clone())),
        Value::Object(map) => {
            let mut json_map = serde_json::Map::new();
            for (k, v) in map {
                json_map.insert(k.clone(), niao_to_json(&*v.borrow())?);
            }
            Ok(BoundValue::Json(JsonValue::Object(json_map)))
        }
        Value::Array(items) => {
            let mut out = Vec::with_capacity(items.len());
            for item in items {
                out.push(niao_to_bound(&*item.borrow(), span)?);
            }
            Ok(BoundValue::Array(out))
        }
        other => Err(crate::RuntimeError::at(
            span,
            codes::E1906_NPG_BIND,
            format!(
                "cannot bind value of type {} to PostgreSQL parameter",
                other.type_name()
            ),
        )),
    }
}

fn niao_to_json(val: &Value) -> Result<JsonValue, crate::RuntimeError> {
    match val {
        Value::Nil => Ok(JsonValue::Null),
        Value::Int(n) => Ok(JsonValue::from(*n)),
        Value::Float(f) => serde_json::Number::from_f64(*f)
            .map(JsonValue::Number)
            .ok_or_else(|| crate::RuntimeError::TypeError {
                message: "float cannot be represented as JSON number".into(),
                line: 0,
                col: 0,
            }),
        Value::Bool(b) => Ok(JsonValue::Bool(*b)),
        Value::String(s) => Ok(JsonValue::String(s.clone())),
        Value::Array(items) => {
            let mut arr = Vec::with_capacity(items.len());
            for item in items {
                arr.push(niao_to_json(&*item.borrow())?);
            }
            Ok(JsonValue::Array(arr))
        }
        Value::Object(map) => {
            let mut json_map = serde_json::Map::new();
            for (k, v) in map {
                json_map.insert(k.clone(), niao_to_json(&*v.borrow())?);
            }
            Ok(JsonValue::Object(json_map))
        }
        other => Ok(JsonValue::String(other.type_name().to_string())),
    }
}

pub fn bound_to_sql_params(params: &[BoundValue]) -> Vec<Box<dyn ToSql + Sync>> {
    params
        .iter()
        .map(|p| -> Box<dyn ToSql + Sync> {
            match p {
                BoundValue::Null => Box::new(Option::<i32>::None),
                BoundValue::Int(n) => Box::new(*n),
                BoundValue::Float(f) => Box::new(*f),
                BoundValue::Bool(b) => Box::new(*b),
                BoundValue::Text(s) => Box::new(s.clone()),
                BoundValue::Blob(b) => Box::new(b.clone()),
                BoundValue::Json(j) => Box::new(j.clone()),
                BoundValue::Array(items) => {
                    let texts: Vec<String> = items
                        .iter()
                        .map(|v| match v {
                            BoundValue::Null => "NULL".to_string(),
                            BoundValue::Int(n) => n.to_string(),
                            BoundValue::Float(f) => f.to_string(),
                            BoundValue::Bool(b) => b.to_string(),
                            BoundValue::Text(s) => s.clone(),
                            _ => format!("{v:?}"),
                        })
                        .collect();
                    Box::new(texts)
                }
            }
        })
        .collect()
}

pub fn sql_param_refs(boxes: &[Box<dyn ToSql + Sync>]) -> Vec<&(dyn ToSql + Sync)> {
    boxes.iter().map(|b| b.as_ref()).collect()
}

pub fn pg_to_niao(row: &Row, i: usize) -> Value {
    if let Ok(v) = row.try_get::<_, Option<i64>>(i) {
        return v.map(Value::Int).unwrap_or(Value::Nil);
    }
    if let Ok(v) = row.try_get::<_, Option<i32>>(i) {
        return v.map(|n| Value::Int(n as i64)).unwrap_or(Value::Nil);
    }
    if let Ok(v) = row.try_get::<_, Option<f64>>(i) {
        return v.map(Value::Float).unwrap_or(Value::Nil);
    }
    if let Ok(v) = row.try_get::<_, Option<bool>>(i) {
        return v.map(Value::Bool).unwrap_or(Value::Nil);
    }
    if let Ok(v) = row.try_get::<_, Option<String>>(i) {
        return v.map(Value::String).unwrap_or(Value::Nil);
    }
    if let Ok(v) = row.try_get::<_, Option<Vec<u8>>>(i) {
        return v.map(Value::ByteArray).unwrap_or(Value::Nil);
    }
    if let Ok(v) = row.try_get::<_, Option<JsonValue>>(i) {
        return v.map(|j| json_to_niao(&j)).unwrap_or(Value::Nil);
    }
    if let Ok(v) = row.try_get::<_, Option<Vec<String>>>(i) {
        return v
            .map(|items| {
                Value::Array(items.into_iter().map(|s| Value::String(s).ref_cell()).collect())
            })
            .unwrap_or(Value::Nil);
    }
    Value::Nil
}

fn json_to_niao(j: &JsonValue) -> Value {
    match j {
        JsonValue::Null => Value::Nil,
        JsonValue::Bool(b) => Value::Bool(*b),
        JsonValue::Number(n) => {
            if let Some(i) = n.as_i64() {
                Value::Int(i)
            } else if let Some(f) = n.as_f64() {
                Value::Float(f)
            } else {
                Value::String(n.to_string())
            }
        }
        JsonValue::String(s) => Value::String(s.clone()),
        JsonValue::Array(items) => {
            Value::Array(items.iter().map(|v| json_to_niao(v).ref_cell()).collect())
        }
        JsonValue::Object(map) => {
            let mut out = HashMap::new();
            for (k, v) in map {
                out.insert(k.clone(), json_to_niao(v).ref_cell());
            }
            Value::Object(out)
        }
    }
}

pub fn rewrite_placeholders(sql: &str) -> String {
    let mut out = String::with_capacity(sql.len());
    let mut n = 1usize;
    let mut in_single = false;
    let mut in_double = false;
    let chars: Vec<char> = sql.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c == '\'' && !in_double {
            if in_single && i + 1 < chars.len() && chars[i + 1] == '\'' {
                out.push(c);
                out.push(chars[i + 1]);
                i += 2;
                continue;
            }
            in_single = !in_single;
            out.push(c);
        } else if c == '"' && !in_single {
            in_double = !in_double;
            out.push(c);
        } else if c == '?' && !in_single && !in_double {
            out.push('$');
            out.push_str(&n.to_string());
            n += 1;
        } else {
            out.push(c);
        }
        i += 1;
    }
    out
}

pub fn quote_ident(name: &str) -> String {
    format!("\"{}\"", name.replace('"', "\"\""))
}

pub fn quote_literal(s: &str) -> String {
    format!("'{}'", s.replace('\'', "''"))
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
        Value::Array(items) => {
            AsyncValue::Array(items.iter().map(|v| value_to_async(&*v.borrow())).collect())
        }
        Value::Object(map) => {
            let mut out = HashMap::with_capacity(map.len());
            for (k, v) in map {
                out.insert(k.clone(), value_to_async(&*v.borrow()));
            }
            AsyncValue::Object(out)
        }
        other => AsyncValue::String(other.type_name().to_string()),
    }
}

pub fn row_column_names(row: &Row) -> Vec<String> {
    row.columns()
        .iter()
        .map(|c| c.name().to_string())
        .collect()
}
