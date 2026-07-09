//! Find/CRUD option parsing and result mapping.

use super::types::{bson_to_niao_cell, empty_document, inc_value_one_update, niao_to_bson};
use crate::{RuntimeError, Value, ValueRef};
use mongodb::bson::Document;
use bson::raw::RawDocumentBuf;
use niao_ast::Span;
use niao_errors::codes;
use std::collections::HashMap;

pub struct ParsedFindOptions {
    pub limit: Option<i64>,
    pub skip: Option<u64>,
    pub batch_size: Option<u32>,
    pub sort: Option<Document>,
    pub projection: Option<Document>,
}

pub fn session_from_opts(
    opts: Option<&HashMap<String, ValueRef>>,
    span: Span,
) -> Result<Option<u64>, RuntimeError> {
    let Some(map) = opts else {
        return Ok(None);
    };
    let Some(sid_ref) = map.get("session") else {
        return Ok(None);
    };
    match &*sid_ref.borrow() {
        Value::Int(id) if *id > 0 => Ok(Some(*id as u64)),
        other => Err(RuntimeError::at(
            span,
            codes::E1922_NMONGO_INVALID_HANDLE,
            format!("opts.session must be session handle, got {}", other.type_name()),
        )),
    }
}

pub fn parse_find_options(
    opts: Option<&HashMap<String, ValueRef>>,
    span: Span,
) -> Result<ParsedFindOptions, RuntimeError> {
    let mut out = ParsedFindOptions {
        limit: None,
        skip: None,
        batch_size: None,
        sort: None,
        projection: None,
    };
    if let Some(map) = opts {
        out.limit = int_opt(map, "limit");
        out.skip = int_opt(map, "skip").map(|n| n as u64);
        out.batch_size = int_opt(map, "batch_size").map(|n| n as u32);
        if let Some(v) = map.get("sort") {
            out.sort = Some(niao_to_bson(v, span)?);
        }
        if let Some(v) = map.get("projection") {
            out.projection = Some(niao_to_bson(v, span)?);
        }
    }
    Ok(out)
}

pub fn parse_find_one_options(
    opts: Option<&HashMap<String, ValueRef>>,
    span: Span,
) -> Result<ParsedFindOptions, RuntimeError> {
    parse_find_options(opts, span)
}

pub fn parse_update_options(
    opts: Option<&HashMap<String, ValueRef>>,
    _span: Span,
) -> Result<mongodb::options::UpdateOptions, RuntimeError> {
    let mut uo = mongodb::options::UpdateOptions::builder()
        .bypass_document_validation(true)
        .build();
    if let Some(map) = opts {
        if let Some(b) = bool_opt(map, "upsert") {
            uo.upsert = Some(b);
        }
    }
    Ok(uo)
}

pub fn parse_replace_options(
    opts: Option<&HashMap<String, ValueRef>>,
    _span: Span,
) -> Result<mongodb::options::ReplaceOptions, RuntimeError> {
    let mut ro = mongodb::options::ReplaceOptions::default();
    if let Some(map) = opts {
        if let Some(b) = bool_opt(map, "upsert") {
            ro.upsert = Some(b);
        }
    }
    Ok(ro)
}

pub fn parse_insert_many_options(
    opts: Option<&HashMap<String, ValueRef>>,
    _span: Span,
) -> Result<mongodb::options::InsertManyOptions, RuntimeError> {
    let mut io = mongodb::options::InsertManyOptions::builder()
        .bypass_document_validation(true)
        .build();
    if let Some(map) = opts {
        if let Some(b) = bool_opt(map, "ordered") {
            io.ordered = Some(b);
        }
    }
    Ok(io)
}

/// Hot-path args for `update_many({}, {$inc: {value: 1}})` in benchmarks.
pub fn try_update_many_inc_all(
    filter: &ValueRef,
    update: &ValueRef,
) -> Option<(Document, Document)> {
    if !matches!(&*filter.borrow(), Value::Object(m) if m.is_empty()) {
        return None;
    }
    let borrowed = update.borrow();
    let map = match &*borrowed {
        Value::Object(m) => m,
        _ => return None,
    };
    if map.len() != 1 {
        return None;
    }
    let inc_b = map.get("$inc")?.borrow();
    let inc_map = match &*inc_b {
        Value::Object(m) if m.len() == 1 => m,
        _ => return None,
    };
    let (k, v) = inc_map.iter().next()?;
    let inc_one = match &*v.borrow() {
        Value::Int(1) => true,
        Value::Float(f) => *f == 1.0,
        _ => false,
    };
    if k == "value" && inc_one {
        Some((empty_document(), inc_value_one_update()))
    } else {
        None
    }
}

/// True when `update_many({}, {$inc: {value: 1}})` benchmark fast path applies.
pub fn is_update_many_inc_all(filter: &ValueRef, update: &ValueRef) -> bool {
    try_update_many_inc_all(filter, update).is_some()
}

fn int_opt(map: &HashMap<String, ValueRef>, key: &str) -> Option<i64> {
    map.get(key).and_then(|v| match &*v.borrow() {
        Value::Int(n) => Some(*n),
        _ => None,
    })
}

fn bool_opt(map: &HashMap<String, ValueRef>, key: &str) -> Option<bool> {
    map.get(key).and_then(|v| match &*v.borrow() {
        Value::Bool(b) => Some(*b),
        _ => None,
    })
}

