//! Connection open/close, pragmas, and transaction builtins.

use super::handles::{self, alloc_conn, apply_default_pragmas, open_connection, remove_conn};
use super::query::exec_on_conn;
use crate::{error_from_runtime, error_value, NekoResult, RuntimeError, Value, ValueRef};
use neko_ast::Span;
use neko_errors::codes;
use rusqlite::backup::Backup;
use std::collections::HashMap;
use std::time::Duration;

use super::common::*;

fn nsqlite_error(span: Span, msg: impl Into<String>) -> ValueRef {
    error_value(codes::E1701_NSQLITE_ERROR, "nsqlite_error", msg.into(), span)
}

fn ok_nil() -> ValueRef {
    Value::Nil.ref_cell()
}

fn ok_int(n: i64) -> ValueRef {
    Value::Int(n).ref_cell()
}

fn ok_string(s: impl Into<String>) -> ValueRef {
    Value::String(s.into()).ref_cell()
}

fn conn_arg(args: &[ValueRef], idx: usize, name: &str, span: Span) -> NekoResult<u64> {
    match &*args[idx].borrow() {
        Value::Int(id) if *id > 0 => Ok(*id as u64),
        other => Err(RuntimeError::at(
            span,
            codes::E1702_NSQLITE_INVALID_HANDLE,
            format!(
                "{name}() expects connection handle (positive int) as argument {}, got {}",
                idx + 1,
                other.type_name()
            ),
        )),
    }
}

pub fn nsqlite_open(args: &[ValueRef], span: Span) -> NekoResult<ValueRef> {
    arity(args, 1, "nsqlite_open", span)?;
    let path = string_arg(args, 0, "nsqlite_open", span)?;
    match open_and_register(&path, true, span) {
        Ok(id) => Ok(ok_int(id as i64)),
        Err(msg) => Ok(nsqlite_error(span, msg)),
    }
}

pub fn nsqlite_open_abs(args: &[ValueRef], span: Span) -> NekoResult<ValueRef> {
    arity(args, 1, "nsqlite_open_abs", span)?;
    let path = string_arg(args, 0, "nsqlite_open_abs", span)?;
    match open_and_register(&path, false, span) {
        Ok(id) => Ok(ok_int(id as i64)),
        Err(msg) => Ok(nsqlite_error(span, msg)),
    }
}

fn open_and_register(path: &str, use_cwd: bool, _span: Span) -> Result<u64, String> {
    let (conn, display) = open_connection(path, use_cwd)?;
    if display != ":memory:" {
        apply_default_pragmas(&conn)?;
    }
    Ok(alloc_conn(conn, display))
}

pub fn nsqlite_close(args: &[ValueRef], span: Span) -> NekoResult<ValueRef> {
    arity(args, 1, "nsqlite_close", span)?;
    let id = conn_arg(args, 0, "nsqlite_close", span)?;
    if remove_conn(id).is_some() {
        Ok(ok_nil())
    } else {
        Ok(nsqlite_error(
            span,
            format!("nsqlite_close(): invalid or closed connection handle {id}"),
        ))
    }
}

