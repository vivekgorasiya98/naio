//! Async find/bulk via shared Tokio task pool.

use super::bulk::{bulk_write_async, parse_bulk_ops_for_async};
use super::common::*;
use super::handles::client_clone;
use super::ops::{apply_find_options, parse_find_options};
use super::types::bson_doc_to_async;
use crate::async_tasks::{
    cancel_task, spawn_tokio, task_done, task_result_value, task_wait_loop, with_task,
    AsyncValue,
};
use crate::{error_value, NekoResult, RuntimeError, Value, ValueRef};
use futures::StreamExt;
use mongodb::bson::Document;
use neko_ast::Span;
use neko_errors::codes;

fn nmongo_async_error(span: Span, msg: impl Into<String>) -> ValueRef {
    error_value(codes::E1921_NMONGO_ERROR, "nmongo_error", msg.into(), span)
}

fn task_arg(args: &[ValueRef], idx: usize, name: &str, span: Span) -> NekoResult<u64> {
    match &*args[idx].borrow() {
        Value::Int(id) if *id > 0 => Ok(*id as u64),
        other => Err(RuntimeError::at(
            span,
            codes::E1920_NMONGO_ARITY,
            format!(
                "{name}() expects task id as argument {}, got {}",
                idx + 1,
                other.type_name()
            ),
        )),
    }
}

pub fn nmongo_async_find(args: &[ValueRef], span: Span) -> NekoResult<ValueRef> {
    arity_range(args, 3, 5, "nmongo_async_find", span)?;
    let (client_id, db, coll) = db_coll_args_range(args, 3, 5, "nmongo_async_find", span)?;
    let filter = optional_doc_arg(args, 3, "nmongo_async_find", span)?;
    let opts_map = optional_object_arg(args, 4);
    let find_opts = parse_find_options(opts_map.as_ref(), span)?;
    let client = client_clone(client_id).ok_or_else(|| {
        RuntimeError::at(
            span,
            codes::E1922_NMONGO_INVALID_HANDLE,
            format!("invalid client handle {client_id}"),
        )
    })?;

    let id = spawn_tokio(async move {
        let collection = client.database(&db).collection::<Document>(&coll);
        let action = apply_find_options(collection.find(filter), &find_opts);
        let mut cursor = action.await.map_err(|e| e.to_string())?;
        let mut rows = Vec::new();
        while let Some(doc) = cursor.next().await {
            let doc = doc.map_err(|e| e.to_string())?;
            rows.push(bson_doc_to_async(&doc));
        }
        Ok(AsyncValue::Array(rows))
    });
    Ok(Value::Int(id as i64).ref_cell())
}

pub fn nmongo_async_bulk_write(args: &[ValueRef], span: Span) -> NekoResult<ValueRef> {
    arity_range(args, 4, 5, "nmongo_async_bulk_write", span)?;
    let (client_id, db, coll) = db_coll_args(args, "nmongo_async_bulk_write", span)?;
    let ops_arr = array_arg(args, 3, "nmongo_async_bulk_write", span)?;
    let ops = parse_bulk_ops_for_async(&ops_arr, span)?;
    let opts_map = optional_object_arg(args, 4);
    let ordered = opts_map
        .as_ref()
        .and_then(|m| m.get("ordered"))
        .and_then(|v| match &*v.borrow() {
            Value::Bool(b) => Some(*b),
            _ => None,
        })
        .unwrap_or(true);
    let client = client_clone(client_id).ok_or_else(|| {
        RuntimeError::at(
            span,
            codes::E1922_NMONGO_INVALID_HANDLE,
            format!("invalid client handle {client_id}"),
        )
    })?;

    let id = spawn_tokio(async move {
        let counts = bulk_write_async(&client, &db, &coll, ops, ordered).await?;
        let mut map = std::collections::HashMap::new();
        map.insert("inserted_count".to_string(), AsyncValue::Int(counts.0));
        map.insert("matched_count".to_string(), AsyncValue::Int(counts.1));
        map.insert("modified_count".to_string(), AsyncValue::Int(counts.2));
        map.insert("deleted_count".to_string(), AsyncValue::Int(counts.3));
        Ok(AsyncValue::Object(map))
    });
    Ok(Value::Int(id as i64).ref_cell())
}

pub fn nmongo_task_done(args: &[ValueRef], span: Span) -> NekoResult<ValueRef> {
    arity(args, 1, "nmongo_task_done", span)?;
    let id = task_arg(args, 0, "nmongo_task_done", span)?;
    with_task(
        id,
        "nmongo_task_done",
        span,
        codes::E1925_NMONGO_TASK_NOT_FOUND,
        "nmongo task cancelled",
        nmongo_async_error,
        |state| Ok(Value::Bool(task_done(state)).ref_cell()),
    )
}

pub fn nmongo_task_wait(args: &[ValueRef], span: Span) -> NekoResult<ValueRef> {
    arity(args, 1, "nmongo_task_wait", span)?;
    let id = task_arg(args, 0, "nmongo_task_wait", span)?;
    task_wait_loop(id);
    with_task(
        id,
        "nmongo_task_wait",
        span,
        codes::E1925_NMONGO_TASK_NOT_FOUND,
        "nmongo task cancelled",
        nmongo_async_error,
        |_| Ok(Value::Nil.ref_cell()),
    )
}

pub fn nmongo_task_result(args: &[ValueRef], span: Span) -> NekoResult<ValueRef> {
    arity(args, 1, "nmongo_task_result", span)?;
    let id = task_arg(args, 0, "nmongo_task_result", span)?;
    with_task(
        id,
        "nmongo_task_result",
        span,
        codes::E1925_NMONGO_TASK_NOT_FOUND,
        "nmongo task cancelled",
        nmongo_async_error,
        |state| {
            Ok(task_result_value(
                state,
                span,
                "nmongo task cancelled",
                nmongo_async_error,
            ))
        },
    )
}

pub fn nmongo_task_cancel(args: &[ValueRef], span: Span) -> NekoResult<ValueRef> {
    arity(args, 1, "nmongo_task_cancel", span)?;
    let id = task_arg(args, 0, "nmongo_task_cancel", span)?;
    let cancelled = cancel_task(id, span, codes::E1925_NMONGO_TASK_NOT_FOUND)?;
    Ok(Value::Bool(cancelled).ref_cell())
}
