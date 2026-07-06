//! Prepared statement builtins.

use super::handles::{self, alloc_stmt, remove_stmt};
use super::query::collect_rows;
use super::types::{bound_to_sql_params, neko_to_bound, rewrite_placeholders, sql_param_refs};
use crate::{error_value, NekoResult, RuntimeError, Value, ValueRef};
use neko_ast::Span;
use neko_errors::codes;

use super::common::*;
use super::query::{parse_row_format, RowFormat};

fn npg_error(span: Span, msg: impl Into<String>) -> ValueRef {
    error_value(codes::E1901_NPG_ERROR, "npg_error", msg.into(), span)
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
            codes::E1902_NPG_INVALID_HANDLE,
            format!(
                "{name}() expects statement handle as argument {}, got {}",
                idx + 1,
                other.type_name()
            ),
        )),
    }
}

pub fn npg_prepare(args: &[ValueRef], span: Span) -> NekoResult<ValueRef> {
    arity(args, 2, "npg_prepare", span)?;
    let conn_id = conn_arg(args, 0, "npg_prepare", span)?;
    let sql = string_arg(args, 1, "npg_prepare", span)?;
    let sql = rewrite_placeholders(&sql);
    let result = handles::with_conn_mut(conn_id, "npg_prepare", span, |handle| {
        handle.client_mut().prepare(&sql).map_err(|e| e.to_string())?;
        Ok(())
    });
    match result {
        Ok(()) => Ok(ok_int(alloc_stmt(conn_id, sql) as i64)),
        Err(e) => Ok(crate::error_from_runtime(&e)),
    }
}

pub fn npg_bind(args: &[ValueRef], span: Span) -> NekoResult<ValueRef> {
    arity(args, 3, "npg_bind", span)?;
    let stmt_id = stmt_arg(args, 0, "npg_bind", span)?;
    let index = int_arg(args, 1, "npg_bind", span)?;
    let bound = neko_to_bound(&*args[2].borrow(), span)?;
    handles::with_stmt_mut(stmt_id, "npg_bind", span, |stmt| {
        stmt.params.retain(|(i, _)| *i != index as i32);
        stmt.params.push((index as i32, bound));
        Ok(())
    })
    .map(|_| ok_nil())
    .or_else(|e| Ok(crate::error_from_runtime(&e)))
}

pub fn npg_stmt_exec(args: &[ValueRef], span: Span) -> NekoResult<ValueRef> {
    arity(args, 1, "npg_stmt_exec", span)?;
    let stmt_id = stmt_arg(args, 0, "npg_stmt_exec", span)?;
    handles::with_stmt_and_conn(stmt_id, "npg_stmt_exec", span, |stmt, conn| {
        let sql = stmt.sql.clone();
        let mut params: Vec<super::types::BoundValue> = Vec::new();
        let mut sorted = stmt.params.clone();
        sorted.sort_by_key(|(i, _)| *i);
        for (_, v) in sorted {
            params.push(v);
        }
        let boxes = bound_to_sql_params(&params);
        let refs = sql_param_refs(&boxes);
        let stmt_prepared = conn.client_mut().prepare(&sql).map_err(|e| e.to_string())?;
        let n = conn.client_mut().execute(&stmt_prepared, &refs).map_err(|e| e.to_string())?;
        Ok(n as i64)
    })
    .map(ok_int)
    .or_else(|e| Ok(crate::error_from_runtime(&e)))
}

pub fn npg_stmt_query(args: &[ValueRef], span: Span) -> NekoResult<ValueRef> {
    arity_range(args, 1, 2, "npg_stmt_query", span)?;
    let stmt_id = stmt_arg(args, 0, "npg_stmt_query", span)?;
    let format = if args.len() == 2 {
        parse_row_format(&string_arg(args, 1, "npg_stmt_query", span)?)
            .map_err(|msg| RuntimeError::at(span, codes::E1901_NPG_ERROR, msg))?
    } else {
        RowFormat::Object
    };
    handles::with_stmt_and_conn(stmt_id, "npg_stmt_query", span, |stmt, conn| {
        let sql = stmt.sql.clone();
        let mut params: Vec<super::types::BoundValue> = Vec::new();
        let mut sorted = stmt.params.clone();
        sorted.sort_by_key(|(i, _)| *i);
        for (_, v) in sorted {
            params.push(v);
        }
        let boxes = bound_to_sql_params(&params);
        let refs = sql_param_refs(&boxes);
        let rows = conn.client_mut().query(&sql, &refs).map_err(|e| e.to_string())?;
        collect_rows(rows, format)
    })
    .map(|v| v.ref_cell())
    .or_else(|e| Ok(crate::error_from_runtime(&e)))
}

pub fn npg_stmt_reset(args: &[ValueRef], span: Span) -> NekoResult<ValueRef> {
    arity(args, 1, "npg_stmt_reset", span)?;
    let stmt_id = stmt_arg(args, 0, "npg_stmt_reset", span)?;
    handles::with_stmt_mut(stmt_id, "npg_stmt_reset", span, |stmt| {
        stmt.params.clear();
        Ok(())
    })
    .map(|_| ok_nil())
    .or_else(|e| Ok(crate::error_from_runtime(&e)))
}

pub fn npg_finalize(args: &[ValueRef], span: Span) -> NekoResult<ValueRef> {
    arity(args, 1, "npg_finalize", span)?;
    let stmt_id = stmt_arg(args, 0, "npg_finalize", span)?;
    if remove_stmt(stmt_id).is_some() {
        Ok(ok_nil())
    } else {
        Ok(npg_error(
            span,
            format!("npg_finalize(): invalid statement handle {stmt_id}"),
        ))
    }
}
