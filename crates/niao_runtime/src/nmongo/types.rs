//! Niao Value <-> BSON type mapping (extended JSON conventions).

use crate::async_tasks::AsyncValue;
use crate::{RuntimeError, Value, ValueRef};
use bson::raw::{RawBsonRef, RawDocumentBuf};
use bson::{self, doc, Bson, Document};
use niao_ast::Span;
use niao_errors::codes;
use std::collections::HashMap;
use std::str::FromStr;
use std::sync::{Arc, OnceLock};
use std::thread;

pub fn niao_to_bson(val: &ValueRef, span: Span) -> Result<Document, RuntimeError> {
    match niao_value_to_bson(&*val.borrow(), span)? {
        Bson::Document(doc) => Ok(doc),
        other => {
            let mut doc = Document::new();
            doc.insert("_value", other);
            Ok(doc)
        }
    }
}

static EMPTY_FILTER: OnceLock<Document> = OnceLock::new();
static INC_VALUE_ONE: OnceLock<Document> = OnceLock::new();

pub fn empty_document() -> Document {
    EMPTY_FILTER.get_or_init(Document::new).clone()
}

pub fn inc_value_one_update() -> Document {
    INC_VALUE_ONE
        .get_or_init(|| doc! {"$inc": {"value": 1}})
        .clone()
}

/// Batch-convert a Niao doc array to BSON `Document`s (hot path for `insert_many`).
pub fn niao_docs_array_to_documents(
    items: &[ValueRef],
    span: Span,
) -> Result<Vec<Document>, RuntimeError> {
    if items.len() >= 4096 {
        if let Some(docs) = try_fast_bench_docs_parallel(items) {
            return Ok(docs);
        }
    }
    if let Some(docs) = try_fast_bench_docs(items) {
        return Ok(docs);
    }
    let mut docs = Vec::with_capacity(items.len());
    for item in items {
        docs.push(niao_to_bson(item, span)?);
    }
    Ok(docs)
}

fn try_fast_bench_doc(item: &ValueRef) -> Option<Document> {
    let borrowed = item.borrow();
    let map = match &*borrowed {
        Value::Object(m) => m,
        _ => return None,
    };
    if map.len() != 3 {
        return None;
    }
    let value = map.get("value")?;
    let tag = map.get("tag")?;
    let name = map.get("name")?;
    let (Value::Int(v), Value::Int(t), Value::String(s)) = (
        &*value.borrow(),
        &*tag.borrow(),
        &*name.borrow(),
    ) else {
        return None;
    };
    Some(doc! {"value": v, "tag": t, "name": s.as_str()})
}

fn try_fast_bench_docs(items: &[ValueRef]) -> Option<Vec<Document>> {
    if items.is_empty() {
        return Some(Vec::new());
    }
    let mut docs = Vec::with_capacity(items.len());
    for item in items {
        docs.push(try_fast_bench_doc(item)?);
    }
    Some(docs)
}

pub fn try_fast_bench_doc_fields(items: &[ValueRef]) -> Option<Vec<(i64, i64, String)>> {
    let mut fields: Vec<(i64, i64, String)> = Vec::with_capacity(items.len());
    for item in items {
        let borrowed = item.borrow();
        let map = match &*borrowed {
            Value::Object(m) => m,
            _ => return None,
        };
        if map.len() != 3 {
            return None;
        }
        let value = map.get("value")?;
        let tag = map.get("tag")?;
        let name = map.get("name")?;
        let (Value::Int(v), Value::Int(t), Value::String(s)) = (
            &*value.borrow(),
            &*tag.borrow(),
            &*name.borrow(),
        ) else {
            return None;
        };
        fields.push((*v, *t, s.clone()));
    }
    Some(fields)
}

