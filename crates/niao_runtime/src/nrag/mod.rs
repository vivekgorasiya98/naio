//! Native nrag standard library — vector RAG via niao_rag + fastembed.

mod common;
mod handles;

use crate::{error_from_runtime, NativeFn, NiaoResult, RuntimeError, Value, ValueRef};
use common::*;
use handles::{with_embedder, with_index, EMBEDDER};
use niao_ast::Span;
use niao_errors::codes;
use niao_rag::{Chunk, Embedder, RagIndex, DEFAULT_DIM, MODEL_NAME};
use std::collections::HashMap;
use std::path::Path;
use std::rc::Rc;

pub const MODULE_NAME: &str = "nrag";
pub const MODULE_PATHS: &[&str] = &["nrag", "std/nrag"];
pub const EMBED_DIM: usize = DEFAULT_DIM;

fn hit_to_object(ch: &Chunk, score: f32) -> ValueRef {
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
        Value::Float(score as f64).ref_cell(),
    );
    Value::Object(m).ref_cell()
}

fn float_array_arg(args: &[ValueRef], idx: usize, name: &str, span: Span) -> Result<Vec<f32>, RuntimeError> {
    match &*args[idx].borrow() {
        Value::FloatArray(v) => Ok(v.iter().map(|&f| f as f32).collect()),
        other => Err(type_err(
            span,
            format!(
                "{name}() expects float[] as argument {}, got {}",
                idx + 1,
                other.type_name()
            ),
        )),
    }
}

fn parse_search_params(args: &[ValueRef], name: &str, span: Span) -> Result<(usize, f32), RuntimeError> {
    let top_k = if args.len() >= 3 {
        int_arg(args, 2, name, span)? as usize
    } else {
        3
    };
    let threshold = if args.len() >= 4 {
        float_arg(args, 3, name, span)? as f32
    } else {
        0.0f32
    };
    Ok((top_k, threshold))
}

/// VM fast path: count chunks for index handle.
pub fn count_handles(id: u64, span: Span) -> Result<i64, RuntimeError> {
    with_index(id, "nrag_count", span, |idx| Ok(idx.chunk_count() as i64))
}

/// VM fast path: search with text query (embed + search in one path).
pub fn search_text_handles(
    id: u64,
    query: String,
    top_k: usize,
    threshold: f32,
    span: Span,
) -> Result<ValueRef, RuntimeError> {
    if embedder_device_label() == "npu" {
        return search_text_npu_batch(id, query, top_k, threshold, span);
    }
    let query_vec = with_embedder(span, |emb| emb.embed_one(&query).map_err(|e| e.to_string()))?;
    search_vec_handles(id, query_vec, top_k, threshold, span)
}

fn npu_query_variants(query: &str) -> Vec<String> {
    let batch = std::env::var("NIAO_RAG_NPU_QUERIES")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(8)
        .clamp(2, 16);
    let mut out = Vec::with_capacity(batch);
    out.push(query.to_string());
    let extras = [
        format!("{query} ayurveda"),
        format!("{query} BAMS exam"),
        format!("{query} dosha gunas"),
        format!("{query} chikitsa treatment"),
        format!("ayurveda {query}"),
        format!("{query} lakshana symptoms"),
        format!("{query} dravya karma"),
        format!("classical ayurveda {query}"),
        format!("{query} ashtanga hridaya"),
        format!("{query} charaka sutra"),
        format!("vata pitta kapha {query}"),
        format!("{query} prakriti vikriti"),
        format!("{query} dhatu srotas"),
        format!("{query} panchakarma"),
        format!("{query} agni ama ojas"),
    ];
    for e in extras {
        if out.len() >= batch {
            break;
        }
        if !out.iter().any(|s| s == &e) {
            out.push(e);
        }
    }
    out
}

fn search_text_npu_batch(
    id: u64,
    query: String,
    top_k: usize,
    threshold: f32,
    span: Span,
) -> Result<ValueRef, RuntimeError> {
    let variants = npu_query_variants(&query);
    let vectors = with_embedder(span, |emb| {
        let texts: Vec<String> = variants.iter().cloned().collect();
        emb.embed_batch(&texts).map_err(|e| e.to_string())
    })?;
    let mut merged: Vec<(usize, f32)> = Vec::new();
    for vec in vectors {
        let hits = with_index(id, "nrag_search_vec", span, |idx| {
            Ok(idx.search(&vec, top_k.saturating_mul(2), threshold))
        })?;
        for hit in hits {
            if let Some(slot) = merged.iter_mut().find(|(i, _)| *i == hit.chunk_index) {
                if hit.score > slot.1 {
                    slot.1 = hit.score;
                }
            } else {
                merged.push((hit.chunk_index, hit.score));
            }
        }
    }
    merged.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    merged.truncate(top_k);
    with_index(id, "nrag_search_vec", span, |idx| {
        let rows: Vec<ValueRef> = merged
            .iter()
            .filter_map(|(ci, score)| {
                idx.get_chunk(*ci)
                    .map(|ch| hit_to_object(ch, *score))
            })
            .collect();
        Ok(Value::Array(rows).ref_cell())
    })
}

