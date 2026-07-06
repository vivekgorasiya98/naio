//! PostgreSQL-specific: LISTEN/NOTIFY, advisory locks, COPY.

use postgres::fallible_iterator::FallibleIterator;
use crate::{error_from_runtime, error_value, NekoResult, Value, ValueRef};
use neko_ast::Span;
use neko_errors::codes;
use std::collections::HashMap;
use std::time::Duration;

use super::common::*;
use super::handles;

fn npg_error(span: Span, msg: impl Into<String>) -> ValueRef {
    error_value(codes::E1901_NPG_ERROR, "npg_error", msg.into(), span)
}

fn ok_nil() -> ValueRef {
    Value::Nil.ref_cell()
}

fn ok_bool(b: bool) -> ValueRef {
    Value::Bool(b).ref_cell()
}

pub fn npg_listen(args: &[ValueRef], span: Span) -> NekoResult<ValueRef> {
    arity(args, 2, "npg_listen", span)?;
    let id = conn_arg(args, 0, "npg_listen", span)?;
    let channel = string_arg(args, 1, "npg_listen", span)?;
    handles::with_conn_mut(id, "npg_listen", span, |handle| {
        handle
            .client_mut()
            .batch_execute(&format!("LISTEN {}", super::types::quote_ident(&channel)))
            .map_err(|e| e.to_string())
    })
    .map(|_| ok_nil())
    .or_else(|e| Ok(error_from_runtime(&e)))
}

pub fn npg_unlisten(args: &[ValueRef], span: Span) -> NekoResult<ValueRef> {
    arity_range(args, 1, 2, "npg_unlisten", span)?;
    let id = conn_arg(args, 0, "npg_unlisten", span)?;
    let sql = if args.len() == 2 {
        let channel = string_arg(args, 1, "npg_unlisten", span)?;
        format!("UNLISTEN {}", super::types::quote_ident(&channel))
    } else {
        "UNLISTEN *".to_string()
    };
    handles::with_conn_mut(id, "npg_unlisten", span, |handle| {
        handle.client_mut().batch_execute(&sql).map_err(|e| e.to_string())
    })
    .map(|_| ok_nil())
    .or_else(|e| Ok(error_from_runtime(&e)))
}

pub fn npg_notify(args: &[ValueRef], span: Span) -> NekoResult<ValueRef> {
    arity_range(args, 2, 3, "npg_notify", span)?;
    let id = conn_arg(args, 0, "npg_notify", span)?;
    let channel = string_arg(args, 1, "npg_notify", span)?;
    let payload = if args.len() == 3 {
        string_arg(args, 2, "npg_notify", span)?
    } else {
        String::new()
    };
    handles::with_conn_mut(id, "npg_notify", span, |handle| {
        handle
            .client_mut()
            .execute("SELECT pg_notify($1, $2)", &[&channel, &payload])
            .map_err(|e| e.to_string())?;
        Ok(())
    })
    .map(|_| ok_nil())
    .or_else(|e| Ok(error_from_runtime(&e)))
}

pub fn npg_poll_notify(args: &[ValueRef], span: Span) -> NekoResult<ValueRef> {
    arity_range(args, 1, 2, "npg_poll_notify", span)?;
    let id = conn_arg(args, 0, "npg_poll_notify", span)?;
    let timeout_ms = if args.len() == 2 {
        int_arg(args, 1, "npg_poll_notify", span)?
    } else {
        0
    };
    handles::with_conn_mut(id, "npg_poll_notify", span, |handle| {
        if timeout_ms > 0 {
            let _ = handle.client_mut().execute("SELECT 1", &[]);
            std::thread::sleep(Duration::from_millis(timeout_ms as u64));
        } else {
            let _ = handle.client_mut().execute("SELECT 1", &[]);
        }
        let mut out = Vec::new();
        let mut notes = handle.client_mut().notifications();
        let mut iter = notes.iter();
        loop {
            match iter.next() {
                Ok(Some(n)) => {
                    let mut map = HashMap::new();
                    map.insert("channel".to_string(), Value::String(n.channel().to_string()).ref_cell());
                    map.insert(
                        "payload".to_string(),
                        Value::String(n.payload().to_string()).ref_cell(),
                    );
                    map.insert("pid".to_string(), Value::Int(n.process_id() as i64).ref_cell());
                    out.push(Value::Object(map).ref_cell());
                }
                Ok(None) => break,
                Err(e) => return Err(e.to_string()),
            }
        }
        Ok(Value::Array(out))
    })
    .map(|v| v.ref_cell())
    .or_else(|e| Ok(error_from_runtime(&e)))
}

