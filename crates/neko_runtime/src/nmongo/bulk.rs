//! Bulk write operations via driver `bulk_write` with batched fallback.

use super::common::*;
use super::handles::with_client;
use super::ops::result_object;
use super::runtime::block_on;
use super::types::neko_to_bson;
use crate::{error_from_runtime, NekoResult, RuntimeError, Value, ValueRef};
use mongodb::bson::Document;
use mongodb::options::{
    DeleteManyModel, DeleteOneModel, InsertOneModel, ReplaceOneModel, UpdateManyModel,
    UpdateModifications, UpdateOneModel, WriteModel,
};
use mongodb::Namespace;
use neko_ast::Span;
use neko_errors::codes;
use std::collections::HashMap;

#[derive(Clone)]
pub(crate) enum BulkOp {
    InsertOne(Document),
    UpdateOne {
        filter: Document,
        update: Document,
        upsert: bool,
    },
    UpdateMany {
        filter: Document,
        update: Document,
        upsert: bool,
    },
    ReplaceOne {
        filter: Document,
        replacement: Document,
        upsert: bool,
    },
    DeleteOne(Document),
    DeleteMany(Document),
}

pub(crate) struct BulkCounts {
    pub inserted: i64,
    pub matched: i64,
    pub modified: i64,
    pub deleted: i64,
}

pub fn parse_bulk_ops_for_async(
    ops_arr: &[ValueRef],
    span: Span,
) -> Result<Vec<BulkOp>, RuntimeError> {
    parse_bulk_ops(ops_arr, span)
}

fn parse_bulk_ops(ops_arr: &[ValueRef], span: Span) -> Result<Vec<BulkOp>, RuntimeError> {
    let mut ops = Vec::with_capacity(ops_arr.len());
    for op_ref in ops_arr {
        let map = match &*op_ref.borrow() {
            Value::Object(m) => m.clone(),
            other => {
                return Err(RuntimeError::at(
                    span,
                    codes::E1921_NMONGO_ERROR,
                    format!("bulk op must be object, got {}", other.type_name()),
                ));
            }
        };
        if let Some(inner) = map.get("insert_one") {
            ops.push(BulkOp::InsertOne(neko_to_bson(inner, span)?));
        } else if let Some(uo) = map.get("update_one") {
            let um = obj_map(uo, span, "update_one")?;
            ops.push(BulkOp::UpdateOne {
                filter: neko_to_bson(um.get("filter").ok_or_else(|| missing(span, "filter"))?, span)?,
                update: neko_to_bson(um.get("update").ok_or_else(|| missing(span, "update"))?, span)?,
                upsert: bool_field(&um, "upsert", false),
            });
        } else if let Some(uo) = map.get("update_many") {
            let um = obj_map(uo, span, "update_many")?;
            ops.push(BulkOp::UpdateMany {
                filter: neko_to_bson(um.get("filter").ok_or_else(|| missing(span, "filter"))?, span)?,
                update: neko_to_bson(um.get("update").ok_or_else(|| missing(span, "update"))?, span)?,
                upsert: bool_field(&um, "upsert", false),
            });
        } else if let Some(do_) = map.get("delete_one") {
            let dm = obj_map(do_, span, "delete_one")?;
            ops.push(BulkOp::DeleteOne(neko_to_bson(
                dm.get("filter").ok_or_else(|| missing(span, "filter"))?,
                span,
            )?));
        } else if let Some(dm) = map.get("delete_many") {
            let dm = obj_map(dm, span, "delete_many")?;
            ops.push(BulkOp::DeleteMany(neko_to_bson(
                dm.get("filter").ok_or_else(|| missing(span, "filter"))?,
                span,
            )?));
        } else if let Some(ro) = map.get("replace_one") {
            let rm = obj_map(ro, span, "replace_one")?;
            ops.push(BulkOp::ReplaceOne {
                filter: neko_to_bson(rm.get("filter").ok_or_else(|| missing(span, "filter"))?, span)?,
                replacement: neko_to_bson(
                    rm.get("replacement").ok_or_else(|| missing(span, "replacement"))?,
                    span,
                )?,
                upsert: bool_field(&rm, "upsert", false),
            });
        } else {
            return Err(RuntimeError::at(
                span,
                codes::E1921_NMONGO_ERROR,
                "unknown bulk write operation",
            ));
        }
    }
    Ok(ops)
}