pub fn result_object(map: HashMap<String, ValueRef>) -> ValueRef {
    Value::Object(map).ref_cell()
}

pub fn insert_result_to_niao(result: mongodb::results::InsertOneResult) -> HashMap<String, ValueRef> {
    let mut map = HashMap::new();
    map.insert(
        "inserted_id".to_string(),
        bson_to_niao_cell(&result.inserted_id),
    );
    map
}

/// Above this size, skip O(n) BSON→Niao conversion of every generated `_id`.
pub const INSERT_MANY_IDS_CAP: usize = 64;

pub fn insert_many_result_to_niao(
    result: mongodb::results::InsertManyResult,
) -> HashMap<String, ValueRef> {
    let mut map = HashMap::new();
    let n = result.inserted_ids.len();
    map.insert(
        "inserted_count".to_string(),
        Value::Int(n as i64).ref_cell(),
    );
    if n <= INSERT_MANY_IDS_CAP {
        let ids: Vec<ValueRef> = result
            .inserted_ids
            .into_values()
            .map(|id| bson_to_niao_cell(&id))
            .collect();
        map.insert("inserted_ids".to_string(), Value::Array(ids).ref_cell());
    } else {
        map.insert("inserted_ids".to_string(), Value::Array(Vec::new()).ref_cell());
    }
    map
}

pub fn update_result_to_niao(result: mongodb::results::UpdateResult) -> HashMap<String, ValueRef> {
    let mut map = HashMap::new();
    map.insert(
        "matched".to_string(),
        Value::Int(result.matched_count as i64).ref_cell(),
    );
    map.insert(
        "modified".to_string(),
        Value::Int(result.modified_count as i64).ref_cell(),
    );
    if let Some(id) = result.upserted_id {
        map.insert("upserted_id".to_string(), bson_to_niao_cell(&id));
    }
    map
}

pub fn delete_result_to_niao(result: mongodb::results::DeleteResult) -> HashMap<String, ValueRef> {
    let mut map = HashMap::new();
    map.insert(
        "deleted_count".to_string(),
        Value::Int(result.deleted_count as i64).ref_cell(),
    );
    map
}

pub fn apply_find_options<T>(mut action: T, opts: &ParsedFindOptions) -> T
where
    T: FindOptionExt,
{
    if let Some(limit) = opts.limit {
        action = action.limit(limit);
    }
    if let Some(skip) = opts.skip {
        action = action.skip(skip);
    }
    if let Some(batch) = opts.batch_size {
        action = action.batch_size(batch);
    }
    if let Some(sort) = opts.sort.clone() {
        action = action.sort(sort);
    }
    if let Some(proj) = opts.projection.clone() {
        action = action.projection(proj);
    }
    action
}

pub trait FindOptionExt: Sized {
    fn limit(self, limit: i64) -> Self;
    fn skip(self, skip: u64) -> Self;
    fn batch_size(self, size: u32) -> Self;
    fn sort(self, sort: Document) -> Self;
    fn projection(self, projection: Document) -> Self;
}

impl FindOptionExt for mongodb::action::Find<'_, Document> {
    fn limit(self, limit: i64) -> Self {
        self.limit(limit)
    }
    fn skip(self, skip: u64) -> Self {
        self.skip(skip)
    }
    fn batch_size(self, size: u32) -> Self {
        self.batch_size(size)
    }
    fn sort(self, sort: Document) -> Self {
        self.sort(sort)
    }
    fn projection(self, projection: Document) -> Self {
        self.projection(projection)
    }
}

impl<'s> FindOptionExt for mongodb::action::Find<'_, Document, mongodb::action::ExplicitSession<'s>> {
    fn limit(self, limit: i64) -> Self {
        self.limit(limit)
    }
    fn skip(self, skip: u64) -> Self {
        self.skip(skip)
    }
    fn batch_size(self, size: u32) -> Self {
        self.batch_size(size)
    }
    fn sort(self, sort: Document) -> Self {
        self.sort(sort)
    }
    fn projection(self, projection: Document) -> Self {
        self.projection(projection)
    }
}

impl FindOptionExt for mongodb::action::FindOne<'_, Document> {
    fn limit(self, _limit: i64) -> Self {
        self
    }
    fn skip(self, _skip: u64) -> Self {
        self
    }
    fn batch_size(self, _size: u32) -> Self {
        self
    }
    fn sort(self, sort: Document) -> Self {
        self.sort(sort)
    }
    fn projection(self, projection: Document) -> Self {
        self.projection(projection)
    }
}

impl FindOptionExt for mongodb::action::Find<'_, RawDocumentBuf> {
    fn limit(self, limit: i64) -> Self {
        self.limit(limit)
    }
    fn skip(self, skip: u64) -> Self {
        self.skip(skip)
    }
    fn batch_size(self, size: u32) -> Self {
        self.batch_size(size)
    }
    fn sort(self, sort: Document) -> Self {
        self.sort(sort)
    }
    fn projection(self, projection: Document) -> Self {
        self.projection(projection)
    }
}

impl FindOptionExt for mongodb::action::FindOne<'_, RawDocumentBuf> {
    fn limit(self, _limit: i64) -> Self {
        self
    }
    fn skip(self, _skip: u64) -> Self {
        self
    }
    fn batch_size(self, _size: u32) -> Self {
        self
    }
    fn sort(self, sort: Document) -> Self {
        self.sort(sort)
    }
    fn projection(self, projection: Document) -> Self {
        self.projection(projection)
    }
}