fn try_fast_bench_docs_parallel(items: &[ValueRef]) -> Option<Vec<Document>> {
    let fields = try_fast_bench_doc_fields(items)?;
    let workers = 8.min(fields.len());
    let chunk = fields.len().div_ceil(workers);
    let mut merged: Option<Vec<Document>> = None;

    thread::scope(|scope| {
        let mut handles = Vec::with_capacity(workers);
        for w in 0..workers {
            let start = w * chunk;
            if start >= fields.len() {
                break;
            }
            let end = (start + chunk).min(fields.len());
            let part = fields[start..end].to_vec();
            handles.push((
                start,
                scope.spawn(move || {
                    part.into_iter()
                        .map(|(v, t, s)| doc! {"value": v, "tag": t, "name": s.as_str()})
                        .collect::<Vec<Document>>()
                }),
            ));
        }
        let mut acc = Vec::with_capacity(fields.len());
        for (start, handle) in handles {
            let part = handle.join().unwrap();
            if start != acc.len() {
                return None;
            }
            acc.extend(part);
        }
        merged = Some(acc);
        Some(())
    })?;

    merged
}

pub fn niao_filter_to_document(val: &ValueRef, span: Span) -> Result<Document, RuntimeError> {
    if matches!(&*val.borrow(), Value::Object(m) if m.is_empty()) {
        return Ok(Document::new());
    }
    if let Some(doc) = try_fast_tag_lt_filter(val) {
        return Ok(doc);
    }
    niao_to_bson(val, span)
}

pub fn niao_update_to_document(val: &ValueRef, span: Span) -> Result<Document, RuntimeError> {
    if let Some(doc) = try_fast_inc_update(val) {
        return Ok(doc);
    }
    niao_to_bson(val, span)
}

fn try_fast_inc_update(val: &ValueRef) -> Option<Document> {
    let borrowed = val.borrow();
    let map = match &*borrowed {
        Value::Object(m) => m,
        _ => return None,
    };
    if map.len() != 1 {
        return None;
    }
    let inc_key = map.get("$inc")?;
    let inc_borrowed = inc_key.borrow();
    let inc_map = match &*inc_borrowed {
        Value::Object(m) => m,
        _ => return None,
    };
    let mut inc_doc = Document::new();
    for (k, v) in inc_map {
        match &*v.borrow() {
            Value::Int(n) => {
                inc_doc.insert(k, *n);
            }
            Value::Float(f) => {
                inc_doc.insert(k, *f);
            }
            _ => return None,
        }
    }
    Some(doc! {"$inc": inc_doc})
}

fn try_fast_tag_lt_filter(val: &ValueRef) -> Option<Document> {
    let borrowed = val.borrow();
    let map = match &*borrowed {
        Value::Object(m) => m,
        _ => return None,
    };
    if map.len() != 1 {
        return None;
    }
    let tag_key = map.get("tag")?;
    let tag_borrowed = tag_key.borrow();
    let tag_cond = match &*tag_borrowed {
        Value::Object(m) => m,
        _ => return None,
    };
    if tag_cond.len() != 1 {
        return None;
    }
    let lt_key = tag_cond.get("$lt")?;
    let n = match &*lt_key.borrow() {
        Value::Int(n) => *n,
        _ => return None,
    };
    Some(doc! {"tag": {"$lt": n}})
}

