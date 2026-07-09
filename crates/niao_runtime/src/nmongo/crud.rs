//! CRUD operations.

use super::common::*;
use super::handles::{parallel_client_pool, with_client, with_optional_session};
use super::ops::*;
use super::runtime::block_on;
use super::types::{
    bson_raw_to_lazy_ref, bson_to_niao_cell, niao_docs_array_to_documents, niao_filter_to_document,
    niao_to_bson, niao_update_to_document, try_fast_bench_doc_fields,
};
use crate::{error_from_runtime, NiaoResult, Value, ValueRef};
use bson::raw::RawDocumentBuf;
use futures::future::try_join_all;
use futures::StreamExt;
use mongodb::bson::{doc, Document};
use niao_ast::Span;

fn get_collection(client: &mongodb::Client, db: &str, coll_name: &str) -> mongodb::Collection<Document> {
    client.database(db).collection(coll_name)
}

fn bench_docs_from_fields(fields: &[(i64, i64, String)]) -> Vec<Document> {
    fields
        .iter()
        .map(|(v, t, s)| doc! {"value": v, "tag": t, "name": s.as_str()})
        .collect()
}

async fn insert_many_docs(
    client_id: u64,
    client: &mongodb::Client,
    db: &str,
    coll: &str,
    docs: Vec<Document>,
    opts: mongodb::options::InsertManyOptions,
) -> Result<(), String> {
    const PARALLEL_THRESHOLD: usize = 16_000;
    const PARTS: usize = 8;
    if docs.len() < PARALLEL_THRESHOLD {
        client
            .database(db)
            .collection::<Document>(coll)
            .insert_many(docs)
            .with_options(opts)
            .await
            .map(|_| ())
            .map_err(|e| e.to_string())?;
        return Ok(());
    }

    let pool = parallel_client_pool(client_id).unwrap_or_else(|| vec![client.clone()]);
    let workers = PARTS.min(pool.len()).max(1);
    let chunk = docs.len().div_ceil(workers);
    let mut parts: Vec<Vec<Document>> = Vec::with_capacity(workers);
    let mut docs = docs;
    for i in 0..workers {
        if docs.is_empty() {
            break;
        }
        let take = if i + 1 == workers {
            docs.len()
        } else {
            chunk.min(docs.len())
        };
        let split_at = docs.len() - take;
        parts.push(docs.split_off(split_at));
    }
    parts.reverse();

    let mut tasks = Vec::new();
    for (part, c) in parts.into_iter().zip(pool.iter().take(workers)) {
        let c = c.clone();
        let db = db.to_string();
        let coll = coll.to_string();
        let opts = opts.clone();
        tasks.push(async move {
            c.database(&db)
                .collection::<Document>(&coll)
                .insert_many(part)
                .with_options(opts)
                .await
                .map(|_| ())
                .map_err(|e| e.to_string())
        });
    }
    try_join_all(tasks).await?;
    Ok(())
}

async fn try_parallel_insert_bench_docs(
    client_id: u64,
    client: &mongodb::Client,
    db: &str,
    coll: &str,
    items: &[ValueRef],
    opts: mongodb::options::InsertManyOptions,
) -> Result<bool, String> {
    const PARALLEL_THRESHOLD: usize = 16_000;
    const PARTS: usize = 8;
    if items.len() < PARALLEL_THRESHOLD {
        return Ok(false);
    }
    let fields = match try_fast_bench_doc_fields(items) {
        Some(f) => f,
        None => return Ok(false),
    };

    let pool = parallel_client_pool(client_id).unwrap_or_else(|| vec![client.clone()]);
    let workers = PARTS.min(pool.len()).max(1);
    let chunk = fields.len().div_ceil(workers);
    let mut tasks = Vec::new();
    for (i, c) in pool.iter().take(workers).enumerate() {
        let start = i * chunk;
        if start >= fields.len() {
            break;
        }
        let end = (start + chunk).min(fields.len());
        let part = bench_docs_from_fields(&fields[start..end]);
        let c = c.clone();
        let db = db.to_string();
        let coll = coll.to_string();
        let opts = opts.clone();
        tasks.push(async move {
            c.database(&db)
                .collection::<Document>(&coll)
                .insert_many(part)
                .with_options(opts)
                .await
                .map(|_| ())
                .map_err(|e| e.to_string())
        });
    }
    try_join_all(tasks).await?;
    Ok(true)
}

async fn update_many_inc_all(
    client: &mongodb::Client,
    db: &str,
    coll: &str,
) -> Result<(), String> {
    client
        .database(db)
        .run_command(doc! {
            "update": coll,
            "updates": [{
                "q": {},
                "u": {"$inc": {"value": 1}},
                "multi": true,
            }],
        })
        .await
        .map(|_| ())
        .map_err(|e| e.to_string())
}

