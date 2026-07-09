//! Aggregation pipeline operations.

use super::common::*;
use super::handles::with_client;
use super::ops::session_from_opts;
use super::runtime::block_on;
use super::types::{bson_doc_to_lazy_ref, niao_to_bson};
use crate::{error_from_runtime, NiaoResult, Value, ValueRef};
use futures::StreamExt;
use mongodb::bson::Document;
use niao_ast::Span;

pub fn nmongo_aggregate(args: &[ValueRef], span: Span) -> NiaoResult<ValueRef> {
    arity_range(args, 4, 5, "nmongo_aggregate", span)?;
    let (client_id, db, coll) = db_coll_args(args, "nmongo_aggregate", span)?;
    let pipeline_arr = array_arg(args, 3, "nmongo_aggregate", span)?;
    let mut pipeline = Vec::with_capacity(pipeline_arr.len());
    for stage in &pipeline_arr {
        pipeline.push(niao_to_bson(stage, span)?);
    }
    let opts_map = optional_object_arg(args, 4);
    let session_id = session_from_opts(opts_map.as_ref(), span)?;
    if session_id.is_some() {
        return Ok(error_from_runtime(&crate::RuntimeError::at(
            span,
            niao_errors::codes::E1921_NMONGO_ERROR,
            "nmongo_aggregate: sessions not supported in ahiru handlers",
        )));
    }

    with_client(client_id, "nmongo_aggregate", span, |client| {
        let db = db.clone();
        let coll = coll.clone();
        block_on(async move {
            let collection = client.database(&db).collection::<Document>(&coll);
            let mut cursor = collection
                .aggregate(pipeline)
                .await
                .map_err(|e| e.to_string())?;
            let mut rows = Vec::new();
            while let Some(doc) = cursor.next().await {
                rows.push(bson_doc_to_lazy_ref(doc.map_err(|e| e.to_string())?));
            }
            Ok(rows)
        })
    })
    .map(|rows| Value::Array(rows).ref_cell())
    .or_else(|e| Ok(error_from_runtime(&e)))
}
