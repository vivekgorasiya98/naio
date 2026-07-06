//! Native npg standard library — fast PostgreSQL via postgres + r2d2.

mod bg;
mod common;
mod config;
mod connection;
mod handles;
mod pg;
mod pool;
mod query;
mod schema;
mod stmt;
mod types;

use crate::{error_from_runtime, error_value, NativeFn, NekoResult, RuntimeError, Value, ValueRef};
use common::*;
use connection::{
    npg_begin, npg_close, npg_commit, npg_configure, npg_connect, npg_connect_opts, npg_conninfo,
    npg_escape_literal, npg_is_in_transaction, npg_ping, npg_quote_ident, npg_rollback,
    npg_rollback_to, npg_savepoint, npg_server_version, npg_version,
};
use bg::{
    npg_async_exec, npg_async_query, npg_task_cancel, npg_task_done, npg_task_result,
    npg_task_wait,
};
use pg::{
    npg_advisory_lock, npg_advisory_unlock, npg_copy_from, npg_listen, npg_notify, npg_poll_notify,
    npg_unlisten,
};
use pool::{npg_pool, npg_pool_close, npg_pool_get, npg_pool_status};
use stmt::{
    npg_bind, npg_finalize, npg_prepare, npg_stmt_exec, npg_stmt_query, npg_stmt_reset,
};
use neko_ast::Span;
use neko_errors::codes;
use query::{
    batch_on_conn, exec_on_conn, insert_on_conn, query_column_on_conn, query_on_conn,
    query_row_on_conn, query_value_on_conn, RowFormat,
};
use schema::{list_indexes, list_tables, parse_migrations, run_migrations, table_exists, table_info};
use std::collections::HashMap;
use std::rc::Rc;

fn npg_error(span: Span, msg: impl Into<String>) -> ValueRef {
    error_value(codes::E1901_NPG_ERROR, "npg_error", msg.into(), span)
}

fn ok_int(n: i64) -> ValueRef {
    Value::Int(n).ref_cell()
}

fn ok_bool(b: bool) -> ValueRef {
    Value::Bool(b).ref_cell()
}

fn npg_exec(args: &[ValueRef], span: Span) -> NekoResult<ValueRef> {
    arity_range(args, 2, 3, "npg_exec", span)?;
    let id = conn_arg(args, 0, "npg_exec", span)?;
    let sql = string_arg(args, 1, "npg_exec", span)?;
    let params = if args.len() == 3 {
        params_array_arg(args, 2, "npg_exec", span)?
    } else {
        Vec::new()
    };
    handles::with_conn_mut(id, "npg_exec", span, |handle| exec_on_conn(handle, &sql, &params))
        .map(|n| ok_int(n as i64))
        .or_else(|e| Ok(error_from_runtime(&e)))
}

fn npg_exec_many(args: &[ValueRef], span: Span) -> NekoResult<ValueRef> {
    arity(args, 2, "npg_exec_many", span)?;
    let id = conn_arg(args, 0, "npg_exec_many", span)?;
    let statements = sql_list_arg(args, 1, "npg_exec_many", span)?;
    handles::with_conn_mut(id, "npg_exec_many", span, |handle| {
        let mut trans = handle.client_mut().transaction().map_err(|e| e.to_string())?;
        let mut count = 0i64;
        for sql in &statements {
            count += trans.execute(sql.as_str(), &[]).map_err(|e| e.to_string())? as i64;
        }
        trans.commit().map_err(|e| e.to_string())?;
        Ok(count)
    })
    .map(ok_int)
    .or_else(|e| Ok(error_from_runtime(&e)))
}

fn npg_migrate(args: &[ValueRef], span: Span) -> NekoResult<ValueRef> {
    arity(args, 2, "npg_migrate", span)?;
    let id = conn_arg(args, 0, "npg_migrate", span)?;
    let migrations = parse_migrations(&args[1], span)?;
    handles::with_conn_mut(id, "npg_migrate", span, |handle| {
        run_migrations(handle, &migrations).map_err(|e| e.to_string())
    })
    .map(ok_int)
    .or_else(|e| Ok(error_from_runtime(&e)))
}

