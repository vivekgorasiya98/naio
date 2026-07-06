//! Async query/execute via shared background task pool.

use super::common::*;
use super::config::connect_url;
use super::handles::{self, ConnHandle, ConnInner};
use super::query::{exec_on_conn, parse_row_format, query_on_conn, RowFormat};
use super::types::value_to_async;
use crate::async_tasks::{spawn_async, task_done, task_result_value, task_wait_loop, with_task, AsyncValue};
use crate::{error_value, NekoResult, RuntimeError, Value, ValueRef};
use neko_ast::Span;
use neko_errors::codes;

fn npg_async_error(span: Span, msg: impl Into<String>) -> ValueRef {
    error_value(codes::E1901_NPG_ERROR, "npg_error", msg.into(), span)
}

fn capture_conninfo(conn_id: u64, span: Span) -> NekoResult<String> {
    handles::conn_info(conn_id).ok_or_else(|| {
        RuntimeError::at(
            span,
            codes::E1902_NPG_INVALID_HANDLE,
            format!("invalid connection handle {conn_id}"),
        )
    })
}

pub fn npg_async_exec(args: &[ValueRef], span: Span) -> NekoResult<ValueRef> {
    arity_range(args, 2, 3, "npg_async_exec", span)?;
    let conn_id = conn_arg(args, 0, "npg_async_exec", span)?;
    let sql = string_arg(args, 1, "npg_async_exec", span)?;
    let params = if args.len() == 3 {
        params_array_arg(args, 2, "npg_async_exec", span)?
    } else {
        Vec::new()
    };
    let conninfo = capture_conninfo(conn_id, span)?;
    let id = spawn_async(move || {
        let client = connect_url(&conninfo).map_err(|e| e.to_string())?;
        let mut handle = ConnHandle {
            inner: ConnInner::Direct(client),
            conninfo: conninfo.clone(),
            in_transaction: false,
        };
        let n = exec_on_conn(&mut handle, &sql, &params)?;
        Ok(AsyncValue::int(n as i64))
    });
    Ok(Value::Int(id as i64).ref_cell())
}

pub fn npg_async_query(args: &[ValueRef], span: Span) -> NekoResult<ValueRef> {
    arity_range(args, 2, 4, "npg_async_query", span)?;
    let conn_id = conn_arg(args, 0, "npg_async_query", span)?;
    let sql = string_arg(args, 1, "npg_async_query", span)?;
    let params = if args.len() >= 3 {
        params_array_arg(args, 2, "npg_async_query", span)?
    } else {
        Vec::new()
    };
    let format = if args.len() >= 4 {
        parse_row_format(&string_arg(args, 3, "npg_async_query", span)?)
            .map_err(|msg| RuntimeError::at(span, codes::E1901_NPG_ERROR, msg))?
    } else {
        RowFormat::Object
    };
    let conninfo = capture_conninfo(conn_id, span)?;
    let id = spawn_async(move || {
        let client = connect_url(&conninfo).map_err(|e| e.to_string())?;
        let mut handle = ConnHandle {
            inner: ConnInner::Direct(client),
            conninfo: conninfo.clone(),
            in_transaction: false,
        };
        let result = query_on_conn(&mut handle, &sql, &params, format)?;
        Ok(value_to_async(&result))
    });
    Ok(Value::Int(id as i64).ref_cell())
}

pub fn npg_task_done(args: &[ValueRef], span: Span) -> NekoResult<ValueRef> {
    arity(args, 1, "npg_task_done", span)?;
    let id = task_arg(args, 0, "npg_task_done", span)?;
    with_task(
        id,
        "npg_task_done",
        span,
        codes::E1905_NPG_TASK_NOT_FOUND,
        "npg task cancelled",
        |s, m| npg_async_error(s, m),
        |state| Ok(Value::Bool(task_done(state)).ref_cell()),
    )
}

pub fn npg_task_wait(args: &[ValueRef], span: Span) -> NekoResult<ValueRef> {
    arity(args, 1, "npg_task_wait", span)?;
    let id = task_arg(args, 0, "npg_task_wait", span)?;
    task_wait_loop(id);
    with_task(
        id,
        "npg_task_wait",
        span,
        codes::E1905_NPG_TASK_NOT_FOUND,
        "npg task cancelled",
        |s, m| npg_async_error(s, m),
        |_| Ok(Value::Nil.ref_cell()),
    )
}

pub fn npg_task_result(args: &[ValueRef], span: Span) -> NekoResult<ValueRef> {
    arity(args, 1, "npg_task_result", span)?;
    let id = task_arg(args, 0, "npg_task_result", span)?;
    with_task(
        id,
        "npg_task_result",
        span,
        codes::E1905_NPG_TASK_NOT_FOUND,
        "npg task cancelled",
        |s, m| npg_async_error(s, m),
        |state| Ok(task_result_value(state, span, "npg task cancelled", |s, m| npg_async_error(s, m))),
    )
}

pub fn npg_task_cancel(args: &[ValueRef], span: Span) -> NekoResult<ValueRef> {
    arity(args, 1, "npg_task_cancel", span)?;
    let id = task_arg(args, 0, "npg_task_cancel", span)?;
    let cancelled = crate::async_tasks::cancel_task(id, span, codes::E1905_NPG_TASK_NOT_FOUND)?;
    Ok(Value::Bool(cancelled).ref_cell())
}

fn task_arg(args: &[ValueRef], idx: usize, name: &str, span: Span) -> NekoResult<u64> {
    match &*args[idx].borrow() {
        Value::Int(id) if *id > 0 => Ok(*id as u64),
        other => Err(RuntimeError::at(
            span,
            codes::E1900_NPG_ARITY,
            format!(
                "{name}() expects task id as argument {}, got {}",
                idx + 1,
                other.type_name()
            ),
        )),
    }
}
