//! CRUD operations.

use super::common::*;
use super::handles::{with_client, with_optional_session};
use super::ops::*;
use super::runtime::block_on;
use super::types::{bson_doc_to_neko_ref, bson_to_neko, neko_to_bson};
use crate::{error_from_runtime, NekoResult, Value, ValueRef};
use futures::StreamExt;
use mongodb::bson::Document;
use neko_ast::Span;

fn get_collection(client: &mongodb::Client, db: &str, coll_name: &str) -> mongodb::Collection<Document> {
    client.database(db).collection(coll_name)
}

async fn collect_docs(mut cursor: mongodb::Cursor<Document>) -> Result<Vec<Document>, String> {
    let mut rows = Vec::new();
    while let Some(doc) = cursor.next().await {
        rows.push(doc.map_err(|e| e.to_string())?);
    }
    Ok(rows)
}

async fn collect_cursor(mut cursor: mongodb::Cursor<Document>) -> Result<Vec<ValueRef>, String> {
    let mut rows = Vec::new();
    while let Some(doc) = cursor.next().await {
        rows.push(bson_doc_to_neko_ref(&doc.map_err(|e| e.to_string())?));
    }
    Ok(rows)
}

async fn collect_session_cursor(
    mut cursor: mongodb::SessionCursor<Document>,
    session: &mut mongodb::ClientSession,
) -> Result<Vec<ValueRef>, String> {
    let mut rows = Vec::new();
    while let Some(doc) = cursor.next(session).await {
        rows.push(bson_doc_to_neko_ref(&doc.map_err(|e| e.to_string())?));
    }
    Ok(rows)
}

pub fn nmongo_find(args: &[ValueRef], span: Span) -> NekoResult<ValueRef> {
    arity_range(args, 3, 5, "nmongo_find", span)?;
    let (client_id, db, coll) = db_coll_args_range(args, 3, 5, "nmongo_find", span)?;
    let filter = optional_doc_arg(args, 3, "nmongo_find", span)?;
    let opts_map = optional_object_arg(args, 4);
    let find_opts = parse_find_options(opts_map.as_ref(), span)?;
    let session_id = session_from_opts(opts_map.as_ref(), span)?;
    if session_id.is_some() {
        return Ok(error_from_runtime(&crate::RuntimeError::at(
            span,
            neko_errors::codes::E1921_NMONGO_ERROR,
            "nmongo_find: sessions not supported in ahiru handlers",
        )));
    }

    with_client(client_id, "nmongo_find", span, |client| {
        let db = db.clone();
        let coll = coll.clone();
        block_on(async move {
            let collection = get_collection(&client, &db, &coll);
            let action = apply_find_options(collection.find(filter), &find_opts);
            collect_docs(action.await.map_err(|e| e.to_string())?).await
        })
    })
    .map(|rows| {
        let refs: Vec<ValueRef> = rows.iter().map(bson_doc_to_neko_ref).collect();
        Value::Array(refs).ref_cell()
    })
    .or_else(|e| Ok(error_from_runtime(&e)))
}

pub fn nmongo_find_one(args: &[ValueRef], span: Span) -> NekoResult<ValueRef> {
    arity_range(args, 3, 5, "nmongo_find_one", span)?;
    let (client_id, db, coll) = db_coll_args_range(args, 3, 5, "nmongo_find_one", span)?;
    let filter = optional_doc_arg(args, 3, "nmongo_find_one", span)?;
    let opts_map = optional_object_arg(args, 4);
    let find_opts = parse_find_one_options(opts_map.as_ref(), span)?;
    let session_id = session_from_opts(opts_map.as_ref(), span)?;
    if session_id.is_some() {
        return Ok(error_from_runtime(&crate::RuntimeError::at(
            span,
            neko_errors::codes::E1921_NMONGO_ERROR,
            "nmongo_find_one: sessions not supported in ahiru handlers",
        )));
    }

    with_client(client_id, "nmongo_find_one", span, |client| {
        let db = db.clone();
        let coll = coll.clone();
        block_on(async move {
            let collection = get_collection(&client, &db, &coll);
            let action = apply_find_options(collection.find_one(filter), &find_opts);
            let doc = action.await.map_err(|e| e.to_string())?;
            Ok(doc)
        })
    })
    .map(|opt| match opt {
        Some(doc) => bson_doc_to_neko_ref(&doc),
        None => Value::Nil.ref_cell(),
    })
    .or_else(|e| Ok(error_from_runtime(&e)))
}

