//! Shared argument helpers for NML builtins.

use crate::{RuntimeError, Value, ValueRef};
use neko_ast::Span;
use neko_errors::codes;

pub fn arity(args: &[ValueRef], n: usize, name: &str, span: Span) -> Result<(), RuntimeError> {
    if args.len() != n {
        return Err(RuntimeError::at(
            span,
            codes::E1970_NML_ARITY,
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
            codes::E1970_NML_ARITY,
            format!("{name}() expects {min}..={max} argument(s), got {}", args.len()),
        ));
    }
    Ok(())
}

pub fn int_arg(args: &[ValueRef], idx: usize, name: &str, span: Span) -> Result<i64, RuntimeError> {
    match &*args[idx].borrow() {
        Value::Int(n) => Ok(*n),
        other => Err(RuntimeError::at(
            span,
            codes::E1974_NML_TYPE,
            format!("{name}() expects int, got {}", other.type_name()),
        )),
    }
}

pub fn float_arg(args: &[ValueRef], idx: usize, name: &str, span: Span) -> Result<f64, RuntimeError> {
    match &*args[idx].borrow() {
        Value::Float(f) => Ok(*f),
        Value::Int(n) => Ok(*n as f64),
        other => Err(RuntimeError::at(
            span,
            codes::E1974_NML_TYPE,
            format!("{name}() expects float, got {}", other.type_name()),
        )),
    }
}

pub fn string_arg(args: &[ValueRef], idx: usize, name: &str, span: Span) -> Result<String, RuntimeError> {
    match &*args[idx].borrow() {
        Value::String(s) => Ok(s.clone()),
        other => Err(RuntimeError::at(
            span,
            codes::E1974_NML_TYPE,
            format!("{name}() expects string, got {}", other.type_name()),
        )),
    }
}

pub fn bool_arg(args: &[ValueRef], idx: usize, name: &str, span: Span) -> Result<bool, RuntimeError> {
    match &*args[idx].borrow() {
        Value::Bool(b) => Ok(*b),
        other => Err(RuntimeError::at(
            span,
            codes::E1974_NML_TYPE,
            format!("{name}() expects bool, got {}", other.type_name()),
        )),
    }
}

pub fn nml_handle_arg(args: &[ValueRef], idx: usize, name: &str, span: Span) -> Result<u64, RuntimeError> {
    match &*args[idx].borrow() {
        Value::NmlHandle(id) => Ok(*id),
        other => Err(RuntimeError::at(
            span,
            codes::E1974_NML_TYPE,
            format!("{name}() expects nml handle, got {}", other.type_name()),
        )),
    }
}

pub fn float_array_arg(args: &[ValueRef], idx: usize, name: &str, span: Span) -> Result<Vec<f64>, RuntimeError> {
    match &*args[idx].borrow() {
        Value::NmlHandle(id) => {
            let t = super::tensor_from_handle(*id, name, span)?;
            let cpu = t
                .to_cpu()
                .map_err(|e| RuntimeError::at(span, codes::E1971_NML_ERROR, e.to_string()))?;
            Ok(cpu.iter().map(|&x| x as f64).collect())
        }
        Value::FloatArray(a) => Ok(a.clone()),
        Value::Array(items) => {
            let mut out = Vec::with_capacity(items.len());
            for item in items {
                match &*item.borrow() {
                    Value::Float(f) => out.push(*f),
                    Value::Int(n) => out.push(*n as f64),
                    other => {
                        return Err(RuntimeError::at(
                            span,
                            codes::E1974_NML_TYPE,
                            format!("{name}() expects numeric array, got {}", other.type_name()),
                        ));
                    }
                }
            }
            Ok(out)
        }
        other => Err(RuntimeError::at(
            span,
            codes::E1974_NML_TYPE,
            format!("{name}() expects FloatArray, got {}", other.type_name()),
        )),
    }
}

pub fn int_array_arg(args: &[ValueRef], idx: usize, name: &str, span: Span) -> Result<Vec<i64>, RuntimeError> {
    match &*args[idx].borrow() {
        Value::IntArray(a) => Ok(a.clone()),
        Value::Array(items) => {
            let mut out = Vec::with_capacity(items.len());
            for item in items {
                match &*item.borrow() {
                    Value::Int(n) => out.push(*n),
                    other => {
                        return Err(RuntimeError::at(
                            span,
                            codes::E1974_NML_TYPE,
                            format!("{name}() expects int array, got {}", other.type_name()),
                        ));
                    }
                }
            }
            Ok(out)
        }
        other => Err(RuntimeError::at(
            span,
            codes::E1974_NML_TYPE,
            format!("{name}() expects int array, got {}", other.type_name()),
        )),
    }
}

pub fn ok_handle(id: u64) -> ValueRef {
    Value::NmlHandle(id).ref_cell()
}

pub fn ok_float(f: f32) -> ValueRef {
    Value::Float(f as f64).ref_cell()
}

pub fn ok_int(n: i64) -> ValueRef {
    Value::Int(n).ref_cell()
}

pub fn ok_bool(b: bool) -> ValueRef {
    Value::Bool(b).ref_cell()
}

pub fn string_array_arg(args: &[ValueRef], idx: usize, name: &str, span: Span) -> Result<Vec<String>, RuntimeError> {
    match &*args[idx].borrow() {
        Value::StringArray(sa) => {
            let mut out = Vec::with_capacity(sa.len());
            for i in 0..sa.len() {
                out.push(sa.get(i).unwrap_or_default());
            }
            Ok(out)
        }
        Value::Array(items) => {
            let mut out = Vec::with_capacity(items.len());
            for item in items {
                match &*item.borrow() {
                    Value::String(s) => out.push(s.clone()),
                    other => {
                        return Err(RuntimeError::at(
                            span,
                            codes::E1974_NML_TYPE,
                            format!("{name}() expects string array, got {}", other.type_name()),
                        ));
                    }
                }
            }
            Ok(out)
        }
        other => Err(RuntimeError::at(
            span,
            codes::E1974_NML_TYPE,
            format!("{name}() expects string array, got {}", other.type_name()),
        )),
    }
}

pub fn ncl_handle_from_arg(args: &[ValueRef], idx: usize, name: &str, span: Span) -> Result<u64, RuntimeError> {
    match &*args[idx].borrow() {
        Value::NclHandle(id) => Ok(*id),
        other => Err(RuntimeError::at(
            span,
            codes::E1974_NML_TYPE,
            format!("{name}() expects ncl handle, got {}", other.type_name()),
        )),
    }
}