fn npg_table_exists(args: &[ValueRef], span: Span) -> NekoResult<ValueRef> {
    arity_range(args, 2, 3, "npg_table_exists", span)?;
    let id = conn_arg(args, 0, "npg_table_exists", span)?;
    let (schema, name) = if args.len() == 3 {
        (
            string_arg(args, 1, "npg_table_exists", span)?,
            string_arg(args, 2, "npg_table_exists", span)?,
        )
    } else {
        ("public".to_string(), string_arg(args, 1, "npg_table_exists", span)?)
    };
    handles::with_conn_mut(id, "npg_table_exists", span, |handle| table_exists(handle, &schema, &name))
        .map(ok_bool)
        .or_else(|e| Ok(error_from_runtime(&e)))
}

fn npg_list_tables(args: &[ValueRef], span: Span) -> NekoResult<ValueRef> {
    arity_range(args, 1, 2, "npg_list_tables", span)?;
    let id = conn_arg(args, 0, "npg_list_tables", span)?;
    let schema = if args.len() == 2 {
        string_arg(args, 1, "npg_list_tables", span)?
    } else {
        "public".to_string()
    };
    handles::with_conn_mut(id, "npg_list_tables", span, |handle| {
        list_tables(handle, &schema).map(|names| {
            Value::Array(names.into_iter().map(|n| Value::String(n).ref_cell()).collect())
        })
    })
    .map(|v| v.ref_cell())
    .or_else(|e| Ok(error_from_runtime(&e)))
}

fn npg_table_info(args: &[ValueRef], span: Span) -> NekoResult<ValueRef> {
    arity_range(args, 2, 3, "npg_table_info", span)?;
    let id = conn_arg(args, 0, "npg_table_info", span)?;
    let (schema, table) = if args.len() == 3 {
        (
            string_arg(args, 1, "npg_table_info", span)?,
            string_arg(args, 2, "npg_table_info", span)?,
        )
    } else {
        ("public".to_string(), string_arg(args, 1, "npg_table_info", span)?)
    };
    handles::with_conn_mut(id, "npg_table_info", span, |handle| {
        table_info(handle, &schema, &table).map(Value::Array)
    })
    .map(|v| v.ref_cell())
    .or_else(|e| Ok(error_from_runtime(&e)))
}

fn npg_list_indexes(args: &[ValueRef], span: Span) -> NekoResult<ValueRef> {
    arity_range(args, 1, 3, "npg_list_indexes", span)?;
    let id = conn_arg(args, 0, "npg_list_indexes", span)?;
    let (schema, table) = match args.len() {
        1 => ("public".to_string(), None),
        2 => (string_arg(args, 1, "npg_list_indexes", span)?, None),
        _ => (
            string_arg(args, 1, "npg_list_indexes", span)?,
            Some(string_arg(args, 2, "npg_list_indexes", span)?),
        ),
    };
    handles::with_conn_mut(id, "npg_list_indexes", span, |handle| {
        list_indexes(handle, &schema, table.as_deref()).map(Value::Array)
    })
    .map(|v| v.ref_cell())
    .or_else(|e| Ok(error_from_runtime(&e)))
}

fn npg_query(args: &[ValueRef], span: Span) -> NekoResult<ValueRef> {
    arity_range(args, 2, 4, "npg_query", span)?;
    let id = conn_arg(args, 0, "npg_query", span)?;
    let sql = string_arg(args, 1, "npg_query", span)?;
    let (params, format) = if args.len() >= 3 {
        let params = params_array_arg(args, 2, "npg_query", span)?;
        let format = if args.len() == 4 {
            query::parse_row_format(&string_arg(args, 3, "npg_query", span)?)
                .map_err(|msg| RuntimeError::at(span, codes::E1901_NPG_ERROR, msg))?
        } else {
            RowFormat::Object
        };
        (params, format)
    } else {
        (Vec::new(), RowFormat::Object)
    };
    handles::with_conn_mut(id, "npg_query", span, |handle| query_on_conn(handle, &sql, &params, format))
        .map(|v| v.ref_cell())
        .or_else(|e| Ok(error_from_runtime(&e)))
}