pub fn nmongo_insert_one(args: &[ValueRef], span: Span) -> NekoResult<ValueRef> {
    arity_range(args, 4, 5, "nmongo_insert_one", span)?;
    let (client_id, db, coll) = db_coll_args(args, "nmongo_insert_one", span)?;
    let doc = neko_to_bson(&args[3], span)?;
    let opts_map = optional_object_arg(args, 4);
    let session_id = session_from_opts(opts_map.as_ref(), span)?;
    if session_id.is_some() {
        return Ok(error_from_runtime(&crate::RuntimeError::at(
            span,
            neko_errors::codes::E1921_NMONGO_ERROR,
            "nmongo_insert_one: sessions not supported in ahiru handlers",
        )));
    }

    with_client(client_id, "nmongo_insert_one", span, |client| {
        let db = db.clone();
        let coll = coll.clone();
        block_on(async move {
            let collection = get_collection(&client, &db, &coll);
            let result = collection
                .insert_one(doc)
                .await
                .map_err(|e| e.to_string())?;
            Ok(insert_result_to_neko(result))
        })
    })
    .map(result_object)
    .or_else(|e| Ok(error_from_runtime(&e)))
}

pub fn nmongo_insert_many(args: &[ValueRef], span: Span) -> NekoResult<ValueRef> {
    arity_range(args, 4, 5, "nmongo_insert_many", span)?;
    let (client_id, db, coll) = db_coll_args(args, "nmongo_insert_many", span)?;
    let docs_arr = array_arg(args, 3, "nmongo_insert_many", span)?;
    let mut docs = Vec::with_capacity(docs_arr.len());
    for d in &docs_arr {
        docs.push(neko_to_bson(d, span)?);
    }
    let opts_map = optional_object_arg(args, 4);
    let insert_opts = parse_insert_many_options(opts_map.as_ref(), span)?;
    let session_id = session_from_opts(opts_map.as_ref(), span)?;
    if session_id.is_some() {
        return Ok(error_from_runtime(&crate::RuntimeError::at(
            span,
            neko_errors::codes::E1921_NMONGO_ERROR,
            "nmongo_insert_many: sessions not supported in ahiru handlers",
        )));
    }

    with_client(client_id, "nmongo_insert_many", span, |client| {
        let db = db.clone();
        let coll = coll.clone();
        block_on(async move {
            let collection = get_collection(&client, &db, &coll);
            let result = collection
                .insert_many(docs)
                .with_options(insert_opts)
                .await
                .map_err(|e| e.to_string())?;
            Ok(insert_many_result_to_neko(result))
        })
    })
    .map(result_object)
    .or_else(|e| Ok(error_from_runtime(&e)))
}

fn update_op(args: &[ValueRef], span: Span, name: &str, many: bool) -> NekoResult<ValueRef> {
    arity_range(args, 5, 6, name, span)?;
    let (client_id, db, coll) = db_coll_args(args, name, span)?;
    let filter = neko_to_bson(&args[3], span)?;
    let update = neko_to_bson(&args[4], span)?;
    let opts_map = optional_object_arg(args, 5);
    let update_opts = parse_update_options(opts_map.as_ref(), span)?;
    let session_id = session_from_opts(opts_map.as_ref(), span)?;
    if session_id.is_some() {
        return Ok(error_from_runtime(&crate::RuntimeError::at(
            span,
            neko_errors::codes::E1921_NMONGO_ERROR,
            format!("{name}: sessions not supported in ahiru handlers"),
        )));
    }

    with_client(client_id, name, span, |client| {
        let db = db.clone();
        let coll = coll.clone();
        block_on(async move {
            let collection = get_collection(&client, &db, &coll);
            let result = if many {
                collection
                    .update_many(filter, update)
                    .with_options(update_opts)
                    .await
            } else {
                collection
                    .update_one(filter, update)
                    .with_options(update_opts)
                    .await
            }
            .map_err(|e| e.to_string())?;
            Ok(update_result_to_neko(result))
        })
    })
    .map(result_object)
    .or_else(|e| Ok(error_from_runtime(&e)))
}

