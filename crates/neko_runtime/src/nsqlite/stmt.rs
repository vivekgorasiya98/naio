//! Prepared statement builtins.

use super::handles::{self, alloc_stmt, remove_stmt};
use super::query::collect_rows;
use super::types::{apply_stmt_bindings, neko_to_bound};
use crate::{error_value, NekoResult, RuntimeError, Value, ValueRef};
use neko_ast::Span;
use neko_errors::codes;

use super::common::*;
use super::query::{parse_row_format, RowFormat};

fn nsqlite_error(span: Span, msg: impl Into<String>) -> ValueRef {
    error_value(codes::E1701_NSQLITE_ERROR, "nsqlite_error", msg.into(), span)
}

fn ok_nil() -> ValueRef {
    Value::Nil.ref_cell()
}

fn ok_int(n: i64) -> ValueRef {
    Value::Int(n).ref_cell()
}

fn stmt_arg(args: &[ValueRef], idx: usize, name: &str, span: Span) -> NekoResult<u64> {
    match &*args[idx].borrow() {
        Value::Int(id) if *id > 0 => Ok(*id as u64),
        other => Err(RuntimeError::at(
            span,
            codes::E1702_NSQLITE_INVALID_HANDLE,
            format!(
                "{name}() expects statement handle as argument {}, got {}",
                idx + 1,
                other.type_name()
            ),
        )),
    }
}

pub fn nsqlite_prepare(args: &[ValueRef], span: Span) -> NekoResult<ValueRef> {
    arity(args, 2, "nsqlite_prepare", span)?;
    let conn_id = conn_arg(args, 0, "nsqlite_prepare", span)?;
    let sql = string_arg(args, 1, "nsqlite_prepare", span)?;
    // Validate SQL compiles
    let result = handles::with_conn_mut(conn_id, "nsqlite_prepare", span, |handle| {
        handle.conn.prepare(&sql).map_err(|e| e.to_string())?;
        Ok(())
    });
    match result {
        Ok(()) => Ok(ok_int(alloc_stmt(conn_id, sql) as i64)),
        Err(e) => Ok(crate::error_from_runtime(&e)),
    }
}

pub fn nsqlite_bind(args: &[ValueRef], span: Span) -> NekoResult<ValueRef> {
    arity(args, 3, "nsqlite_bind", span)?;
    let stmt_id = stmt_arg(args, 0, "nsqlite_bind", span)?;
    let index = int_arg(args, 1, "nsqlite_bind", span)?;
    let bound = neko_to_bound(&*args[2].borrow(), span)?;
    handles::with_stmt_mut(stmt_id, "nsqlite_bind", span, |stmt| {
        stmt.params.retain(|(i, _)| *i != index as i32);
        stmt.params.push((index as i32, bound));
        Ok(())
    })
    .map(|_| ok_nil())
    .or_else(|e| Ok(crate::error_from_runtime(&e)))
}

pub fn nsqlite_bind_named(args: &[ValueRef], span: Span) -> NekoResult<ValueRef> {
    arity(args, 3, "nsqlite_bind_named", span)?;
    let stmt_id = stmt_arg(args, 0, "nsqlite_bind_named", span)?;
    let name = string_arg(args, 1, "nsqlite_bind_named", span)?;
    let bound = neko_to_bound(&*args[2].borrow(), span)?;
    handles::with_stmt_mut(stmt_id, "nsqlite_bind_named", span, |stmt| {
        stmt.named_params.insert(name, bound);
        Ok(())
    })
    .map(|_| ok_nil())
    .or_else(|e| Ok(crate::error_from_runtime(&e)))
}

pub fn nsqlite_stmt_exec(args: &[ValueRef], span: Span) -> NekoResult<ValueRef> {
    arity(args, 1, "nsqlite_stmt_exec", span)?;
    let stmt_id = stmt_arg(args, 0, "nsqlite_stmt_exec", span)?;
    handles::with_stmt_and_conn(stmt_id, "nsqlite_stmt_exec", span, |stmt, conn| {
        let sql = stmt.sql.clone();
        let params = stmt.params.clone();
        let named = stmt.named_params.clone();
        let handle = super::handles::StmtHandle {
            conn_id: stmt.conn_id,
            sql: sql.clone(),
            params,
            named_params: named,
        };
        let mut prepared = conn.conn.prepare_cached(&sql).map_err(|e| e.to_string())?;
        apply_stmt_bindings(&mut prepared, &handle)?;
        let n = prepared.raw_execute().map_err(|e| e.to_string())?;
        Ok(n as i64)
    })
    .map(ok_int)
    .or_else(|e| Ok(crate::error_from_runtime(&e)))
}

pub fn nsqlite_stmt_query(args: &[ValueRef], span: Span) -> NekoResult<ValueRef> {
    arity_range(args, 1, 2, "nsqlite_stmt_query", span)?;
    let stmt_id = stmt_arg(args, 0, "nsqlite_stmt_query", span)?;
    let format = if args.len() == 2 {
        parse_row_format(&string_arg(args, 1, "nsqlite_stmt_query", span)?)
            .map_err(|msg| RuntimeError::at(span, codes::E1701_NSQLITE_ERROR, msg))?
    } else {
        RowFormat::Object
    };
    handles::with_stmt_and_conn(stmt_id, "nsqlite_stmt_query", span, |stmt, conn| {
        let sql = stmt.sql.clone();
        let params = stmt.params.clone();
        let named = stmt.named_params.clone();
        let handle = super::handles::StmtHandle {
            conn_id: stmt.conn_id,
            sql: sql.clone(),
            params,
            named_params: named,
        };
        let mut prepared = conn.conn.prepare(&sql).map_err(|e| e.to_string())?;
        apply_stmt_bindings(&mut prepared, &handle)?;
        collect_rows(prepared, format).map_err(|e| e.to_string())
    })
    .map(|v| v.ref_cell())
    .or_else(|e| Ok(crate::error_from_runtime(&e)))
}

pub fn nsqlite_stmt_reset(args: &[ValueRef], span: Span) -> NekoResult<ValueRef> {
    arity(args, 1, "nsqlite_stmt_reset", span)?;
    let stmt_id = stmt_arg(args, 0, "nsqlite_stmt_reset", span)?;
    handles::with_stmt_mut(stmt_id, "nsqlite_stmt_reset", span, |stmt| {
        stmt.params.clear();
        stmt.named_params.clear();
        Ok(())
    })
    .map(|_| ok_nil())
    .or_else(|e| Ok(crate::error_from_runtime(&e)))
}

pub fn nsqlite_finalize(args: &[ValueRef], span: Span) -> NekoResult<ValueRef> {
    arity(args, 1, "nsqlite_finalize", span)?;
    let stmt_id = stmt_arg(args, 0, "nsqlite_finalize", span)?;
    if remove_stmt(stmt_id).is_some() {
        Ok(ok_nil())
    } else {
        Ok(nsqlite_error(
            span,
            format!("nsqlite_finalize(): invalid statement handle {stmt_id}"),
        ))
    }
}

fn conn_arg(args: &[ValueRef], idx: usize, name: &str, span: Span) -> NekoResult<u64> {
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
