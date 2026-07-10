//! r2d2 connection pool builtins.

use super::config::{pool_manager, pool_opts_from_map};
use super::handles::{self, alloc_pooled_conn, alloc_pool};
use crate::{error_from_runtime, error_value, NiaoResult, RuntimeError, Value, ValueRef};
use niao_ast::Span;
use niao_errors::codes;
use r2d2::Pool;
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

pub fn npg_pool(args: &[ValueRef], span: Span) -> NiaoResult<ValueRef> {
    arity(args, 1, "npg_pool", span)?;
    let opts = match &*args[0].borrow() {
        Value::Object(map) => map.clone(),
        other => {
            return Ok(npg_error(
                span,
                format!("npg_pool() expects options object, got {}", other.type_name()),
            ));
        }
    };
    let (config, display, max_size, min_idle, max_lifetime, connection_timeout) =
        pool_opts_from_map(&opts).map_err(|msg| RuntimeError::at(span, codes::E1907_NPG_TLS, msg))?;

    let manager = pool_manager(&config).map_err(|msg| RuntimeError::at(span, codes::E1907_NPG_TLS, msg))?;
    let mut builder = Pool::builder()
        .max_size(max_size)
        .min_idle(Some(min_idle))
        .connection_timeout(connection_timeout);
    if let Some(lifetime) = max_lifetime {
        builder = builder.max_lifetime(Some(lifetime));
    }
    let pool = builder.build(manager).map_err(|e| {
        RuntimeError::at(span, codes::E1907_NPG_TLS, e.to_string())
    })?;

    Ok(ok_int(alloc_pool(pool, display) as i64))
}

pub fn npg_pool_close(args: &[ValueRef], span: Span) -> NiaoResult<ValueRef> {
    arity(args, 1, "npg_pool_close", span)?;
    let id = pool_arg(args, 0, "npg_pool_close", span)?;
    if handles::remove_pool(id).is_some() {
        Ok(ok_nil())
    } else {
        Ok(npg_error(
            span,
            format!("npg_pool_close(): invalid or closed pool handle {id}"),
        ))
    }
}

pub fn npg_pool_get(args: &[ValueRef], span: Span) -> NiaoResult<ValueRef> {
    arity(args, 1, "npg_pool_get", span)?;
    let id = pool_arg(args, 0, "npg_pool_get", span)?;
    handles::with_pool(id, "npg_pool_get", span, |pool_handle| {
        let pooled = pool_handle.pool.get().map_err(|e| e.to_string())?;
        Ok(alloc_pooled_conn(pooled, pool_handle.conninfo.clone()) as i64)
    })
    .map(ok_int)
    .or_else(|e| Ok(error_from_runtime(&e)))
}

pub fn npg_pool_status(args: &[ValueRef], span: Span) -> NiaoResult<ValueRef> {
    arity(args, 1, "npg_pool_status", span)?;
    let id = pool_arg(args, 0, "npg_pool_status", span)?;
    handles::with_pool(id, "npg_pool_status", span, |pool_handle| {
        let state = pool_handle.pool.state();
        let mut map = HashMap::new();
        map.insert("size".to_string(), Value::Int(state.connections as i64).ref_cell());
        map.insert("idle".to_string(), Value::Int(state.idle_connections as i64).ref_cell());
        let in_use = state.connections.saturating_sub(state.idle_connections);
        map.insert("in_use".to_string(), Value::Int(in_use as i64).ref_cell());
        Ok(Value::Object(map))
    })
    .map(|v| v.ref_cell())
    .or_else(|e| Ok(error_from_runtime(&e)))
}