pub fn npg_advisory_lock(args: &[ValueRef], span: Span) -> NekoResult<ValueRef> {
    arity(args, 2, "npg_advisory_lock", span)?;
    let id = conn_arg(args, 0, "npg_advisory_lock", span)?;
    let key = int_arg(args, 1, "npg_advisory_lock", span)?;
    handles::with_conn_mut(id, "npg_advisory_lock", span, |handle| {
        let row = handle
            .client_mut()
            .query_one("SELECT pg_advisory_lock($1)", &[&key])
            .map_err(|e| e.to_string())?;
        Ok(row.get::<_, bool>(0))
    })
    .map(ok_bool)
    .or_else(|e| Ok(error_from_runtime(&e)))
}

pub fn npg_advisory_unlock(args: &[ValueRef], span: Span) -> NekoResult<ValueRef> {
    arity(args, 2, "npg_advisory_unlock", span)?;
    let id = conn_arg(args, 0, "npg_advisory_unlock", span)?;
    let key = int_arg(args, 1, "npg_advisory_unlock", span)?;
    handles::with_conn_mut(id, "npg_advisory_unlock", span, |handle| {
        let row = handle
            .client_mut()
            .query_one("SELECT pg_advisory_unlock($1)", &[&key])
            .map_err(|e| e.to_string())?;
        Ok(row.get::<_, bool>(0))
    })
    .map(ok_bool)
    .or_else(|e| Ok(error_from_runtime(&e)))
}

pub fn npg_copy_from(args: &[ValueRef], span: Span) -> NekoResult<ValueRef> {
    arity(args, 4, "npg_copy_from", span)?;
    let id = conn_arg(args, 0, "npg_copy_from", span)?;
    let table = string_arg(args, 1, "npg_copy_from", span)?;
    let columns_val = &*args[2].borrow();
    let rows_val = &*args[3].borrow();
    let columns: Vec<String> = match columns_val {
        Value::Array(items) => {
            let mut out = Vec::new();
            for item in items {
                match &*item.borrow() {
                    Value::String(s) => out.push(s.clone()),
                    other => {
                        return Ok(npg_error(
                            span,
                            format!(
                                "npg_copy_from() expects array of column names, got {}",
                                other.type_name()
                            ),
                        ));
                    }
                }
            }
            out
        }
        other => {
            return Ok(npg_error(
                span,
                format!(
                    "npg_copy_from() expects columns array as argument 3, got {}",
                    other.type_name()
                ),
            ));
        }
    };
    let rows: Vec<Vec<String>> = match rows_val {
        Value::Array(outer) => {
            let mut rows = Vec::with_capacity(outer.len());
            for row_ref in outer {
                match &*row_ref.borrow() {
                    Value::Array(cells) => {
                        let mut row = Vec::with_capacity(cells.len());
                        for cell in cells {
                            match &*cell.borrow() {
                                Value::String(s) => row.push(s.clone()),
                                Value::Int(n) => row.push(n.to_string()),
                                Value::Float(f) => row.push(f.to_string()),
                                Value::Bool(b) => row.push(b.to_string()),
                                Value::Nil => row.push(String::new()),
                                other => {
                                    return Ok(npg_error(
                                        span,
                                        format!(
                                            "npg_copy_from() row cells must be string/scalar, got {}",
                                            other.type_name()
                                        ),
                                    ));
                                }
                            }
                        }
                        rows.push(row);
                    }
                    other => {
                        return Ok(npg_error(
                            span,
                            format!(
                                "npg_copy_from() expects array of row arrays, got {}",
                                other.type_name()
                            ),
                        ));
                    }
                }
            }
            rows
        }
        other => {
            return Ok(npg_error(
                span,
                format!(
                    "npg_copy_from() expects rows array as argument 4, got {}",
                    other.type_name()
                ),
            ));
        }
    };
    handles::with_conn_mut(id, "npg_copy_from", span, |handle| {
        super::query::copy_from_on_conn(handle, None, &table, &columns, &rows)
    })
    .map(|n| Value::Int(n as i64).ref_cell())
    .or_else(|e| Ok(error_from_runtime(&e)))
}
