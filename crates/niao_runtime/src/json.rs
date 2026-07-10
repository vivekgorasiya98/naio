//! Native JSON standard library — fast parse/stringify via `serde_json`.
//!
//! Registered as prefixed builtins (`json_parse`, `json_stringify`, ...).
//! Import with `import "json"` (or `import "std/json"`).

use crate::{NativeFn, NiaoResult, RuntimeError, Value, ValueRef};
use niao_ast::Span;
use niao_errors::codes;
use num_bigint::BigInt;
use num_traits::cast::ToPrimitive;
use serde::de::{self, DeserializeSeed, Deserializer, MapAccess, SeqAccess, Visitor};
use serde_json::{Map, Number, Value as JsonValue};
use std::cell::RefCell;
use std::collections::HashMap;
use std::fmt;
use std::rc::Rc;

#[cfg(feature = "nmongo")]
use crate::nmongo::bson_field_from_raw;

fn type_err(span: Span, msg: impl Into<String>) -> RuntimeError {
    RuntimeError::TypeError {
        message: msg.into(),
        line: span.line,
        col: span.col,
    }
}

fn arity(args: &[ValueRef], n: usize, name: &str, span: Span) -> NiaoResult<()> {
    if args.len() != n {
        return Err(RuntimeError::at(
            span,
            codes::E1012_JSON_ARITY,
            format!("{name}() expects {n} argument(s), got {}", args.len()),
        ));
    }
    Ok(())
}

fn arity_range(args: &[ValueRef], min: usize, max: usize, name: &str, span: Span) -> NiaoResult<()> {
    if args.len() < min || args.len() > max {
        return Err(RuntimeError::at(
            span,
            codes::E1012_JSON_ARITY,
            format!("{name}() expects {min}..={max} argument(s), got {}", args.len()),
        ));
    }
    Ok(())
}

fn int_arg(args: &[ValueRef], idx: usize, name: &str, span: Span) -> NiaoResult<i64> {
    match &*args[idx].borrow() {
        Value::Int(n) => Ok(*n),
        other => Err(type_err(
            span,
            format!(
                "{name}() expects an int as argument {}, got {}",
                idx + 1,
                other.type_name()
            ),
        )),
    }
}

#[cfg(test)]
fn json_to_value(j: JsonValue) -> Value {
    match j {
        JsonValue::Null => Value::Nil,
        JsonValue::Bool(b) => Value::Bool(b),
        JsonValue::Number(n) => {
            if let Some(i) = n.as_i64() {
                Value::Int(i)
            } else if let Some(u) = n.as_u64() {
                if u <= i64::MAX as u64 {
                    Value::Int(u as i64)
                } else {
                    Value::BigInt(BigInt::from(u))
                }
            } else if let Some(f) = n.as_f64() {
                if f.fract() == 0.0 && f >= i64::MIN as f64 && f <= i64::MAX as f64 {
                    Value::Int(f as i64)
                } else {
                    Value::Float(f)
                }
            } else {
                Value::String(n.to_string())
            }
        }
        JsonValue::String(s) => Value::String(s),
        JsonValue::Array(items) => {
            let mut out = Vec::with_capacity(items.len());
            for item in items {
                out.push(json_to_value(item).ref_cell());
            }
            Value::Array(out)
        }
        JsonValue::Object(map) => {
            let mut out = HashMap::with_capacity(map.len());
            for (k, v) in map {
                out.insert(k, json_to_value(v).ref_cell());
            }
            Value::Object(out)
        }
    }
}

