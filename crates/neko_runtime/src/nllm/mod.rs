//! Native nllm standard library — GGUF inference via neko_llm.

mod common;
mod handles;

use crate::{NativeFn, NekoResult, RuntimeError, Value, ValueRef};
use common::*;
use handles::{with_session_mut, SESSIONS};
use neko_ast::Span;
use neko_errors::codes;
use neko_llm::{BackendKind, ChatMessage, GenerateOptions, LlmError, LoadOptions, LlmSession};
use std::collections::HashMap;
use std::path::Path;
use std::rc::Rc;

pub const MODULE_NAME: &str = "nllm";
pub const MODULE_PATHS: &[&str] = &["nllm", "std/nllm"];

fn parse_messages(val: &ValueRef, span: Span) -> Result<Vec<ChatMessage>, RuntimeError> {
    match &*val.borrow() {
        Value::Array(items) => {
            let mut out = Vec::new();
            for item in items {
                let borrowed = item.borrow();
                let m = match &*borrowed {
                    Value::Object(map) => map,
                    other => {
                        return Err(type_err(
                            span,
                            format!("message must be object, got {}", other.type_name()),
                        ))
                    }
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
            Ok(out)
        }
        other => Err(type_err(
            span,
            format!("messages must be array, got {}", other.type_name()),
        )),
    }
}

fn parse_load_opts(args: &[ValueRef], span: Span) -> Result<LoadOptions, RuntimeError> {
    let mut opts = LoadOptions::default();
    if args.len() < 2 {
        return Ok(opts);
    }
    let map = object_arg(args, 1, "nllm_load", span)?;
    if let Some(v) = map.get("tokenizer_path").and_then(|v| match &*v.borrow() {
        Value::String(s) => Some(s.clone()),
        _ => None,
    }) {
        opts.tokenizer_path = Some(v);
    }
    if let Some(v) = map.get("cpu").and_then(|v| match &*v.borrow() {
        Value::Bool(b) => Some(*b),
        _ => None,
    }) {
        opts.cpu = v;
    }
    if let Some(v) = map.get("backend").and_then(|v| match &*v.borrow() {
        Value::String(s) => Some(BackendKind::parse(s)),
        _ => None,
    }) {
        opts.backend = v;
    }
    if let Some(v) = map.get("n_gpu_layers").and_then(|v| match &*v.borrow() {
        Value::Int(n) => Some(*n as u32),
        _ => None,
    }) {
        opts.n_gpu_layers = Some(v);
    }
    if let Some(v) = map.get("threads").and_then(|v| match &*v.borrow() {
        Value::Int(n) => Some(*n as u32),
        _ => None,
    }) {
        opts.threads = Some(v);
    }
    Ok(opts)
}

fn parse_gen_opts(map: Option<&HashMap<String, ValueRef>>, span: Span) -> Result<GenerateOptions, RuntimeError> {
    let mut opts = GenerateOptions::default();
    let Some(map) = map else {
        return Ok(opts);
    };
    if let Some(v) = map.get("max_tokens").and_then(|v| match &*v.borrow() {
        Value::Int(n) => Some(*n as u32),
        _ => None,
    }) {
        opts.max_tokens = v;
    }
    if let Some(v) = map.get("temperature").and_then(|v| match &*v.borrow() {
        Value::Float(n) => Some(*n as f32),
        Value::Int(n) => Some(*n as f32),
        _ => None,
    }) {
        opts.temperature = v;
    }
    Ok(opts)
}

fn nllm_load(args: &[ValueRef], span: Span) -> NekoResult<ValueRef> {
    arity_range(args, 1, 2, "nllm_load", span)?;
    let path = string_arg(args, 0, "nllm_load", span)?;
    let opts = parse_load_opts(args, span)?;
    let session = LlmSession::load(Path::new(&path), opts).map_err(|e| llm_err(span, e.to_string()))?;
    let id = handles::alloc_session(session);
    Ok(Value::Int(id as i64).ref_cell())
}

fn nllm_unload(args: &[ValueRef], span: Span) -> NekoResult<ValueRef> {
    arity(args, 1, "nllm_unload", span)?;
    let id = session_arg(args, 0, "nllm_unload", span)?;
    handles::free_session(id);
    Ok(Value::Bool(true).ref_cell())
}

fn nllm_chat(args: &[ValueRef], span: Span) -> NekoResult<ValueRef> {
    arity_range(args, 2, 3, "nllm_chat", span)?;
    let id = session_arg(args, 0, "nllm_chat", span)?;
    let messages = parse_messages(&args[1], span)?;
    let gen_opts = if args.len() == 3 {
        let map = object_arg(args, 2, "nllm_chat", span)?;
        parse_gen_opts(Some(&map), span)?
    } else {
        GenerateOptions::default()
    };
    let text = with_session_mut(id, "nllm_chat", span, |sess| {
        sess.chat(&messages, gen_opts).map_err(|e| e.to_string())
    })?;
    Ok(Value::String(text).ref_cell())
}

fn parse_sse_handle(val: &ValueRef, span: Span) -> Result<Option<u64>, RuntimeError> {
    match &*val.borrow() {
        Value::Nil => Ok(None),
        Value::Int(n) if *n > 0 => Ok(Some(*n as u64)),
        Value::Object(map) => Ok(map.get("stream_handle").and_then(|v| match &*v.borrow() {
            Value::Int(n) if *n > 0 => Some(*n as u64),
            _ => None,
        })),
        other => Err(type_err(
            span,
            format!("sse handle must be int or stream object, got {}", other.type_name()),
        )),
    }
}

fn sse_delta_line(delta: &str) -> String {
    format!(
        "data: {}\n\n",
        serde_json::json!({"type": "delta", "content": delta})
    )
}

fn nllm_chat_stream(args: &[ValueRef], span: Span) -> NekoResult<ValueRef> {
    arity_range(args, 2, 4, "nllm_chat_stream", span)?;
    let id = session_arg(args, 0, "nllm_chat_stream", span)?;
    let messages = parse_messages(&args[1], span)?;
    let (gen_opts, sse_handle) = match args.len() {
        2 => (GenerateOptions::default(), None),
        3 => {
            if let Value::Object(_) = &*args[2].borrow() {
                let map = object_arg(args, 2, "nllm_chat_stream", span)?;
                (parse_gen_opts(Some(&map), span)?, None)
            } else {
                (
                    GenerateOptions::default(),
                    parse_sse_handle(&args[2], span)?,
                )
            }
        }
        _ => {
            let map = object_arg(args, 2, "nllm_chat_stream", span)?;
            let gen_opts = parse_gen_opts(Some(&map), span)?;
            let sse = parse_sse_handle(&args[3], span)?;
            (gen_opts, sse)
        }
    };

    let mut deltas: Vec<ValueRef> = Vec::new();
    let text = with_session_mut(id, "nllm_chat_stream", span, |sess| {
        sess.chat_stream(&messages, gen_opts, |delta| {
            if let Some(handle) = sse_handle {
                crate::ahiru::stream::sse_write(handle, &sse_delta_line(delta))
                    .map_err(LlmError::Msg)?;
            }
            deltas.push(Value::String(delta.to_string()).ref_cell());
            Ok(())
        })
        .map_err(|e| e.to_string())
    })?;

    let mut out = HashMap::new();
    out.insert("content".into(), Value::String(text).ref_cell());
    out.insert("deltas".into(), Value::Array(deltas).ref_cell());
    Ok(Value::Object(out).ref_cell())
}

fn nllm_backend(args: &[ValueRef], span: Span) -> NekoResult<ValueRef> {
    arity(args, 1, "nllm_backend", span)?;
    let id = session_arg(args, 0, "nllm_backend", span)?;
    let name = with_session_mut(id, "nllm_backend", span, |sess| {
        Ok(sess.backend_name().to_string())
    })?;
    Ok(Value::String(name).ref_cell())
}

fn nllm_ready(args: &[ValueRef], span: Span) -> NekoResult<ValueRef> {
    arity(args, 1, "nllm_ready", span)?;
    let id = session_arg(args, 0, "nllm_ready", span)?;
    let ok = SESSIONS
        .get()
        .and_then(|m| m.lock().ok())
        .map(|m| m.contains_key(&id))
        .unwrap_or(false);
    Ok(Value::Bool(ok).ref_cell())
}

fn llm_err(span: Span, msg: String) -> RuntimeError {
    RuntimeError::at(span, codes::E1986_NLLM_ERROR, msg)
}

fn all_builtins() -> Vec<(&'static str, NativeFn)> {
    vec![
        ("nllm_load", Rc::new(nllm_load)),
        ("nllm_unload", Rc::new(nllm_unload)),
        ("nllm_chat", Rc::new(nllm_chat)),
        ("nllm_chat_stream", Rc::new(nllm_chat_stream)),
        ("nllm_backend", Rc::new(nllm_backend)),
        ("nllm_ready", Rc::new(nllm_ready)),
    ]
}

pub fn builtins() -> Vec<(&'static str, NativeFn)> {
    all_builtins()
}

fn bind(map: &mut HashMap<String, NativeFn>, name: &str, f: NativeFn) {
    map.insert(name.to_string(), f);
}

pub fn namespace() -> ValueRef {
    let mut map = HashMap::new();
    bind(&mut map, "load", Rc::new(nllm_load));
    bind(&mut map, "unload", Rc::new(nllm_unload));
    bind(&mut map, "chat", Rc::new(nllm_chat));
    bind(&mut map, "chat_stream", Rc::new(nllm_chat_stream));
    bind(&mut map, "backend", Rc::new(nllm_backend));
    bind(&mut map, "ready", Rc::new(nllm_ready));
    let out: HashMap<String, ValueRef> = map
        .into_iter()
        .map(|(k, v)| (k, Value::NativeFunction(v).ref_cell()))
        .collect();
    Value::Object(out).ref_cell()
}
