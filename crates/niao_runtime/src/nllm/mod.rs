//! Native nllm standard library — GGUF inference via niao_llm.

mod common;
mod handles;

use crate::{NativeFn, NiaoResult, RuntimeError, Value, ValueRef};
use common::*;
use niao_ast::Span;
use niao_errors::codes;
use niao_llm::{BackendKind, ChatMessage, GenerateOptions, LoadOptions, LlmSession};
use std::collections::HashMap;
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
    if let Some(v) = map.get("n_ctx").and_then(|v| match &*v.borrow() {
        Value::Int(n) => Some(*n as u32),
        _ => None,
    }) {
        opts.n_ctx = Some(v);
    }
    if let Some(v) = map.get("auto").and_then(|v| match &*v.borrow() {
        Value::Bool(b) => Some(*b),
        _ => None,
    }) {
        opts.auto = v;
    }
    Ok(opts)
}

fn parse_gen_opts(map: Option<&HashMap<String, ValueRef>>, _span: Span) -> Result<GenerateOptions, RuntimeError> {
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
    if let Some(v) = map.get("repeat_penalty").and_then(|v| match &*v.borrow() {
        Value::Float(n) => Some(*n as f32),
        Value::Int(n) => Some(*n as f32),
        _ => None,
    }) {
        opts.repeat_penalty = v;
    }
    if let Some(v) = map.get("seed").and_then(|v| match &*v.borrow() {
        Value::Int(n) => Some(*n as u64),
        _ => None,
    }) {
        opts.seed = v;
    }
    if let Some(v) = map.get("top_p").and_then(|v| match &*v.borrow() {
        Value::Float(n) => Some(*n as f32),
        Value::Int(n) => Some(*n as f32),
        _ => None,
    }) {
        opts.top_p = v;
    }
    if let Some(v) = map.get("top_k").and_then(|v| match &*v.borrow() {
        Value::Int(n) => Some(*n as u32),
        _ => None,
    }) {
        opts.top_k = v;
    }
    Ok(opts)
}

/// VM fast path: single-turn completion.
pub fn complete_handles(
    id: u64,
    prompt: String,
    opts: GenerateOptions,
    span: Span,
) -> Result<String, RuntimeError> {
    let messages = vec![ChatMessage {
        role: "user".into(),
        content: prompt,
    }];
    handles::chat_session(id, messages, opts, span)
}

/// VM fast path: chat with pre-parsed messages.
pub fn chat_handles(
    id: u64,
    messages: Vec<ChatMessage>,
    opts: GenerateOptions,
    span: Span,
) -> Result<String, RuntimeError> {
    handles::chat_session(id, messages, opts, span)
}

/// VM fast path: count tokens without building a token array.
pub fn count_tokens_handles(id: u64, text: String, span: Span) -> Result<i64, RuntimeError> {
    handles::count_tokens_session(id, text, span)
}

/// VM fast path: backend name string.
pub fn backend_name_handles(id: u64, span: Span) -> Result<String, RuntimeError> {
    handles::backend_session(id, span)
}

/// VM fast path: reset session KV/context.
pub fn reset_handles(id: u64, span: Span) -> Result<(), RuntimeError> {
    handles::reset_session(id, span)
}

/// VM fast path: free session handle.
pub fn unload_handles(id: u64) {
    handles::free_session(id);
}

fn nllm_load(args: &[ValueRef], span: Span) -> NiaoResult<ValueRef> {
    arity_range(args, 1, 2, "nllm_load", span)?;
    let path = string_arg(args, 0, "nllm_load", span)?;
    let opts = parse_load_opts(args, span)?;
    let id = handles::alloc_session(std::path::PathBuf::from(path), opts)
        .map_err(|e| llm_err(span, e))?;
    Ok(Value::Int(id as i64).ref_cell())
}

fn nllm_unload(args: &[ValueRef], span: Span) -> NiaoResult<ValueRef> {
    arity(args, 1, "nllm_unload", span)?;
    let id = session_arg(args, 0, "nllm_unload", span)?;
    handles::free_session(id);
    Ok(Value::Bool(true).ref_cell())
}

fn nllm_complete(args: &[ValueRef], span: Span) -> NiaoResult<ValueRef> {
    arity_range(args, 2, 3, "nllm_complete", span)?;
    let id = session_arg(args, 0, "nllm_complete", span)?;
    let prompt = string_arg(args, 1, "nllm_complete", span)?;
    let gen_opts = if args.len() == 3 {
        let map = object_arg(args, 2, "nllm_complete", span)?;
        parse_gen_opts(Some(&map), span)?
    } else {
        GenerateOptions::default()
    };
    let text = complete_handles(id, prompt, gen_opts, span)?;
    Ok(Value::String(text).ref_cell())
}