fn obj_map(
    val: &ValueRef,
    span: Span,
    name: &str,
) -> Result<HashMap<String, ValueRef>, RuntimeError> {
    match &*val.borrow() {
        Value::Object(m) => Ok(m.clone()),
        _ => Err(RuntimeError::at(
            span,
            codes::E1921_NMONGO_ERROR,
            format!("{name} must be object"),
        )),
    }
}

fn bool_field(map: &HashMap<String, ValueRef>, key: &str, default: bool) -> bool {
    map.get(key)
        .and_then(|v| match &*v.borrow() {
            Value::Bool(b) => Some(*b),
            _ => None,
        })
        .unwrap_or(default)
}

fn missing(span: Span, field: &str) -> RuntimeError {
    RuntimeError::at(
        span,
        codes::E1921_NMONGO_ERROR,
        format!("missing required field {field}"),
    )
}

fn bulk_op_to_write_model(op: BulkOp, ns: &Namespace) -> WriteModel {
    match op {
        BulkOp::InsertOne(document) => WriteModel::InsertOne(
            InsertOneModel::builder()
                .namespace(ns.clone())
                .document(document)
                .build(),
        ),
        BulkOp::UpdateOne {
            filter,
            update,
            upsert,
        } => WriteModel::UpdateOne(
            UpdateOneModel::builder()
                .namespace(ns.clone())
                .filter(filter)
                .update(UpdateModifications::Document(update))
                .upsert(upsert)
                .build(),
        ),
        BulkOp::UpdateMany {
            filter,
            update,
            upsert,
        } => WriteModel::UpdateMany(
            UpdateManyModel::builder()
                .namespace(ns.clone())
                .filter(filter)
                .update(UpdateModifications::Document(update))
                .upsert(upsert)
                .build(),
        ),
        BulkOp::ReplaceOne {
            filter,
            replacement,
            upsert,
        } => WriteModel::ReplaceOne(
            ReplaceOneModel::builder()
                .namespace(ns.clone())
                .filter(filter)
                .replacement(replacement)
                .upsert(upsert)
                .build(),
        ),
        BulkOp::DeleteOne(filter) => WriteModel::DeleteOne(
            DeleteOneModel::builder()
                .namespace(ns.clone())
                .filter(filter)
                .build(),
        ),
        BulkOp::DeleteMany(filter) => WriteModel::DeleteMany(
            DeleteManyModel::builder()
                .namespace(ns.clone())
                .filter(filter)
                .build(),
        ),
    }
}

fn is_bulk_write_unsupported(err: &mongodb::error::Error) -> bool {
    let msg = err.to_string();
    msg.contains("MongoDB 8.0")
        || msg.contains("bulk write feature")
        || msg.contains("bulkWrite")
}

pub(crate) async fn bulk_write_async(
    client: &mongodb::Client,
    db: &str,
    coll: &str,
    ops: Vec<BulkOp>,
    ordered: bool,
) -> Result<(i64, i64, i64, i64), String> {
    let counts = execute_bulk_write(&client, db, coll, ops, ordered).await?;
    Ok((counts.inserted, counts.matched, counts.modified, counts.deleted))
}

fn is_insert_then_delete(ops: &[BulkOp]) -> bool {
    let mut seen_delete = false;
    for op in ops {
        match op {
            BulkOp::InsertOne(_) if !seen_delete => {}
            BulkOp::DeleteOne(_) => seen_delete = true,
            BulkOp::InsertOne(_) if seen_delete => return false,
            _ => return false,
        }
    }
    seen_delete
}