fn value_to_json(v: &Value, span: Span) -> NiaoResult<JsonValue> {
    match v {
        Value::Nil => Ok(JsonValue::Null),
        Value::Bool(b) => Ok(JsonValue::Bool(*b)),
        Value::Int(n) => Ok(JsonValue::Number(Number::from(*n))),
        Value::BigInt(n) => {
            if let Some(i) = n.to_i64() {
                Ok(JsonValue::Number(Number::from(i)))
            } else if let Some(u) = n.to_u64() {
                Ok(JsonValue::Number(Number::from(u)))
            } else {
                Err(type_err(
                    span,
                    format!("json_stringify: bigint {n} does not fit in JSON number"),
                ))
            }
        }
        Value::Float(f) => {
            if !f.is_finite() {
                Ok(JsonValue::Null)
            } else {
                Ok(JsonValue::Number(
                    Number::from_f64(*f)
                        .ok_or_else(|| type_err(span, format!("json_stringify: invalid float {f}")))?,
                ))
            }
        }
        Value::String(s) => Ok(JsonValue::String(s.clone())),
        Value::IntArray(items) => {
            let mut out = Vec::with_capacity(items.len());
            for &n in items {
                out.push(JsonValue::Number(Number::from(n)));
            }
            Ok(JsonValue::Array(out))
        }
        Value::FloatArray(items) => {
            let mut out = Vec::with_capacity(items.len());
            for &f in items {
                if !f.is_finite() {
                    out.push(JsonValue::Null);
                } else {
                    out.push(JsonValue::Number(
                        Number::from_f64(f)
                            .ok_or_else(|| type_err(span, format!("json_stringify: invalid float {f}")))?,
                    ));
                }
            }
            Ok(JsonValue::Array(out))
        }
        Value::BoolArray(items) => {
            let mut out = Vec::with_capacity(items.len());
            for &b in items {
                out.push(JsonValue::Bool(b != 0));
            }
            Ok(JsonValue::Array(out))
        }
        Value::ByteArray(items) => {
            let mut out = Vec::with_capacity(items.len());
            for &b in items {
                out.push(JsonValue::Number(Number::from(b as i64)));
            }
            Ok(JsonValue::Array(out))
        }
        Value::StringArray(items) => {
            let mut out = Vec::with_capacity(items.len());
            for i in 0..items.len() {
                out.push(JsonValue::String(items.get(i).unwrap_or_default()));
            }
            Ok(JsonValue::Array(out))
        }
        Value::Array(items) => {
            let mut out = Vec::with_capacity(items.len());
            for slot in items {
                out.push(value_to_json(&slot.borrow(), span)?);
            }
            Ok(JsonValue::Array(out))
        }
        Value::Object(map) => {
            let mut out = Map::new();
            let mut keys: Vec<&String> = map.keys().collect();
            keys.sort();
            for k in keys {
                out.insert(k.clone(), value_to_json(&map[k].borrow(), span)?);
            }
            Ok(JsonValue::Object(out))
        }
        #[cfg(feature = "nmongo")]
        Value::BsonDoc(buf) => {
            let mut out = Map::new();
            for elem in buf.iter() {
                let (k, v) = elem.map_err(|e| type_err(span, e.to_string()))?;
                out.insert(
                    k.to_string(),
                    value_to_json(&crate::nmongo::raw_bson_ref_to_niao_cell(v).borrow(), span)?,
                );
            }
            Ok(JsonValue::Object(out))
        }
        Value::Instance(inst) => {
            let mut out = Map::new();
            let mut keys: Vec<&String> = inst.fields.keys().collect();
            keys.sort();
            for k in keys {
                out.insert(k.clone(), value_to_json(&inst.fields[k].borrow(), span)?);
            }
            Ok(JsonValue::Object(out))
        }
        Value::Function(_) | Value::NativeFunction(_) => Err(type_err(
            span,
            "json_stringify: functions cannot be serialized to JSON",
        )),
        Value::Native(_) => Err(type_err(
            span,
            "json_stringify: native DSA values cannot be serialized to JSON",
        )),
        Value::Error(_) => Err(type_err(
            span,
            "json_stringify: error values cannot be serialized to JSON",
        )),
        Value::NclHandle(id) => {
            if let Some(v) = crate::ncl::handles::handle_to_json_value(*id) {
                value_to_json(&v, span)
            } else {
                Err(type_err(
                    span,
                    "json_stringify: this NCL handle cannot be serialized to JSON",
                ))
            }
        }
        Value::NmlHandle(_) => Err(type_err(
            span,
            "json_stringify: NML handles cannot be serialized to JSON",
        )),
    }
}

fn json_type_name(v: &Value) -> &'static str {
    match v {
        Value::Nil => "null",
        Value::Bool(_) => "bool",
        Value::Int(_) | Value::BigInt(_) | Value::Float(_) => "number",
        Value::String(_) => "string",
        Value::IntArray(_) | Value::FloatArray(_) | Value::BoolArray(_) | Value::ByteArray(_) | Value::StringArray(_) | Value::Array(_) => "array",
        Value::Object(_) | Value::BsonDoc(_) => "object",
        Value::Instance(_) => "object",
        Value::Function(_) | Value::NativeFunction(_) => "unsupported",
        Value::Native(_) | Value::Error(_) | Value::NclHandle(_) | Value::NmlHandle(_) => "unsupported",
    }
}

fn is_json_value(v: &Value) -> bool {
    match v {
        Value::Nil
        | Value::Bool(_)
        | Value::Int(_)
        | Value::BigInt(_)
        | Value::Float(_)
        | Value::String(_)
        | Value::IntArray(_)
        | Value::FloatArray(_)
        | Value::BoolArray(_)
        | Value::ByteArray(_)
        | Value::StringArray(_)
        | Value::Array(_)
        | Value::Object(_)
        | Value::BsonDoc(_) => true,
        Value::Function(_)
        | Value::NativeFunction(_)
        | Value::Native(_)
        | Value::Error(_)
        | Value::NclHandle(_)
        | Value::NmlHandle(_)
        | Value::Instance(_) => false,
    }
}

fn parse_path(path: &str) -> Result<Vec<PathToken>, String> {
    if path.is_empty() {
        return Err("json path must not be empty".into());
    }
    let mut tokens = Vec::new();
    let mut i = 0;
    let bytes = path.as_bytes();
    while i < bytes.len() {
        if bytes[i] == b'.' {
            i += 1;
            continue;
        }
        if bytes[i] == b'[' {
            let start = i + 1;
            let end = path[start..]
                .find(']')
                .ok_or_else(|| format!("unclosed '[' in json path: {path}"))?;
            let idx_str = &path[start..start + end];
            let idx: usize = idx_str
                .parse()
                .map_err(|_| format!("invalid array index '{idx_str}' in json path"))?;
            tokens.push(PathToken::Index(idx));
            i = start + end + 1;
            continue;
        }
        let start = i;
        while i < bytes.len() && bytes[i] != b'.' && bytes[i] != b'[' {
            i += 1;
        }
        let key = &path[start..i];
        if key.is_empty() {
            return Err(format!("empty key segment in json path: {path}"));
        }
        tokens.push(PathToken::Key(key.to_string()));
    }
    if tokens.is_empty() {
        return Err(format!("invalid json path: {path}"));
    }
    Ok(tokens)
}

