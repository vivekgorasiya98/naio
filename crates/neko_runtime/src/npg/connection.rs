//! Connection open/close, configure, and transaction builtins.

use super::config::{connect_config, connect_url, parse_connect_opts};
use super::handles::{self, alloc_conn, remove_conn};
use crate::{error_from_runtime, error_value, NekoResult, Value, ValueRef};
use neko_ast::Span;
use neko_errors::codes;
use std::collections::HashMap;

use super::common::*;

fn npg_error(span: Span, msg: impl Into<String>) -> ValueRef {
    error_value(codes::E1901_NPG_ERROR, "npg_error", msg.into(), span)
}

fn ok_nil() -> ValueRef {
    Value::Nil.ref_cell()
}

fn ok_int(n: i64) -> ValueRef {
    Value::Int(n).ref_cell()
}

fn ok_bool(b: bool) -> ValueRef {
    Value::Bool(b).ref_cell()
}

fn ok_string(s: impl Into<String>) -> ValueRef {
    Value::String(s.into()).ref_cell()
}

pub fn npg_connect(args: &[ValueRef], span: Span) -> NekoResult<ValueRef> {
    arity(args, 1, "npg_connect", span)?;
    let url = string_arg(args, 0, "npg_connect", span)?;
    match connect_and_register(&url, span) {
        Ok(id) => Ok(ok_int(id as i64)),
        Err(msg) => Ok(npg_error(span, msg)),
    }
}

pub fn npg_connect_opts(args: &[ValueRef], span: Span) -> NekoResult<ValueRef> {
    arity(args, 1, "npg_connect_opts", span)?;
    let (config, display) = parse_connect_opts(&args[0], span)?;
    match connect_config(&config) {
        Ok(client) => Ok(ok_int(alloc_conn(client, display) as i64)),
        Err(msg) => Ok(error_value(codes::E1907_NPG_TLS, "npg_error", msg, span)),
    }
}

fn connect_and_register(url: &str, _span: Span) -> Result<u64, String> {
    let client = connect_url(url)?;
    let display = handles::redact_conninfo(url);
    Ok(alloc_conn(client, display))
}

pub fn npg_close(args: &[ValueRef], span: Span) -> NekoResult<ValueRef> {
    arity(args, 1, "npg_close", span)?;
    let id = conn_arg(args, 0, "npg_close", span)?;
    if remove_conn(id).is_some() {
        Ok(ok_nil())
    } else {
        Ok(npg_error(
            span,
            format!("npg_close(): invalid or closed connection handle {id}"),
        ))
    }
}

pub fn npg_ping(args: &[ValueRef], span: Span) -> NekoResult<ValueRef> {
    arity(args, 1, "npg_ping", span)?;
    let id = conn_arg(args, 0, "npg_ping", span)?;
    handles::with_conn_mut(id, "npg_ping", span, |handle| {
        handle.client_mut()
            .query_one("SELECT 1", &[])
            .map(|_| ())
            .map_err(|e| e.to_string())
    })
    .map(|_| ok_bool(true))
    .or_else(|e| Ok(error_from_runtime(&e)))
}