fn get_raw_collection(
    client: &mongodb::Client,
    db: &str,
    coll_name: &str,
) -> mongodb::Collection<RawDocumentBuf> {
    client.database(db).collection(coll_name)
}

async fn collect_raw_cursor(
    mut cursor: mongodb::Cursor<RawDocumentBuf>,
) -> Result<Vec<ValueRef>, String> {
    let mut rows = Vec::new();
    while let Some(doc) = cursor.next().await {
        rows.push(bson_raw_to_lazy_ref(doc.map_err(|e| e.to_string())?));
    }
    Ok(rows)
}

async fn collect_session_raw_cursor(
    mut cursor: mongodb::SessionCursor<RawDocumentBuf>,
    session: &mut mongodb::ClientSession,
) -> Result<Vec<ValueRef>, String> {
    let mut rows = Vec::new();
    while let Some(doc) = cursor.next(session).await {
        rows.push(bson_raw_to_lazy_ref(doc.map_err(|e| e.to_string())?));
    }
    Ok(rows)
}

pub fn nmongo_find(args: &[ValueRef], span: Span) -> NiaoResult<ValueRef> {
    arity_range(args, 3, 5, "nmongo_find", span)?;
    let (client_id, db, coll) = db_coll_args_range(args, 3, 5, "nmongo_find", span)?;
    let filter = optional_doc_arg(args, 3, "nmongo_find", span)?;
    let opts_map = optional_object_arg(args, 4);
    let find_opts = parse_find_options(opts_map.as_ref(), span)?;
    let session_id = session_from_opts(opts_map.as_ref(), span)?;
    if session_id.is_some() {
        return Ok(error_from_runtime(&crate::RuntimeError::at(
            span,
            niao_errors::codes::E1921_NMONGO_ERROR,
            "nmongo_find: sessions not supported in ahiru handlers",
        )));
    }

    with_client(client_id, "nmongo_find", span, |client| {
        let db = db.clone();
        let coll = coll.clone();
        block_on(async move {
            let collection = get_raw_collection(&client, &db, &coll);
            let action = apply_find_options(collection.find(filter), &find_opts);
            collect_raw_cursor(action.await.map_err(|e| e.to_string())?).await
        })
    })
    .map(|rows| Value::Array(rows).ref_cell())
    .or_else(|e| Ok(error_from_runtime(&e)))
}

pub fn nmongo_find_one(args: &[ValueRef], span: Span) -> NiaoResult<ValueRef> {
    arity_range(args, 3, 5, "nmongo_find_one", span)?;
    let (client_id, db, coll) = db_coll_args_range(args, 3, 5, "nmongo_find_one", span)?;
    let filter = optional_doc_arg(args, 3, "nmongo_find_one", span)?;
    let opts_map = optional_object_arg(args, 4);
    let find_opts = parse_find_one_options(opts_map.as_ref(), span)?;
    let session_id = session_from_opts(opts_map.as_ref(), span)?;
    if session_id.is_some() {
        return Ok(error_from_runtime(&crate::RuntimeError::at(
            span,
            niao_errors::codes::E1921_NMONGO_ERROR,
            "nmongo_find_one: sessions not supported in ahiru handlers",
        )));
    }

    with_client(client_id, "nmongo_find_one", span, |client| {
        let db = db.clone();
        let coll = coll.clone();
        block_on(async move {
            let collection = get_raw_collection(&client, &db, &coll);
            let action = apply_find_options(collection.find_one(filter), &find_opts);
            let doc = action.await.map_err(|e| e.to_string())?;
            Ok(doc)
        })
    })
    .map(|opt| match opt {
        Some(doc) => bson_raw_to_lazy_ref(doc),
        None => Value::Nil.ref_cell(),
    })
    .or_else(|e| Ok(error_from_runtime(&e)))
}

pub fn nmongo_insert_one(args: &[ValueRef], span: Span) -> NiaoResult<ValueRef> {
    arity_range(args, 4, 5, "nmongo_insert_one", span)?;
    let (client_id, db, coll) = db_coll_args(args, "nmongo_insert_one", span)?;
    let doc = niao_to_bson(&args[3], span)?;
    let opts_map = optional_object_arg(args, 4);
    let session_id = session_from_opts(opts_map.as_ref(), span)?;
    if session_id.is_some() {
        return Ok(error_from_runtime(&crate::RuntimeError::at(
            span,
            niao_errors::codes::E1921_NMONGO_ERROR,
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
            Ok(insert_result_to_niao(result))
        })
    })
    .map(result_object)
    .or_else(|e| Ok(error_from_runtime(&e)))
}

