//! VM fast paths for hot NCL builtins.

use crate::fast_val::FastVal;
use niao_runtime::{Value, ValueRef};

#[derive(Clone, Copy)]
pub enum NclFastPath {
    Sum = 0,
    Mean = 1,
}

impl NclFastPath {
    pub fn from_name(name: &str) -> Option<Self> {
        match name {
            "ncl_sum" => Some(Self::Sum),
            "ncl_mean" => Some(Self::Mean),
            _ => None,
        }
    }

    pub fn try_execute(stack: &mut Vec<FastVal>, heap: &[ValueRef], argc: usize, path: Self) -> bool {
        if argc != 1 || stack.len() < argc {
            return false;
        }
        let base = stack.len() - argc;
        let arr = match stack[base] {
            FastVal::Heap(idx) => &heap[idx as usize],
            _ => return false,
        };
        let out = match path {
            NclFastPath::Sum => match &*arr.borrow() {
                Value::IntArray(v) => {
                    let s: i128 = v.iter().map(|&x| x as i128).sum();
                    FastVal::Int(s as i64)
                }
                _ => return false,
            },
            NclFastPath::Mean => match &*arr.borrow() {
                Value::IntArray(v) if !v.is_empty() => {
                    let n = v.len();
                    let s: i128 = v.iter().map(|&x| x as i128).sum();
                    FastVal::Float(s as f64 / n as f64)
                }
                Value::FloatArray(v) if !v.is_empty() => {
                    let n = v.len();
                    let s: f64 = v.iter().sum();
                    FastVal::Float(s / n as f64)
                }
                _ => return false,
            },
        };
        stack.truncate(base);
        stack.push(out);
        true
    }
}
