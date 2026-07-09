//! Shared helpers for the `parallel` standard library.

use crate::parallel::sendable::{sendable_to_value_ref, value_to_sendable, SendableValue};
use crate::{error_value, NiaoResult, RuntimeError, Value, ValueRef};
use niao_ast::Span;
use niao_errors::codes;

pub(crate) fn type_err(span: Span, msg: impl Into<String>) -> RuntimeError {
    RuntimeError::TypeError {
        message: msg.into(),
        line: span.line,
        col: span.col,
    }
}

pub(crate) fn arity(args: &[ValueRef], n: usize, name: &str, span: Span) -> NiaoResult<()> {
    if args.len() != n {
        return Err(RuntimeError::at(
            span,
            codes::E1500_PARALLEL_ARITY,
            format!("{name}() expects {n} argument(s), got {}", args.len()),
        ));
    }
    Ok(())
}

pub(crate) fn arity_range(
    args: &[ValueRef],
    min: usize,
    max: usize,
    name: &str,
    span: Span,
) -> NiaoResult<()> {
    if args.len() < min || args.len() > max {
        return Err(RuntimeError::at(
            span,
            codes::E1500_PARALLEL_ARITY,
            format!("{name}() expects {min}..={max} argument(s), got {}", args.len()),
        ));
    }
    Ok(())
}

pub(crate) fn arity_min(args: &[ValueRef], min: usize, name: &str, span: Span) -> NiaoResult<()> {
    if args.len() < min {
        return Err(RuntimeError::at(
            span,
            codes::E1500_PARALLEL_ARITY,
            format!("{name}() expects at least {min} argument(s), got {}", args.len()),
        ));
    }
    Ok(())
}

pub(crate) fn int_arg(args: &[ValueRef], idx: usize, name: &str, span: Span) -> NiaoResult<i64> {
    match &*args[idx].borrow() {
        Value::Int(n) => Ok(*n),
        other => Err(type_err(
            span,
            format!(
                "{name}() expects an int as argument {}, got {}",
                idx + 1,
                other.type_name()
            ),
        )),
    }
}

pub(crate) fn handle_arg(args: &[ValueRef], idx: usize, name: &str, span: Span) -> NiaoResult<u64> {
    let id = int_arg(args, idx, name, span)?;
    if id <= 0 {
        return Err(type_err(
            span,
            format!("{name}() expects a positive handle as argument {}", idx + 1),
        ));
    }
    Ok(id as u64)
}

pub(crate) fn parallel_error(span: Span, code: u32, msg: impl Into<String>) -> ValueRef {
    error_value(code, "parallel_error", msg.into(), span)
}

pub(crate) fn ok_nil() -> ValueRef {
    Value::Nil.ref_cell()
}

pub(crate) fn ok_bool(b: bool) -> ValueRef {
    Value::Bool(b).ref_cell()
}

pub(crate) fn ok_int(n: i64) -> ValueRef {
    Value::Int(n).ref_cell()
}

pub(crate) fn sendable_arg(
    args: &[ValueRef],
    idx: usize,
    name: &str,
    span: Span,
) -> Result<SendableValue, ValueRef> {
    value_to_sendable(&args[idx].borrow()).map_err(|msg| {
        parallel_error(
            span,
            codes::E1504_PARALLEL_NOT_SENDABLE,
            format!("{name}() argument {}: {msg}", idx + 1),
        )
    })
}

pub(crate) fn sendable_arg_or_err(
    args: &[ValueRef],
    idx: usize,
    name: &str,
    span: Span,
) -> Result<SendableValue, RuntimeError> {
    sendable_arg(args, idx, name, span).map_err(|v| {
        if let Value::Error(e) = &*v.borrow() {
            RuntimeError::at(span, e.code, e.message.clone())
        } else {
            RuntimeError::at(span, codes::E1504_PARALLEL_NOT_SENDABLE, "not sendable")
        }
    })
}

pub(crate) fn sendable_args_rest_or_err(
    args: &[ValueRef],
    start: usize,
    name: &str,
    span: Span,
) -> Result<Vec<SendableValue>, RuntimeError> {
    let mut out = Vec::new();
    for (i, a) in args[start..].iter().enumerate() {
        out.push(value_to_sendable(&a.borrow()).map_err(|msg| {
            RuntimeError::at(
                span,
                codes::E1504_PARALLEL_NOT_SENDABLE,
                format!("{name}() argument {}: {msg}", start + i + 1),
            )
        })?);
    }
    Ok(out)
}

pub(crate) fn sendable_result(val: SendableValue) -> ValueRef {
    sendable_to_value_ref(val)
}

pub(crate) fn function_arg(args: &[ValueRef], idx: usize, name: &str, span: Span) -> NiaoResult<ValueRef> {
    let is_fn = matches!(&*args[idx].borrow(), Value::Function(_));
    if is_fn {
        Ok(args[idx].clone())
    } else {
        Err(type_err(
            span,
            format!(
                "{name}() expects a function as argument {}, got {}",
                idx + 1,
                args[idx].borrow().type_name()
            ),
        ))
    }
}

pub type ParallelResult = NiaoResult<ValueRef>;