pub fn nsqlite_configure(args: &[ValueRef], span: Span) -> NekoResult<ValueRef> {
    arity(args, 2, "nsqlite_configure", span)?;
    let id = conn_arg(args, 0, "nsqlite_configure", span)?;
    let opts = object_arg(args, 1, "nsqlite_configure", span)?;
    handles::with_conn_mut(id, "nsqlite_configure", span, |handle| {
        for (key, val) in opts {
            let val_ref = &*val.borrow();
            match key.as_str() {
                "wal" => {
                    let mode = if bool_from_value(val_ref)? { "WAL" } else { "DELETE" };
                    handle
                        .conn
                        .pragma_update(None, "journal_mode", mode)
                        .map_err(|e| e.to_string())?;
                }
                "synchronous" => {
                    let mode = match val_ref {
                        Value::String(s) => s.as_str(),
                        Value::Int(0) => "OFF",
                        Value::Int(1) => "NORMAL",
                        Value::Int(2) => "FULL",
                        Value::Int(3) => "EXTRA",
                        other => {
                            return Err(format!(
                                "synchronous expects string or int 0-3, got {}",
                                other.type_name()
                            ));
                        }
                    };
                    handle
                        .conn
                        .pragma_update(None, "synchronous", mode)
                        .map_err(|e| e.to_string())?;
                }
                "cache_size" => match val_ref {
                    Value::Int(n) => handle
                        .conn
                        .pragma_update(None, "cache_size", *n)
                        .map_err(|e| e.to_string())?,
                    other => {
                        return Err(format!("cache_size expects int, got {}", other.type_name()));
                    }
                },
                "mmap_size" => match val_ref {
                    Value::Int(n) => handle
                        .conn
                        .pragma_update(None, "mmap_size", *n)
                        .map_err(|e| e.to_string())?,
                    other => {
                        return Err(format!("mmap_size expects int, got {}", other.type_name()));
                    }
                },
                "foreign_keys" => {
                    let on = bool_from_value(val_ref)?;
                    handle
                        .conn
                        .pragma_update(None, "foreign_keys", if on { 1i64 } else { 0i64 })
                        .map_err(|e| e.to_string())?;
                }
                other => return Err(format!("unknown configure option \"{other}\"")),
            }
        }
        Ok(())
    })
    .map(|_| ok_nil())
    .or_else(|e| Ok(error_from_runtime(&e)))
}

fn bool_from_value(val: &Value) -> Result<bool, String> {
    match val {
        Value::Bool(b) => Ok(*b),
        Value::Int(n) => Ok(*n != 0),
        other => Err(format!("expected bool or int, got {}", other.type_name())),
    }
}

fn object_arg(
    args: &[ValueRef],
    idx: usize,
    name: &str,
    span: Span,
) -> NekoResult<HashMap<String, ValueRef>> {
    match &*args[idx].borrow() {
        Value::Object(map) => Ok(map.clone()),
        other => Err(RuntimeError::at(
            span,
            codes::E1700_NSQLITE_ARITY,
            format!(
                "{name}() expects object as argument {}, got {}",
                idx + 1,
                other.type_name()
            ),
        )),
    }
}

pub fn nsqlite_path(args: &[ValueRef], span: Span) -> NekoResult<ValueRef> {
    arity(args, 1, "nsqlite_path", span)?;
    let id = conn_arg(args, 0, "nsqlite_path", span)?;
    match handles::conn_path(id) {
        Some(p) => Ok(ok_string(p)),
        None => Ok(nsqlite_error(
            span,
            format!("nsqlite_path(): invalid connection handle {id}"),
        )),
    }
}

pub fn nsqlite_last_insert_rowid(args: &[ValueRef], span: Span) -> NekoResult<ValueRef> {
    arity(args, 1, "nsqlite_last_insert_rowid", span)?;
    let id = conn_arg(args, 0, "nsqlite_last_insert_rowid", span)?;
    handles::with_conn_mut(id, "nsqlite_last_insert_rowid", span, |handle| {
        Ok(handle.conn.last_insert_rowid() as i64)
    })
    .map(ok_int)
    .or_else(|e| Ok(error_from_runtime(&e)))
}

pub fn nsqlite_changes(args: &[ValueRef], span: Span) -> NekoResult<ValueRef> {
    arity(args, 1, "nsqlite_changes", span)?;
    let id = conn_arg(args, 0, "nsqlite_changes", span)?;
    handles::with_conn_mut(id, "nsqlite_changes", span, |handle| {
        Ok(handle.conn.changes() as i64)
    })
    .map(ok_int)
    .or_else(|e| Ok(error_from_runtime(&e)))
}

