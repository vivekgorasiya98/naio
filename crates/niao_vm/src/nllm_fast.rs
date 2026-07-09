//! VM fast paths for hot nllm builtins — direct stack reads, zero boxing on scalars.

use crate::fast_val::{FastVal, HeapAlloc};
use niao_ast::Span;
use niao_llm::{ChatMessage, GenerateOptions};
use niao_runtime::nllm::{
    backend_name_handles, chat_handles, complete_handles, count_tokens_handles, reset_handles, session_ready,
    unload_handles,
};
use niao_runtime::{Value, ValueRef};
use std::collections::HashMap;
use std::rc::Rc;

#[derive(Clone, Copy)]
pub enum NllmFastPath {
    Complete = 0,
    Chat = 1,
    Ready = 2,
    CountTokens = 3,
    Backend = 4,
    Reset = 5,
    Unload = 6,
}

impl NllmFastPath {
    pub fn from_name(name: &str) -> Option<Self> {
        match name {
            "nllm_complete" => Some(Self::Complete),
            "nllm_chat" => Some(Self::Chat),
            "nllm_ready" => Some(Self::Ready),
            "nllm_count_tokens" => Some(Self::CountTokens),
            "nllm_backend" => Some(Self::Backend),
            "nllm_reset" => Some(Self::Reset),
            "nllm_unload" => Some(Self::Unload),
            _ => None,
        }
    }

    /// Scalar fast path: mutates stack in place, no heap allocation for results.
    pub fn try_execute_stack(
        stack: &mut Vec<FastVal>,
        heap: &[ValueRef],
        native_refs: &[ValueRef],
        argc: usize,
        path: Self,
    ) -> bool {
        if stack.len() < argc {
            return false;
        }
        let base = stack.len() - argc;
        let span = Span::dummy();
        match path {
            Self::Ready if argc == 1 => {
                let Some(id) = int_handle(stack[base]) else {
                    return false;
                };
                stack.truncate(base);
                stack.push(FastVal::Bool(session_ready(id)));
                true
            }
            Self::CountTokens if argc == 2 => {
                let Some(id) = int_handle(stack[base]) else {
                    return false;
                };
                let Some(text) = read_string(stack[base + 1], heap, native_refs) else {
                    return false;
                };
                let Some(n) = count_tokens_handles(id, text, span).ok() else {
                    return false;
                };
                stack.truncate(base);
                stack.push(FastVal::Int(n));
                true
            }
            Self::Reset if argc == 1 => {
                let Some(id) = int_handle(stack[base]) else {
                    return false;
                };
                if reset_handles(id, span).is_err() {
                    return false;
                }
                stack.truncate(base);
                stack.push(FastVal::Bool(true));
                true
            }
            Self::Unload if argc == 1 => {
                let Some(id) = int_handle(stack[base]) else {
                    return false;
                };
                unload_handles(id);
                stack.truncate(base);
                stack.push(FastVal::Bool(true));
                true
            }
            _ => false,
        }
    }

    /// Heap fast path with direct FastVal arg reads (no intermediate ValueRef vec).
    pub fn try_execute_heap(
        args: &[FastVal],
        heap: &[ValueRef],
        native_refs: &[ValueRef],
        path: Self,
        heap_mut: &mut impl HeapAlloc,
    ) -> Option<FastVal> {
        let span = Span::dummy();
        let result = match path {
            Self::Complete if (2..=3).contains(&args.len()) => {
                let id = int_handle(args[0])?;
                let prompt = read_string(args[1], heap, native_refs)?;
                let opts = optional_gen_opts(args, 2, heap, native_refs).unwrap_or_default();
                let text = complete_handles(id, prompt, opts, span).ok()?;
                Value::String(text).ref_cell()
            }
            Self::Chat if (2..=3).contains(&args.len()) => {
                let id = int_handle(args[0])?;
                let messages = read_messages(args[1], heap, native_refs)?;
                let opts = optional_gen_opts(args, 2, heap, native_refs).unwrap_or_default();
                let text = chat_handles(id, messages, opts, span).ok()?;
                Value::String(text).ref_cell()
            }
            Self::Backend if args.len() == 1 => {
                let id = int_handle(args[0])?;
                let name = backend_name_handles(id, span).ok()?;
                Value::String(name).ref_cell()
            }
            _ => return None,
        };
        Some(value_to_fast(&result, heap_mut))
    }
}

