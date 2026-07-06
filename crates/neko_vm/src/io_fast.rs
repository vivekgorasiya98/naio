//! VM fast paths for whole-file I/O builtins.

use crate::fast_val::{FastVal, HeapAlloc};
use neko_ast::Span;
use neko_runtime::{io_read_file, io_write_file, Value, ValueRef};
use std::rc::Rc;

#[derive(Clone, Copy)]
pub enum IoFastPath {
    ReadFile = 0,
    WriteFile = 1,
}

impl IoFastPath {
    pub fn from_name(name: &str) -> Option<Self> {
        match name {
            "io_read_file" => Some(Self::ReadFile),
            "io_write_file" => Some(Self::WriteFile),
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
            IoFastPath::ReadFile if refs.len() == 1 => io_read_file(&refs, span).ok(),
            IoFastPath::WriteFile if refs.len() == 2 => io_write_file(&refs, span).ok(),
            _ => None,
        }?;
        Some(value_from_ref(&result, heap_mut))
    }
}

fn value_from_ref(result: &ValueRef, heap: &mut impl HeapAlloc) -> FastVal {
    match &*result.borrow() {
        Value::Int(v) => FastVal::Int(*v),
        Value::Nil => FastVal::Nil,
        Value::String(s) => FastVal::Heap(heap.push_heap(Value::String(s.clone()).ref_cell())),
        _ => FastVal::Heap(heap.push_heap(Rc::clone(result))),
    }
}