fn npg_query_row(args: &[ValueRef], span: Span) -> NekoResult<ValueRef> {
    arity_range(args, 2, 3, "npg_query_row", span)?;
    let id = conn_arg(args, 0, "npg_query_row", span)?;
    let sql = string_arg(args, 1, "npg_query_row", span)?;
    let params = if args.len() == 3 {
        params_array_arg(args, 2, "npg_query_row", span)?
    } else {
        Vec::new()
    };
    handles::with_conn_mut(id, "npg_query_row", span, |handle| query_row_on_conn(handle, &sql, &params))
        .map(|v| v.ref_cell())
        .or_else(|e| Ok(error_from_runtime(&e)))
}

fn npg_query_value(args: &[ValueRef], span: Span) -> NekoResult<ValueRef> {
    arity_range(args, 2, 3, "npg_query_value", span)?;
    let id = conn_arg(args, 0, "npg_query_value", span)?;
    let sql = string_arg(args, 1, "npg_query_value", span)?;
    let params = if args.len() == 3 {
        params_array_arg(args, 2, "npg_query_value", span)?
    } else {
        Vec::new()
    };
    handles::with_conn_mut(id, "npg_query_value", span, |handle| {
        query_value_on_conn(handle, &sql, &params)
    })
    .map(|v| v.ref_cell())
    .or_else(|e| Ok(error_from_runtime(&e)))
}

fn npg_query_column(args: &[ValueRef], span: Span) -> NekoResult<ValueRef> {
    arity_range(args, 2, 3, "npg_query_column", span)?;
    let id = conn_arg(args, 0, "npg_query_column", span)?;
    let sql = string_arg(args, 1, "npg_query_column", span)?;
    let params = if args.len() == 3 {
        params_array_arg(args, 2, "npg_query_column", span)?
    } else {
        Vec::new()
    };
    handles::with_conn_mut(id, "npg_query_column", span, |handle| {
        query_column_on_conn(handle, &sql, &params)
    })
    .map(|v| v.ref_cell())
    .or_else(|e| Ok(error_from_runtime(&e)))
}