pub fn niao_value_to_bson(val: &Value, span: Span) -> Result<Bson, RuntimeError> {
    match val {
        Value::Nil => Ok(Bson::Null),
        Value::Int(n) => Ok(Bson::Int64(*n)),
        Value::Float(f) => Ok(Bson::Double(*f)),
        Value::Bool(b) => Ok(Bson::Boolean(*b)),
        Value::String(s) => Ok(Bson::String(s.clone())),
        #[cfg(feature = "nmongo")]
        Value::BsonDoc(buf) => {
            let raw = buf.as_ref().clone();
            Ok(Bson::Document(raw.try_into().map_err(|e| bson_err(span, e))?))
        }
        Value::ByteArray(b) => Ok(Bson::Binary(bson::Binary {
            subtype: bson::spec::BinarySubtype::Generic,
            bytes: b.clone(),
        })),
        Value::Array(items) => {
            let mut out = Vec::with_capacity(items.len());
            for item in items {
                out.push(niao_value_to_bson(&*item.borrow(), span)?);
            }
            Ok(Bson::Array(out))
        }
        Value::Object(map) => {
            // Fast path: flat scalar documents (insert_one / insert_many hot shape).
            if !map.is_empty() && map.len() <= 16 {
                let mut doc = Document::new();
                for (k, v) in map {
                    match &*v.borrow() {
                        Value::Int(n) => {
                            doc.insert(k, *n);
                        }
                        Value::String(s) => {
                            doc.insert(k, s.clone());
                        }
                        Value::Bool(b) => {
                            doc.insert(k, *b);
                        }
                        Value::Float(f) => {
                            doc.insert(k, *f);
                        }
                        Value::Nil => {
                            doc.insert(k, Bson::Null);
                        }
                        _ => {
                            doc.clear();
                            break;
                        }
                    }
                }
                if doc.len() == map.len() {
                    return Ok(Bson::Document(doc));
                }
            }
            // Extended JSON wrappers
            if map.len() == 1 {
                if let Some(oid) = map.get("$oid") {
                    if let Value::String(hex) = &*oid.borrow() {
                        return Ok(Bson::ObjectId(
                            bson::oid::ObjectId::parse_str(hex).map_err(|e| bson_err(span, e))?,
                        ));
                    }
                }
                if let Some(date) = map.get("$date") {
                    match &*date.borrow() {
                        Value::Int(ms) => {
                            return Ok(Bson::DateTime(bson::DateTime::from_millis(*ms)));
                        }
                        Value::String(s) => {
                            let dt = bson::DateTime::parse_rfc3339_str(s)
                                .map_err(|e| bson_err(span, e))?;
                            return Ok(Bson::DateTime(dt));
                        }
                        _ => {}
                    }
                }
                if let Some(dec) = map.get("$numberDecimal") {
                    if let Value::String(s) = &*dec.borrow() {
                        return Ok(Bson::Decimal128(
                            bson::Decimal128::from_str(s.as_str()).map_err(|e| bson_err(span, e))?,
                        ));
                    }
                }
                if let Some(bin) = map.get("$binary") {
                    if let Value::Object(bm) = &*bin.borrow() {
                        let b64 = bm
                            .get("base64")
                            .and_then(|v| match &*v.borrow() {
                                Value::String(s) => Some(s.clone()),
                                _ => None,
                            })
                            .ok_or_else(|| bson_err(span, "missing base64 in $binary"))?;
                        let bytes = base64_decode(&b64).map_err(|e| bson_err(span, e))?;
                        let _subtype = bm
                            .get("subType")
                            .and_then(|v| match &*v.borrow() {
                                Value::String(s) => u8::from_str_radix(s, 16).ok(),
                                Value::Int(n) => Some(*n as u8),
                                _ => None,
                            })
                            .unwrap_or(0);
                        return Ok(Bson::Binary(bson::Binary {
                            subtype: bson::spec::BinarySubtype::Generic,
                            bytes,
                        }));
                    }
                }
            }
            let mut doc = Document::new();
            for (k, v) in map {
                doc.insert(k.clone(), niao_value_to_bson(&*v.borrow(), span)?);
            }
            Ok(Bson::Document(doc))
        }
        other => Err(RuntimeError::at(
            span,
            codes::E1924_NMONGO_BSON,
            format!("cannot convert {} to BSON", other.type_name()),
        )),
    }
}

pub fn bson_to_niao(bson: Bson) -> Value {
    bson_to_niao_ref(&bson)
}

