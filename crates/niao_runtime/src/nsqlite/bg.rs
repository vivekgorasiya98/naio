//! Async query/execute via shared background task pool.

use super::common::*;
use super::handles::{apply_default_pragmas, open_connection};
use super::query::{exec_on_conn, parse_row_format, query_on_conn, RowFormat};
use super::types::value_to_async;
use crate::async_tasks::{spawn_async, task_done, task_result_value, task_wait_loop, with_task, AsyncValue};
use crate::{error_value, NiaoResult, RuntimeError, Value, ValueRef};
use niao_ast::Span;
use niao_errors::codes;

fn nsqlite_async_error(span: Span, msg: impl Into<String>) -> ValueRef {
    error_value(codes::E1701_NSQLITE_ERROR, "nsqlite_error", msg.into(), span)
}

fn conn_arg(args: &[ValueRef], idx: usize, name: &str, span: Span) -> NiaoResult<u64> {
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

fn capture_db_path(conn_id: u64, span: Span) -> NiaoResult<String> {
    super::handles::conn_path(conn_id).ok_or_else(|| {
        RuntimeError::at(
            span,
            codes::E1702_NSQLITE_INVALID_HANDLE,
            format!("invalid connection handle {conn_id}"),
        )
    })
}

pub fn nsqlite_async_exec(args: &[ValueRef], span: Span) -> NiaoResult<ValueRef> {
    arity(args, 2, "nsqlite_async_exec", span)?;
    let conn_id = conn_arg(args, 0, "nsqlite_async_exec", span)?;
    let sql = string_arg(args, 1, "nsqlite_async_exec", span)?;
    let path = capture_db_path(conn_id, span)?;
    let id = spawn_async(move || {
        let (conn, _) = open_connection(&path, false).map_err(|e| e.to_string())?;
        if path != ":memory:" {
            apply_default_pragmas(&conn)?;
        }
        let mut handle = super::handles::ConnHandle {
            conn,
            path: path.clone(),
        };
        exec_on_conn(&mut handle, &sql, &[])?;
        Ok(AsyncValue::int(handle.conn.changes() as i64))
    });
    Ok(Value::Int(id as i64).ref_cell())
}

pub fn nsqlite_async_query(args: &[ValueRef], span: Span) -> NiaoResult<ValueRef> {
    arity_range(args, 2, 4, "nsqlite_async_query", span)?;
    let conn_id = conn_arg(args, 0, "nsqlite_async_query", span)?;
    let sql = string_arg(args, 1, "nsqlite_async_query", span)?;
    let params = if args.len() >= 3 {
        super::common::params_array_arg(args, 2, "nsqlite_async_query", span)?
    } else {
        Vec::new()
    };
    let format = if args.len() >= 4 {
        parse_row_format(&string_arg(args, 3, "nsqlite_async_query", span)?)
            .map_err(|msg| RuntimeError::at(span, codes::E1701_NSQLITE_ERROR, msg))?
    } else {
        RowFormat::Object
    };
    let path = capture_db_path(conn_id, span)?;
    let id = spawn_async(move || {
        let (conn, _) = open_connection(&path, false).map_err(|e| e.to_string())?;
        if path != ":memory:" {
            apply_default_pragmas(&conn)?;
        }
        let mut handle = super::handles::ConnHandle {
            conn,
            path: path.clone(),
        };
        let result = query_on_conn(&mut handle, &sql, &params, format)?;
        Ok(value_to_async(&result))
    });
    Ok(Value::Int(id as i64).ref_cell())
}

pub fn nsqlite_task_done(args: &[ValueRef], span: Span) -> NiaoResult<ValueRef> {
    arity(args, 1, "nsqlite_task_done", span)?;
    let id = task_arg(args, 0, "nsqlite_task_done", span)?;
    with_task(
        id,
        "nsqlite_task_done",
        span,
        codes::E1705_NSQLITE_TASK_NOT_FOUND,
        "nsqlite task cancelled",
        |s, m| nsqlite_async_error(s, m),
        |state| Ok(Value::Bool(task_done(state)).ref_cell()),
    )
}

pub fn nsqlite_task_wait(args: &[ValueRef], span: Span) -> NiaoResult<ValueRef> {
    arity(args, 1, "nsqlite_task_wait", span)?;
    let id = task_arg(args, 0, "nsqlite_task_wait", span)?;
    task_wait_loop(id);
    with_task(
        id,
        "nsqlite_task_wait",
        span,
        codes::E1705_NSQLITE_TASK_NOT_FOUND,
        "nsqlite task cancelled",
        |s, m| nsqlite_async_error(s, m),
        |_| Ok(Value::Nil.ref_cell()),
    )
}

pub fn nsqlite_task_result(args: &[ValueRef], span: Span) -> NiaoResult<ValueRef> {
    arity(args, 1, "nsqlite_task_result", span)?;
    let id = task_arg(args, 0, "nsqlite_task_result", span)?;
    with_task(
        id,
        "nsqlite_task_result",
        span,
        codes::E1705_NSQLITE_TASK_NOT_FOUND,
        "nsqlite task cancelled",
        |s, m| nsqlite_async_error(s, m),
        |state| Ok(task_result_value(state, span, "nsqlite task cancelled", |s, m| nsqlite_async_error(s, m))),
    )
}

pub fn nsqlite_task_cancel(args: &[ValueRef], span: Span) -> NiaoResult<ValueRef> {
    arity(args, 1, "nsqlite_task_cancel", span)?;
    let id = task_arg(args, 0, "nsqlite_task_cancel", span)?;
    let cancelled = crate::async_tasks::cancel_task(id, span, codes::E1705_NSQLITE_TASK_NOT_FOUND)?;
    Ok(Value::Bool(cancelled).ref_cell())
}

fn task_arg(args: &[ValueRef], idx: usize, name: &str, span: Span) -> NiaoResult<u64> {
    match &*args[idx].borrow() {
        Value::Int(id) if *id > 0 => Ok(*id as u64),
        other => Err(RuntimeError::at(
            span,
            codes::E1700_NSQLITE_ARITY,
            format!(
                "{name}() expects task id as argument {}, got {}",
                idx + 1,
                other.type_name()
            ),
        )),
    }
}