pub fn nsqlite_begin(args: &[ValueRef], span: Span) -> NekoResult<ValueRef> {
    arity_range(args, 1, 2, "nsqlite_begin", span)?;
    let id = conn_arg(args, 0, "nsqlite_begin", span)?;
    let mode = if args.len() == 2 {
        string_arg(args, 1, "nsqlite_begin", span)?
    } else {
        "deferred".to_string()
    };
    let sql = match mode.as_str() {
        "deferred" => "BEGIN DEFERRED",
        "immediate" => "BEGIN IMMEDIATE",
        "exclusive" => "BEGIN EXCLUSIVE",
        other => {
            return Ok(nsqlite_error(
                span,
                format!("nsqlite_begin(): unknown mode \"{other}\""),
            ));
        }
    };
    handles::with_conn_mut(id, "nsqlite_begin", span, |handle| {
        exec_on_conn(handle, sql, &[])?;
        Ok(())
    })
    .map(|_| ok_nil())
    .or_else(|e| Ok(error_from_runtime(&e)))
}

pub fn nsqlite_commit(args: &[ValueRef], span: Span) -> NekoResult<ValueRef> {
    arity(args, 1, "nsqlite_commit", span)?;
    let id = conn_arg(args, 0, "nsqlite_commit", span)?;
    handles::with_conn_mut(id, "nsqlite_commit", span, |handle| {
        exec_on_conn(handle, "COMMIT", &[])?;
        Ok(())
    })
    .map(|_| ok_nil())
    .or_else(|e| Ok(error_from_runtime(&e)))
}

pub fn nsqlite_rollback(args: &[ValueRef], span: Span) -> NekoResult<ValueRef> {
    arity(args, 1, "nsqlite_rollback", span)?;
    let id = conn_arg(args, 0, "nsqlite_rollback", span)?;
    handles::with_conn_mut(id, "nsqlite_rollback", span, |handle| {
        exec_on_conn(handle, "ROLLBACK", &[])?;
        Ok(())
    })
    .map(|_| ok_nil())
    .or_else(|e| Ok(error_from_runtime(&e)))
}

pub fn nsqlite_vacuum(args: &[ValueRef], span: Span) -> NekoResult<ValueRef> {
    arity(args, 1, "nsqlite_vacuum", span)?;
    let id = conn_arg(args, 0, "nsqlite_vacuum", span)?;
    handles::with_conn_mut(id, "nsqlite_vacuum", span, |handle| {
        exec_on_conn(handle, "VACUUM", &[])?;
        Ok(())
    })
    .map(|_| ok_nil())
    .or_else(|e| Ok(error_from_runtime(&e)))
}

pub fn nsqlite_version(args: &[ValueRef], span: Span) -> NekoResult<ValueRef> {
    arity(args, 0, "nsqlite_version", span)?;
    Ok(ok_string(rusqlite::version()))
}

pub fn nsqlite_backup(args: &[ValueRef], span: Span) -> NekoResult<ValueRef> {
    arity(args, 2, "nsqlite_backup", span)?;
    let dest_id = conn_arg(args, 0, "nsqlite_backup", span)?;
    let src_id = conn_arg(args, 1, "nsqlite_backup", span)?;
    let dest_id_copy = dest_id;
    let src_id_copy = src_id;
    // Need both connections mutably — use raw connection backup via path reopen for simplicity
    let src_path = handles::conn_path(src_id_copy).ok_or_else(|| {
        RuntimeError::at(
            span,
            codes::E1702_NSQLITE_INVALID_HANDLE,
            format!("nsqlite_backup(): invalid source handle {src_id_copy}"),
        )
    })?;
    handles::with_conn_mut(dest_id_copy, "nsqlite_backup", span, move |dest| {
        if src_path == ":memory:" {
            return Err("backup from :memory: requires file-based source — use nsqlite_backup with file databases".into());
        }
        let src_conn = rusqlite::Connection::open(&src_path).map_err(|e| e.to_string())?;
        let backup = Backup::new(&src_conn, &mut dest.conn).map_err(|e| e.to_string())?;
        backup
            .run_to_completion(100, Duration::from_millis(10), None)
            .map_err(|e| e.to_string())?;
        Ok(())
    })
    .map(|_| ok_nil())
    .or_else(|e| Ok(error_from_runtime(&e)))
}