pub fn nmongo_update_one(args: &[ValueRef], span: Span) -> NekoResult<ValueRef> {
    update_op(args, span, "nmongo_update_one", false)
}

pub fn nmongo_update_many(args: &[ValueRef], span: Span) -> NekoResult<ValueRef> {
    update_op(args, span, "nmongo_update_many", true)
}

pub fn nmongo_replace_one(args: &[ValueRef], span: Span) -> NekoResult<ValueRef> {
    arity_range(args, 5, 6, "nmongo_replace_one", span)?;
    let (client_id, db, coll) = db_coll_args(args, "nmongo_replace_one", span)?;
    let filter = neko_to_bson(&args[3], span)?;
    let replacement = neko_to_bson(&args[4], span)?;
    let opts_map = optional_object_arg(args, 5);
    let replace_opts = parse_replace_options(opts_map.as_ref(), span)?;
    let session_id = session_from_opts(opts_map.as_ref(), span)?;
    if session_id.is_some() {
        return Ok(error_from_runtime(&crate::RuntimeError::at(
            span,
            neko_errors::codes::E1921_NMONGO_ERROR,
            "nmongo_replace_one: sessions not supported in ahiru handlers",
        )));
    }

    with_client(client_id, "nmongo_replace_one", span, |client| {
        let db = db.clone();
        let coll = coll.clone();
        block_on(async move {
            let collection = get_collection(&client, &db, &coll);
            let result = collection
                .replace_one(filter, replacement)
                .with_options(replace_opts)
                .await
                .map_err(|e| e.to_string())?;
            Ok(update_result_to_neko(result))
        })
    })
    .map(result_object)
    .or_else(|e| Ok(error_from_runtime(&e)))
}

fn delete_op(args: &[ValueRef], span: Span, name: &str, many: bool) -> NekoResult<ValueRef> {
    arity_range(args, 4, 5, name, span)?;
    let (client_id, db, coll) = db_coll_args(args, name, span)?;
    let filter = neko_to_bson(&args[3], span)?;
    let opts_map = optional_object_arg(args, 4);
    let session_id = session_from_opts(opts_map.as_ref(), span)?;
    if session_id.is_some() {
        return Ok(error_from_runtime(&crate::RuntimeError::at(
            span,
            neko_errors::codes::E1921_NMONGO_ERROR,
            format!("{name}: sessions not supported in ahiru handlers"),
        )));
    }

    with_client(client_id, name, span, |client| {
        let db = db.clone();
        let coll = coll.clone();
        block_on(async move {
            let collection = get_collection(&client, &db, &coll);
            let result = if many {
                collection.delete_many(filter).await
            } else {
                collection.delete_one(filter).await
            }
            .map_err(|e| e.to_string())?;
            Ok(delete_result_to_neko(result))
        })
    })
    .map(result_object)
    .or_else(|e| Ok(error_from_runtime(&e)))
}

pub fn nmongo_delete_one(args: &[ValueRef], span: Span) -> NekoResult<ValueRef> {
    delete_op(args, span, "nmongo_delete_one", false)
}

pub fn nmongo_delete_many(args: &[ValueRef], span: Span) -> NekoResult<ValueRef> {
    delete_op(args, span, "nmongo_delete_many", true)
}

