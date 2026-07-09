//! One-dimensional labeled column.

use super::column::{Column, Validity};
use super::dtypes::Dtype;
use crate::{StringArray, Value};

#[derive(Clone)]
pub struct Series {
    pub name: String,
    pub data: Column,
    pub validity: Option<Validity>,
    pub index: Option<Vec<i64>>,
}

impl Series {
    pub fn new(name: impl Into<String>, data: Column) -> Self {
        let len = data.len();
        Self {
            name: name.into(),
            data,
            validity: None,
            index: None,
        }
        .with_len_check(len)
    }

    fn with_len_check(self, _len: usize) -> Self {
        self
    }

    pub fn len(&self) -> usize {
        self.data.len()
    }

    pub fn dtype(&self) -> Dtype {
        self.data.dtype()
    }

    pub fn from_int_array(name: impl Into<String>, v: Vec<i64>) -> Self {
        Self::new(name, Column::Int(v))
    }

    pub fn from_float_array(name: impl Into<String>, v: Vec<f64>) -> Self {
        Self::new(name, Column::Float(v))
    }

    pub fn from_value(name: impl Into<String>, val: &Value) -> Option<Self> {
        Column::from_value_array(val).map(|col| Self::new(name, col))
    }

    pub fn to_value_array(&self) -> Value {
        self.data.to_value_array()
    }

    pub fn slice(&self, start: usize, end: usize) -> Self {
        let data = match &self.data {
            Column::Int(v) => Column::Int(v[start..end.min(v.len())].to_vec()),
            Column::Float(v) => Column::Float(v[start..end.min(v.len())].to_vec()),
            Column::Bool(v) => Column::Bool(v[start..end.min(v.len())].to_vec()),
            Column::String(s) => {
                let dense = (start..end.min(s.len()))
                    .map(|i| s.get(i).unwrap_or_default())
                    .collect();
                Column::String(StringArray::dense(dense))
            }
            Column::Any(v) => Column::Any(v[start..end.min(v.len())].to_vec()),
        };
        Self {
            name: self.name.clone(),
            data,
            validity: self.validity.clone(),
            index: self.index.clone(),
        }
    }
}