/// VM fast path: whether embedder is initialized.
pub fn embedder_ready() -> bool {
    EMBEDDER
        .lock()
        .map(|g| g.is_some())
        .unwrap_or(false)
}

pub fn embedder_device_label() -> &'static str {
    EMBEDDER
        .lock()
        .ok()
        .and_then(|g| g.as_ref().map(|e| e.device()))
        .unwrap_or("cpu")
}

/// VM fast path: initialize embedder.
pub fn init_embedder(span: Span) -> Result<(), RuntimeError> {
    let mut guard = EMBEDDER.lock().map_err(|e| rag_err(span, e.to_string()))?;
    if guard.is_none() {
        let emb = Embedder::new().map_err(|e| rag_err(span, e.to_string()))?;
        *guard = Some(emb);
    }
    Ok(())
}

/// VM fast path: embed single string.
pub fn embed_handles(text: String, span: Span) -> Result<ValueRef, RuntimeError> {
    with_embedder(span, |emb| {
        let vec = emb.embed_one(&text).map_err(|e| e.to_string())?;
        Ok(Value::FloatArray(vec.into_iter().map(|f| f as f64).collect()).ref_cell())
    })
}

/// VM fast path: search with precomputed query vector.
pub fn search_vec_handles(
    id: u64,
    query: Vec<f32>,
    top_k: usize,
    threshold: f32,
    span: Span,
) -> Result<ValueRef, RuntimeError> {
    with_index(id, "nrag_search_vec", span, |idx| {
        let hits = idx.search(&query, top_k, threshold);
        let rows: Vec<ValueRef> = hits
            .iter()
            .filter_map(|hit| idx.get_chunk(hit.chunk_index).map(|ch| hit_to_object(ch, hit.score)))
            .collect();
        Ok(Value::Array(rows).ref_cell())
    })
}

fn nrag_init(args: &[ValueRef], span: Span) -> NiaoResult<ValueRef> {
    arity(args, 0, "nrag_init", span)?;
    let mut guard = EMBEDDER.lock().map_err(|e| rag_err(span, e.to_string()))?;
    if guard.is_none() {
        let emb = Embedder::new().map_err(|e| rag_err(span, e.to_string()))?;
        *guard = Some(emb);
    }
    Ok(Value::Bool(true).ref_cell())
}

fn nrag_ready(args: &[ValueRef], span: Span) -> NiaoResult<ValueRef> {
    arity(args, 0, "nrag_ready", span)?;
    let ok = EMBEDDER
        .lock()
        .map(|g| g.is_some())
        .unwrap_or(false);
    Ok(Value::Bool(ok).ref_cell())
}

fn nrag_dim(args: &[ValueRef], span: Span) -> NiaoResult<ValueRef> {
    arity(args, 0, "nrag_dim", span)?;
    Ok(Value::Int(DEFAULT_DIM as i64).ref_cell())
}

fn nrag_model(args: &[ValueRef], span: Span) -> NiaoResult<ValueRef> {
    arity(args, 0, "nrag_model", span)?;
    Ok(Value::String(MODEL_NAME.into()).ref_cell())
}

fn nrag_device(args: &[ValueRef], span: Span) -> NiaoResult<ValueRef> {
    arity(args, 0, "nrag_device", span)?;
    Ok(Value::String(embedder_device_label().into()).ref_cell())
}

fn nrag_embed(args: &[ValueRef], span: Span) -> NiaoResult<ValueRef> {
    arity(args, 1, "nrag_embed", span)?;
    let text = string_arg(args, 0, "nrag_embed", span)?;
    embed_handles(text, span).or_else(|e| Ok(error_from_runtime(&e)))
}

