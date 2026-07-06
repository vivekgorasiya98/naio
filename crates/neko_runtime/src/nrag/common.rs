//! Shared helpers for nrag builtins.

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
            codes::E1980_NRAG_ARITY,
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
            codes::E1980_NRAG_ARITY,
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

pub fn float_arg(args: &[ValueRef], idx: usize, name: &str, span: Span) -> Result<f64, RuntimeError> {
    match &*args[idx].borrow() {
        Value::Float(n) => Ok(*n),
        Value::Int(n) => Ok(*n as f64),
        other => Err(type_err(
            span,
            format!(
                "{name}() expects float as argument {}, got {}",
                idx + 1,
                other.type_name()
            ),
        )),
    }
}

pub fn index_arg(args: &[ValueRef], idx: usize, name: &str, span: Span) -> Result<u64, RuntimeError> {
    match &*args[idx].borrow() {
        Value::Int(id) if *id > 0 => Ok(*id as u64),
        other => Err(RuntimeError::at(
            span,
            codes::E1982_NRAG_INVALID_HANDLE,
            format!(
                "{name}() expects index handle as argument {}, got {}",
                idx + 1,
                other.type_name()
            ),
        )),
    }
}
