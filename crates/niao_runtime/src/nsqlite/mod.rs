//! Native nsqlite standard library — fast SQLite via rusqlite.

mod bg;
mod common;
mod connection;
pub(crate) mod handles;
mod query;
mod schema;
mod stmt;
pub(crate) mod types;

use crate::{error_from_runtime, error_value, NativeFn, NiaoResult, RuntimeError, Value, ValueRef};
use common::*;
use connection::{
    nsqlite_backup, nsqlite_begin, nsqlite_changes, nsqlite_close, nsqlite_commit, nsqlite_configure,
    nsqlite_last_insert_rowid, nsqlite_open, nsqlite_open_abs, nsqlite_path, nsqlite_rollback,
    nsqlite_vacuum, nsqlite_version,
};
use bg::{
    nsqlite_async_exec, nsqlite_async_query, nsqlite_task_cancel, nsqlite_task_done,
    nsqlite_task_result, nsqlite_task_wait,
};
use stmt::{
    nsqlite_bind, nsqlite_bind_named, nsqlite_finalize, nsqlite_prepare, nsqlite_stmt_exec,
    nsqlite_stmt_query, nsqlite_stmt_reset,
};
use niao_ast::Span;
use niao_errors::codes;
use query::{batch_on_conn, exec_on_conn, query_column_on_conn, query_on_conn, query_row_on_conn,
    query_value_on_conn, RowFormat};
use schema::{list_indexes, list_tables, parse_migrations, run_migrations, table_exists, table_info};
use std::collections::HashMap;
use std::rc::Rc;

fn nsqlite_error(span: Span, msg: impl Into<String>) -> ValueRef {
    error_value(codes::E1701_NSQLITE_ERROR, "nsqlite_error", msg.into(), span)
}

fn ok_int(n: i64) -> ValueRef {
    Value::Int(n).ref_cell()
}

fn ok_bool(b: bool) -> ValueRef {
    Value::Bool(b).ref_cell()
}

fn nsqlite_exec(args: &[ValueRef], span: Span) -> NiaoResult<ValueRef> {
    arity_range(args, 2, 3, "nsqlite_exec", span)?;
    let id = conn_arg(args, 0, "nsqlite_exec", span)?;
    let sql = string_arg(args, 1, "nsqlite_exec", span)?;
    let params = if args.len() == 3 {
        params_array_arg(args, 2, "nsqlite_exec", span)?
    } else {
        Vec::new()
    };
    handles::with_conn_mut(id, "nsqlite_exec", span, |handle| {
        exec_on_conn(handle, &sql, &params)?;
        Ok(handle.conn.changes() as i64)
    })
    .map(ok_int)
    .or_else(|e| Ok(error_from_runtime(&e)))
}

fn nsqlite_exec_many(args: &[ValueRef], span: Span) -> NiaoResult<ValueRef> {
    arity(args, 2, "nsqlite_exec_many", span)?;
    let id = conn_arg(args, 0, "nsqlite_exec_many", span)?;
    let statements = sql_list_arg(args, 1, "nsqlite_exec_many", span)?;
    handles::with_conn_mut(id, "nsqlite_exec_many", span, |handle| {
        let tx = handle.conn.transaction().map_err(|e| e.to_string())?;
        for sql in &statements {
            tx.execute(sql.as_str(), []).map_err(|e| e.to_string())?;
        }
        tx.commit().map_err(|e| e.to_string())?;
        Ok(statements.len() as i64)
    })
    .map(ok_int)
    .or_else(|e| Ok(error_from_runtime(&e)))
}

fn nsqlite_migrate(args: &[ValueRef], span: Span) -> NiaoResult<ValueRef> {
    arity(args, 2, "nsqlite_migrate", span)?;
    let id = conn_arg(args, 0, "nsqlite_migrate", span)?;
    let migrations = parse_migrations(&args[1], span)?;
    handles::with_conn_mut(id, "nsqlite_migrate", span, |handle| {
        run_migrations(handle, &migrations).map_err(|e| e.to_string())
    })
    .map(ok_int)
    .or_else(|e| Ok(error_from_runtime(&e)))
}

