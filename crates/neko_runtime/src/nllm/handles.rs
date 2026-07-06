//! LLM session handles.

use super::common::*;
use crate::RuntimeError;
use neko_ast::Span;
use neko_errors::codes;
use neko_llm::LlmSession;
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

pub static SESSIONS: OnceLock<Mutex<HashMap<u64, LlmSession>>> = OnceLock::new();
static NEXT_ID: OnceLock<Mutex<u64>> = OnceLock::new();

fn sessions() -> &'static Mutex<HashMap<u64, LlmSession>> {
    SESSIONS.get_or_init(|| Mutex::new(HashMap::new()))
}

fn next_id() -> &'static Mutex<u64> {
    NEXT_ID.get_or_init(|| Mutex::new(1))
}

pub fn alloc_session(session: LlmSession) -> u64 {
    let mut id = next_id().lock().unwrap();
    let n = *id;
    *id = n + 1;
    drop(id);
    sessions().lock().unwrap().insert(n, session);
    n
}

pub fn free_session(id: u64) {
    sessions().lock().unwrap().remove(&id);
}

pub fn with_session_mut<F, R>(id: u64, name: &str, span: Span, f: F) -> Result<R, RuntimeError>
where
    F: FnOnce(&mut LlmSession) -> Result<R, String>,
{
    let mut guard = sessions().lock().map_err(|e| {
        RuntimeError::at(span, codes::E1986_NLLM_ERROR, format!("{name}(): {e}"))
    })?;
    let sess = guard.get_mut(&id).ok_or_else(|| {
        RuntimeError::at(
            span,
            codes::E1987_NLLM_INVALID_HANDLE,
            format!("{name}(): invalid session handle {id}"),
        )
    })?;
    f(sess).map_err(|msg| RuntimeError::at(span, codes::E1986_NLLM_ERROR, format!("{name}(): {msg}")))
}
