//! Shared helpers for ahiru builtins.

use crate::{RuntimeError, Value, ValueRef};
use ahiru_core::AhiruConfig;
use neko_ast::Span;
use neko_errors::codes;
use std::collections::HashMap;

pub fn type_err(span: Span, msg: impl Into<String>) -> RuntimeError {
    RuntimeError::TypeError {
        message: msg.into(),
        line: span.line,
        col: span.col,
    }
}

pub fn runtime_err(span: Span, msg: &str) -> RuntimeError {
    RuntimeError::at(span, codes::E2101_AHIRU_ERROR, msg)
}

pub fn arity(args: &[ValueRef], n: usize, name: &str, span: Span) -> Result<(), RuntimeError> {
    if args.len() != n {
        return Err(RuntimeError::at(
            span,
            codes::E2100_AHIRU_ARITY,
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
            codes::E2100_AHIRU_ARITY,
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

pub fn object_arg(
    args: &[ValueRef],
    idx: usize,
    name: &str,
    span: Span,
) -> Result<HashMap<String, ValueRef>, RuntimeError> {
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

pub fn ok_nil() -> Result<ValueRef, RuntimeError> {
    Ok(Value::Nil.ref_cell())
}

pub fn object_to_config(map: &HashMap<String, ValueRef>, span: Span) -> Result<AhiruConfig, RuntimeError> {
    if let Some(path_ref) = map.get("config_path").or_else(|| map.get("path")) {
        if let Value::String(path) = &*path_ref.borrow() {
            return AhiruConfig::from_file(std::path::Path::new(path))
                .map_err(|e| runtime_err(span, &e));
        }
    }
    Ok(AhiruConfig::default())
}

pub fn route_meta_from_opts(
    opts: &ValueRef,
    _span: Span,
) -> Result<ahiru_core::RouteMeta, RuntimeError> {
    let map = match &*opts.borrow() {
        Value::Object(m) => m.clone(),
        _ => HashMap::new(),
    };
    let permission = map
        .get("permission")
        .and_then(|v| match &*v.borrow() {
            Value::String(s) => Some(s.clone()),
            _ => None,
        });
    let ws = map
        .get("ws")
        .and_then(|v| match &*v.borrow() {
            Value::Bool(b) => Some(*b),
            _ => None,
        })
        .unwrap_or(false);
    let public = map
        .get("is_public")
        .or_else(|| map.get("public"))
        .and_then(|v| match &*v.borrow() {
            Value::Bool(b) => Some(*b),
            _ => None,
        })
        .unwrap_or(false);
    let body_limit_mb = map.get("body_limit_mb").and_then(|v| match &*v.borrow() {
        Value::Int(n) => Some(*n as u64),
        _ => None,
    });
    let timeout_ms = map.get("timeout_ms").and_then(|v| match &*v.borrow() {
        Value::Int(n) => Some(*n as u64),
        _ => None,
    });
    let stream = map
        .get("stream")
        .and_then(|v| match &*v.borrow() {
            Value::Bool(b) => Some(*b),
            _ => None,
        })
        .unwrap_or(false);
    Ok(ahiru_core::RouteMeta {
        permission,
        ws,
        public,
        body_limit_mb,
        timeout_ms,
        stream,
        schema: None,
        guard: None,
    })
}

pub fn middleware_from_name(
    name: &str,
    opts: &HashMap<String, ValueRef>,
    span: Span,
) -> Result<ahiru_core::MiddlewareKind, RuntimeError> {
    match name.to_lowercase().as_str() {
        "cors" => {
            let origins: Vec<String> = opts
                .get("origins")
                .and_then(|v| match &*v.borrow() {
                    Value::Array(items) => Some(
                        items
                            .iter()
                            .filter_map(|i| match &*i.borrow() {
                                Value::String(s) => Some(s.clone()),
                                _ => None,
                            })
                            .collect(),
                    ),
                    Value::String(s) => Some(vec![s.clone()]),
                    _ => None,
                })
                .unwrap_or_else(|| vec!["*".into()]);
            Ok(ahiru_core::MiddlewareKind::Cors(origins))
        }
        "rate_limit" | "ratelimit" => {
            let rps = opts
                .get("rps")
                .and_then(|v| match &*v.borrow() {
                    Value::Int(n) => Some(*n as u32),
                    _ => None,
                })
                .unwrap_or(100);
            Ok(ahiru_core::MiddlewareKind::RateLimit { rps })
        }
        "request_id" | "requestid" => {
            let enabled = opts
                .get("enabled")
                .and_then(|v| match &*v.borrow() {
                    Value::Bool(b) => Some(*b),
                    _ => None,
                })
                .unwrap_or(true);
            Ok(ahiru_core::MiddlewareKind::RequestId { enabled })
        }
        "logging" | "log" => {
            let enabled = opts
                .get("enabled")
                .and_then(|v| match &*v.borrow() {
                    Value::Bool(b) => Some(*b),
                    _ => None,
                })
                .unwrap_or(true);
            let json = opts.get("json").and_then(|v| match &*v.borrow() {
                Value::Bool(b) => Some(*b),
                _ => None,
            });
            let skip_paths: Vec<String> = opts
                .get("skip")
                .and_then(|v| match &*v.borrow() {
                    Value::Array(items) => Some(
                        items
                            .iter()
                            .filter_map(|i| match &*i.borrow() {
                                Value::String(s) => Some(s.clone()),
                                _ => None,
                            })
                            .collect(),
                    ),
                    _ => None,
                })
                .unwrap_or_default();
            Ok(ahiru_core::MiddlewareKind::Logging(
                ahiru_core::LoggingOptions {
                    enabled,
                    json,
                    skip_paths,
                },
            ))
        }
        "secure_headers" | "helmet" => Ok(ahiru_core::MiddlewareKind::SecureHeaders {
            csp_policy: opts
                .get("csp_policy")
                .and_then(|v| match &*v.borrow() {
                    Value::String(s) => Some(s.clone()),
                    _ => None,
                }),
        }),
        "gzip" | "compression" => Ok(ahiru_core::MiddlewareKind::Compression(
            ahiru_core::CompressionAlgo::Gzip,
        )),
        "brotli" => Ok(ahiru_core::MiddlewareKind::Compression(
            ahiru_core::CompressionAlgo::Brotli,
        )),
        "etag" => Ok(ahiru_core::MiddlewareKind::Etag),
        other => Err(runtime_err(span, &format!("unknown middleware: {other}"))),
    }
}