fn nsqlite_table_exists(args: &[ValueRef], span: Span) -> NiaoResult<ValueRef> {
    arity(args, 2, "nsqlite_table_exists", span)?;
    let id = conn_arg(args, 0, "nsqlite_table_exists", span)?;
    let name = string_arg(args, 1, "nsqlite_table_exists", span)?;
    handles::with_conn_mut(id, "nsqlite_table_exists", span, |handle| table_exists(handle, &name))
        .map(ok_bool)
        .or_else(|e| Ok(error_from_runtime(&e)))
}

fn nsqlite_list_tables(args: &[ValueRef], span: Span) -> NiaoResult<ValueRef> {
    arity(args, 1, "nsqlite_list_tables", span)?;
    let id = conn_arg(args, 0, "nsqlite_list_tables", span)?;
    handles::with_conn_mut(id, "nsqlite_list_tables", span, |handle| {
        list_tables(handle).map(|names| {
            Value::Array(names.into_iter().map(|n| Value::String(n).ref_cell()).collect())
        })
    })
    .map(|v| v.ref_cell())
    .or_else(|e| Ok(error_from_runtime(&e)))
}

fn nsqlite_table_info(args: &[ValueRef], span: Span) -> NiaoResult<ValueRef> {
    arity(args, 2, "nsqlite_table_info", span)?;
    let id = conn_arg(args, 0, "nsqlite_table_info", span)?;
    let table = string_arg(args, 1, "nsqlite_table_info", span)?;
    handles::with_conn_mut(id, "nsqlite_table_info", span, |handle| {
        table_info(handle, &table).map(Value::Array)
    })
    .map(|v| v.ref_cell())
    .or_else(|e| Ok(error_from_runtime(&e)))
}

fn nsqlite_list_indexes(args: &[ValueRef], span: Span) -> NiaoResult<ValueRef> {
    arity_range(args, 1, 2, "nsqlite_list_indexes", span)?;
    let id = conn_arg(args, 0, "nsqlite_list_indexes", span)?;
    let table = if args.len() == 2 {
        Some(string_arg(args, 1, "nsqlite_list_indexes", span)?)
    } else {
        None
    };
    handles::with_conn_mut(id, "nsqlite_list_indexes", span, |handle| {
        list_indexes(handle, table.as_deref()).map(Value::Array)
    })
    .map(|v| v.ref_cell())
    .or_else(|e| Ok(error_from_runtime(&e)))
}

fn nsqlite_query(args: &[ValueRef], span: Span) -> NiaoResult<ValueRef> {
    arity_range(args, 2, 4, "nsqlite_query", span)?;
    let id = conn_arg(args, 0, "nsqlite_query", span)?;
    let sql = string_arg(args, 1, "nsqlite_query", span)?;
    let (params, format) = if args.len() >= 3 {
        let params = params_array_arg(args, 2, "nsqlite_query", span)?;
        let format = if args.len() == 4 {
            query::parse_row_format(&string_arg(args, 3, "nsqlite_query", span)?)
                .map_err(|msg| RuntimeError::at(span, codes::E1701_NSQLITE_ERROR, msg))?
        } else {
            RowFormat::Object
        };
        (params, format)
    } else {
        (Vec::new(), RowFormat::Object)
    };
    handles::with_conn_mut(id, "nsqlite_query", span, |handle| {
        query_on_conn(handle, &sql, &params, format)
    })
    .map(|v| v.ref_cell())
    .or_else(|e| Ok(error_from_runtime(&e)))
}