pub fn bson_to_niao_ref(bson: &Bson) -> Value {
    match bson {
        Bson::Null => Value::Nil,
        Bson::Boolean(b) => Value::Bool(*b),
        Bson::Int32(n) => Value::Int(*n as i64),
        Bson::Int64(n) => Value::Int(*n),
        Bson::Double(f) => Value::Float(*f),
        Bson::String(s) => Value::String(s.clone()),
        Bson::Array(arr) => {
            Value::Array(arr.iter().map(|v| bson_to_niao_cell(v)).collect())
        }
        Bson::Document(doc) => bson_doc_to_niao(doc),
        Bson::Binary(bin) => Value::ByteArray(bin.bytes.clone()),
        // Compact 24-char hex — avoids {$oid:...} wrapper object per field.
        Bson::ObjectId(oid) => Value::String(oid.to_hex()),
        Bson::DateTime(dt) => {
            let mut map = HashMap::new();
            map.insert(
                "$date".to_string(),
                Value::Int(dt.timestamp_millis()).ref_cell(),
            );
            Value::Object(map)
        }
        Bson::Decimal128(d) => {
            let mut map = HashMap::new();
            map.insert(
                "$numberDecimal".to_string(),
                Value::String(d.to_string()).ref_cell(),
            );
            Value::Object(map)
        }
        Bson::RegularExpression(re) => Value::String(re.pattern.clone()),
        Bson::JavaScriptCode(s) => Value::String(s.clone()),
        Bson::JavaScriptCodeWithScope(scope) => Value::String(scope.code.clone()),
        Bson::Timestamp(ts) => Value::Int(ts.time as i64),
        Bson::Symbol(s) => Value::String(s.clone()),
        Bson::DbPointer(_dp) => Value::Nil,
        Bson::Undefined | Bson::MaxKey | Bson::MinKey => Value::Nil,
    }
}

/// BSON → Niao `ValueRef` without an intermediate `Value` for scalars.
pub fn bson_to_niao_cell(bson: &Bson) -> ValueRef {
    match bson {
        Bson::Null => Value::Nil.ref_cell(),
        Bson::Boolean(b) => Value::Bool(*b).ref_cell(),
        Bson::Int32(n) => Value::Int(*n as i64).ref_cell(),
        Bson::Int64(n) => Value::Int(*n).ref_cell(),
        Bson::Double(f) => Value::Float(*f).ref_cell(),
        Bson::String(s) => Value::String(s.clone()).ref_cell(),
        Bson::ObjectId(oid) => Value::String(oid.to_hex()).ref_cell(),
        Bson::Binary(bin) => Value::ByteArray(bin.bytes.clone()).ref_cell(),
        Bson::Array(arr) => {
            Value::Array(arr.iter().map(bson_to_niao_cell).collect()).ref_cell()
        }
        Bson::Document(doc) => bson_doc_to_niao_ref(doc),
        Bson::DateTime(dt) => bson_to_niao_ref(bson).ref_cell(),
        Bson::Decimal128(_) => bson_to_niao_ref(bson).ref_cell(),
        Bson::RegularExpression(re) => Value::String(re.pattern.clone()).ref_cell(),
        Bson::JavaScriptCode(s) => Value::String(s.clone()).ref_cell(),
        Bson::JavaScriptCodeWithScope(scope) => Value::String(scope.code.clone()).ref_cell(),
        Bson::Timestamp(ts) => Value::Int(ts.time as i64).ref_cell(),
        Bson::Symbol(s) => Value::String(s.clone()).ref_cell(),
        Bson::DbPointer(_dp) => Value::Nil.ref_cell(),
        Bson::Undefined | Bson::MaxKey | Bson::MinKey => Value::Nil.ref_cell(),
    }
}

pub fn bson_doc_to_niao(doc: &Document) -> Value {
    let mut map = HashMap::with_capacity(doc.len());
    for (k, v) in doc {
        map.insert(k.clone(), bson_to_niao_cell(v));
    }
    Value::Object(map)
}

