//! Packed column storage with optional validity bitmap.

use crate::{StringArray, Value, ValueRef};
use super::dtypes::Dtype;

#[derive(Clone)]
pub struct Validity {
    bits: Vec<u8>,
    len: usize,
}

impl Validity {
    pub fn all_valid(len: usize) -> Self {
        let byte_len = len.div_ceil(8);
        Self {
            bits: vec![0xFF; byte_len],
            len,
        }
    }

    pub fn with_nulls(len: usize, null_indices: &[usize]) -> Self {
        let mut v = Self::all_valid(len);
        for &i in null_indices {
            if i < len {
                v.set_invalid(i);
            }
        }
        v
    }

    pub fn len(&self) -> usize {
        self.len
    }

    pub fn is_valid(&self, i: usize) -> bool {
        if i >= self.len {
            return false;
        }
        (self.bits[i / 8] >> (i % 8)) & 1 == 1
    }

    pub fn set_invalid(&mut self, i: usize) {
        if i < self.len {
            self.bits[i / 8] &= !(1 << (i % 8));
        }
    }

    pub fn valid_count(&self) -> usize {
        (0..self.len).filter(|&i| self.is_valid(i)).count()
    }
}

#[derive(Clone)]
pub enum Column {
    Int(Vec<i64>),
    Float(Vec<f64>),
    Bool(Vec<u8>),
    String(StringArray),
    Any(Vec<ValueRef>),
}

impl Column {
    pub fn len(&self) -> usize {
        match self {
            Column::Int(v) => v.len(),
            Column::Float(v) => v.len(),
            Column::Bool(v) => v.len(),
            Column::String(v) => v.len(),
            Column::Any(v) => v.len(),
        }
    }

    pub fn dtype(&self) -> Dtype {
        match self {
            Column::Int(_) => Dtype::Int,
            Column::Float(_) => Dtype::Float,
            Column::Bool(_) => Dtype::Bool,
            Column::String(_) => Dtype::String,
            Column::Any(_) => Dtype::Any,
        }
    }

    pub fn empty(dtype: Dtype) -> Self {
        match dtype {
            Dtype::Int => Column::Int(Vec::new()),
            Dtype::Float => Column::Float(Vec::new()),
            Dtype::Bool => Column::Bool(Vec::new()),
            Dtype::String => Column::String(StringArray::dense(Vec::new())),
            Dtype::Any => Column::Any(Vec::new()),
        }
    }

    pub fn promote_to_float(&mut self) {
        if let Column::Int(v) = std::mem::replace(self, Column::Float(Vec::new())) {
            *self = Column::Float(v.into_iter().map(|n| n as f64).collect());
        }
    }

    pub fn as_int_slice(&self) -> Option<&[i64]> {
        match self {
            Column::Int(v) => Some(v),
            _ => None,
        }
    }

    pub fn as_float_slice(&self) -> Option<&[f64]> {
        match self {
            Column::Float(v) => Some(v),
            _ => None,
        }
    }

    pub fn to_value_array(&self) -> Value {
        match self {
            Column::Int(v) => Value::IntArray(v.clone()),
            Column::Float(v) => Value::FloatArray(v.clone()),
            Column::Bool(v) => Value::BoolArray(v.clone()),
            Column::String(s) => Value::StringArray(s.clone()),
            Column::Any(v) => Value::Array(v.clone()),
        }
    }

    /// Coerce VM `array` literals (`[1, 2, 3]` or `[1.0, 2.0]`) into packed arrays.
    pub fn coerce_packed_array(val: &Value) -> Option<Value> {
        match val {
            Value::IntArray(_) | Value::FloatArray(_) | Value::BoolArray(_) => Some(val.clone()),
            Value::Array(items) => {
                if items.is_empty() {
                    return Some(Value::IntArray(Vec::new()));
                }
                let mut all_int = true;
                let mut ints = Vec::with_capacity(items.len());
                for item in items {
                    match &*item.borrow() {
                        Value::Int(n) => ints.push(*n),
                        _ => {
                            all_int = false;
                            break;
                        }
                    }
                }
                if all_int {
                    return Some(Value::IntArray(ints));
                }
                let mut floats = Vec::with_capacity(items.len());
                for item in items {
                    match &*item.borrow() {
                        Value::Int(n) => floats.push(*n as f64),
                        Value::Float(f) => floats.push(*f),
                        _ => return None,
                    }
                }
                Some(Value::FloatArray(floats))
            }
            _ => None,
        }
    }

    pub fn from_value_array(val: &Value) -> Option<Self> {
        if let Some(packed) = Self::coerce_packed_array(val) {
            return match &packed {
                Value::IntArray(v) => Some(Column::Int(v.clone())),
                Value::FloatArray(v) => Some(Column::Float(v.clone())),
                Value::BoolArray(v) => Some(Column::Bool(v.clone())),
                _ => None,
            };
        }
        match val {
            Value::StringArray(v) => Some(Column::String(v.clone())),
            Value::Array(v) => Some(Column::Any(v.clone())),
            _ => None,
        }
    }
}
