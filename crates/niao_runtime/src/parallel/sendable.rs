//! Cross-thread value marshaling for the `parallel` standard library.

use crate::{StringArray, Value, ValueRef};
use std::collections::HashMap;

/// Subset of Niao values safe to move between OS threads.
#[derive(Clone, Debug)]
pub enum SendableValue {
    Nil,
    Int(i64),
    Float(f64),
    Bool(bool),
    String(String),
    IntArray(Vec<i64>),
    FloatArray(Vec<f64>),
    BoolArray(Vec<u8>),
    ByteArray(Vec<u8>),
    StringArray(Vec<String>),
    Array(Vec<SendableValue>),
    Object(HashMap<String, SendableValue>),
}

impl SendableValue {
    pub fn nil() -> Self {
        Self::Nil
    }

    pub fn to_value(self) -> Value {
        match self {
            Self::Nil => Value::Nil,
            Self::Int(n) => Value::Int(n),
            Self::Float(f) => Value::Float(f),
            Self::Bool(b) => Value::Bool(b),
            Self::String(s) => Value::String(s),
            Self::IntArray(v) => Value::IntArray(v),
            Self::FloatArray(v) => Value::FloatArray(v),
            Self::BoolArray(v) => Value::BoolArray(v),
            Self::ByteArray(v) => Value::ByteArray(v),
            Self::StringArray(v) => Value::StringArray(StringArray::dense(v)),
            Self::Array(items) => {
                Value::Array(items.into_iter().map(|v| v.to_value().ref_cell()).collect())
            }
            Self::Object(map) => {
                let mut out = HashMap::with_capacity(map.len());
                for (k, v) in map {
                    out.insert(k, v.to_value().ref_cell());
                }
                Value::Object(out)
            }
        }
    }
}

pub fn value_to_sendable(val: &Value) -> Result<SendableValue, String> {
    match val {
        Value::Nil => Ok(SendableValue::Nil),
        Value::Int(n) => Ok(SendableValue::Int(*n)),
        Value::Float(f) => Ok(SendableValue::Float(*f)),
        Value::Bool(b) => Ok(SendableValue::Bool(*b)),
        Value::String(s) => Ok(SendableValue::String(s.clone())),
        Value::IntArray(v) => Ok(SendableValue::IntArray(v.clone())),
        Value::FloatArray(v) => Ok(SendableValue::FloatArray(v.clone())),
        Value::BoolArray(v) => Ok(SendableValue::BoolArray(v.clone())),
        Value::ByteArray(v) => Ok(SendableValue::ByteArray(v.clone())),
        Value::StringArray(v) => Ok(SendableValue::StringArray(v.dense_vec())),
        Value::Array(items) => {
            let mut out = Vec::with_capacity(items.len());
            for item in items {
                out.push(value_to_sendable(&item.borrow())?);
            }
            Ok(SendableValue::Array(out))
        }
        Value::Object(map) => {
            let mut out = HashMap::with_capacity(map.len());
            for (k, v) in map {
                out.insert(k.clone(), value_to_sendable(&v.borrow())?);
            }
            Ok(SendableValue::Object(out))
        }
        Value::BigInt(_) => Err("bigint values cannot be sent across threads".into()),
        Value::Function(_) => Err("functions cannot be sent across threads".into()),
        Value::NativeFunction(_) => Err("native functions cannot be sent across threads".into()),
        Value::Instance(inst) => Err(format!(
            "{} instances cannot be sent across threads",
            inst.class_name
        )),
        Value::Native(ds) => Err(format!(
            "{} handles cannot be sent across threads",
            ds.borrow().kind_name()
        )),
        Value::Error(e) => Err(format!("error values cannot be sent across threads: {}", e.message)),
        Value::NclHandle(_) => Err("NCL handles cannot be sent across threads".into()),
        Value::NmlHandle(_) => Err("NML handles cannot be sent across threads".into()),
        #[cfg(feature = "nmongo")]
        Value::BsonDoc(_) => Err("BSON documents cannot be sent across threads".into()),
    }
}

pub fn sendable_to_value_ref(val: SendableValue) -> ValueRef {
    val.to_value().ref_cell()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_primitives() {
        for v in [
            Value::Nil,
            Value::Int(42),
            Value::Float(3.14),
            Value::Bool(true),
            Value::String("hi".into()),
            Value::IntArray(vec![1, 2, 3]),
        ] {
            let s = value_to_sendable(&v).unwrap();
            assert_eq!(format!("{:?}", v), format!("{:?}", s.to_value()));
        }
    }

    #[test]
    fn rejects_native_function() {
        let f = Value::NativeFunction(std::rc::Rc::new(|_, _| Ok(Value::Nil.ref_cell())));
        assert!(value_to_sendable(&f).is_err());
    }
}