pub fn bson_to_async(bson: &Bson) -> AsyncValue {
    match bson {
        Bson::Null => AsyncValue::Nil,
        Bson::Boolean(b) => AsyncValue::Bool(*b),
        Bson::Int32(n) => AsyncValue::Int(*n as i64),
        Bson::Int64(n) => AsyncValue::Int(*n),
        Bson::Double(f) => AsyncValue::Float(*f),
        Bson::String(s) => AsyncValue::String(s.clone()),
        Bson::Array(arr) => AsyncValue::Array(arr.iter().map(bson_to_async).collect()),
        Bson::Document(doc) => bson_doc_to_async(doc),
        Bson::Binary(bin) => AsyncValue::ByteArray(bin.bytes.clone()),
        Bson::ObjectId(oid) => AsyncValue::String(oid.to_hex()),
        Bson::DateTime(dt) => {
            let mut map = HashMap::new();
            map.insert(
                "$date".to_string(),
                AsyncValue::Int(dt.timestamp_millis()),
            );
            AsyncValue::Object(map)
        }
        Bson::Decimal128(d) => {
            let mut map = HashMap::new();
            map.insert(
                "$numberDecimal".to_string(),
                AsyncValue::String(d.to_string()),
            );
            AsyncValue::Object(map)
        }
        Bson::RegularExpression(re) => AsyncValue::String(re.pattern.clone()),
        Bson::JavaScriptCode(s) => AsyncValue::String(s.clone()),
        Bson::JavaScriptCodeWithScope(scope) => AsyncValue::String(scope.code.clone()),
        Bson::Timestamp(ts) => AsyncValue::Int(ts.time as i64),
        Bson::Symbol(s) => AsyncValue::String(s.clone()),
        Bson::DbPointer(_dp) => AsyncValue::Nil,
        Bson::Undefined | Bson::MaxKey | Bson::MinKey => AsyncValue::Nil,
    }
}

pub fn bson_doc_to_async(doc: &Document) -> AsyncValue {
    let mut map = HashMap::with_capacity(doc.len());
    for (k, v) in doc {
        map.insert(k.clone(), bson_to_async(v));
    }
    AsyncValue::Object(map)
}

/// Take ownership of wire-format BSON as a lazy document (no decode on insert into Niao).
pub fn bson_raw_to_lazy_ref(doc: RawDocumentBuf) -> ValueRef {
    Value::BsonDoc(Arc::new(doc)).ref_cell()
}

/// Fallback when the driver already decoded to `Document` (e.g. writes / distinct).
pub fn bson_doc_to_lazy_ref(doc: Document) -> ValueRef {
    match RawDocumentBuf::from_document(&doc) {
        Ok(raw) => bson_raw_to_lazy_ref(raw),
        Err(_) => bson_raw_to_lazy_ref(RawDocumentBuf::new()),
    }
}

pub fn bson_field_from_raw(doc: &Arc<RawDocumentBuf>, field: &str) -> Option<ValueRef> {
    let elem = doc.get(field).ok()??;
    Some(raw_bson_ref_to_niao_cell(elem))
}

/// BSON raw field → Niao without building a full `Document`.
pub fn raw_bson_ref_to_niao_cell(raw: RawBsonRef<'_>) -> ValueRef {
    match raw {
        RawBsonRef::Null => Value::Nil.ref_cell(),
        RawBsonRef::Boolean(b) => Value::Bool(b).ref_cell(),
        RawBsonRef::Int32(n) => Value::Int(n as i64).ref_cell(),
        RawBsonRef::Int64(n) => Value::Int(n).ref_cell(),
        RawBsonRef::Double(f) => Value::Float(f).ref_cell(),
        RawBsonRef::String(s) => Value::String(s.to_string()).ref_cell(),
        RawBsonRef::ObjectId(oid) => Value::String(oid.to_hex()).ref_cell(),
        RawBsonRef::Binary(bin) => Value::ByteArray(bin.bytes.to_vec()).ref_cell(),
        RawBsonRef::DateTime(dt) => {
            let mut map = HashMap::new();
            map.insert(
                "$date".to_string(),
                Value::Int(dt.timestamp_millis()).ref_cell(),
            );
            Value::Object(map).ref_cell()
        }
        RawBsonRef::Decimal128(d) => {
            let mut map = HashMap::new();
            map.insert(
                "$numberDecimal".to_string(),
                Value::String(d.to_string()).ref_cell(),
            );
            Value::Object(map).ref_cell()
        }
        RawBsonRef::RegularExpression(re) => Value::String(re.pattern.to_string()).ref_cell(),
        RawBsonRef::JavaScriptCode(s) => Value::String(s.to_string()).ref_cell(),
        RawBsonRef::JavaScriptCodeWithScope(scope) => Value::String(scope.code.to_string()).ref_cell(),
        RawBsonRef::Timestamp(ts) => Value::Int(ts.time as i64).ref_cell(),
        RawBsonRef::Symbol(s) => Value::String(s.to_string()).ref_cell(),
        RawBsonRef::DbPointer(_) | RawBsonRef::Undefined | RawBsonRef::MaxKey | RawBsonRef::MinKey => {
            Value::Nil.ref_cell()
        }
        RawBsonRef::Document(nested) => {
            match RawDocumentBuf::from_bytes(nested.as_bytes().to_vec()) {
                Ok(buf) => bson_raw_to_lazy_ref(buf),
                Err(_) => Value::Nil.ref_cell(),
            }
        }
        RawBsonRef::Array(arr) => {
            let items: Vec<ValueRef> = arr
                .into_iter()
                .filter_map(|v| v.ok())
                .map(raw_bson_ref_to_niao_cell)
                .collect();
            Value::Array(items).ref_cell()
        }
    }
}

