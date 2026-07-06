//! Handle table for loaded RAG indexes + global embedder.

use super::common::*;
use crate::RuntimeError;
use neko_ast::Span;
use neko_errors::codes;
use neko_rag::{Embedder, RagIndex};
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

pub static EMBEDDER: Mutex<Option<Embedder>> = Mutex::new(None);

static INDEXES: OnceLock<Mutex<HashMap<u64, RagIndex>>> = OnceLock::new();
static NEXT_ID: OnceLock<Mutex<u64>> = OnceLock::new();

fn indexes() -> &'static Mutex<HashMap<u64, RagIndex>> {
    INDEXES.get_or_init(|| Mutex::new(HashMap::new()))
}

fn next_id() -> &'static Mutex<u64> {
    NEXT_ID.get_or_init(|| Mutex::new(1))
}

pub fn alloc_index(index: RagIndex) -> u64 {
    let mut id = next_id().lock().unwrap();
    let n = *id;
    *id = n + 1;
    drop(id);
    indexes().lock().unwrap().insert(n, index);
    n
}

pub fn free_index(id: u64) {
    indexes().lock().unwrap().remove(&id);
}

pub fn with_index<F, R>(id: u64, name: &str, span: Span, f: F) -> Result<R, RuntimeError>
where
    F: FnOnce(&RagIndex) -> Result<R, String>,
{
    let guard = indexes().lock().map_err(|e| {
        RuntimeError::at(span, codes::E1981_NRAG_ERROR, format!("{name}(): {e}"))
    })?;
    let idx = guard.get(&id).ok_or_else(|| {
        RuntimeError::at(
            span,
            codes::E1982_NRAG_INVALID_HANDLE,
            format!("{name}(): invalid index handle {id}"),
        )
    })?;
    f(idx).map_err(|msg| RuntimeError::at(span, codes::E1981_NRAG_ERROR, format!("{name}(): {msg}")))
}

pub fn with_embedder<F, R>(span: Span, f: F) -> Result<R, RuntimeError>
where
    F: FnOnce(&mut Embedder) -> Result<R, String>,
{
    let mut guard = EMBEDDER.lock().map_err(|e| {
        RuntimeError::at(span, codes::E1981_NRAG_ERROR, format!("nrag embedder lock: {e}"))
    })?;
    if guard.is_none() {
        let emb = Embedder::new().map_err(|e| {
            RuntimeError::at(span, codes::E1981_NRAG_ERROR, e.to_string())
        })?;
        *guard = Some(emb);
    }
    let emb = guard.as_mut().unwrap();
    f(emb).map_err(|msg| RuntimeError::at(span, codes::E1981_NRAG_ERROR, msg))
}

pub type IndexHandle = u64;
