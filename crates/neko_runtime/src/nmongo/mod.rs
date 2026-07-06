//! Native nmongo standard library — MongoDB via official Rust driver.
#![allow(dead_code)]

mod aggregate;
mod bg;
mod bulk;
mod changestream;
mod common;
mod connection;
pub mod crud;
mod gridfs;
mod handles;
mod indexes;
mod ops;
mod runtime;
mod transactions;
mod types;

use crate::{NativeFn, NekoResult, Value, ValueRef};
use aggregate::nmongo_aggregate;
use bg::{
    nmongo_async_bulk_write, nmongo_async_find, nmongo_task_cancel, nmongo_task_done,
    nmongo_task_result, nmongo_task_wait,
};
use bulk::nmongo_bulk_write;
use changestream::{nmongo_watch, nmongo_watch_close, nmongo_watch_next};
use common::*;
use connection::{
    nmongo_close, nmongo_connect, nmongo_connect_uri, nmongo_list_databases, nmongo_ping,
};
use crud::{
    nmongo_count_documents, nmongo_delete_many, nmongo_delete_one, nmongo_distinct,
    nmongo_drop_collection, nmongo_find, nmongo_find_one, nmongo_insert_many, nmongo_insert_one,
    nmongo_list_collections, nmongo_replace_one, nmongo_update_many, nmongo_update_one,
};
use gridfs::{
    nmongo_gridfs_delete, nmongo_gridfs_download, nmongo_gridfs_list, nmongo_gridfs_upload,
};
use indexes::{nmongo_create_index, nmongo_drop_index, nmongo_list_indexes};
use neko_ast::Span;
use std::collections::HashMap;
use std::rc::Rc;
use transactions::{
    nmongo_abort_transaction, nmongo_commit_transaction, nmongo_end_session,
    nmongo_start_session, nmongo_start_transaction,
};
use self::types::{from_extended_json, is_object_id_hex, object_id_to_bson, to_extended_json};

fn nmongo_object_id(args: &[ValueRef], span: Span) -> NekoResult<ValueRef> {
    arity(args, 1, "nmongo_object_id", span)?;
    let hex = string_arg(args, 0, "nmongo_object_id", span)?;
    let bson = object_id_to_bson(&hex, span)?;
    Ok(self::types::bson_to_neko(bson).ref_cell())
}

fn nmongo_is_object_id(args: &[ValueRef], span: Span) -> NekoResult<ValueRef> {
    arity(args, 1, "nmongo_is_object_id", span)?;
    let hex = string_arg(args, 0, "nmongo_is_object_id", span)?;
    Ok(Value::Bool(is_object_id_hex(&hex)).ref_cell())
}

fn nmongo_to_extended_json(args: &[ValueRef], span: Span) -> NekoResult<ValueRef> {
    arity(args, 1, "nmongo_to_extended_json", span)?;
    let s = to_extended_json(&args[0], span)?;
    Ok(Value::String(s).ref_cell())
}

fn nmongo_from_extended_json(args: &[ValueRef], span: Span) -> NekoResult<ValueRef> {
    arity(args, 1, "nmongo_from_extended_json", span)?;
    let s = string_arg(args, 0, "nmongo_from_extended_json", span)?;
    from_extended_json(&s, span)
}

fn all_builtins() -> Vec<(&'static str, NativeFn)> {
    vec![
        ("nmongo_connect", Rc::new(nmongo_connect)),
        ("nmongo_connect_uri", Rc::new(nmongo_connect_uri)),
        ("nmongo_close", Rc::new(nmongo_close)),
        ("nmongo_ping", Rc::new(nmongo_ping)),
        ("nmongo_list_databases", Rc::new(nmongo_list_databases)),
        ("nmongo_find", Rc::new(nmongo_find)),
        ("nmongo_find_one", Rc::new(nmongo_find_one)),
        ("nmongo_insert_one", Rc::new(nmongo_insert_one)),
        ("nmongo_insert_many", Rc::new(nmongo_insert_many)),
        ("nmongo_update_one", Rc::new(nmongo_update_one)),
        ("nmongo_update_many", Rc::new(nmongo_update_many)),
        ("nmongo_replace_one", Rc::new(nmongo_replace_one)),
        ("nmongo_delete_one", Rc::new(nmongo_delete_one)),
        ("nmongo_delete_many", Rc::new(nmongo_delete_many)),
        ("nmongo_count_documents", Rc::new(nmongo_count_documents)),
        ("nmongo_distinct", Rc::new(nmongo_distinct)),
        ("nmongo_aggregate", Rc::new(nmongo_aggregate)),
        ("nmongo_create_index", Rc::new(nmongo_create_index)),
        ("nmongo_list_indexes", Rc::new(nmongo_list_indexes)),
        ("nmongo_drop_index", Rc::new(nmongo_drop_index)),
        ("nmongo_list_collections", Rc::new(nmongo_list_collections)),
        ("nmongo_drop_collection", Rc::new(nmongo_drop_collection)),
        ("nmongo_bulk_write", Rc::new(nmongo_bulk_write)),
        ("nmongo_start_session", Rc::new(nmongo_start_session)),
        ("nmongo_start_transaction", Rc::new(nmongo_start_transaction)),
        ("nmongo_commit_transaction", Rc::new(nmongo_commit_transaction)),
        ("nmongo_abort_transaction", Rc::new(nmongo_abort_transaction)),
        ("nmongo_end_session", Rc::new(nmongo_end_session)),
        ("nmongo_gridfs_upload", Rc::new(nmongo_gridfs_upload)),
        ("nmongo_gridfs_download", Rc::new(nmongo_gridfs_download)),
        ("nmongo_gridfs_delete", Rc::new(nmongo_gridfs_delete)),
        ("nmongo_gridfs_list", Rc::new(nmongo_gridfs_list)),
        ("nmongo_watch", Rc::new(nmongo_watch)),
        ("nmongo_watch_next", Rc::new(nmongo_watch_next)),
        ("nmongo_watch_close", Rc::new(nmongo_watch_close)),
        ("nmongo_object_id", Rc::new(nmongo_object_id)),
        ("nmongo_is_object_id", Rc::new(nmongo_is_object_id)),
        ("nmongo_to_extended_json", Rc::new(nmongo_to_extended_json)),
        ("nmongo_from_extended_json", Rc::new(nmongo_from_extended_json)),
        ("nmongo_async_find", Rc::new(nmongo_async_find)),
        ("nmongo_async_bulk_write", Rc::new(nmongo_async_bulk_write)),
        ("nmongo_task_done", Rc::new(nmongo_task_done)),
        ("nmongo_task_wait", Rc::new(nmongo_task_wait)),
        ("nmongo_task_result", Rc::new(nmongo_task_result)),
        ("nmongo_task_cancel", Rc::new(nmongo_task_cancel)),
    ]
}