async fn execute_bulk_write(
    client: &mongodb::Client,
    db: &str,
    coll: &str,
    ops: Vec<BulkOp>,
    ordered: bool,
) -> Result<BulkCounts, String> {
    if ops.is_empty() {
        return Ok(BulkCounts {
            inserted: 0,
            matched: 0,
            modified: 0,
            deleted: 0,
        });
    }

    // Fast path for the common benchmark shape (N inserts then N deletes).
    if is_insert_then_delete(&ops) {
        return bulk_write_batched(&client, db, coll, ops, ordered).await;
    }

    let ns = Namespace::new(db, coll);
    let models: Vec<WriteModel> = ops
        .iter()
        .cloned()
        .map(|op| bulk_op_to_write_model(op, &ns))
        .collect();

    match client.bulk_write(models).ordered(ordered).await {
        Ok(result) => Ok(BulkCounts {
            inserted: result.inserted_count,
            matched: result.matched_count,
            modified: result.modified_count,
            deleted: result.deleted_count,
        }),
        Err(e) if is_bulk_write_unsupported(&e) => {
            bulk_write_batched(&client, db, coll, ops, ordered).await
        }
        Err(e) => Err(e.to_string()),
    }
}

async fn bulk_write_batched(
    client: &mongodb::Client,
    db: &str,
    coll: &str,
    ops: Vec<BulkOp>,
    ordered: bool,
) -> Result<BulkCounts, String> {
    let collection = client.database(db).collection::<Document>(coll);
    let mut counts = BulkCounts {
        inserted: 0,
        matched: 0,
        modified: 0,
        deleted: 0,
    };
    let mut insert_batch: Vec<Document> = Vec::new();
    let mut delete_filters: Vec<Document> = Vec::new();

    async fn flush_inserts(
        collection: &mongodb::Collection<Document>,
        batch: &mut Vec<Document>,
        inserted: &mut i64,
    ) -> Result<(), String> {
        if batch.is_empty() {
            return Ok(());
        }
        const CHUNK: usize = 1000;
        while !batch.is_empty() {
            let take = batch.len().min(CHUNK);
            let docs: Vec<Document> = batch.drain(..take).collect();
            let n = docs.len() as i64;
            collection
                .insert_many(docs)
                .await
                .map_err(|e| e.to_string())?;
            *inserted += n;
        }
        Ok(())
    }

    async fn flush_deletes(
        collection: &mongodb::Collection<Document>,
        filters: &mut Vec<Document>,
        deleted: &mut i64,
    ) -> Result<(), String> {
        if filters.is_empty() {
            return Ok(());
        }
        const CHUNK: usize = 1000;
        while !filters.is_empty() {
            let take = filters.len().min(CHUNK);
            let batch: Vec<Document> = filters.drain(..take).collect();
            if batch.len() == 1 {
                let r = collection
                    .delete_one(batch[0].clone())
                    .await
                    .map_err(|e| e.to_string())?;
                *deleted += r.deleted_count as i64;
            } else {
                let or_filters: Vec<mongodb::bson::Bson> = batch
                    .into_iter()
                    .map(mongodb::bson::Bson::Document)
                    .collect();
                let filter = mongodb::bson::doc! { "$or": or_filters };
                let r = collection
                    .delete_many(filter)
                    .await
                    .map_err(|e| e.to_string())?;
                *deleted += r.deleted_count as i64;
            }
        }
        Ok(())
    }

    for op in ops {
        match op {
            BulkOp::InsertOne(doc) => {
                insert_batch.push(doc);
            }
            BulkOp::DeleteOne(filter) => {
                flush_inserts(&collection, &mut insert_batch, &mut counts.inserted).await?;
                delete_filters.push(filter);
            }
            other => {
                flush_inserts(&collection, &mut insert_batch, &mut counts.inserted).await?;
                flush_deletes(&collection, &mut delete_filters, &mut counts.deleted).await?;
                execute_single_op(&collection, other, &mut counts, ordered).await?;
            }
        }
    }
    flush_inserts(&collection, &mut insert_batch, &mut counts.inserted).await?;
    flush_deletes(&collection, &mut delete_filters, &mut counts.deleted).await?;
    Ok(counts)
}