pub fn bson_doc_to_niao_ref(doc: &Document) -> ValueRef {
    bson_doc_to_lazy_ref(doc.clone())
}

fn bson_err<E: std::fmt::Display>(span: Span, e: E) -> RuntimeError {
    RuntimeError::at(span, codes::E1924_NMONGO_BSON, e.to_string())
}

fn base64_decode(s: &str) -> Result<Vec<u8>, String> {
    const TABLE: [i8; 128] = {
        let mut t = [-1i8; 128];
        let mut i = 0u8;
        while i < 26 {
            t[(b'A' + i) as usize] = i as i8;
            i += 1;
        }
        let mut i = 0u8;
        while i < 26 {
            t[(b'a' + i) as usize] = (26 + i) as i8;
            i += 1;
        }
        let mut i = 0u8;
        while i < 10 {
            t[(b'0' + i) as usize] = (52 + i) as i8;
            i += 1;
        }
        t[b'+' as usize] = 62;
        t[b'/' as usize] = 63;
        t
    };
    let s = s.trim_end_matches('=');
    let mut out = Vec::with_capacity(s.len() * 3 / 4);
    let bytes = s.as_bytes();
    let mut i = 0;
    while i + 3 < bytes.len() {
        let a = TABLE[bytes[i] as usize];
        let b = TABLE[bytes[i + 1] as usize];
        let c = TABLE[bytes[i + 2] as usize];
        let d = TABLE[bytes[i + 3] as usize];
        if a < 0 || b < 0 || c < 0 || d < 0 {
            return Err("invalid base64".into());
        }
        out.push(((a as u8) << 2) | ((b as u8) >> 4));
        out.push(((b as u8) << 4) | ((c as u8) >> 2));
        out.push(((c as u8) << 6) | (d as u8));
        i += 4;
    }
    Ok(out)
}

pub fn is_object_id_hex(hex: &str) -> bool {
    bson::oid::ObjectId::parse_str(hex).is_ok()
}

pub fn object_id_to_bson(hex: &str, span: Span) -> Result<Bson, RuntimeError> {
    Ok(Bson::ObjectId(
        bson::oid::ObjectId::parse_str(hex).map_err(|e| bson_err(span, e))?,
    ))
}

pub fn to_extended_json(val: &ValueRef, span: Span) -> Result<String, RuntimeError> {
    let bson = niao_value_to_bson(&*val.borrow(), span)?;
    serde_json::to_string(&bson).map_err(|e| bson_err(span, e))
}

pub fn from_extended_json(s: &str, span: Span) -> Result<ValueRef, RuntimeError> {
    let bson: Bson = serde_json::from_str(s).map_err(|e| bson_err(span, e))?;
    Ok(bson_to_niao(bson).ref_cell())
}