pub fn nmongo_count_documents(args: &[ValueRef], span: Span) -> NekoResult<ValueRef> {
    arity_range(args, 3, 5, "nmongo_count_documents", span)?;
    let (client_id, db, coll) = db_coll_args_range(args, 3, 5, "nmongo_count_documents", span)?;
    let filter = optional_doc_arg(args, 3, "nmongo_count_documents", span)?;
    let opts_map = optional_object_arg(args, 4);
    let session_id = session_from_opts(opts_map.as_ref(), span)?;
    if session_id.is_some() {
        return Ok(error_from_runtime(&crate::RuntimeError::at(
            span,
            neko_errors::codes::E1921_NMONGO_ERROR,
            "nmongo_count_documents: sessions not supported in ahiru handlers",
        )));
    }

    with_client(client_id, "nmongo_count_documents", span, |client| {
        let db = db.clone();
        let coll = coll.clone();
        block_on(async move {
            let collection = get_collection(&client, &db, &coll);
            let count = collection
                .count_documents(filter)
                .await
                .map_err(|e| e.to_string())?;
            Ok(count as i64)
        })
    })
    .map(|n| Value::Int(n).ref_cell())
    .or_else(|e| Ok(error_from_runtime(&e)))
}

pub fn nmongo_distinct(args: &[ValueRef], span: Span) -> NekoResult<ValueRef> {
    arity_range(args, 4, 5, "nmongo_distinct", span)?;
    let (client_id, db, coll) = db_coll_args_range(args, 4, 5, "nmongo_distinct", span)?;
    let field = string_arg(args, 3, "nmongo_distinct", span)?;
    let filter = if args.len() >= 5 {
        optional_doc_arg(args, 4, "nmongo_distinct", span)?
    } else {
        Document::new()
    };

    with_client(client_id, "nmongo_distinct", span, |client| {
        let db = db.clone();
        let coll = coll.clone();
        block_on(async move {
            let collection = get_collection(&client, &db, &coll);
            let values = collection
                .distinct(field, filter)
                .await
                .map_err(|e| e.to_string())?;
            Ok(values)
        })
    })
    .map(|values| {
        let vals: Vec<ValueRef> = values.into_iter().map(|v| bson_to_neko(v).ref_cell()).collect();
        Value::Array(vals).ref_cell()
    })
    .or_else(|e| Ok(error_from_runtime(&e)))
}

pub fn nmongo_list_collections(args: &[ValueRef], span: Span) -> NekoResult<ValueRef> {
    arity(args, 2, "nmongo_list_collections", span)?;
    let client_id = client_arg(args, 0, "nmongo_list_collections", span)?;
    let db = string_arg(args, 1, "nmongo_list_collections", span)?;
    validate_name(&db, "database", span)?;

    with_client(client_id, "nmongo_list_collections", span, |client| {
        block_on(async move {
            let names = client
                .database(&db)
                .list_collection_names()
                .await
                .map_err(|e| e.to_string())?;
            Ok(names)
        })
    })
    .map(|names| {
        Value::Array(names.into_iter().map(|n| Value::String(n).ref_cell()).collect()).ref_cell()
    })
    .or_else(|e| Ok(error_from_runtime(&e)))
}

pub fn nmongo_drop_collection(args: &[ValueRef], span: Span) -> NekoResult<ValueRef> {
    arity(args, 3, "nmongo_drop_collection", span)?;
    let (client_id, db, coll) = db_coll_args(args, "nmongo_drop_collection", span)?;

    with_client(client_id, "nmongo_drop_collection", span, |client| {
        let db = db.clone();
        let coll = coll.clone();
        block_on(async move {
            client
                .database(&db)
                .collection::<Document>(&coll)
                .drop()
                .await
                .map_err(|e| e.to_string())?;
            Ok(())
        })
    })
    .map(|_| Value::Nil.ref_cell())
    .or_else(|e| Ok(error_from_runtime(&e)))
}
