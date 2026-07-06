//! Native nrag standard library — vector RAG via neko_rag + fastembed.

mod common;
mod handles;

use crate::{error_from_runtime, NativeFn, NekoResult, RuntimeError, Value, ValueRef};
use common::*;
use handles::{with_embedder, with_index, EMBEDDER};
use neko_ast::Span;
use neko_errors::codes;
use neko_rag::{Embedder, RagIndex};
use std::collections::HashMap;
use std::path::Path;
use std::rc::Rc;

pub const MODULE_NAME: &str = "nrag";
pub const MODULE_PATHS: &[&str] = &["nrag", "std/nrag"];

fn nrag_init(args: &[ValueRef], span: Span) -> NekoResult<ValueRef> {
    arity(args, 0, "nrag_init", span)?;
    let mut guard = EMBEDDER.lock().map_err(|e| rag_err(span, e.to_string()))?;
    if guard.is_none() {
        let emb = Embedder::new().map_err(|e| rag_err(span, e.to_string()))?;
        *guard = Some(emb);
    }
    Ok(Value::Bool(true).ref_cell())
}

fn nrag_embed(args: &[ValueRef], span: Span) -> NekoResult<ValueRef> {
    arity(args, 1, "nrag_embed", span)?;
    let text = string_arg(args, 0, "nrag_embed", span)?;
    with_embedder(span, |emb| {
        let vec = emb.embed_one(&text).map_err(|e| e.to_string())?;
        Ok(Value::FloatArray(vec.into_iter().map(|f| f as f64).collect()).ref_cell())
    })
}

fn nrag_load(args: &[ValueRef], span: Span) -> NekoResult<ValueRef> {
    arity(args, 1, "nrag_load", span)?;
    let path = string_arg(args, 0, "nrag_load", span)?;
    let index = RagIndex::load(Path::new(&path)).map_err(|e| rag_err(span, e.to_string()))?;
    let id = handles::alloc_index(index);
    Ok(Value::Int(id as i64).ref_cell())
}

fn nrag_unload(args: &[ValueRef], span: Span) -> NekoResult<ValueRef> {
    arity(args, 1, "nrag_unload", span)?;
    let id = index_arg(args, 0, "nrag_unload", span)?;
    handles::free_index(id);
    Ok(Value::Bool(true).ref_cell())
}

fn nrag_count(args: &[ValueRef], span: Span) -> NekoResult<ValueRef> {
    arity(args, 1, "nrag_count", span)?;
    let id = index_arg(args, 0, "nrag_count", span)?;
    with_index(id, "nrag_count", span, |idx| Ok(idx.chunk_count() as i64))
        .map(|n| Value::Int(n).ref_cell())
        .or_else(|e| Ok(error_from_runtime(&e)))
}

fn nrag_search(args: &[ValueRef], span: Span) -> NekoResult<ValueRef> {
    arity_range(args, 2, 4, "nrag_search", span)?;
    let id = index_arg(args, 0, "nrag_search", span)?;
    let query = string_arg(args, 1, "nrag_search", span)?;
    let top_k = if args.len() >= 3 {
        int_arg(args, 2, "nrag_search", span)? as usize
    } else {
        3
    };
    let threshold = if args.len() >= 4 {
        float_arg(args, 3, "nrag_search", span)? as f32
    } else {
        0.0f32
    };

    let query_vec = with_embedder(span, |emb| emb.embed_one(&query).map_err(|e| e.to_string()))?;

    let hits = with_index(id, "nrag_search", span, |idx| Ok(idx.search(&query_vec, top_k, threshold)))?;

    let mut rows = Vec::new();
    for hit in hits {
        let row = with_index(id, "nrag_search", span, |idx| {
            let ch = &idx.chunks[hit.chunk_index];
            let mut m = HashMap::new();
            m.insert("chunk_id".into(), Value::String(ch.id.clone()).ref_cell());
            m.insert("content".into(), Value::String(ch.content.clone()).ref_cell());
            m.insert("source".into(), Value::String(ch.source.clone()).ref_cell());
            m.insert(
                "chapter".into(),
                ch.chapter
                    .as_ref()
                    .map(|s| Value::String(s.clone()).ref_cell())
                    .unwrap_or(Value::Nil.ref_cell()),
            );
            m.insert(
                "section".into(),
                ch.section
                    .as_ref()
                    .map(|s| Value::String(s.clone()).ref_cell())
                    .unwrap_or(Value::Nil.ref_cell()),
            );
            m.insert(
                "relevance_score".into(),
                Value::Float(hit.score as f64).ref_cell(),
            );
            Ok(Value::Object(m).ref_cell())
        })?;
        rows.push(row);
    }
    Ok(Value::Array(rows).ref_cell())
}

fn nrag_build(args: &[ValueRef], span: Span) -> NekoResult<ValueRef> {
    arity(args, 2, "nrag_build", span)?;
    let json_path = string_arg(args, 0, "nrag_build", span)?;
    let out_path = string_arg(args, 1, "nrag_build", span)?;
    let n = neko_rag::build_index_from_json(Path::new(&json_path), Path::new(&out_path))
        .map_err(|e| rag_err(span, e.to_string()))?;
    Ok(Value::Int(n as i64).ref_cell())
}

fn rag_err(span: Span, msg: String) -> RuntimeError {
    RuntimeError::at(span, codes::E1981_NRAG_ERROR, msg)
}

fn all_builtins() -> Vec<(&'static str, NativeFn)> {
    vec![
        ("nrag_init", Rc::new(nrag_init)),
        ("nrag_embed", Rc::new(nrag_embed)),
        ("nrag_load", Rc::new(nrag_load)),
        ("nrag_unload", Rc::new(nrag_unload)),
        ("nrag_count", Rc::new(nrag_count)),
        ("nrag_search", Rc::new(nrag_search)),
        ("nrag_build", Rc::new(nrag_build)),
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
    bind(&mut map, "init", Rc::new(nrag_init));
    bind(&mut map, "embed", Rc::new(nrag_embed));
    bind(&mut map, "load", Rc::new(nrag_load));
    bind(&mut map, "unload", Rc::new(nrag_unload));
    bind(&mut map, "count", Rc::new(nrag_count));
    bind(&mut map, "search", Rc::new(nrag_search));
    bind(&mut map, "build", Rc::new(nrag_build));
    let out: HashMap<String, ValueRef> = map
        .into_iter()
        .map(|(k, v)| (k, Value::NativeFunction(v).ref_cell()))
        .collect();
    Value::Object(out).ref_cell()
}