#[derive(Debug, Clone)]
enum PathToken {
    Key(String),
    Index(usize),
}

fn path_has_syntax(path: &str) -> bool {
    path.as_bytes()
        .iter()
        .any(|&b| b == b'.' || b == b'[')
}

fn get_object_key(map: &HashMap<String, ValueRef>, key: &str, span: Span) -> NiaoResult<ValueRef> {
    map.get(key)
        .map(Rc::clone)
        .ok_or_else(|| type_err(span, format!("json_get: key '{key}' not found")))
}

fn get_at_path_rc(val: &ValueRef, tokens: &[PathToken], span: Span) -> NiaoResult<ValueRef> {
    if tokens.is_empty() {
        return Err(type_err(span, "json_get: path must not be empty"));
    }
    let mut cur = Rc::clone(val);
    for (i, token) in tokens.iter().enumerate() {
        let is_last = i + 1 == tokens.len();
        if is_last {
            return match (&*cur.borrow(), token) {
                (Value::Object(map), PathToken::Key(key)) => get_object_key(map, key, span),
                (Value::Array(arr), PathToken::Index(idx)) => arr
                    .get(*idx)
                    .map(Rc::clone)
                    .ok_or_else(|| {
                        type_err(
                            span,
                            format!(
                                "json_get: array index {idx} out of bounds (len {})",
                                arr.len()
                            ),
                        )
                    }),
                (Value::IntArray(arr), PathToken::Index(idx)) => {
                    let idx = *idx;
                    Ok(Value::Int(
                        *arr.get(idx).ok_or_else(|| {
                            type_err(
                                span,
                                format!(
                                    "json_get: array index {idx} out of bounds (len {})",
                                    arr.len()
                                ),
                            )
                        })?,
                    )
                    .ref_cell())
                }
                (Value::ByteArray(arr), PathToken::Index(idx)) => {
                    let idx = *idx;
                    Ok(Value::Int(
                        arr.get(idx)
                            .copied()
                            .map(|b| b as i64)
                            .ok_or_else(|| {
                                type_err(
                                    span,
                                    format!(
                                        "json_get: array index {idx} out of bounds (len {})",
                                        arr.len()
                                    ),
                                )
                            })?,
                    )
                    .ref_cell())
                }
                (Value::StringArray(arr), PathToken::Index(idx)) => {
                    let idx = *idx;
                    Ok(Value::String(
                        arr.get(idx).ok_or_else(|| {
                            type_err(
                                span,
                                format!(
                                    "json_get: array index {idx} out of bounds (len {})",
                                    arr.len()
                                ),
                            )
                        })?,
                    )
                    .ref_cell())
                }
                (other, PathToken::Key(key)) => Err(type_err(
                    span,
                    format!(
                        "json_get: cannot access key '{key}' on {}",
                        other.type_name()
                    ),
                )),
                (other, PathToken::Index(idx)) => Err(type_err(
                    span,
                    format!(
                        "json_get: cannot index {} at [{idx}]",
                        other.type_name()
                    ),
                )),
            };
        }
        cur = {
            let borrowed = cur.borrow();
            match (&*borrowed, token) {
                (Value::Object(map), PathToken::Key(key)) => Rc::clone(
                    map.get(key)
                        .ok_or_else(|| type_err(span, format!("json_get: key '{key}' not found")))?,
                ),
                (Value::Array(arr), PathToken::Index(idx)) => Rc::clone(
                    arr.get(*idx).ok_or_else(|| {
                        type_err(
                            span,
                            format!(
                                "json_get: array index {idx} out of bounds (len {})",
                                arr.len()
                            ),
                        )
                    })?,
                ),
                (Value::IntArray(_), PathToken::Index(idx)) => {
                    return Err(type_err(
                        span,
                        format!("json_get: cannot traverse int array at index [{idx}]"),
                    ));
                }
                (Value::ByteArray(_), PathToken::Index(idx)) => {
                    return Err(type_err(
                        span,
                        format!("json_get: cannot traverse byte array at index [{idx}]"),
                    ));
                }
                (Value::StringArray(_), PathToken::Index(idx)) => {
                    return Err(type_err(
                        span,
                        format!("json_get: cannot traverse string array at index [{idx}]"),
                    ));
                }
                (other, PathToken::Key(key)) => {
                    return Err(type_err(
                        span,
                        format!(
                            "json_get: cannot access key '{key}' on {}",
                            other.type_name()
                        ),
                    ));
                }
                (other, PathToken::Index(idx)) => {
                    return Err(type_err(
                        span,
                        format!(
                            "json_get: cannot index {} at [{idx}]",
                            other.type_name()
                        ),
                    ));
                }
            }
        };
    }
    unreachable!("non-empty tokens always return from loop")
}

fn set_at_leaf(target: &mut Value, token: &PathToken, new_val: ValueRef, span: Span) -> NiaoResult<()> {
    match (target, token) {
        (Value::Object(map), PathToken::Key(key)) => {
            map.insert(key.clone(), new_val);
            Ok(())
        }
        (Value::Array(arr), PathToken::Index(idx)) => {
            let idx = *idx;
            if idx >= arr.len() {
                return Err(type_err(
                    span,
                    format!("json_set: array index {idx} out of bounds (len {})", arr.len()),
                ));
            }
            arr[idx] = new_val;
            Ok(())
        }
        (other, PathToken::Key(key)) => Err(type_err(
            span,
            format!("json_set: cannot set key '{key}' on {}", other.type_name()),
        )),
        (other, PathToken::Index(idx)) => Err(type_err(
            span,
            format!("json_set: cannot set index [{idx}] on {}", other.type_name()),
        )),
    }
}