fn nrag_embed_batch(args: &[ValueRef], span: Span) -> NiaoResult<ValueRef> {
    arity(args, 1, "nrag_embed_batch", span)?;
    let texts = match &*args[0].borrow() {
        Value::Array(items) => {
            let mut out = Vec::with_capacity(items.len());
            for item in items {
                match &*item.borrow() {
                    Value::String(s) => out.push(s.clone()),
                    other => {
                        return Err(type_err(
                            span,
                            format!("nrag_embed_batch() expects string[], got {}", other.type_name()),
                        ))
                    }
                }
            }
            out
        }
        other => {
            return Err(type_err(
                span,
                format!("nrag_embed_batch() expects array, got {}", other.type_name()),
            ))
        }
    };
    with_embedder(span, |emb| {
        let vecs = emb.embed_batch(&texts).map_err(|e| e.to_string())?;
        let rows: Vec<ValueRef> = vecs
            .into_iter()
            .map(|v| Value::FloatArray(v.into_iter().map(|f| f as f64).collect()).ref_cell())
            .collect();
        Ok(Value::Array(rows).ref_cell())
    })
    .or_else(|e| Ok(error_from_runtime(&e)))
}

fn nrag_load(args: &[ValueRef], span: Span) -> NiaoResult<ValueRef> {
    arity(args, 1, "nrag_load", span)?;
    let path = string_arg(args, 0, "nrag_load", span)?;
    let index = RagIndex::load(Path::new(&path)).map_err(|e| rag_err(span, e.to_string()))?;
    let id = handles::alloc_index(index);
    Ok(Value::Int(id as i64).ref_cell())
}

fn nrag_unload(args: &[ValueRef], span: Span) -> NiaoResult<ValueRef> {
    arity(args, 1, "nrag_unload", span)?;
    let id = index_arg(args, 0, "nrag_unload", span)?;
    handles::free_index(id);
    Ok(Value::Bool(true).ref_cell())
}

fn nrag_count(args: &[ValueRef], span: Span) -> NiaoResult<ValueRef> {
    arity(args, 1, "nrag_count", span)?;
    let id = index_arg(args, 0, "nrag_count", span)?;
    count_handles(id, span)
        .map(|n| Value::Int(n).ref_cell())
        .or_else(|e| Ok(error_from_runtime(&e)))
}

fn nrag_search(args: &[ValueRef], span: Span) -> NiaoResult<ValueRef> {
    arity_range(args, 2, 4, "nrag_search", span)?;
    let id = index_arg(args, 0, "nrag_search", span)?;
    let query = string_arg(args, 1, "nrag_search", span)?;
    let (top_k, threshold) = parse_search_params(args, "nrag_search", span)?;
    search_text_handles(id, query, top_k, threshold, span).or_else(|e| Ok(error_from_runtime(&e)))
}

fn nrag_search_vec(args: &[ValueRef], span: Span) -> NiaoResult<ValueRef> {
    arity_range(args, 2, 4, "nrag_search_vec", span)?;
    let id = index_arg(args, 0, "nrag_search_vec", span)?;
    let query = float_array_arg(args, 1, "nrag_search_vec", span)?;
    let (top_k, threshold) = parse_search_params(args, "nrag_search_vec", span)?;
    search_vec_handles(id, query, top_k, threshold, span).or_else(|e| Ok(error_from_runtime(&e)))
}

fn nrag_get_chunk(args: &[ValueRef], span: Span) -> NiaoResult<ValueRef> {
    arity(args, 2, "nrag_get_chunk", span)?;
    let id = index_arg(args, 0, "nrag_get_chunk", span)?;
    let chunk_id = string_arg(args, 1, "nrag_get_chunk", span)?;
    with_index(id, "nrag_get_chunk", span, |idx| {
        let ch = idx
            .get_chunk_by_id(&chunk_id)
            .ok_or_else(|| format!("chunk not found: {chunk_id}"))?;
        Ok(hit_to_object(ch, 0.0))
    })
    .or_else(|e| Ok(error_from_runtime(&e)))
}

fn nrag_save(args: &[ValueRef], span: Span) -> NiaoResult<ValueRef> {
    arity(args, 2, "nrag_save", span)?;
    let id = index_arg(args, 0, "nrag_save", span)?;
    let path = string_arg(args, 1, "nrag_save", span)?;
    with_index(id, "nrag_save", span, |idx| {
        idx.save(Path::new(&path)).map_err(|e| e.to_string())?;
        Ok(true)
    })
    .map(|_| Value::Bool(true).ref_cell())
    .or_else(|e| Ok(error_from_runtime(&e)))
}

fn nrag_build(args: &[ValueRef], span: Span) -> NiaoResult<ValueRef> {
    arity(args, 2, "nrag_build", span)?;
    let json_path = string_arg(args, 0, "nrag_build", span)?;
    let out_path = string_arg(args, 1, "nrag_build", span)?;
    let n = niao_rag::build_index_from_json(Path::new(&json_path), Path::new(&out_path))
        .map_err(|e| rag_err(span, e.to_string()))?;
    Ok(Value::Int(n as i64).ref_cell())
}

