//! Index management.

use super::common::*;
use super::handles::with_client;
use super::runtime::block_on;
use super::types::{bson_doc_to_niao_ref, niao_to_bson};
use crate::{error_from_runtime, NiaoResult, Value, ValueRef};
use futures::StreamExt;
use mongodb::bson::Document;
use mongodb::options::IndexOptions;
use niao_ast::Span;

pub fn nmongo_create_index(args: &[ValueRef], span: Span) -> NiaoResult<ValueRef> {
    arity_range(args, 4, 5, "nmongo_create_index", span)?;
    let (client_id, db, coll) = db_coll_args(args, "nmongo_create_index", span)?;
    let keys = niao_to_bson(&args[3], span)?;
    let opts_map = optional_object_arg(args, 4);

    let mut index_options = IndexOptions::default();
    if let Some(map) = &opts_map {
        if let Some(name) = map.get("name").and_then(|v| match &*v.borrow() {
            crate::Value::String(s) => Some(s.clone()),
            _ => None,
        }) {
            index_options.name = Some(name);
        }
        if let Some(unique) = map.get("unique").and_then(|v| match &*v.borrow() {
            crate::Value::Bool(b) => Some(*b),
            _ => None,
        }) {
            index_options.unique = Some(unique);
        }
        if let Some(sparse) = map.get("sparse").and_then(|v| match &*v.borrow() {
            crate::Value::Bool(b) => Some(*b),
            _ => None,
        }) {
            index_options.sparse = Some(sparse);
        }
    }

    let model = mongodb::IndexModel::builder()
        .keys(keys)
        .options(index_options)
        .build();

    with_client(client_id, "nmongo_create_index", span, |client| {
        block_on(async move {
            let collection = client.database(&db).collection::<Document>(&coll);
            let result = collection
                .create_index(model)
                .await
                .map_err(|e| e.to_string())?;
            Ok(result.index_name)
        })
    })
    .map(|name| Value::String(name).ref_cell())
    .or_else(|e| Ok(error_from_runtime(&e)))
}

pub fn nmongo_list_indexes(args: &[ValueRef], span: Span) -> NiaoResult<ValueRef> {
    arity(args, 3, "nmongo_list_indexes", span)?;
    let (client_id, db, coll) = db_coll_args(args, "nmongo_list_indexes", span)?;

    with_client(client_id, "nmongo_list_indexes", span, |client| {
        block_on(async move {
            let collection = client.database(&db).collection::<Document>(&coll);
            let mut cursor = collection.list_indexes().await.map_err(|e| e.to_string())?;
            let mut indexes = Vec::new();
            while let Some(model) = cursor.next().await {
                let model = model.map_err(|e| e.to_string())?;
                let doc = bson::to_document(&model).map_err(|e| e.to_string())?;
                indexes.push(bson_doc_to_niao_ref(&doc));
            }
            Ok(indexes)
        })
    })
    .map(|rows| Value::Array(rows).ref_cell())
    .or_else(|e| Ok(error_from_runtime(&e)))
}

pub fn nmongo_drop_index(args: &[ValueRef], span: Span) -> NiaoResult<ValueRef> {
    arity(args, 4, "nmongo_drop_index", span)?;
    let (client_id, db, coll) = db_coll_args(args, "nmongo_drop_index", span)?;
    let name = string_arg(args, 3, "nmongo_drop_index", span)?;

    with_client(client_id, "nmongo_drop_index", span, |client| {
        block_on(async move {
            let collection = client.database(&db).collection::<Document>(&coll);
            collection
                .drop_index(name)
                .await
                .map_err(|e| e.to_string())?;
            Ok(())
        })
    })
    .map(|_| Value::Nil.ref_cell())
    .or_else(|e| Ok(error_from_runtime(&e)))
}