fn set_at_path(target: &ValueRef, tokens: &[PathToken], new_val: ValueRef, span: Span) -> NiaoResult<()> {
    if tokens.is_empty() {
        return Err(type_err(span, "json_set: path must not be empty"));
    }
    if tokens.len() == 1 {
        return set_at_leaf(&mut target.borrow_mut(), &tokens[0], new_val, span);
    }
    let child = {
        let mut borrowed = target.borrow_mut();
        match &mut *borrowed {
            Value::Object(map) => {
                let PathToken::Key(key) = &tokens[0] else {
                    return Err(type_err(span, "json_set: expected object key in path"));
                };
                let entry = map
                    .entry(key.clone())
                    .or_insert_with(|| Value::Object(HashMap::new()).ref_cell());
                Rc::clone(entry)
            }
            Value::Array(arr) => {
                let PathToken::Index(idx) = &tokens[0] else {
                    return Err(type_err(span, "json_set: expected array index in path"));
                };
                if *idx >= arr.len() {
                    return Err(type_err(
                        span,
                        format!("json_set: array index {idx} out of bounds (len {})", arr.len()),
                    ));
                }
                Rc::clone(&arr[*idx])
            }
            other => {
                return Err(type_err(
                    span,
                    format!(
                        "json_set: cannot traverse path on {}",
                        other.type_name()
                    ),
                ));
            }
        }
    };
    set_at_path(&child, &tokens[1..], new_val, span)
}

fn deep_merge_into(target: &mut Value, source: &Value, span: Span) -> NiaoResult<()> {
    match (target, source) {
        (Value::Object(tmap), Value::Object(smap)) => {
            for (k, sv) in smap {
                if let Some(tv) = tmap.get_mut(k) {
                    deep_merge_into(&mut tv.borrow_mut(), &sv.borrow(), span)?;
                } else {
                    tmap.insert(k.clone(), Rc::new(RefCell::new(sv.borrow().clone())));
                }
            }
            Ok(())
        }
        (t, s) => {
            *t = s.clone();
            Ok(())
        }
    }
}

fn json_deep_equal(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Nil, Value::Nil) => true,
        (Value::Bool(x), Value::Bool(y)) => x == y,
        (Value::Int(x), Value::Int(y)) => x == y,
        (Value::BigInt(x), Value::BigInt(y)) => x == y,
        (Value::Int(x), Value::BigInt(y)) => &BigInt::from(*x) == y,
        (Value::BigInt(x), Value::Int(y)) => x == &BigInt::from(*y),
        (Value::Float(x), Value::Float(y)) => {
            if x.is_nan() && y.is_nan() {
                true
            } else {
                (x - y).abs() < f64::EPSILON || (*x == *y)
            }
        }
        (Value::String(x), Value::String(y)) => x == y,
        (Value::IntArray(ax), Value::IntArray(bx)) => ax == bx,
        (Value::ByteArray(ax), Value::ByteArray(bx)) => ax == bx,
        (Value::StringArray(ax), Value::StringArray(bx)) => ax == bx,
        (Value::Array(ax), Value::Array(bx)) => {
            ax.len() == bx.len()
                && ax
                    .iter()
                    .zip(bx.iter())
                    .all(|(a, b)| json_deep_equal(&a.borrow(), &b.borrow()))
        }
        (Value::Object(ax), Value::Object(bx)) => {
            if ax.len() != bx.len() {
                return false;
            }
            ax.iter().all(|(k, v)| {
                bx.get(k)
                    .map(|bv| json_deep_equal(&v.borrow(), &bv.borrow()))
                    .unwrap_or(false)
            })
        }
        _ => false,
    }
}

fn clone_json_value(v: &Value) -> Value {
    match v {
        Value::Nil => Value::Nil,
        Value::Bool(b) => Value::Bool(*b),
        Value::Int(n) => Value::Int(*n),
        Value::BigInt(n) => Value::BigInt(n.clone()),
        Value::Float(f) => Value::Float(*f),
        Value::String(s) => Value::String(s.clone()),
        Value::IntArray(a) => Value::IntArray(a.clone()),
        Value::ByteArray(a) => Value::ByteArray(a.clone()),
        Value::StringArray(a) => Value::StringArray(a.clone()),
        Value::Array(items) => Value::Array(
            items
                .iter()
                .map(|slot| clone_json_value(&slot.borrow()).ref_cell())
                .collect(),
        ),
        Value::Object(map) => {
            let mut out = HashMap::with_capacity(map.len());
            for (k, v) in map {
                out.insert(k.clone(), clone_json_value(&v.borrow()).ref_cell());
            }
            Value::Object(out)
        }
        other => other.clone(),
    }
}

// ---------------------------------------------------------------------------
// Direct parse (serde visitor → Niao Value, no JsonValue tree)
// ---------------------------------------------------------------------------

struct NiaoJsonVisitor;

struct NiaoJsonSeed;

impl<'de> DeserializeSeed<'de> for NiaoJsonSeed {
    type Value = Value;