fn npg_batch(args: &[ValueRef], span: Span) -> NekoResult<ValueRef> {
    arity(args, 3, "npg_batch", span)?;
    let id = conn_arg(args, 0, "npg_batch", span)?;
    let sql = string_arg(args, 1, "npg_batch", span)?;
    let rows_val = &*args[2].borrow();
    let rows = match rows_val {
        Value::Array(outer) => {
            let mut rows = Vec::with_capacity(outer.len());
            for row_ref in outer {
                match &*row_ref.borrow() {
                    Value::Array(cells) => {
                        let mut row = Vec::with_capacity(cells.len());
                        for cell in cells {
                            row.push(types::neko_to_bound(&*cell.borrow(), span)?);
                        }
                        rows.push(row);
                    }
                    other => {
                        return Ok(npg_error(
                            span,
                            format!(
                                "npg_batch() expects array of param arrays, got {}",
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
                    "npg_batch() expects rows array as argument 3, got {}",
                    other.type_name()
                ),
            ));
        }
    };
    handles::with_conn_mut(id, "npg_batch", span, |handle| batch_on_conn(handle, &sql, &rows))
        .map(|n| ok_int(n as i64))
        .or_else(|e| Ok(error_from_runtime(&e)))
}

fn npg_insert(args: &[ValueRef], span: Span) -> NekoResult<ValueRef> {
    arity_range(args, 3, 4, "npg_insert", span)?;
    let id = conn_arg(args, 0, "npg_insert", span)?;
    let table = string_arg(args, 1, "npg_insert", span)?;
    let data = match &*args[2].borrow() {
        Value::Object(map) => map.clone(),
        other => {
            return Ok(npg_error(
                span,
                format!(
                    "npg_insert() expects data object as argument 3, got {}",
                    other.type_name()
                ),
            ));
        }
    };
    let schema = optional_string_arg(args, 3, "npg_insert", span)?;
    handles::with_conn_mut(id, "npg_insert", span, |handle| {
        insert_on_conn(
            handle,
            schema.as_deref(),
            &table,
            &data,
            span,
        )
    })
    .map(|v| v.ref_cell())
    .or_else(|e| Ok(error_from_runtime(&e)))
}

fn all_builtins() -> Vec<(&'static str, NativeFn)> {
    vec![
        ("npg_connect", Rc::new(npg_connect)),
        ("npg_connect_opts", Rc::new(npg_connect_opts)),
        ("npg_close", Rc::new(npg_close)),
        ("npg_ping", Rc::new(npg_ping)),
        ("npg_configure", Rc::new(npg_configure)),
        ("npg_conninfo", Rc::new(npg_conninfo)),
        ("npg_server_version", Rc::new(npg_server_version)),
        ("npg_is_in_transaction", Rc::new(npg_is_in_transaction)),
        ("npg_pool", Rc::new(npg_pool)),
        ("npg_pool_close", Rc::new(npg_pool_close)),
        ("npg_pool_get", Rc::new(npg_pool_get)),
        ("npg_pool_status", Rc::new(npg_pool_status)),
        ("npg_exec", Rc::new(npg_exec)),
        ("npg_exec_many", Rc::new(npg_exec_many)),
        ("npg_migrate", Rc::new(npg_migrate)),
        ("npg_table_exists", Rc::new(npg_table_exists)),
        ("npg_list_tables", Rc::new(npg_list_tables)),
        ("npg_table_info", Rc::new(npg_table_info)),
        ("npg_list_indexes", Rc::new(npg_list_indexes)),
        ("npg_query", Rc::new(npg_query)),
        ("npg_query_row", Rc::new(npg_query_row)),
        ("npg_query_value", Rc::new(npg_query_value)),
        ("npg_query_column", Rc::new(npg_query_column)),
        ("npg_prepare", Rc::new(npg_prepare)),
        ("npg_bind", Rc::new(npg_bind)),
        ("npg_stmt_exec", Rc::new(npg_stmt_exec)),
        ("npg_stmt_query", Rc::new(npg_stmt_query)),
        ("npg_stmt_reset", Rc::new(npg_stmt_reset)),
        ("npg_finalize", Rc::new(npg_finalize)),
        ("npg_begin", Rc::new(npg_begin)),
        ("npg_commit", Rc::new(npg_commit)),
        ("npg_rollback", Rc::new(npg_rollback)),
        ("npg_savepoint", Rc::new(npg_savepoint)),
        ("npg_rollback_to", Rc::new(npg_rollback_to)),
        ("npg_batch", Rc::new(npg_batch)),
        ("npg_insert", Rc::new(npg_insert)),
        ("npg_copy_from", Rc::new(npg_copy_from)),
        ("npg_listen", Rc::new(npg_listen)),
        ("npg_unlisten", Rc::new(npg_unlisten)),
        ("npg_notify", Rc::new(npg_notify)),
        ("npg_poll_notify", Rc::new(npg_poll_notify)),
        ("npg_advisory_lock", Rc::new(npg_advisory_lock)),
        ("npg_advisory_unlock", Rc::new(npg_advisory_unlock)),
        ("npg_version", Rc::new(npg_version)),
        ("npg_escape_literal", Rc::new(npg_escape_literal)),
        ("npg_quote_ident", Rc::new(npg_quote_ident)),
        ("npg_async_exec", Rc::new(npg_async_exec)),
        ("npg_async_query", Rc::new(npg_async_query)),
        ("npg_task_done", Rc::new(npg_task_done)),
        ("npg_task_wait", Rc::new(npg_task_wait)),
        ("npg_task_result", Rc::new(npg_task_result)),
        ("npg_task_cancel", Rc::new(npg_task_cancel)),
    ]
}

pub fn namespace() -> Value {
    let mut map = HashMap::new();
    let bind = |map: &mut HashMap<String, ValueRef>, name: &str, f: NativeFn| {
        map.insert(name.to_string(), Value::NativeFunction(f).ref_cell());
    };
    bind(&mut map, "connect", Rc::new(npg_connect));
    bind(&mut map, "connect_opts", Rc::new(npg_connect_opts));
    bind(&mut map, "close", Rc::new(npg_close));
    bind(&mut map, "ping", Rc::new(npg_ping));
    bind(&mut map, "configure", Rc::new(npg_configure));
    bind(&mut map, "conninfo", Rc::new(npg_conninfo));
    bind(&mut map, "server_version", Rc::new(npg_server_version));
    bind(&mut map, "is_in_transaction", Rc::new(npg_is_in_transaction));
    bind(&mut map, "pool", Rc::new(npg_pool));
    bind(&mut map, "pool_close", Rc::new(npg_pool_close));
    bind(&mut map, "pool_get", Rc::new(npg_pool_get));
    bind(&mut map, "pool_status", Rc::new(npg_pool_status));
    bind(&mut map, "exec", Rc::new(npg_exec));
    bind(&mut map, "exec_many", Rc::new(npg_exec_many));
    bind(&mut map, "migrate", Rc::new(npg_migrate));
    bind(&mut map, "table_exists", Rc::new(npg_table_exists));
    bind(&mut map, "list_tables", Rc::new(npg_list_tables));
    bind(&mut map, "table_info", Rc::new(npg_table_info));
    bind(&mut map, "list_indexes", Rc::new(npg_list_indexes));
    bind(&mut map, "query", Rc::new(npg_query));
    bind(&mut map, "query_row", Rc::new(npg_query_row));
    bind(&mut map, "query_value", Rc::new(npg_query_value));
    bind(&mut map, "query_column", Rc::new(npg_query_column));
    bind(&mut map, "prepare", Rc::new(npg_prepare));
    bind(&mut map, "bind", Rc::new(npg_bind));
    bind(&mut map, "stmt_exec", Rc::new(npg_stmt_exec));
    bind(&mut map, "stmt_query", Rc::new(npg_stmt_query));
    bind(&mut map, "stmt_reset", Rc::new(npg_stmt_reset));
    bind(&mut map, "finalize", Rc::new(npg_finalize));
    bind(&mut map, "begin", Rc::new(npg_begin));
    bind(&mut map, "commit", Rc::new(npg_commit));
    bind(&mut map, "rollback", Rc::new(npg_rollback));
    bind(&mut map, "savepoint", Rc::new(npg_savepoint));
    bind(&mut map, "rollback_to", Rc::new(npg_rollback_to));
    bind(&mut map, "batch", Rc::new(npg_batch));
    bind(&mut map, "insert", Rc::new(npg_insert));
    bind(&mut map, "copy_from", Rc::new(npg_copy_from));
    bind(&mut map, "listen", Rc::new(npg_listen));
    bind(&mut map, "unlisten", Rc::new(npg_unlisten));
    bind(&mut map, "notify", Rc::new(npg_notify));
    bind(&mut map, "poll_notify", Rc::new(npg_poll_notify));
    bind(&mut map, "advisory_lock", Rc::new(npg_advisory_lock));
    bind(&mut map, "advisory_unlock", Rc::new(npg_advisory_unlock));
    bind(&mut map, "version", Rc::new(npg_version));
    bind(&mut map, "escape_literal", Rc::new(npg_escape_literal));
    bind(&mut map, "quote_ident", Rc::new(npg_quote_ident));
    bind(&mut map, "async_exec", Rc::new(npg_async_exec));
    bind(&mut map, "async_query", Rc::new(npg_async_query));
    bind(&mut map, "task_done", Rc::new(npg_task_done));
    bind(&mut map, "task_wait", Rc::new(npg_task_wait));
    bind(&mut map, "task_result", Rc::new(npg_task_result));
    bind(&mut map, "task_cancel", Rc::new(npg_task_cancel));
    Value::Object(map)
}

pub const MODULE_NAME: &str = "npg";
pub const MODULE_PATHS: &[&str] = &["npg", "std/npg"];

/// Array-format query result for NCL / NML dataframe bridges.
pub fn query_table(
    conn_id: u64,
    sql: &str,
    params: &[ValueRef],
    span: Span,
) -> Result<Value, crate::RuntimeError> {
    let bound: Vec<types::BoundValue> = params
        .iter()
        .map(|p| types::neko_to_bound(&p.borrow(), span))
        .collect::<Result<_, _>>()?;
    handles::with_conn_mut(conn_id, "npg_query_table", span, |handle| {
        query::query_on_conn(handle, sql, &bound, RowFormat::Array)
    })
}

pub fn builtins() -> Vec<(&'static str, NativeFn)> {
    all_builtins()
}
