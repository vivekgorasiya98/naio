//! Shared argument helpers for NCL builtins.

use crate::{RuntimeError, Value, ValueRef};
use neko_ast::Span;
use neko_errors::codes;
use super::handles::is_ncl_handle;

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
            codes::E1960_NCL_ARITY,
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
            codes::E1960_NCL_ARITY,
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
        Value::Float(f) => Ok(*f),
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

pub fn bool_arg(args: &[ValueRef], idx: usize, name: &str, span: Span) -> Result<bool, RuntimeError> {
    match &*args[idx].borrow() {
        Value::Bool(b) => Ok(*b),
        other => Err(type_err(
            span,
            format!(
                "{name}() expects bool as argument {}, got {}",
                idx + 1,
                other.type_name()
            ),
        )),
    }
}

pub fn ncl_handle_arg(args: &[ValueRef], idx: usize, name: &str, span: Span) -> Result<u64, RuntimeError> {
    match is_ncl_handle(&*args[idx].borrow()) {
        Some(id) => Ok(id),
        None => Err(RuntimeError::at(
            span,
            codes::E1962_NCL_INVALID_HANDLE,
            format!(
                "{name}() expects NCL handle as argument {}, got {}",
                idx + 1,
                args[idx].borrow().type_name()
            ),
        )),
    }
}

pub fn array_arg(
    args: &[ValueRef],
    idx: usize,
    name: &str,
    span: Span,
) -> Result<Value, RuntimeError> {
    match &*args[idx].borrow() {
        v @ (Value::IntArray(_) | Value::FloatArray(_) | Value::BoolArray(_) | Value::Array(_)) => Ok(v.clone()),
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

pub fn ok_handle(id: u64) -> ValueRef {
    Value::NclHandle(id).ref_cell()
}

pub fn ok_int(n: i64) -> ValueRef {
    Value::Int(n).ref_cell()
}

pub fn ok_float(f: f64) -> ValueRef {
    Value::Float(f).ref_cell()
}

pub fn ok_bool(b: bool) -> ValueRef {
    Value::Bool(b).ref_cell()
}

pub fn ok_value(v: Value) -> ValueRef {
    v.ref_cell()
}

pub fn ncl_error(span: Span, msg: impl Into<String>) -> ValueRef {
    crate::error_value(codes::E1961_NCL_ERROR, "ncl_error", msg.into(), span)
}