pub fn npg_configure(args: &[ValueRef], span: Span) -> NekoResult<ValueRef> {
    arity(args, 2, "npg_configure", span)?;
    let id = conn_arg(args, 0, "npg_configure", span)?;
    let opts = object_arg(args, 1, "npg_configure", span)?;
    handles::with_conn_mut(id, "npg_configure", span, |handle| {
        for (key, val) in opts {
            let val_ref = &*val.borrow();
            match key.as_str() {
                "statement_timeout" => match val_ref {
                    Value::Int(ms) if *ms >= 0 => {
                        handle.client_mut()
                            .batch_execute(&format!("SET statement_timeout = {ms}"))
                            .map_err(|e| e.to_string())?;
                    }
                    other => {
                        return Err(format!(
                            "statement_timeout expects non-negative int (ms), got {}",
                            other.type_name()
                        ));
                    }
                },
                "lock_timeout" => match val_ref {
                    Value::Int(ms) if *ms >= 0 => {
                        handle.client_mut()
                            .batch_execute(&format!("SET lock_timeout = {ms}"))
                            .map_err(|e| e.to_string())?;
                    }
                    other => {
                        return Err(format!(
                            "lock_timeout expects non-negative int (ms), got {}",
                            other.type_name()
                        ));
                    }
                },
                "search_path" => match val_ref {
                    Value::String(s) => {
                        handle.client_mut()
                            .execute("SELECT set_config('search_path', $1, false)", &[&s.as_str()])
                            .map_err(|e| e.to_string())?;
                    }
                    other => {
                        return Err(format!("search_path expects string, got {}", other.type_name()));
                    }
                },
                "timezone" => match val_ref {
                    Value::String(s) => {
                        handle.client_mut()
                            .execute("SET TIME ZONE $1", &[&s.as_str()])
                            .map_err(|e| e.to_string())?;
                    }
                    other => {
                        return Err(format!("timezone expects string, got {}", other.type_name()));
                    }
                },
                other => return Err(format!("unknown configure option \"{other}\"")),
            }
        }
        Ok(())
    })
    .map(|_| ok_nil())
    .or_else(|e| Ok(error_from_runtime(&e)))
}

pub fn npg_conninfo(args: &[ValueRef], span: Span) -> NekoResult<ValueRef> {
    arity(args, 1, "npg_conninfo", span)?;
    let id = conn_arg(args, 0, "npg_conninfo", span)?;
    match handles::conn_info(id) {
        Some(p) => Ok(ok_string(p)),
        None => Ok(npg_error(
            span,
            format!("npg_conninfo(): invalid connection handle {id}"),
        )),
    }
}

pub fn npg_server_version(args: &[ValueRef], span: Span) -> NekoResult<ValueRef> {
    arity(args, 1, "npg_server_version", span)?;
    let id = conn_arg(args, 0, "npg_server_version", span)?;
    handles::with_conn_mut(id, "npg_server_version", span, |handle| {
        let row = handle.client_mut()
            .query_one("SHOW server_version", &[])
            .map_err(|e| e.to_string())?;
        Ok(row.get::<_, String>(0))
    })
    .map(ok_string)
    .or_else(|e| Ok(error_from_runtime(&e)))
}

pub fn npg_is_in_transaction(args: &[ValueRef], span: Span) -> NekoResult<ValueRef> {
    arity(args, 1, "npg_is_in_transaction", span)?;
    let id = conn_arg(args, 0, "npg_is_in_transaction", span)?;
    handles::with_conn_mut(id, "npg_is_in_transaction", span, |handle| Ok(handle.in_transaction))
        .map(ok_bool)
        .or_else(|e| Ok(error_from_runtime(&e)))
}

pub fn npg_begin(args: &[ValueRef], span: Span) -> NekoResult<ValueRef> {
    arity_range(args, 1, 2, "npg_begin", span)?;
    let id = conn_arg(args, 0, "npg_begin", span)?;
    let opts = if args.len() == 2 {
        object_arg(args, 1, "npg_begin", span)?
    } else {
        HashMap::new()
    };
    handles::with_conn_mut(id, "npg_begin", span, |handle| {
        let mut parts = vec!["BEGIN".to_string()];
        if let Some(iso_ref) = opts.get("isolation") {
            let iso = match &*iso_ref.borrow() {
                Value::String(s) => s.clone(),
                other => {
                    return Err(format!(
                        "isolation expects string, got {}",
                        other.type_name()
                    ));
                }
            };
            parts.push("ISOLATION LEVEL".to_string());
            parts.push(match iso.to_lowercase().as_str() {
                "read committed" | "read_committed" => "READ COMMITTED".to_string(),
                "repeatable read" | "repeatable_read" => "REPEATABLE READ".to_string(),
                "serializable" => "SERIALIZABLE".to_string(),
                other => return Err(format!("unknown isolation level \"{other}\"")),
            });
        }
        if let Some(ro_ref) = opts.get("read_only") {
            if bool_from_value(&*ro_ref.borrow())? {
                parts.push("READ ONLY".to_string());
            }
        }
        if let Some(def_ref) = opts.get("deferrable") {
            if bool_from_value(&*def_ref.borrow())? {
                parts.push("DEFERRABLE".to_string());
            }
        }
        handle.client_mut()
            .batch_execute(&parts.join(" "))
            .map_err(|e| e.to_string())?;
        handle.in_transaction = true;
        Ok(())
    })
    .map(|_| ok_nil())
    .or_else(|e| Ok(error_from_runtime(&e)))
}