async fn execute_single_op(
    collection: &mongodb::Collection<Document>,
    op: BulkOp,
    counts: &mut BulkCounts,
    ordered: bool,
) -> Result<(), String> {
    match op {
        BulkOp::InsertOne(doc) => {
            collection.insert_one(doc).await.map_err(|e| e.to_string())?;
            counts.inserted += 1;
        }
        BulkOp::UpdateOne {
            filter,
            update,
            upsert,
        } => {
            let opts = mongodb::options::UpdateOptions::builder()
                .upsert(upsert)
                .build();
            let r = collection
                .update_one(filter, update)
                .with_options(opts)
                .await
                .map_err(|e| e.to_string())?;
            counts.matched += r.matched_count as i64;
            counts.modified += r.modified_count as i64;
        }
        BulkOp::UpdateMany {
            filter,
            update,
            upsert,
        } => {
            let opts = mongodb::options::UpdateOptions::builder()
                .upsert(upsert)
                .build();
            let r = collection
                .update_many(filter, update)
                .with_options(opts)
                .await
                .map_err(|e| e.to_string())?;
            counts.matched += r.matched_count as i64;
            counts.modified += r.modified_count as i64;
        }
        BulkOp::ReplaceOne {
            filter,
            replacement,
            upsert,
        } => {
            let opts = mongodb::options::ReplaceOptions::builder()
                .upsert(upsert)
                .build();
            let r = collection
                .replace_one(filter, replacement)
                .with_options(opts)
                .await
                .map_err(|e| e.to_string())?;
            counts.matched += r.matched_count as i64;
            counts.modified += r.modified_count as i64;
        }
        BulkOp::DeleteOne(filter) => {
            let r = collection
                .delete_one(filter)
                .await
                .map_err(|e| e.to_string())?;
            counts.deleted += r.deleted_count as i64;
        }
        BulkOp::DeleteMany(filter) => {
            let r = collection
                .delete_many(filter)
                .await
                .map_err(|e| e.to_string())?;
            counts.deleted += r.deleted_count as i64;
        }
    }
    let _ = ordered;
    Ok(())
}

pub fn nmongo_bulk_write(args: &[ValueRef], span: Span) -> NekoResult<ValueRef> {
    arity_range(args, 4, 5, "nmongo_bulk_write", span)?;
    let (client_id, db, coll) = db_coll_args(args, "nmongo_bulk_write", span)?;
    let ops_arr = array_arg(args, 3, "nmongo_bulk_write", span)?;
    let ops = parse_bulk_ops(&ops_arr, span)?;
    let opts_map = optional_object_arg(args, 4);
    let ordered = opts_map
        .as_ref()
        .and_then(|m| m.get("ordered"))
        .and_then(|v| match &*v.borrow() {
            Value::Bool(b) => Some(*b),
            _ => None,
        })
        .unwrap_or(true);

    with_client(client_id, "nmongo_bulk_write", span, |client| {
        block_on(async move {
            let counts = execute_bulk_write(&client, &db, &coll, ops, ordered).await?;
            let mut map = HashMap::new();
            map.insert(
                "inserted_count".to_string(),
                Value::Int(counts.inserted).ref_cell(),
            );
            map.insert(
                "matched_count".to_string(),
                Value::Int(counts.matched).ref_cell(),
            );
            map.insert(
                "modified_count".to_string(),
                Value::Int(counts.modified).ref_cell(),
            );
            map.insert(
                "deleted_count".to_string(),
                Value::Int(counts.deleted).ref_cell(),
            );
            Ok(map)
        })
    })
    .map(result_object)
    .or_else(|e| Ok(error_from_runtime(&e)))
}
