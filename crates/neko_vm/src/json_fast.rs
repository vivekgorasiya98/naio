//! VM fast paths for JSON builtins (skip full native dispatch).

use crate::fast_val::{FastVal, HeapAlloc};
use neko_ast::Span;
use neko_runtime::{json_parse, json_stringify, Value, ValueRef};
use std::rc::Rc;

#[derive(Clone, Copy)]
pub enum JsonFastPath {
    Parse = 0,
    Stringify = 1,
}

impl JsonFastPath {
    pub fn from_name(name: &str) -> Option<Self> {
        match name {
            "json.parse" | "json_parse" => Some(Self::Parse),
            "json.stringify" | "json_stringify" => Some(Self::Stringify),
            _ => None,
        }
    }

    pub fn try_execute(
        args: &[FastVal],
        heap: &[ValueRef],
        native_refs: &[ValueRef],
        path: Self,
        heap_mut: &mut impl HeapAlloc,
    ) -> Option<FastVal> {
        let refs: Vec<ValueRef> = args
            .iter()
            .map(|v| v.to_value_ref(heap, native_refs))
            .collect();
        let span = Span::dummy();
        let result = match path {
            JsonFastPath::Parse if refs.len() == 1 => json_parse(&refs, span).ok(),
            JsonFastPath::Stringify if (1..=2).contains(&refs.len()) => {
                json_stringify(&refs, span).ok()
            }
            _ => None,
        }?;
        Some(value_from_ref(&result, heap_mut))
    }
}

fn value_from_ref(result: &ValueRef, heap: &mut impl HeapAlloc) -> FastVal {
    match &*result.borrow() {
        Value::Int(n) => FastVal::Int(*n),
        Value::Float(n) => FastVal::Float(*n),
        Value::Bool(b) => FastVal::Bool(*b),
        Value::Nil => FastVal::Nil,
        _ => FastVal::Heap(heap.push_heap(Rc::clone(result))),
    }
}