pub fn namespace() -> Value {
    let mut map = HashMap::new();
    let bind = |map: &mut HashMap<String, ValueRef>, name: &str, f: NativeFn| {
        map.insert(name.to_string(), Value::NativeFunction(f).ref_cell());
    };
    bind(&mut map, "connect", Rc::new(nmongo_connect));
    bind(&mut map, "connect_uri", Rc::new(nmongo_connect_uri));
    bind(&mut map, "close", Rc::new(nmongo_close));
    bind(&mut map, "ping", Rc::new(nmongo_ping));
    bind(&mut map, "list_databases", Rc::new(nmongo_list_databases));
    bind(&mut map, "find", Rc::new(nmongo_find));
    bind(&mut map, "find_one", Rc::new(nmongo_find_one));
    bind(&mut map, "insert_one", Rc::new(nmongo_insert_one));
    bind(&mut map, "insert_many", Rc::new(nmongo_insert_many));
    bind(&mut map, "update_one", Rc::new(nmongo_update_one));
    bind(&mut map, "update_many", Rc::new(nmongo_update_many));
    bind(&mut map, "replace_one", Rc::new(nmongo_replace_one));
    bind(&mut map, "delete_one", Rc::new(nmongo_delete_one));
    bind(&mut map, "delete_many", Rc::new(nmongo_delete_many));
    bind(&mut map, "count_documents", Rc::new(nmongo_count_documents));
    bind(&mut map, "distinct", Rc::new(nmongo_distinct));
    bind(&mut map, "aggregate", Rc::new(nmongo_aggregate));
    bind(&mut map, "create_index", Rc::new(nmongo_create_index));
    bind(&mut map, "list_indexes", Rc::new(nmongo_list_indexes));
    bind(&mut map, "drop_index", Rc::new(nmongo_drop_index));
    bind(&mut map, "list_collections", Rc::new(nmongo_list_collections));
    bind(&mut map, "drop_collection", Rc::new(nmongo_drop_collection));
    bind(&mut map, "bulk_write", Rc::new(nmongo_bulk_write));
    bind(&mut map, "start_session", Rc::new(nmongo_start_session));
    bind(&mut map, "start_transaction", Rc::new(nmongo_start_transaction));
    bind(&mut map, "commit_transaction", Rc::new(nmongo_commit_transaction));
    bind(&mut map, "abort_transaction", Rc::new(nmongo_abort_transaction));
    bind(&mut map, "end_session", Rc::new(nmongo_end_session));
    bind(&mut map, "gridfs_upload", Rc::new(nmongo_gridfs_upload));
    bind(&mut map, "gridfs_download", Rc::new(nmongo_gridfs_download));
    bind(&mut map, "gridfs_delete", Rc::new(nmongo_gridfs_delete));
    bind(&mut map, "gridfs_list", Rc::new(nmongo_gridfs_list));
    bind(&mut map, "watch", Rc::new(nmongo_watch));
    bind(&mut map, "watch_next", Rc::new(nmongo_watch_next));
    bind(&mut map, "watch_close", Rc::new(nmongo_watch_close));
    bind(&mut map, "object_id", Rc::new(nmongo_object_id));
    bind(&mut map, "is_object_id", Rc::new(nmongo_is_object_id));
    bind(&mut map, "to_extended_json", Rc::new(nmongo_to_extended_json));
    bind(&mut map, "from_extended_json", Rc::new(nmongo_from_extended_json));
    bind(&mut map, "async_find", Rc::new(nmongo_async_find));
    bind(&mut map, "async_bulk_write", Rc::new(nmongo_async_bulk_write));
    bind(&mut map, "task_done", Rc::new(nmongo_task_done));
    bind(&mut map, "task_wait", Rc::new(nmongo_task_wait));
    bind(&mut map, "task_result", Rc::new(nmongo_task_result));
    bind(&mut map, "task_cancel", Rc::new(nmongo_task_cancel));
    Value::Object(map)
}

pub const MODULE_NAME: &str = "nmongo";
pub const MODULE_PATHS: &[&str] = &["nmongo", "std/nmongo"];

pub fn builtins() -> Vec<(&'static str, NativeFn)> {
    all_builtins()
}
