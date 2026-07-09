//! VM fast paths for hot nmongo builtins (skip native `Rc` clone on dispatch).

use crate::fast_val::{FastVal, HeapAlloc};
use niao_ast::Span;
use niao_runtime::nmongo::crud::{
    nmongo_count_documents, nmongo_find, nmongo_find_one, nmongo_insert_many, nmongo_insert_one,
};
use niao_runtime::{Value, ValueRef};
use std::rc::Rc;

#[derive(Clone, Copy)]
pub struct NmongoFastPath(pub u8);

impl NmongoFastPath {
    pub fn from_name(name: &str) -> Option<Self> {
        match name {
            "nmongo_count_documents" => Some(Self(0)),
            "nmongo_find_one" => Some(Self(1)),
            "nmongo_insert_one" => Some(Self(2)),
            "nmongo_insert_many" => Some(Self(4)),
            "nmongo_find" => Some(Self(3)),
            _ => None,
        }
    }

    pub fn try_execute_args(args: &[ValueRef], path: Self) -> Option<ValueRef> {
        let span = Span::dummy();
        let argc = args.len();
        match path.0 {
            0 if (3..=4).contains(&argc) => nmongo_count_documents(args, span).ok(),
            1 if (3..=5).contains(&argc) => nmongo_find_one(args, span).ok(),
            2 if (4..=5).contains(&argc) => nmongo_insert_one(args, span).ok(),
            3 if (3..=5).contains(&argc) => nmongo_find(args, span).ok(),
            4 if (4..=5).contains(&argc) => nmongo_insert_many(args, span).ok(),
            _ => None,
        }
    }
}

pub fn to_fast_val(result: ValueRef, heap: &mut impl HeapAlloc) -> FastVal {
    if let Value::Int(v) = *result.borrow() {
        return FastVal::Int(v);
    }
    if let Value::Float(v) = *result.borrow() {
        return FastVal::Float(v);
    }
    if let Value::Bool(v) = *result.borrow() {
        return FastVal::Bool(v);
    }
    if matches!(*result.borrow(), Value::Nil) {
        return FastVal::Nil;
    }
    FastVal::Heap(heap.push_heap(result))
}

pub fn args_from_stack(
    stack: &[FastVal],
    base: usize,
    argc: usize,
    heap: &[ValueRef],
    native_refs: &[ValueRef],
) -> Vec<ValueRef> {
    stack[base..base + argc]
        .iter()
        .map(|v| fast_to_value_ref(*v, heap, native_refs))
        .collect()
}

fn fast_to_value_ref(v: FastVal, heap: &[ValueRef], native_refs: &[ValueRef]) -> ValueRef {
    match v {
        FastVal::Native(i) => Rc::clone(&native_refs[i as usize]),
        other => other.to_value_ref(heap, native_refs),
    }
}
