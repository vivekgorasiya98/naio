//! Neko Value <-> BSON type mapping (extended JSON conventions).

use crate::async_tasks::AsyncValue;
use crate::{RuntimeError, Value, ValueRef};
use bson::{self, Bson, Document};
use neko_ast::Span;
use neko_errors::codes;
use std::collections::HashMap;
use std::str::FromStr;

pub fn neko_to_bson(val: &ValueRef, span: Span) -> Result<Document, RuntimeError> {
    match neko_value_to_bson(&*val.borrow(), span)? {
        Bson::Document(doc) => Ok(doc),
        other => {
            let mut doc = Document::new();
            doc.insert("_value", other);
            Ok(doc)
        }
    }
}

pub fn neko_value_to_bson(val: &Value, span: Span) -> Result<Bson, RuntimeError> {
    match val {
        Value::Nil => Ok(Bson::Null),
        Value::Int(n) => Ok(Bson::Int64(*n)),
        Value::Float(f) => Ok(Bson::Double(*f)),
        Value::Bool(b) => Ok(Bson::Boolean(*b)),
        Value::String(s) => Ok(Bson::String(s.clone())),
        Value::ByteArray(b) => Ok(Bson::Binary(bson::Binary {
            subtype: bson::spec::BinarySubtype::Generic,
            bytes: b.clone(),
        })),
        Value::Array(items) => {
            let mut out = Vec::with_capacity(items.len());
            for item in items {
                out.push(neko_value_to_bson(&*item.borrow(), span)?);
            }
            Ok(Bson::Array(out))
        }
        Value::Object(map) => {
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
                doc.insert(k.clone(), neko_value_to_bson(&*v.borrow(), span)?);
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

pub fn bson_to_neko(bson: Bson) -> Value {
    bson_to_neko_ref(&bson)
}

pub fn bson_to_neko_ref(bson: &Bson) -> Value {
    match bson {
        Bson::Null => Value::Nil,
        Bson::Boolean(b) => Value::Bool(*b),
        Bson::Int32(n) => Value::Int(*n as i64),
        Bson::Int64(n) => Value::Int(*n),
        Bson::Double(f) => Value::Float(*f),
        Bson::String(s) => Value::String(s.clone()),
        Bson::Array(arr) => {
            Value::Array(arr.iter().map(|v| bson_to_neko_ref(v).ref_cell()).collect())
        }
        Bson::Document(doc) => bson_doc_to_neko(doc),
        Bson::Binary(bin) => Value::ByteArray(bin.bytes.clone()),
        Bson::ObjectId(oid) => {
            let mut map = HashMap::new();
            map.insert("$oid".to_string(), Value::String(oid.to_hex()).ref_cell());
            Value::Object(map)
        }
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

pub fn bson_doc_to_neko(doc: &Document) -> Value {
    let mut map = HashMap::with_capacity(doc.len());
    for (k, v) in doc {
        map.insert(k.clone(), bson_to_neko_ref(v).ref_cell());
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
        Bson::ObjectId(oid) => {
            let mut map = HashMap::new();
            map.insert("$oid".to_string(), AsyncValue::String(oid.to_hex()));
            AsyncValue::Object(map)
        }
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

pub fn bson_doc_to_neko_ref(doc: &Document) -> ValueRef {
    bson_doc_to_neko(doc).ref_cell()
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
    let bson = neko_value_to_bson(&*val.borrow(), span)?;
    serde_json::to_string(&bson).map_err(|e| bson_err(span, e))
}

pub fn from_extended_json(s: &str, span: Span) -> Result<ValueRef, RuntimeError> {
    let bson: Bson = serde_json::from_str(s).map_err(|e| bson_err(span, e))?;
    Ok(bson_to_neko(bson).ref_cell())
}