fn nsqlite_query_row(args: &[ValueRef], span: Span) -> NiaoResult<ValueRef> {
    arity_range(args, 2, 3, "nsqlite_query_row", span)?;
    let id = conn_arg(args, 0, "nsqlite_query_row", span)?;
    let sql = string_arg(args, 1, "nsqlite_query_row", span)?;
    let params = if args.len() == 3 {
        params_array_arg(args, 2, "nsqlite_query_row", span)?
    } else {
        Vec::new()
    };
    handles::with_conn_mut(id, "nsqlite_query_row", span, |handle| {
        query_row_on_conn(handle, &sql, &params)
    })
    .map(|v| v.ref_cell())
    .or_else(|e| Ok(error_from_runtime(&e)))
}

fn nsqlite_query_value(args: &[ValueRef], span: Span) -> NiaoResult<ValueRef> {
    arity_range(args, 2, 3, "nsqlite_query_value", span)?;
    let id = conn_arg(args, 0, "nsqlite_query_value", span)?;
    let sql = string_arg(args, 1, "nsqlite_query_value", span)?;
    let params = if args.len() == 3 {
        params_array_arg(args, 2, "nsqlite_query_value", span)?
    } else {
        Vec::new()
    };
    handles::with_conn_mut(id, "nsqlite_query_value", span, |handle| {
        query_value_on_conn(handle, &sql, &params)
    })
    .map(|v| v.ref_cell())
    .or_else(|e| Ok(error_from_runtime(&e)))
}

fn nsqlite_query_column(args: &[ValueRef], span: Span) -> NiaoResult<ValueRef> {
    arity_range(args, 2, 3, "nsqlite_query_column", span)?;
    let id = conn_arg(args, 0, "nsqlite_query_column", span)?;
    let sql = string_arg(args, 1, "nsqlite_query_column", span)?;
    let params = if args.len() == 3 {
        params_array_arg(args, 2, "nsqlite_query_column", span)?
    } else {
        Vec::new()
    };
    handles::with_conn_mut(id, "nsqlite_query_column", span, |handle| {
        query_column_on_conn(handle, &sql, &params)
    })
    .map(|v| v.ref_cell())
    .or_else(|e| Ok(error_from_runtime(&e)))
}

fn nsqlite_batch(args: &[ValueRef], span: Span) -> NiaoResult<ValueRef> {
    arity(args, 3, "nsqlite_batch", span)?;
    let id = conn_arg(args, 0, "nsqlite_batch", span)?;
    let sql = string_arg(args, 1, "nsqlite_batch", span)?;
    let rows_val = &*args[2].borrow();
    let rows = match rows_val {
        Value::Array(outer) => {
            let mut rows = Vec::with_capacity(outer.len());
            for row_ref in outer {
                match &*row_ref.borrow() {
                    Value::Array(cells) => {
                        let mut row = Vec::with_capacity(cells.len());
                        for cell in cells {
                            row.push(types::niao_to_bound(&*cell.borrow(), span)?);
                        }
                        rows.push(row);
                    }
                    other => {
                        return Ok(nsqlite_error(
                            span,
                            format!(
                                "nsqlite_batch() expects array of param arrays, got {}",
                                other.type_name()
                            ),
                        ));
                    }
                }
            }
            rows
        }
        other => {
            return Ok(nsqlite_error(
                span,
                format!(
                    "nsqlite_batch() expects rows array as argument 3, got {}",
                    other.type_name()
                ),
            ));
        }
    };
    handles::with_conn_mut(id, "nsqlite_batch", span, |handle| batch_on_conn(handle, &sql, &rows))
        .map(ok_int)
        .or_else(|e| Ok(error_from_runtime(&e)))
}

