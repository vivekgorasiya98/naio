//! Shared argument helpers for nsqlite builtins.

use super::types::{neko_to_bound, BoundValue};
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
            codes::E1700_NSQLITE_ARITY,
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
            codes::E1700_NSQLITE_ARITY,
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

pub fn conn_arg(args: &[ValueRef], idx: usize, name: &str, span: Span) -> Result<u64, RuntimeError> {
    match &*args[idx].borrow() {
        Value::Int(id) if *id > 0 => Ok(*id as u64),
        other => Err(RuntimeError::at(
            span,
            codes::E1702_NSQLITE_INVALID_HANDLE,
            format!(
                "{name}() expects connection handle as argument {}, got {}",
                idx + 1,
                other.type_name()
            ),
        )),
    }
}

pub fn params_array_arg(
    args: &[ValueRef],
    idx: usize,
    name: &str,
    span: Span,
) -> Result<Vec<BoundValue>, RuntimeError> {
    match &*args[idx].borrow() {
        Value::Array(items) => {
            let mut out = Vec::with_capacity(items.len());
            for item in items {
                out.push(neko_to_bound(&*item.borrow(), span)?);
            }
            Ok(out)
        }
        other => Err(type_err(
            span,
            format!(
                "{name}() expects params array as argument {}, got {}",
                idx + 1,
                other.type_name()
            ),
        )),
    }
}

pub fn sql_list_arg(args: &[ValueRef], idx: usize, name: &str, span: Span) -> Result<Vec<String>, RuntimeError> {
    match &*args[idx].borrow() {
        Value::Array(items) => {
            let mut out = Vec::with_capacity(items.len());
            for item in items {
                match &*item.borrow() {
                    Value::String(s) => out.push(s.clone()),
                    other => {
                        return Err(type_err(
                            span,
                            format!(
                                "{name}() expects array of SQL strings, got {}",
                                other.type_name()
                            ),
                        ));
                    }
                }
            }
            Ok(out)
        }
        other => Err(type_err(
            span,
            format!(
                "{name}() expects SQL string array as argument {}, got {}",
                idx + 1,
                other.type_name()
            ),
        )),
    }
}