pub fn npg_commit(args: &[ValueRef], span: Span) -> NekoResult<ValueRef> {
    arity(args, 1, "npg_commit", span)?;
    let id = conn_arg(args, 0, "npg_commit", span)?;
    handles::with_conn_mut(id, "npg_commit", span, |handle| {
        handle.client_mut().batch_execute("COMMIT").map_err(|e| e.to_string())?;
        handle.in_transaction = false;
        Ok(())
    })
    .map(|_| ok_nil())
    .or_else(|e| Ok(error_from_runtime(&e)))
}

pub fn npg_rollback(args: &[ValueRef], span: Span) -> NekoResult<ValueRef> {
    arity(args, 1, "npg_rollback", span)?;
    let id = conn_arg(args, 0, "npg_rollback", span)?;
    handles::with_conn_mut(id, "npg_rollback", span, |handle| {
        handle.client_mut().batch_execute("ROLLBACK").map_err(|e| e.to_string())?;
        handle.in_transaction = false;
        Ok(())
    })
    .map(|_| ok_nil())
    .or_else(|e| Ok(error_from_runtime(&e)))
}

pub fn npg_savepoint(args: &[ValueRef], span: Span) -> NekoResult<ValueRef> {
    arity(args, 2, "npg_savepoint", span)?;
    let id = conn_arg(args, 0, "npg_savepoint", span)?;
    let name = string_arg(args, 1, "npg_savepoint", span)?;
    let ident = super::types::quote_ident(&name);
    handles::with_conn_mut(id, "npg_savepoint", span, |handle| {
        handle.client_mut()
            .batch_execute(&format!("SAVEPOINT {ident}"))
            .map_err(|e| e.to_string())
    })
    .map(|_| ok_nil())
    .or_else(|e| Ok(error_from_runtime(&e)))
}

pub fn npg_rollback_to(args: &[ValueRef], span: Span) -> NekoResult<ValueRef> {
    arity(args, 2, "npg_rollback_to", span)?;
    let id = conn_arg(args, 0, "npg_rollback_to", span)?;
    let name = string_arg(args, 1, "npg_rollback_to", span)?;
    let ident = super::types::quote_ident(&name);
    handles::with_conn_mut(id, "npg_rollback_to", span, |handle| {
        handle.client_mut()
            .batch_execute(&format!("ROLLBACK TO SAVEPOINT {ident}"))
            .map_err(|e| e.to_string())
    })
    .map(|_| ok_nil())
    .or_else(|e| Ok(error_from_runtime(&e)))
}

pub fn npg_version(args: &[ValueRef], span: Span) -> NekoResult<ValueRef> {
    arity(args, 0, "npg_version", span)?;
    Ok(ok_string(env!("CARGO_PKG_VERSION")))
}

pub fn npg_escape_literal(args: &[ValueRef], span: Span) -> NekoResult<ValueRef> {
    arity(args, 1, "npg_escape_literal", span)?;
    let s = string_arg(args, 0, "npg_escape_literal", span)?;
    Ok(ok_string(super::types::quote_literal(&s)))
}

pub fn npg_quote_ident(args: &[ValueRef], span: Span) -> NekoResult<ValueRef> {
    arity(args, 1, "npg_quote_ident", span)?;
    let s = string_arg(args, 0, "npg_quote_ident", span)?;
    Ok(ok_string(super::types::quote_ident(&s)))
}
