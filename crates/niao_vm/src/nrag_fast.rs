//! VM fast paths for hot nrag builtins — direct stack reads, zero boxing on scalars.

use crate::fast_val::{FastVal, HeapAlloc};
use niao_ast::Span;
use niao_runtime::nrag::{
    count_handles, embed_handles, embedder_ready, init_embedder, search_text_handles, search_vec_handles, EMBED_DIM,
};
use niao_runtime::{Value, ValueRef};
use std::rc::Rc;

#[derive(Clone, Copy)]
pub enum NragFastPath {
    Count = 0,
    Embed = 1,
    SearchVec = 2,
    Search = 3,
    Ready = 4,
    Dim = 5,
    Init = 6,
}

impl NragFastPath {
    pub fn from_name(name: &str) -> Option<Self> {
        match name {
            "nrag_count" => Some(Self::Count),
            "nrag_embed" => Some(Self::Embed),
            "nrag_search_vec" => Some(Self::SearchVec),
            "nrag_search" => Some(Self::Search),
            "nrag_ready" => Some(Self::Ready),
            "nrag_dim" => Some(Self::Dim),
            "nrag_init" => Some(Self::Init),
            _ => None,
        }
    }

    /// Scalar fast path: mutates stack in place, no heap allocation.
    pub fn try_execute_stack(stack: &mut Vec<FastVal>, argc: usize, path: Self) -> bool {
        if stack.len() < argc {
            return false;
        }
        let base = stack.len() - argc;
        let span = Span::dummy();
        match path {
            Self::Count if argc == 1 => {
                let Some(id) = int_handle(stack[base]) else {
                    return false;
                };
                let Some(n) = count_handles(id, span).ok() else {
                    return false;
                };
                stack.truncate(base);
                stack.push(FastVal::Int(n));
                true
            }
            Self::Ready if argc == 0 => {
                stack.truncate(base);
                stack.push(FastVal::Bool(embedder_ready()));
                true
            }
            Self::Dim if argc == 0 => {
                stack.truncate(base);
                stack.push(FastVal::Int(EMBED_DIM as i64));
                true
            }
            Self::Init if argc == 0 => {
                if init_embedder(span).is_err() {
                    return false;
                }
                stack.truncate(base);
                stack.push(FastVal::Bool(true));
                true
            }
            _ => false,
        }
    }

    /// Heap fast path: reads args directly from FastVal stack slots.
    pub fn try_execute_heap(
        args: &[FastVal],
        heap: &[ValueRef],
        native_refs: &[ValueRef],
        path: Self,
        heap_mut: &mut impl HeapAlloc,
    ) -> Option<FastVal> {
        let span = Span::dummy();
        let result = match path {
            Self::Embed if args.len() == 1 => {
                let text = heap_string(args[0], heap, native_refs)?;
                embed_handles(text, span).ok()
            }
            Self::SearchVec if (2..=4).contains(&args.len()) => {
                let id = int_handle(args[0])?;
                let query = heap_float_array(args[1], heap, native_refs)?;
                let top_k = optional_usize(args, 2).unwrap_or(3);
                let threshold = optional_f32(args, 3).unwrap_or(0.0);
                search_vec_handles(id, query, top_k, threshold, span).ok()
            }
            Self::Search if (2..=4).contains(&args.len()) => {
                let id = int_handle(args[0])?;
                let query = heap_string(args[1], heap, native_refs)?;
                let top_k = optional_usize(args, 2).unwrap_or(3);
                let threshold = optional_f32(args, 3).unwrap_or(0.0);
                search_text_handles(id, query, top_k, threshold, span).ok()
            }
            _ => None,
        }?;
        Some(value_to_fast(&result, heap_mut))
    }
}

fn value_to_fast(result: &ValueRef, heap: &mut impl HeapAlloc) -> FastVal {
    match &*result.borrow() {
        Value::Int(v) => FastVal::Int(*v),
        Value::Float(v) => FastVal::Float(*v),
        Value::Bool(v) => FastVal::Bool(*v),
        Value::Nil => FastVal::Nil,
        Value::String(s) => FastVal::Heap(heap.push_heap(Value::String(s.clone()).ref_cell())),
        Value::FloatArray(v) => FastVal::Heap(heap.push_heap(Value::FloatArray(v.clone()).ref_cell())),
        _ => FastVal::Heap(heap.push_heap(Rc::clone(result))),
    }
}

#[inline(always)]
fn int_handle(v: FastVal) -> Option<u64> {
    match v {
        FastVal::Int(id) if id > 0 => Some(id as u64),
        _ => None,
    }
}

fn heap_string(v: FastVal, heap: &[ValueRef], native_refs: &[ValueRef]) -> Option<String> {
    match v {
        FastVal::Heap(idx) => match &*heap[idx as usize].borrow() {
            Value::String(s) => Some(s.clone()),
            _ => None,
        },
        FastVal::Native(i) => match &*native_refs[i as usize].borrow() {
            Value::String(s) => Some(s.clone()),
            _ => None,
        },
        _ => None,
    }
}

fn heap_float_array(v: FastVal, heap: &[ValueRef], native_refs: &[ValueRef]) -> Option<Vec<f32>> {
    let arr = match v {
        FastVal::Heap(idx) => &*heap[idx as usize].borrow(),
        FastVal::Native(i) => &*native_refs[i as usize].borrow(),
        _ => return None,
    };
    match arr {
        Value::FloatArray(v) => Some(v.iter().map(|&f| f as f32).collect()),
        _ => None,
    }
}

fn optional_usize(args: &[FastVal], idx: usize) -> Option<usize> {
    match args.get(idx)? {
        FastVal::Int(n) if *n > 0 => Some(*n as usize),
        _ => None,
    }
}

fn optional_f32(args: &[FastVal], idx: usize) -> Option<f32> {
    match args.get(idx)? {
        FastVal::Float(n) => Some(*n as f32),
        FastVal::Int(n) => Some(*n as f32),
        _ => None,
    }
}