    fn deserialize<D: Deserializer<'de>>(self, deserializer: D) -> Result<Self::Value, D::Error> {
        deserializer.deserialize_any(NiaoJsonVisitor)
    }
}

impl<'de> Visitor<'de> for NiaoJsonVisitor {
    type Value = Value;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("JSON value")
    }

    fn visit_bool<E: de::Error>(self, v: bool) -> Result<Self::Value, E> {
        Ok(Value::Bool(v))
    }

    fn visit_i64<E: de::Error>(self, v: i64) -> Result<Self::Value, E> {
        Ok(Value::Int(v))
    }

    fn visit_u64<E: de::Error>(self, v: u64) -> Result<Self::Value, E> {
        if v <= i64::MAX as u64 {
            Ok(Value::Int(v as i64))
        } else {
            Ok(Value::BigInt(BigInt::from(v)))
        }
    }

    fn visit_f64<E: de::Error>(self, v: f64) -> Result<Self::Value, E> {
        if v.fract() == 0.0 && v >= i64::MIN as f64 && v <= i64::MAX as f64 {
            Ok(Value::Int(v as i64))
        } else {
            Ok(Value::Float(v))
        }
    }

    fn visit_str<E: de::Error>(self, v: &str) -> Result<Self::Value, E> {
        Ok(Value::String(v.to_string()))
    }

    fn visit_string<E: de::Error>(self, v: String) -> Result<Self::Value, E> {
        Ok(Value::String(v))
    }

    fn visit_none<E: de::Error>(self) -> Result<Self::Value, E> {
        Ok(Value::Nil)
    }

    fn visit_unit<E: de::Error>(self) -> Result<Self::Value, E> {
        Ok(Value::Nil)
    }

    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Self::Value, A::Error> {
        let mut out = Vec::with_capacity(seq.size_hint().unwrap_or(0));
        while let Some(v) = seq.next_element_seed(NiaoJsonSeed)? {
            out.push(v.ref_cell());
        }
        Ok(Value::Array(out))
    }

    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Self::Value, A::Error> {
        let mut out = HashMap::with_capacity(map.size_hint().unwrap_or(0));
        while let Some(key) = map.next_key::<String>()? {
            let v = map.next_value_seed(NiaoJsonSeed)?;
            out.insert(key, v.ref_cell());
        }
        Ok(Value::Object(out))
    }
}

fn parse_json_text(text: &str, span: Span) -> NiaoResult<Value> {
    let mut de = serde_json::Deserializer::from_slice(text.as_bytes());
    NiaoJsonSeed.deserialize(&mut de).map_err(|e| {
        RuntimeError::at(
            span,
            codes::E1013_JSON_PARSE,
            format!("json_parse: {e}"),
        )
    })
}

// ---------------------------------------------------------------------------
// Direct stringify (Niao Value → JSON text, no JsonValue tree)
// ---------------------------------------------------------------------------

fn append_json_string(s: &str, out: &mut String) {
    out.reserve(s.len() + 2);
    out.push('"');
    for ch in s.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c.is_control() => {
                use std::fmt::Write;
                let _ = write!(out, "\\u{:04x}", c as u32);
            }
            c => out.push(c),
        }
    }
    out.push('"');
}

fn append_json_number(n: i64, out: &mut String) {
    out.push_str(&n.to_string());
}

fn append_json_float(f: f64, out: &mut String) {
    if let Some(n) = Number::from_f64(f) {
        out.push_str(&n.to_string());
    } else {
        out.push_str("null");
    }
}