fn nsqlite_insert(args: &[ValueRef], span: Span) -> NiaoResult<ValueRef> {
    arity(args, 3, "nsqlite_insert", span)?;
    let id = conn_arg(args, 0, "nsqlite_insert", span)?;
    let table = string_arg(args, 1, "nsqlite_insert", span)?;
    let data = match &*args[2].borrow() {
        Value::Object(map) => map.clone(),
        other => {
            return Ok(nsqlite_error(
                span,
                format!(
                    "nsqlite_insert() expects data object as argument 3, got {}",
                    other.type_name()
                ),
            ));
        }
    };
    if data.is_empty() {
        return Ok(nsqlite_error(span, "nsqlite_insert() data object is empty"));
    }
    let mut cols = Vec::new();
    let mut placeholders = Vec::new();
    let mut params = Vec::new();
    for (k, v) in &data {
        cols.push(format!("\"{}\"", k.replace('"', "\"\"")));
        placeholders.push("?".to_string());
        params.push(types::niao_to_bound(&*v.borrow(), span)?);
    }
    let sql = format!(
        "INSERT INTO \"{}\" ({}) VALUES ({})",
        table.replace('"', "\"\""),
        cols.join(", "),
        placeholders.join(", ")
    );
    handles::with_conn_mut(id, "nsqlite_insert", span, |handle| {
        exec_on_conn(handle, &sql, &params)?;
        Ok(handle.conn.last_insert_rowid() as i64)
    })
    .map(ok_int)
    .or_else(|e| Ok(error_from_runtime(&e)))
}

fn all_builtins() -> Vec<(&'static str, NativeFn)> {
    vec![
        ("nsqlite_open", Rc::new(nsqlite_open)),
        ("nsqlite_open_abs", Rc::new(nsqlite_open_abs)),
        ("nsqlite_close", Rc::new(nsqlite_close)),
        ("nsqlite_configure", Rc::new(nsqlite_configure)),
        ("nsqlite_path", Rc::new(nsqlite_path)),
        ("nsqlite_last_insert_rowid", Rc::new(nsqlite_last_insert_rowid)),
        ("nsqlite_changes", Rc::new(nsqlite_changes)),
        ("nsqlite_exec", Rc::new(nsqlite_exec)),
        ("nsqlite_exec_many", Rc::new(nsqlite_exec_many)),
        ("nsqlite_migrate", Rc::new(nsqlite_migrate)),
        ("nsqlite_table_exists", Rc::new(nsqlite_table_exists)),
        ("nsqlite_list_tables", Rc::new(nsqlite_list_tables)),
        ("nsqlite_table_info", Rc::new(nsqlite_table_info)),
        ("nsqlite_list_indexes", Rc::new(nsqlite_list_indexes)),
        ("nsqlite_query", Rc::new(nsqlite_query)),
        ("nsqlite_query_row", Rc::new(nsqlite_query_row)),
        ("nsqlite_query_value", Rc::new(nsqlite_query_value)),
        ("nsqlite_query_column", Rc::new(nsqlite_query_column)),
        ("nsqlite_prepare", Rc::new(nsqlite_prepare)),
        ("nsqlite_bind", Rc::new(nsqlite_bind)),
        ("nsqlite_bind_named", Rc::new(nsqlite_bind_named)),
        ("nsqlite_stmt_exec", Rc::new(nsqlite_stmt_exec)),
        ("nsqlite_stmt_query", Rc::new(nsqlite_stmt_query)),
        ("nsqlite_stmt_reset", Rc::new(nsqlite_stmt_reset)),
        ("nsqlite_finalize", Rc::new(nsqlite_finalize)),
        ("nsqlite_begin", Rc::new(nsqlite_begin)),
        ("nsqlite_commit", Rc::new(nsqlite_commit)),
        ("nsqlite_rollback", Rc::new(nsqlite_rollback)),
        ("nsqlite_batch", Rc::new(nsqlite_batch)),
        ("nsqlite_insert", Rc::new(nsqlite_insert)),
        ("nsqlite_backup", Rc::new(nsqlite_backup)),
        ("nsqlite_vacuum", Rc::new(nsqlite_vacuum)),
        ("nsqlite_version", Rc::new(nsqlite_version)),
        ("nsqlite_async_exec", Rc::new(nsqlite_async_exec)),
        ("nsqlite_async_query", Rc::new(nsqlite_async_query)),
        ("nsqlite_task_done", Rc::new(nsqlite_task_done)),
        ("nsqlite_task_wait", Rc::new(nsqlite_task_wait)),
        ("nsqlite_task_result", Rc::new(nsqlite_task_result)),
        ("nsqlite_task_cancel", Rc::new(nsqlite_task_cancel)),
    ]
}