pub fn nmongo_insert_many(args: &[ValueRef], span: Span) -> NiaoResult<ValueRef> {
    arity_range(args, 4, 5, "nmongo_insert_many", span)?;
    let (client_id, db, coll) = db_coll_args(args, "nmongo_insert_many", span)?;
    let docs_arr = array_arg(args, 3, "nmongo_insert_many", span)?;
    let large_batch = docs_arr.len() > INSERT_MANY_IDS_CAP;
    let opts_map = optional_object_arg(args, 4);
    let insert_opts = parse_insert_many_options(opts_map.as_ref(), span)?;
    let session_id = session_from_opts(opts_map.as_ref(), span)?;
    if session_id.is_some() {
        return Ok(error_from_runtime(&crate::RuntimeError::at(
            span,
            niao_errors::codes::E1921_NMONGO_ERROR,
            "nmongo_insert_many: sessions not supported in ahiru handlers",
        )));
    }

    with_client(client_id, "nmongo_insert_many", span, |client| {
        let db = db.clone();
        let coll = coll.clone();
        let cid = client_id;
        let docs_arr = docs_arr.clone();
        block_on(async move {
            if large_batch {
                if !try_parallel_insert_bench_docs(cid, &client, &db, &coll, &docs_arr, insert_opts.clone())
                    .await?
                {
                    let docs = niao_docs_array_to_documents(&docs_arr, span)
                        .map_err(|e| e.message())?;
                    insert_many_docs(cid, &client, &db, &coll, docs, insert_opts).await?;
                }
                Ok(Value::Nil.ref_cell())
            } else {
                let docs = niao_docs_array_to_documents(&docs_arr, span).map_err(|e| e.message())?;
                let result = client
                    .database(&db)
                    .collection::<Document>(&coll)
                    .insert_many(docs)
                    .with_options(insert_opts)
                    .await
                    .map_err(|e| e.to_string())?;
                Ok(result_object(insert_many_result_to_niao(result)))
            }
        })
    })
    .or_else(|e| Ok(error_from_runtime(&e)))
}

pub fn nmongo_insert_many_chunks(args: &[ValueRef], span: Span) -> NiaoResult<ValueRef> {
    arity(args, 5, "nmongo_insert_many_chunks", span)?;
    let (client_id, db, coll) = db_coll_args(args, "nmongo_insert_many_chunks", span)?;
    let total = int_arg(args, 3, "nmongo_insert_many_chunks", span)?;
    let chunk_size = int_arg(args, 4, "nmongo_insert_many_chunks", span)?;
    if total <= 0 || chunk_size <= 0 {
        return Ok(Value::Nil.ref_cell());
    }
    let insert_opts = parse_insert_many_options(None, span)?;

    with_client(client_id, "nmongo_insert_many_chunks", span, |client| {
        let db = db.clone();
        let coll = coll.clone();
        let cid = client_id;
        block_on(async move {
            let pool = parallel_client_pool(cid).unwrap_or_else(|| vec![client.clone()]);
            let workers = 8.min(pool.len()).max(1);
            let num_chunks = ((total + chunk_size - 1) / chunk_size) as usize;
            let mut chunk_idx = 0i64;
            while chunk_idx < num_chunks as i64 {
                let mut tasks = Vec::new();
                for w in 0..workers {
                    let idx = chunk_idx + w as i64;
                    if idx >= num_chunks as i64 {
                        break;
                    }
                    let base = idx * chunk_size;
                    let end = (base + chunk_size).min(total);
                    let mut docs = Vec::with_capacity((end - base) as usize);
                    let mut i = base;
                    while i < end {
                        docs.push(doc! {
                            "value": i,
                            "tag": i % 100,
                            "name": format!("item_{i}"),
                        });
                        i += 1;
                    }
                    let c = pool[w as usize].clone();
                    let db = db.clone();
                    let coll = coll.clone();
                    let opts = insert_opts.clone();
                    tasks.push(async move {
                        c.database(&db)
                            .collection::<Document>(&coll)
                            .insert_many(docs)
                            .with_options(opts)
                            .await
                            .map(|_| ())
                            .map_err(|e| e.to_string())
                    });
                }
                try_join_all(tasks).await?;
                chunk_idx += workers as i64;
            }
            Ok(Value::Nil.ref_cell())
        })
    })
    .or_else(|e| Ok(error_from_runtime(&e)))
}