fn stringify_value(v: &Value, out: &mut String, span: Span) -> NiaoResult<()> {
    match v {
        Value::Nil => out.push_str("null"),
        Value::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
        Value::Int(n) => append_json_number(*n, out),
        Value::BigInt(n) => {
            if let Some(i) = n.to_i64() {
                append_json_number(i, out);
            } else if let Some(u) = n.to_u64() {
                out.push_str(&u.to_string());
            } else {
                return Err(type_err(
                    span,
                    format!("json_stringify: bigint {n} does not fit in JSON number"),
                ));
            }
        }
        Value::Float(f) => {
            if !f.is_finite() {
                out.push_str("null");
            } else {
                append_json_float(*f, out);
            }
        }
        Value::String(s) => append_json_string(s, out),
        Value::IntArray(items) => {
            out.push('[');
            for (i, &n) in items.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                append_json_number(n, out);
            }
            out.push(']');
        }
        Value::FloatArray(items) => {
            out.push('[');
            for (i, &f) in items.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                if !f.is_finite() {
                    out.push_str("null");
                } else {
                    append_json_float(f, out);
                }
            }
            out.push(']');
        }
        Value::BoolArray(items) => {
            out.push('[');
            for (i, &b) in items.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                out.push_str(if b != 0 { "true" } else { "false" });
            }
            out.push(']');
        }
        Value::ByteArray(items) => {
            out.push('[');
            for (i, &b) in items.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                append_json_number(b as i64, out);
            }
            out.push(']');
        }
        Value::StringArray(items) => {
            out.push('[');
            for i in 0..items.len() {
                if i > 0 {
                    out.push(',');
                }
                append_json_string(&items.get(i).unwrap_or_default(), out);
            }
            out.push(']');
        }
        Value::Array(items) => {
            out.push('[');
            for (i, slot) in items.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                stringify_value(&slot.borrow(), out, span)?;
            }
            out.push(']');
        }
        Value::Object(map) => {
            out.push('{');
            let mut keys: Vec<&String> = map.keys().collect();
            keys.sort();
            for (i, k) in keys.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                append_json_string(k, out);
                out.push(':');
                stringify_value(&map[*k].borrow(), out, span)?;
            }
            out.push('}');
        }
        #[cfg(feature = "nmongo")]
        Value::BsonDoc(buf) => {
            out.push('{');
            let mut first = true;
            for elem in buf.iter() {
                let (k, v) = elem.map_err(|e| type_err(span, e.to_string()))?;
                if !first {
                    out.push(',');
                }
                first = false;
                append_json_string(k, out);
                out.push(':');
                stringify_value(
                    &crate::nmongo::raw_bson_ref_to_niao_cell(v).borrow(),
                    out,
                    span,
                )?;
            }
            out.push('}');
        }
        #[cfg(not(feature = "nmongo"))]
        Value::BsonDoc(_) => {
            return Err(type_err(
                span,
                "json_stringify: BSON documents require the nmongo feature",
            ));
        }
        Value::Instance(inst) => {
            out.push('{');
            let mut keys: Vec<&String> = inst.fields.keys().collect();
            keys.sort();
            for (i, k) in keys.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                append_json_string(k, out);
                out.push(':');
                stringify_value(&inst.fields[*k].borrow(), out, span)?;
            }
            out.push('}');
        }
        Value::Function(_) | Value::NativeFunction(_) => {
            return Err(type_err(
                span,
                "json_stringify: functions cannot be serialized to JSON",
            ));
        }
        Value::Native(_) => {
            return Err(type_err(
                span,
                "json_stringify: native DSA values cannot be serialized to JSON",
            ));
        }
        Value::Error(_) => {
            return Err(type_err(
                span,
                "json_stringify: error values cannot be serialized to JSON",
            ));
        }
        Value::NclHandle(id) => {
            if let Some(v) = crate::ncl::handles::handle_to_json_value(*id) {
                stringify_value(&v, out, span)?;
            } else {
                return Err(type_err(
                    span,
                    "json_stringify: this NCL handle cannot be serialized to JSON",
                ));
            }
        }
        Value::NmlHandle(_) => {
            return Err(type_err(
                span,
                "json_stringify: NML handles cannot be serialized to JSON",
            ));
        }
    }
    Ok(())
}

fn stringify_value_to_string(v: &Value, span: Span) -> NiaoResult<String> {
    let cap = estimate_json_len(v);
    let mut out = String::with_capacity(cap);
    stringify_value(v, &mut out, span)?;
    Ok(out)
}

fn estimate_json_len(v: &Value) -> usize {
    match v {
        Value::Nil => 4,
        Value::Bool(_) => 5,
        Value::Int(n) => n.unsigned_abs().max(1).ilog10() as usize + 2,
        Value::BigInt(_) | Value::Float(_) => 24,
        Value::String(s) => s.len() + 2,
        Value::IntArray(items) => items.len() * 4 + 2,
        Value::FloatArray(items) => items.len() * 8 + 2,
        Value::BoolArray(items) => items.len() * 5 + 2,
        Value::ByteArray(items) => items.len() * 4 + 2,
        Value::StringArray(items) => {
            2 + (0..items.len())
                .map(|i| items.get(i).map(|s| s.len() + 2).unwrap_or(2))
                .sum::<usize>()
        }
        Value::Array(items) => {
            2 + items
                .iter()
                .map(|slot| estimate_json_len(&slot.borrow()))
                .sum::<usize>()
        }
        Value::Object(map) => {
            2 + map
                .iter()
                .map(|(k, v)| k.len() + 3 + estimate_json_len(&v.borrow()))
                .sum::<usize>()
        }
        Value::Instance(inst) => {
            2 + inst
                .fields
                .iter()
                .map(|(k, v)| k.len() + 3 + estimate_json_len(&v.borrow()))
                .sum::<usize>()
        }
        _ => 32,
    }
}


pub fn json_parse(args: &[ValueRef], span: Span) -> NiaoResult<ValueRef> {
    arity(args, 1, "json_parse", span)?;
    let borrowed = args[0].borrow();
    let text = match &*borrowed {
        Value::String(s) => s.as_str(),
        other => {
            return Err(type_err(
                span,
                format!(
                    "json_parse() expects a string as argument 1, got {}",
                    other.type_name()
                ),
            ));
        }
    };
    Ok(parse_json_text(text, span)?.ref_cell())
}

pub fn json_stringify(args: &[ValueRef], span: Span) -> NiaoResult<ValueRef> {
    arity(args, 1, "json_stringify", span)?;
    let out = stringify_value_to_string(&args[0].borrow(), span)?;
    Ok(Value::String(out).ref_cell())
}

fn json_stringify_pretty(args: &[ValueRef], span: Span) -> NiaoResult<ValueRef> {
    arity_range(args, 1, 2, "json_stringify_pretty", span)?;
    let indent = if args.len() == 2 {
        int_arg(args, 1, "json_stringify_pretty", span)?
    } else {
        2
    };
    if indent < 0 {
        return Err(type_err(
            span,
            "json_stringify_pretty: indent must be non-negative",
        ));
    }
    let j = value_to_json(&args[0].borrow(), span)?;
    let out = serde_json::to_string_pretty(&j).map_err(|e| {
        RuntimeError::at(
            span,
            codes::E1014_JSON_TYPE,
            format!("json_stringify_pretty: {e}"),
        )
    })?;
    // serde_json pretty always uses 2-space indent; re-indent if needed via manual replace is wasteful.
    // For custom indent, format manually when != 2
    let out = if indent == 2 {
        out
    } else {
        let space = " ".repeat(indent as usize);
        out.replace("  ", &space)
    };
    Ok(Value::String(out).ref_cell())
}