pub fn namespace() -> Value {
    let mut map = HashMap::new();
    let bind = |map: &mut HashMap<String, ValueRef>, name: &str, f: NativeFn| {
        map.insert(name.to_string(), Value::NativeFunction(f).ref_cell());
    };
    bind(&mut map, "open", Rc::new(nsqlite_open));
    bind(&mut map, "open_abs", Rc::new(nsqlite_open_abs));
    bind(&mut map, "close", Rc::new(nsqlite_close));
    bind(&mut map, "configure", Rc::new(nsqlite_configure));
    bind(&mut map, "path", Rc::new(nsqlite_path));
    bind(&mut map, "last_insert_rowid", Rc::new(nsqlite_last_insert_rowid));
    bind(&mut map, "changes", Rc::new(nsqlite_changes));
    bind(&mut map, "exec", Rc::new(nsqlite_exec));
    bind(&mut map, "exec_many", Rc::new(nsqlite_exec_many));
    bind(&mut map, "migrate", Rc::new(nsqlite_migrate));
    bind(&mut map, "table_exists", Rc::new(nsqlite_table_exists));
    bind(&mut map, "list_tables", Rc::new(nsqlite_list_tables));
    bind(&mut map, "table_info", Rc::new(nsqlite_table_info));
    bind(&mut map, "list_indexes", Rc::new(nsqlite_list_indexes));
    bind(&mut map, "query", Rc::new(nsqlite_query));
    bind(&mut map, "query_row", Rc::new(nsqlite_query_row));
    bind(&mut map, "query_value", Rc::new(nsqlite_query_value));
    bind(&mut map, "query_column", Rc::new(nsqlite_query_column));
    bind(&mut map, "prepare", Rc::new(nsqlite_prepare));
    bind(&mut map, "bind", Rc::new(nsqlite_bind));
    bind(&mut map, "bind_named", Rc::new(nsqlite_bind_named));
    bind(&mut map, "stmt_exec", Rc::new(nsqlite_stmt_exec));
    bind(&mut map, "stmt_query", Rc::new(nsqlite_stmt_query));
    bind(&mut map, "stmt_reset", Rc::new(nsqlite_stmt_reset));
    bind(&mut map, "finalize", Rc::new(nsqlite_finalize));
    bind(&mut map, "begin", Rc::new(nsqlite_begin));
    bind(&mut map, "commit", Rc::new(nsqlite_commit));
    bind(&mut map, "rollback", Rc::new(nsqlite_rollback));
    bind(&mut map, "batch", Rc::new(nsqlite_batch));
    bind(&mut map, "insert", Rc::new(nsqlite_insert));
    bind(&mut map, "backup", Rc::new(nsqlite_backup));
    bind(&mut map, "vacuum", Rc::new(nsqlite_vacuum));
    bind(&mut map, "version", Rc::new(nsqlite_version));
    bind(&mut map, "async_exec", Rc::new(nsqlite_async_exec));
    bind(&mut map, "async_query", Rc::new(nsqlite_async_query));
    bind(&mut map, "task_done", Rc::new(nsqlite_task_done));
    bind(&mut map, "task_wait", Rc::new(nsqlite_task_wait));
    bind(&mut map, "task_result", Rc::new(nsqlite_task_result));
    bind(&mut map, "task_cancel", Rc::new(nsqlite_task_cancel));
    Value::Object(map)
}

pub const MODULE_NAME: &str = "nsqlite";
pub const MODULE_PATHS: &[&str] = &["nsqlite", "std/nsqlite"];

pub fn builtins() -> Vec<(&'static str, NativeFn)> {
    all_builtins()
}