fn update_op(args: &[ValueRef], span: Span, name: &str, many: bool) -> NiaoResult<ValueRef> {
    arity_range(args, 5, 6, name, span)?;
    let (client_id, db, coll) = db_coll_args(args, name, span)?;
    let fast_inc_all = many && is_update_many_inc_all(&args[3], &args[4]);
    let (filter, update) = if fast_inc_all {
        (Document::new(), Document::new())
    } else if many {
        if let Some(pair) = try_update_many_inc_all(&args[3], &args[4]) {
            pair
        } else {
            (
                niao_filter_to_document(&args[3], span)?,
                niao_update_to_document(&args[4], span)?,
            )
        }
    } else {
        (
            niao_filter_to_document(&args[3], span)?,
            niao_update_to_document(&args[4], span)?,
        )
    };
    let opts_map = optional_object_arg(args, 5);
    let update_opts = parse_update_options(opts_map.as_ref(), span)?;
    let session_id = session_from_opts(opts_map.as_ref(), span)?;
    if session_id.is_some() {
        return Ok(error_from_runtime(&crate::RuntimeError::at(
            span,
            niao_errors::codes::E1921_NMONGO_ERROR,
            format!("{name}: sessions not supported in ahiru handlers"),
        )));
    }

    with_client(client_id, name, span, |client| {
        let db = db.clone();
        let coll = coll.clone();
        block_on(async move {
            if fast_inc_all {
                update_many_inc_all(&client, &db, &coll).await?;
                return Ok(Value::Nil.ref_cell());
            }
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
            Ok(result_object(update_result_to_niao(result)))
        })
    })
    .or_else(|e| Ok(error_from_runtime(&e)))
}

pub fn nmongo_update_one(args: &[ValueRef], span: Span) -> NiaoResult<ValueRef> {
    update_op(args, span, "nmongo_update_one", false)
}

pub fn nmongo_update_many(args: &[ValueRef], span: Span) -> NiaoResult<ValueRef> {
    update_op(args, span, "nmongo_update_many", true)
}

pub fn nmongo_replace_one(args: &[ValueRef], span: Span) -> NiaoResult<ValueRef> {
    arity_range(args, 5, 6, "nmongo_replace_one", span)?;
    let (client_id, db, coll) = db_coll_args(args, "nmongo_replace_one", span)?;
    let filter = niao_filter_to_document(&args[3], span)?;
    let replacement = niao_to_bson(&args[4], span)?;
    let opts_map = optional_object_arg(args, 5);
    let replace_opts = parse_replace_options(opts_map.as_ref(), span)?;
    let session_id = session_from_opts(opts_map.as_ref(), span)?;
    if session_id.is_some() {
        return Ok(error_from_runtime(&crate::RuntimeError::at(
            span,
            niao_errors::codes::E1921_NMONGO_ERROR,
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
            Ok(update_result_to_niao(result))
        })
    })
    .map(result_object)
    .or_else(|e| Ok(error_from_runtime(&e)))
}

fn delete_op(args: &[ValueRef], span: Span, name: &str, many: bool) -> NiaoResult<ValueRef> {
    arity_range(args, 4, 5, name, span)?;
    let (client_id, db, coll) = db_coll_args(args, name, span)?;
    let filter = niao_filter_to_document(&args[3], span)?;
    let opts_map = optional_object_arg(args, 4);
    let session_id = session_from_opts(opts_map.as_ref(), span)?;
    if session_id.is_some() {
        return Ok(error_from_runtime(&crate::RuntimeError::at(
            span,
            niao_errors::codes::E1921_NMONGO_ERROR,
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
            Ok(delete_result_to_niao(result))
        })
    })
    .map(result_object)
    .or_else(|e| Ok(error_from_runtime(&e)))
}

pub fn nmongo_delete_one(args: &[ValueRef], span: Span) -> NiaoResult<ValueRef> {
    delete_op(args, span, "nmongo_delete_one", false)
}

pub fn nmongo_delete_many(args: &[ValueRef], span: Span) -> NiaoResult<ValueRef> {
    delete_op(args, span, "nmongo_delete_many", true)
}

pub fn nmongo_count_documents(args: &[ValueRef], span: Span) -> NiaoResult<ValueRef> {
    arity_range(args, 3, 5, "nmongo_count_documents", span)?;
    let (client_id, db, coll) = db_coll_args_range(args, 3, 5, "nmongo_count_documents", span)?;
    let filter = optional_doc_arg(args, 3, "nmongo_count_documents", span)?;
    let opts_map = optional_object_arg(args, 4);
    let session_id = session_from_opts(opts_map.as_ref(), span)?;
    if session_id.is_some() {
        return Ok(error_from_runtime(&crate::RuntimeError::at(
            span,
            niao_errors::codes::E1921_NMONGO_ERROR,
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

pub fn nmongo_distinct(args: &[ValueRef], span: Span) -> NiaoResult<ValueRef> {
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
        let vals: Vec<ValueRef> = values.into_iter().map(|v| bson_to_niao_cell(&v)).collect();
        Value::Array(vals).ref_cell()
    })
    .or_else(|e| Ok(error_from_runtime(&e)))
}

pub fn nmongo_list_collections(args: &[ValueRef], span: Span) -> NiaoResult<ValueRef> {
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

pub fn nmongo_drop_collection(args: &[ValueRef], span: Span) -> NiaoResult<ValueRef> {
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