fn json_valid(args: &[ValueRef], span: Span) -> NiaoResult<ValueRef> {
    arity(args, 1, "json_valid", span)?;
    let borrowed = args[0].borrow();
    let text = match &*borrowed {
        Value::String(s) => s.as_str(),
        other => {
            return Err(type_err(
                span,
                format!(
                    "json_valid() expects a string as argument 1, got {}",
                    other.type_name()
                ),
            ));
        }
    };
    Ok(Value::Bool(serde_json::from_slice::<JsonValue>(text.as_bytes()).is_ok()).ref_cell())
}

fn json_type(args: &[ValueRef], span: Span) -> NiaoResult<ValueRef> {
    arity(args, 1, "json_type", span)?;
    Ok(Value::String(json_type_name(&args[0].borrow()).to_string()).ref_cell())
}

fn json_is_json(args: &[ValueRef], span: Span) -> NiaoResult<ValueRef> {
    arity(args, 1, "json_is_json", span)?;
    Ok(Value::Bool(is_json_value(&args[0].borrow())).ref_cell())
}

fn json_keys(args: &[ValueRef], span: Span) -> NiaoResult<ValueRef> {
    arity(args, 1, "json_keys", span)?;
    match &*args[0].borrow() {
        Value::Object(map) => {
            let mut keys: Vec<String> = map.keys().cloned().collect();
            keys.sort();
            let arr: Vec<ValueRef> = keys
                .into_iter()
                .map(|k| Value::String(k).ref_cell())
                .collect();
            Ok(Value::Array(arr).ref_cell())
        }
        other => Err(type_err(
            span,
            format!("json_keys() expects object, got {}", other.type_name()),
        )),
    }
}

fn json_has(args: &[ValueRef], span: Span) -> NiaoResult<ValueRef> {
    arity(args, 2, "json_has", span)?;
    let key_borrowed = args[1].borrow();
    let key = match &*key_borrowed {
        Value::String(s) => s.as_str(),
        other => {
            return Err(type_err(
                span,
                format!(
                    "json_has() expects a string as argument 2, got {}",
                    other.type_name()
                ),
            ));
        }
    };
    let found = match &*args[0].borrow() {
        Value::Object(map) => map.contains_key(key),
        #[cfg(feature = "nmongo")]
        Value::BsonDoc(buf) => buf.get(key).ok().flatten().is_some(),
        other => {
            return Err(type_err(
                span,
                format!("json_has() expects object, got {}", other.type_name()),
            ));
        }
    };
    Ok(Value::Bool(found).ref_cell())
}

fn json_get(args: &[ValueRef], span: Span) -> NiaoResult<ValueRef> {
    arity(args, 2, "json_get", span)?;
    match &*args[1].borrow() {
        Value::String(path) => {
            if !path_has_syntax(path) {
                return match &*args[0].borrow() {
                    Value::Object(map) => get_object_key(map, path, span),
                    #[cfg(feature = "nmongo")]
                    Value::BsonDoc(buf) => bson_field_from_raw(buf, path).ok_or_else(|| {
                        type_err(span, format!("json_get: key '{path}' not found"))
                    }),
                    other => Err(type_err(
                        span,
                        format!(
                            "json_get: cannot access key '{path}' on {}",
                            other.type_name()
                        ),
                    )),
                };
            }
            let tokens = parse_path(path).map_err(|e| type_err(span, e))?;
            get_at_path_rc(&args[0], &tokens, span)
        }
        other => Err(type_err(
            span,
            format!(
                "json_get() expects a string as argument 2, got {}",
                other.type_name()
            ),
        )),
    }
}

fn json_set(args: &[ValueRef], span: Span) -> NiaoResult<ValueRef> {
    arity(args, 3, "json_set", span)?;
    let new_val = Rc::clone(&args[2]);
    match &*args[1].borrow() {
        Value::String(path) => {
            if !path_has_syntax(path) {
                return match &mut *args[0].borrow_mut() {
                    Value::Object(map) => {
                        map.insert(path.clone(), new_val);
                        Ok(Value::Nil.ref_cell())
                    }
                    other => Err(type_err(
                        span,
                        format!("json_set: cannot set key '{path}' on {}", other.type_name()),
                    )),
                };
            }
            let tokens = parse_path(path).map_err(|e| type_err(span, e))?;
            set_at_path(&args[0], &tokens, new_val, span)?;
            Ok(Value::Nil.ref_cell())
        }
        other => Err(type_err(
            span,
            format!(
                "json_set() expects a string as argument 2, got {}",
                other.type_name()
            ),
        )),
    }
}

fn json_merge(args: &[ValueRef], span: Span) -> NiaoResult<ValueRef> {
    arity(args, 2, "json_merge", span)?;
    deep_merge_into(&mut args[0].borrow_mut(), &args[1].borrow(), span)?;
    Ok(Value::Nil.ref_cell())
}

