//! GridFS file storage.

use super::common::*;
use super::handles::with_client;
use super::runtime::block_on;
use super::types::{bson_doc_to_niao_ref, niao_to_bson};
use crate::{error_from_runtime, NiaoResult, RuntimeError, Value, ValueRef};
use futures::StreamExt;
use mongodb::bson::doc;
use niao_ast::Span;
use niao_errors::codes;
use futures::{AsyncReadExt, AsyncWriteExt};

fn data_to_bytes(val: &ValueRef, span: Span) -> Result<Vec<u8>, RuntimeError> {
    match &*val.borrow() {
        Value::ByteArray(b) => Ok(b.clone()),
        Value::String(s) => Ok(s.as_bytes().to_vec()),
        other => Err(RuntimeError::at(
            span,
            codes::E1927_NMONGO_GRIDFS,
            format!("data must be string or byte_array, got {}", other.type_name()),
        )),
    }
}

pub fn nmongo_gridfs_upload(args: &[ValueRef], span: Span) -> NiaoResult<ValueRef> {
    arity_range(args, 4, 5, "nmongo_gridfs_upload", span)?;
    let client_id = client_arg(args, 0, "nmongo_gridfs_upload", span)?;
    let db = string_arg(args, 1, "nmongo_gridfs_upload", span)?;
    validate_name(&db, "database", span)?;
    let filename = string_arg(args, 2, "nmongo_gridfs_upload", span)?;
    let data = data_to_bytes(&args[3], span)?;
    let opts_map = optional_object_arg(args, 4);
    let metadata = if let Some(map) = &opts_map {
        if let Some(m) = map.get("metadata") {
            Some(niao_to_bson(m, span)?)
        } else {
            None
        }
    } else {
        None
    };
    let chunk_size = opts_map.as_ref().and_then(|m| {
        m.get("chunk_size")
            .and_then(|v| match &*v.borrow() {
                Value::Int(n) => Some(*n as u32),
                _ => None,
            })
    });

    with_client(client_id, "nmongo_gridfs_upload", span, |client| {
        block_on(async move {
            let bucket = client.database(&db).gridfs_bucket(None);
            let mut open = bucket.open_upload_stream(filename);
            if let Some(meta) = metadata {
                open = open.metadata(meta);
            }
            if let Some(cs) = chunk_size {
                open = open.chunk_size_bytes(cs);
            }
            let mut stream = open.await.map_err(|e| e.to_string())?;
            stream
                .write_all(&data)
                .await
                .map_err(|e| e.to_string())?;
            stream.close().await.map_err(|e| e.to_string())?;
            Ok(stream.id().clone())
        })
    })
    .map(|id| super::types::bson_to_niao(id).ref_cell())
    .or_else(|e| Ok(error_from_runtime(&e)))
}

pub fn nmongo_gridfs_download(args: &[ValueRef], span: Span) -> NiaoResult<ValueRef> {
    arity_range(args, 3, 4, "nmongo_gridfs_download", span)?;
    let client_id = client_arg(args, 0, "nmongo_gridfs_download", span)?;
    let db = string_arg(args, 1, "nmongo_gridfs_download", span)?;
    validate_name(&db, "database", span)?;
    let filename = string_arg(args, 2, "nmongo_gridfs_download", span)?;

    with_client(client_id, "nmongo_gridfs_download", span, |client| {
        block_on(async move {
            let bucket = client.database(&db).gridfs_bucket(None);
            let mut stream = bucket
                .open_download_stream_by_name(filename)
                .await
                .map_err(|e| e.to_string())?;
            let mut data = Vec::new();
            stream
                .read_to_end(&mut data)
                .await
                .map_err(|e| e.to_string())?;
            Ok(data)
        })
    })
    .map(|bytes| Value::ByteArray(bytes).ref_cell())
    .or_else(|e| Ok(error_from_runtime(&e)))
}

pub fn nmongo_gridfs_delete(args: &[ValueRef], span: Span) -> NiaoResult<ValueRef> {
    arity_range(args, 3, 4, "nmongo_gridfs_delete", span)?;
    let client_id = client_arg(args, 0, "nmongo_gridfs_delete", span)?;
    let db = string_arg(args, 1, "nmongo_gridfs_delete", span)?;
    validate_name(&db, "database", span)?;
    let filename = string_arg(args, 2, "nmongo_gridfs_delete", span)?;

    with_client(client_id, "nmongo_gridfs_delete", span, |client| {
        block_on(async move {
            let bucket = client.database(&db).gridfs_bucket(None);
            bucket
                .delete_by_name(filename)
                .await
                .map_err(|e| e.to_string())?;
            Ok(())
        })
    })
    .map(|_| Value::Nil.ref_cell())
    .or_else(|e| Ok(error_from_runtime(&e)))
}

pub fn nmongo_gridfs_list(args: &[ValueRef], span: Span) -> NiaoResult<ValueRef> {
    arity_range(args, 2, 3, "nmongo_gridfs_list", span)?;
    let client_id = client_arg(args, 0, "nmongo_gridfs_list", span)?;
    let db = string_arg(args, 1, "nmongo_gridfs_list", span)?;
    validate_name(&db, "database", span)?;

    with_client(client_id, "nmongo_gridfs_list", span, |client| {
        block_on(async move {
            let bucket = client.database(&db).gridfs_bucket(None);
            let mut cursor = bucket.find(doc! {}).await.map_err(|e| e.to_string())?;
            let mut files = Vec::new();
            while let Some(doc) = cursor.next().await {
                let fcd = doc.map_err(|e| e.to_string())?;
                let bson_doc = bson::to_document(&fcd).map_err(|e| e.to_string())?;
                files.push(bson_doc_to_niao_ref(&bson_doc));
            }
            Ok(files)
        })
    })
    .map(|rows| Value::Array(rows).ref_cell())
    .or_else(|e| Ok(error_from_runtime(&e)))
}