fn nllm_chat(args: &[ValueRef], span: Span) -> NiaoResult<ValueRef> {
    arity_range(args, 2, 3, "nllm_chat", span)?;
    let id = session_arg(args, 0, "nllm_chat", span)?;
    let messages = parse_messages(&args[1], span)?;
    let gen_opts = if args.len() == 3 {
        let map = object_arg(args, 2, "nllm_chat", span)?;
        parse_gen_opts(Some(&map), span)?
    } else {
        GenerateOptions::default()
    };
    let text = chat_handles(id, messages, gen_opts, span)?;
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


fn nllm_chat_stream(args: &[ValueRef], span: Span) -> NiaoResult<ValueRef> {
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

    let deltas: Vec<ValueRef> = Vec::new();
    let text = handles::chat_stream_session(id, messages, gen_opts, sse_handle, span)?;

    let mut out = HashMap::new();
    out.insert("content".into(), Value::String(text).ref_cell());
    out.insert("deltas".into(), Value::Array(deltas).ref_cell());
    Ok(Value::Object(out).ref_cell())
}

fn nllm_reset(args: &[ValueRef], span: Span) -> NiaoResult<ValueRef> {
    arity(args, 1, "nllm_reset", span)?;
    let id = session_arg(args, 0, "nllm_reset", span)?;
    reset_handles(id, span)?;
    Ok(Value::Bool(true).ref_cell())
}

fn nllm_tokenize(args: &[ValueRef], span: Span) -> NiaoResult<ValueRef> {
    arity(args, 2, "nllm_tokenize", span)?;
    let id = session_arg(args, 0, "nllm_tokenize", span)?;
    let text = string_arg(args, 1, "nllm_tokenize", span)?;
    let n = count_tokens_handles(id, text, span)?;
    Ok(Value::Int(n).ref_cell())
}

fn nllm_count_tokens(args: &[ValueRef], span: Span) -> NiaoResult<ValueRef> {
    arity(args, 2, "nllm_count_tokens", span)?;
    let id = session_arg(args, 0, "nllm_count_tokens", span)?;
    let text = string_arg(args, 1, "nllm_count_tokens", span)?;
    let n = count_tokens_handles(id, text, span)?;
    Ok(Value::Int(n).ref_cell())
}

fn nllm_apply_template(args: &[ValueRef], span: Span) -> NiaoResult<ValueRef> {
    arity(args, 2, "nllm_apply_template", span)?;
    let _id = session_arg(args, 0, "nllm_apply_template", span)?;
    let messages = parse_messages(&args[1], span)?;
    let prompt = messages
        .iter()
        .map(|m| format!("{}: {}", m.role, m.content))
        .collect::<Vec<_>>()
        .join("\n");
    Ok(Value::String(prompt).ref_cell())
}

fn nllm_device_info(args: &[ValueRef], span: Span) -> NiaoResult<ValueRef> {
    arity(args, 0, "nllm_device_info", span)?;
    let info = LlmSession::probe_device();
    let mut m = HashMap::new();
    for (k, v) in info.to_map() {
        m.insert(k.into(), Value::String(v).ref_cell());
    }
    Ok(Value::Object(m).ref_cell())
}

fn nllm_list_backends(args: &[ValueRef], span: Span) -> NiaoResult<ValueRef> {
    arity(args, 0, "nllm_list_backends", span)?;
    let backends = LlmSession::list_backends();
    let arr: Vec<ValueRef> = backends
        .into_iter()
        .map(|b| Value::String(b.into()).ref_cell())
        .collect();
    Ok(Value::Array(arr).ref_cell())
}

fn nllm_backend(args: &[ValueRef], span: Span) -> NiaoResult<ValueRef> {
    arity(args, 1, "nllm_backend", span)?;
    let id = session_arg(args, 0, "nllm_backend", span)?;
    let name = backend_name_handles(id, span)?;
    Ok(Value::String(name).ref_cell())
}

pub fn session_ready(id: u64) -> bool {
    handles::session_ready(id)
}

fn nllm_ready(args: &[ValueRef], span: Span) -> NiaoResult<ValueRef> {
    arity(args, 1, "nllm_ready", span)?;
    let id = session_arg(args, 0, "nllm_ready", span)?;
    let ok = handles::session_ready(id);
    Ok(Value::Bool(ok).ref_cell())
}

fn llm_err(span: Span, msg: String) -> RuntimeError {
    RuntimeError::at(span, codes::E1986_NLLM_ERROR, msg)
}

fn all_builtins() -> Vec<(&'static str, NativeFn)> {
    vec![
        ("nllm_load", Rc::new(nllm_load)),
        ("nllm_unload", Rc::new(nllm_unload)),
        ("nllm_complete", Rc::new(nllm_complete)),
        ("nllm_chat", Rc::new(nllm_chat)),
        ("nllm_chat_stream", Rc::new(nllm_chat_stream)),
        ("nllm_reset", Rc::new(nllm_reset)),
        ("nllm_tokenize", Rc::new(nllm_tokenize)),
        ("nllm_count_tokens", Rc::new(nllm_count_tokens)),
        ("nllm_apply_template", Rc::new(nllm_apply_template)),
        ("nllm_device_info", Rc::new(nllm_device_info)),
        ("nllm_list_backends", Rc::new(nllm_list_backends)),
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
    bind(&mut map, "complete", Rc::new(nllm_complete));
    bind(&mut map, "chat", Rc::new(nllm_chat));
    bind(&mut map, "chat_stream", Rc::new(nllm_chat_stream));
    bind(&mut map, "reset", Rc::new(nllm_reset));
    bind(&mut map, "tokenize", Rc::new(nllm_tokenize));
    bind(&mut map, "count_tokens", Rc::new(nllm_count_tokens));
    bind(&mut map, "apply_template", Rc::new(nllm_apply_template));
    bind(&mut map, "device_info", Rc::new(nllm_device_info));
    bind(&mut map, "list_backends", Rc::new(nllm_list_backends));
    bind(&mut map, "backend", Rc::new(nllm_backend));
    bind(&mut map, "ready", Rc::new(nllm_ready));
    let out: HashMap<String, ValueRef> = map
        .into_iter()
        .map(|(k, v)| (k, Value::NativeFunction(v).ref_cell()))
        .collect();
    Value::Object(out).ref_cell()
}