fn nrag_build_chunks(args: &[ValueRef], span: Span) -> NiaoResult<ValueRef> {
    arity(args, 2, "nrag_build_chunks", span)?;
    let out_path = string_arg(args, 1, "nrag_build_chunks", span)?;
    let chunks = match &*args[0].borrow() {
        Value::Array(items) => {
            let mut out = Vec::with_capacity(items.len());
            for item in items {
                let borrowed = item.borrow();
                let m = match &*borrowed {
                    Value::Object(map) => map,
                    other => {
                        return Err(type_err(
                            span,
                            format!("nrag_build_chunks() expects object[], got {}", other.type_name()),
                        ))
                    }
                };
                let id = m
                    .get("id")
                    .and_then(|v| match &*v.borrow() {
                        Value::String(s) => Some(s.clone()),
                        _ => None,
                    })
                    .unwrap_or_default();
                let content = m
                    .get("content")
                    .and_then(|v| match &*v.borrow() {
                        Value::String(s) => Some(s.clone()),
                        _ => None,
                    })
                    .unwrap_or_default();
                let source = m
                    .get("source")
                    .and_then(|v| match &*v.borrow() {
                        Value::String(s) => Some(s.clone()),
                        _ => None,
                    })
                    .unwrap_or_default();
                let chapter = m.get("chapter").and_then(|v| match &*v.borrow() {
                    Value::String(s) => Some(s.clone()),
                    _ => None,
                });
                let section = m.get("section").and_then(|v| match &*v.borrow() {
                    Value::String(s) => Some(s.clone()),
                    _ => None,
                });
                out.push(Chunk {
                    id,
                    content,
                    source,
                    chapter,
                    section,
                });
            }
            out
        }
        other => {
            return Err(type_err(
                span,
                format!("nrag_build_chunks() expects array, got {}", other.type_name()),
            ))
        }
    };
    let n = niao_rag::build_index_from_chunks(&chunks, Path::new(&out_path))
        .map_err(|e| rag_err(span, e.to_string()))?;
    Ok(Value::Int(n as i64).ref_cell())
}

fn rag_err(span: Span, msg: String) -> RuntimeError {
    RuntimeError::at(span, codes::E1981_NRAG_ERROR, msg)
}

fn all_builtins() -> Vec<(&'static str, NativeFn)> {
    vec![
        ("nrag_init", Rc::new(nrag_init)),
        ("nrag_ready", Rc::new(nrag_ready)),
        ("nrag_dim", Rc::new(nrag_dim)),
        ("nrag_model", Rc::new(nrag_model)),
        ("nrag_device", Rc::new(nrag_device)),
        ("nrag_embed", Rc::new(nrag_embed)),
        ("nrag_embed_batch", Rc::new(nrag_embed_batch)),
        ("nrag_load", Rc::new(nrag_load)),
        ("nrag_unload", Rc::new(nrag_unload)),
        ("nrag_count", Rc::new(nrag_count)),
        ("nrag_search", Rc::new(nrag_search)),
        ("nrag_search_vec", Rc::new(nrag_search_vec)),
        ("nrag_get_chunk", Rc::new(nrag_get_chunk)),
        ("nrag_save", Rc::new(nrag_save)),
        ("nrag_build", Rc::new(nrag_build)),
        ("nrag_build_chunks", Rc::new(nrag_build_chunks)),
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
    bind(&mut map, "ready", Rc::new(nrag_ready));
    bind(&mut map, "dim", Rc::new(nrag_dim));
    bind(&mut map, "model", Rc::new(nrag_model));
    bind(&mut map, "device", Rc::new(nrag_device));
    bind(&mut map, "embed", Rc::new(nrag_embed));
    bind(&mut map, "embed_batch", Rc::new(nrag_embed_batch));
    bind(&mut map, "load", Rc::new(nrag_load));
    bind(&mut map, "unload", Rc::new(nrag_unload));
    bind(&mut map, "count", Rc::new(nrag_count));
    bind(&mut map, "search", Rc::new(nrag_search));
    bind(&mut map, "search_vec", Rc::new(nrag_search_vec));
    bind(&mut map, "get_chunk", Rc::new(nrag_get_chunk));
    bind(&mut map, "save", Rc::new(nrag_save));
    bind(&mut map, "build", Rc::new(nrag_build));
    bind(&mut map, "build_chunks", Rc::new(nrag_build_chunks));
    let out: HashMap<String, ValueRef> = map
        .into_iter()
        .map(|(k, v)| (k, Value::NativeFunction(v).ref_cell()))
        .collect();
    Value::Object(out).ref_cell()
}