fn json_clone(args: &[ValueRef], span: Span) -> NiaoResult<ValueRef> {
    arity(args, 1, "json_clone", span)?;
    Ok(clone_json_value(&args[0].borrow()).ref_cell())
}

fn json_equal(args: &[ValueRef], span: Span) -> NiaoResult<ValueRef> {
    arity(args, 2, "json_equal", span)?;
    Ok(
        Value::Bool(json_deep_equal(&args[0].borrow(), &args[1].borrow())).ref_cell(),
    )
}

fn json_array_len(args: &[ValueRef], span: Span) -> NiaoResult<ValueRef> {
    arity(args, 1, "json_array_len", span)?;
    let len = match &*args[0].borrow() {
        Value::Array(a) => a.len(),
        Value::IntArray(a) => a.len(),
        Value::ByteArray(a) => a.len(),
        Value::StringArray(a) => a.len(),
        other => {
            return Err(type_err(
                span,
                format!("json_array_len() expects array, got {}", other.type_name()),
            ));
        }
    };
    Ok(Value::Int(len as i64).ref_cell())
}

fn json_object_len(args: &[ValueRef], span: Span) -> NiaoResult<ValueRef> {
    arity(args, 1, "json_object_len", span)?;
    match &*args[0].borrow() {
        Value::Object(map) => Ok(Value::Int(map.len() as i64).ref_cell()),
        other => Err(type_err(
            span,
            format!("json_object_len() expects object, got {}", other.type_name()),
        )),
    }
}

/// Short-name JSON module object for `json.parse`, `json.stringify`, etc.
pub fn namespace() -> Value {
    let mut map = HashMap::new();
    let bind = |map: &mut HashMap<String, ValueRef>, name: &str, f: NativeFn| {
        map.insert(name.to_string(), Value::NativeFunction(f).ref_cell());
    };
    bind(&mut map, "parse", Rc::new(json_parse));
    bind(&mut map, "stringify", Rc::new(json_stringify));
    bind(&mut map, "stringify_pretty", Rc::new(json_stringify_pretty));
    bind(&mut map, "valid", Rc::new(json_valid));
    bind(&mut map, "type", Rc::new(json_type));
    bind(&mut map, "is_json", Rc::new(json_is_json));
    bind(&mut map, "keys", Rc::new(json_keys));
    bind(&mut map, "has", Rc::new(json_has));
    bind(&mut map, "get", Rc::new(json_get));
    bind(&mut map, "set", Rc::new(json_set));
    bind(&mut map, "merge", Rc::new(json_merge));
    bind(&mut map, "clone", Rc::new(json_clone));
    bind(&mut map, "equal", Rc::new(json_equal));
    bind(&mut map, "array_len", Rc::new(json_array_len));
    bind(&mut map, "object_len", Rc::new(json_object_len));
    Value::Object(map)
}

/// Export name used when `import "json"` (or `import "std/json"`) is loaded.
pub const MODULE_NAME: &str = "json";

/// Paths that resolve to this native module.
pub const MODULE_PATHS: &[&str] = &["json", "std/json"];

/// All JSON builtins in registration order (legacy `json_*` names).
pub fn builtins() -> Vec<(&'static str, NativeFn)> {
    vec![
        ("json_parse", Rc::new(json_parse)),
        ("json_stringify", Rc::new(json_stringify)),
        ("json_stringify_pretty", Rc::new(json_stringify_pretty)),
        ("json_valid", Rc::new(json_valid)),
        ("json_type", Rc::new(json_type)),
        ("json_is_json", Rc::new(json_is_json)),
        ("json_keys", Rc::new(json_keys)),
        ("json_has", Rc::new(json_has)),
        ("json_get", Rc::new(json_get)),
        ("json_set", Rc::new(json_set)),
        ("json_merge", Rc::new(json_merge)),
        ("json_clone", Rc::new(json_clone)),
        ("json_equal", Rc::new(json_equal)),
        ("json_array_len", Rc::new(json_array_len)),
        ("json_object_len", Rc::new(json_object_len)),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use niao_ast::Span;

    fn dummy_span() -> Span {
        Span::dummy()
    }

    #[test]
    fn round_trip_object() {
        let span = dummy_span();
        let mut map = HashMap::new();
        map.insert("name".into(), Value::String("Niao".into()).ref_cell());
        map.insert("version".into(), Value::Int(1).ref_cell());
        let val = Value::Object(map).ref_cell();
        let j = value_to_json(&val.borrow(), span).unwrap();
        let s = serde_json::to_string(&j).unwrap();
        assert_eq!(s, r#"{"name":"Niao","version":1}"#);
        let back = json_to_value(serde_json::from_str(&s).unwrap());
        assert!(json_deep_equal(&val.borrow(), &back));
    }

    #[test]
    fn parse_path_segments() {
        let tokens = parse_path("user.items[0].name").unwrap();
        assert_eq!(tokens.len(), 4);
        match &tokens[0] {
            PathToken::Key(k) => assert_eq!(k, "user"),
            _ => panic!("expected key"),
        }
        match &tokens[1] {
            PathToken::Key(k) => assert_eq!(k, "items"),
            _ => panic!("expected key"),
        }
        match &tokens[2] {
            PathToken::Index(0) => {}
            _ => panic!("expected index"),
        }
        match &tokens[3] {
            PathToken::Key(k) => assert_eq!(k, "name"),
            _ => panic!("expected key"),
        }
    }
}
