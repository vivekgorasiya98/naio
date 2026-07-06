//! Shared argument helpers for nmongo builtins.

use crate::{RuntimeError, Value, ValueRef};
use neko_ast::Span;
use neko_errors::codes;

pub fn type_err(span: Span, msg: impl Into<String>) -> RuntimeError {
    RuntimeError::TypeError {
        message: msg.into(),
        line: span.line,
        col: span.col,
    }
}

pub fn arity(args: &[ValueRef], n: usize, name: &str, span: Span) -> Result<(), RuntimeError> {
    if args.len() != n {
        return Err(RuntimeError::at(
            span,
            codes::E1920_NMONGO_ARITY,
            format!("{name}() expects {n} argument(s), got {}", args.len()),
        ));
    }
    Ok(())
}

pub fn arity_range(
    args: &[ValueRef],
    min: usize,
    max: usize,
    name: &str,
    span: Span,
) -> Result<(), RuntimeError> {
    if args.len() < min || args.len() > max {
        return Err(RuntimeError::at(
            span,
            codes::E1920_NMONGO_ARITY,
            format!("{name}() expects {min}..={max} argument(s), got {}", args.len()),
        ));
    }
    Ok(())
}

pub fn string_arg(args: &[ValueRef], idx: usize, name: &str, span: Span) -> Result<String, RuntimeError> {
    match &*args[idx].borrow() {
        Value::String(s) => Ok(s.clone()),
        other => Err(type_err(
            span,
            format!(
                "{name}() expects string as argument {}, got {}",
                idx + 1,
                other.type_name()
            ),
        )),
    }
}

pub fn int_arg(args: &[ValueRef], idx: usize, name: &str, span: Span) -> Result<i64, RuntimeError> {
    match &*args[idx].borrow() {
        Value::Int(n) => Ok(*n),
        other => Err(type_err(
            span,
            format!(
                "{name}() expects int as argument {}, got {}",
                idx + 1,
                other.type_name()
            ),
        )),
    }
}

pub fn client_arg(args: &[ValueRef], idx: usize, name: &str, span: Span) -> Result<u64, RuntimeError> {
    match &*args[idx].borrow() {
        Value::Int(id) if *id > 0 => Ok(*id as u64),
        other => Err(RuntimeError::at(
            span,
            codes::E1922_NMONGO_INVALID_HANDLE,
            format!(
                "{name}() expects client handle as argument {}, got {}",
                idx + 1,
                other.type_name()
            ),
        )),
    }
}

pub fn session_arg(args: &[ValueRef], idx: usize, name: &str, span: Span) -> Result<u64, RuntimeError> {
    match &*args[idx].borrow() {
        Value::Int(id) if *id > 0 => Ok(*id as u64),
        other => Err(RuntimeError::at(
            span,
            codes::E1922_NMONGO_INVALID_HANDLE,
            format!(
                "{name}() expects session handle as argument {}, got {}",
                idx + 1,
                other.type_name()
            ),
        )),
    }
}

pub fn object_arg(
    args: &[ValueRef],
    idx: usize,
    name: &str,
    span: Span,
) -> Result<std::collections::HashMap<String, ValueRef>, RuntimeError> {
    match &*args[idx].borrow() {
        Value::Object(map) => Ok(map.clone()),
        other => Err(type_err(
            span,
            format!(
                "{name}() expects object as argument {}, got {}",
                idx + 1,
                other.type_name()
            ),
        )),
    }
}

#[allow(dead_code)]
pub fn object_arg_ref(
    args: &[ValueRef],
    idx: usize,
    name: &str,
    span: Span,
) -> Result<std::collections::HashMap<String, ValueRef>, RuntimeError> {
    match &*args[idx].borrow() {
        Value::Object(map) => Ok(map.clone()),
        other => Err(type_err(
            span,
            format!(
                "{name}() expects object as argument {}, got {}",
                idx + 1,
                other.type_name()
            ),
        )),
    }
}

pub fn array_arg(args: &[ValueRef], idx: usize, name: &str, span: Span) -> Result<Vec<ValueRef>, RuntimeError> {
    match &*args[idx].borrow() {
        Value::Array(items) => Ok(items.clone()),
        other => Err(type_err(
            span,
            format!(
                "{name}() expects array as argument {}, got {}",
                idx + 1,
                other.type_name()
            ),
        )),
    }
}

pub fn optional_object_arg(
    args: &[ValueRef],
    idx: usize,
) -> Option<std::collections::HashMap<String, ValueRef>> {
    if args.len() <= idx {
        return None;
    }
    match &*args[idx].borrow() {
        Value::Object(map) => Some(map.clone()),
        _ => None,
    }
}

pub fn optional_doc_arg(
    args: &[ValueRef],
    idx: usize,
    name: &str,
    span: Span,
) -> Result<bson::Document, RuntimeError> {
    if args.len() <= idx {
        return Ok(bson::Document::new());
    }
    match &*args[idx].borrow() {
        Value::Nil => Ok(bson::Document::new()),
        Value::Object(_) => super::types::neko_to_bson(&args[idx], span),
        other => Err(type_err(
            span,
            format!(
                "{name}() expects object or nil as argument {}, got {}",
                idx + 1,
                other.type_name()
            ),
        )),
    }
}

/// Validate database or collection name.
pub fn validate_name(name: &str, kind: &str, span: Span) -> Result<(), RuntimeError> {
    if name.is_empty() || name.len() > 120 {
        return Err(RuntimeError::at(
            span,
            codes::E1923_NMONGO_INVALID_NAME,
            format!("invalid {kind} name: length must be 1..=120"),
        ));
    }
    if !name
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
    {
        return Err(RuntimeError::at(
            span,
            codes::E1923_NMONGO_INVALID_NAME,
            format!("invalid {kind} name: only alphanumeric, _, - allowed"),
        ));
    }
    Ok(())
}

/// Redact password from error messages.
pub fn redact_secrets(msg: &str, password: Option<&str>) -> String {
    let mut out = msg.to_string();
    if let Some(pw) = password {
        if !pw.is_empty() {
            out = out.replace(pw, "***");
        }
    }
    // Common URI patterns
    if let Some(at) = out.find("://") {
        if let Some(colon) = out[at + 3..].find(':') {
            let start = at + 3 + colon + 1;
            if let Some(end) = out[start..].find('@') {
                let before = &out[..start];
                let after = &out[start + end..];
                out = format!("{before}***{after}");
            }
        }
    }
    out
}

pub fn db_coll_args(
    args: &[ValueRef],
    name: &str,
    span: Span,
) -> Result<(u64, String, String), RuntimeError> {
    if args.len() < 3 {
        return Err(RuntimeError::at(
            span,
            codes::E1920_NMONGO_ARITY,
            format!("{name}() expects at least 3 argument(s), got {}", args.len()),
        ));
    }
    let client = client_arg(args, 0, name, span)?;
    let db = string_arg(args, 1, name, span)?;
    let coll = string_arg(args, 2, name, span)?;
    validate_name(&db, "database", span)?;
    validate_name(&coll, "collection", span)?;
    Ok((client, db, coll))
}

pub fn db_coll_args_range(
    args: &[ValueRef],
    min: usize,
    max: usize,
    name: &str,
    span: Span,
) -> Result<(u64, String, String), RuntimeError> {
    arity_range(args, min, max, name, span)?;
    let client = client_arg(args, 0, name, span)?;
    let db = string_arg(args, 1, name, span)?;
    let coll = string_arg(args, 2, name, span)?;
    validate_name(&db, "database", span)?;
    validate_name(&coll, "collection", span)?;
    Ok((client, db, coll))
}