fn value_to_fast(result: &ValueRef, heap: &mut impl HeapAlloc) -> FastVal {
    match &*result.borrow() {
        Value::Int(v) => FastVal::Int(*v),
        Value::Bool(v) => FastVal::Bool(*v),
        Value::Nil => FastVal::Nil,
        Value::String(s) => FastVal::Heap(heap.push_heap(Value::String(s.clone()).ref_cell())),
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

fn read_string(v: FastVal, heap: &[ValueRef], native_refs: &[ValueRef]) -> Option<String> {
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

fn read_messages(v: FastVal, heap: &[ValueRef], native_refs: &[ValueRef]) -> Option<Vec<ChatMessage>> {
    let items = match v {
        FastVal::Heap(idx) => match &*heap[idx as usize].borrow() {
            Value::Array(items) => items.clone(),
            _ => return None,
        },
        FastVal::Native(i) => match &*native_refs[i as usize].borrow() {
            Value::Array(items) => items.clone(),
            _ => return None,
        },
        _ => return None,
    };
    let mut out = Vec::with_capacity(items.len());
    for item in items {
        let borrowed = item.borrow();
        let m = match &*borrowed {
            Value::Object(map) => map,
            _ => return None,
        };
        let role = m
            .get("role")
            .and_then(|v| match &*v.borrow() {
                Value::String(s) => Some(s.clone()),
                _ => None,
            })
            .unwrap_or_else(|| "user".into());
        let content = m
            .get("content")
            .and_then(|v| match &*v.borrow() {
                Value::String(s) => Some(s.clone()),
                _ => None,
            })
            .unwrap_or_default();
        out.push(ChatMessage { role, content });
    }
    Some(out)
}

fn optional_gen_opts(
    args: &[FastVal],
    idx: usize,
    heap: &[ValueRef],
    native_refs: &[ValueRef],
) -> Option<GenerateOptions> {
    let val = args.get(idx)?;
    let map = match val {
        FastVal::Heap(i) => match &*heap[(*i) as usize].borrow() {
            Value::Object(m) => m.clone(),
            _ => return None,
        },
        FastVal::Native(i) => match &*native_refs[(*i) as usize].borrow() {
            Value::Object(m) => m.clone(),
            _ => return None,
        },
        _ => return None,
    };
    Some(gen_opts_from_map(&map))
}

fn gen_opts_from_map(map: &HashMap<String, ValueRef>) -> GenerateOptions {
    let mut opts = GenerateOptions::default();
    if let Some(n) = map.get("max_tokens").and_then(int_field) {
        opts.max_tokens = n as u32;
    }
    if let Some(n) = map.get("temperature").and_then(float_field) {
        opts.temperature = n as f32;
    }
    if let Some(n) = map.get("repeat_penalty").and_then(float_field) {
        opts.repeat_penalty = n as f32;
    }
    if let Some(n) = map.get("seed").and_then(int_field) {
        opts.seed = n as u64;
    }
    if let Some(n) = map.get("top_p").and_then(float_field) {
        opts.top_p = n as f32;
    }
    if let Some(n) = map.get("top_k").and_then(int_field) {
        opts.top_k = n as u32;
    }
    opts
}

fn int_field(v: &ValueRef) -> Option<i64> {
    match &*v.borrow() {
        Value::Int(n) => Some(*n),
        _ => None,
    }
}

fn float_field(v: &ValueRef) -> Option<f64> {
    match &*v.borrow() {
        Value::Float(n) => Some(*n),
        Value::Int(n) => Some(*n as f64),
        _ => None,
    }
}
