//! LLM session handles — single inference thread owns all llama.cpp sessions.

use crate::RuntimeError;
use niao_ast::Span;
use niao_errors::codes;
use niao_llm::{ChatMessage, GenerateOptions, LoadOptions, LlmSession};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::mpsc::{self, Sender};
use std::sync::{Mutex, OnceLock};
use std::thread::{self, JoinHandle};

enum InferenceRequest {
    Load {
        path: PathBuf,
        opts: LoadOptions,
        reply: Sender<Result<u64, String>>,
    },
    Unload {
        id: u64,
        reply: Sender<()>,
    },
    Ready {
        id: u64,
        reply: Sender<bool>,
    },
    Backend {
        id: u64,
        reply: Sender<Result<String, String>>,
    },
    Chat {
        id: u64,
        messages: Vec<ChatMessage>,
        opts: GenerateOptions,
        reply: Sender<Result<String, String>>,
    },
    ChatStream {
        id: u64,
        messages: Vec<ChatMessage>,
        opts: GenerateOptions,
        on_delta: Option<u64>,
        reply: Sender<Result<String, String>>,
    },
    CountTokens {
        id: u64,
        text: String,
        reply: Sender<Result<i64, String>>,
    },
    Reset {
        id: u64,
        reply: Sender<Result<(), String>>,
    },
}

struct InferenceWorker {
    tx: Sender<InferenceRequest>,
    _thread: JoinHandle<()>,
}

impl InferenceWorker {
    fn start() -> Self {
        let (tx, rx) = mpsc::channel();
        let thread = thread::Builder::new()
            .name("nllm-inference".into())
            .spawn(move || inference_loop(rx))
            .expect("nllm inference thread");
        Self { tx, _thread: thread }
    }

    fn request<R, F>(&self, build: F) -> Result<R, String>
    where
        R: Send + 'static,
        F: FnOnce(Sender<R>) -> InferenceRequest,
    {
        let (reply_tx, reply_rx) = mpsc::channel();
        self.tx
            .send(build(reply_tx))
            .map_err(|e| format!("inference thread stopped: {e}"))?;
        reply_rx
            .recv()
            .map_err(|e| format!("inference thread dropped reply: {e}"))
    }
}

static WORKER: OnceLock<Mutex<InferenceWorker>> = OnceLock::new();

fn with_worker<F, R>(f: F) -> Result<R, String>
where
    F: FnOnce(&InferenceWorker) -> Result<R, String>,
{
    let worker = WORKER.get_or_init(|| Mutex::new(InferenceWorker::start()));
    let guard = worker.lock().map_err(|e| e.to_string())?;
    f(&guard)
}

fn inference_loop(rx: mpsc::Receiver<InferenceRequest>) {
    let mut sessions: HashMap<u64, LlmSession> = HashMap::new();
    let mut next_id = 1u64;

    while let Ok(req) = rx.recv() {
        match req {
            InferenceRequest::Load { path, opts, reply } => {
                let result = LlmSession::load(&path, opts).map(|sess| {
                    let id = next_id;
                    next_id += 1;
                    sessions.insert(id, sess);
                    id
                });
                let _ = reply.send(result.map_err(|e| e.to_string()));
            }
            InferenceRequest::Unload { id, reply } => {
                sessions.remove(&id);
                let _ = reply.send(());
            }
            InferenceRequest::Ready { id, reply } => {
                let _ = reply.send(sessions.contains_key(&id));
            }
            InferenceRequest::Backend { id, reply } => {
                let result = sessions
                    .get(&id)
                    .map(|s| s.backend_name().to_string())
                    .ok_or_else(|| format!("invalid session handle {id}"));
                let _ = reply.send(result);
            }
            InferenceRequest::Chat {
                id,
                messages,
                opts,
                reply,
            } => {
                let result = with_session_mut_local(&mut sessions, id, |sess| {
                    sess.chat(&messages, opts).map_err(|e| e.to_string())
                });
                let _ = reply.send(result);
            }
            InferenceRequest::ChatStream {
                id,
                messages,
                opts,
                on_delta,
                reply,
            } => {
                let result = with_session_mut_local(&mut sessions, id, |sess| {
                    sess.chat_stream(&messages, opts, |delta| {
                        if let Some(handle) = on_delta {
                            crate::ahiru::stream::sse_write(handle, &sse_delta_line(delta))
                                .map_err(|e| niao_llm::LlmError::Msg(e))?;
                        }
                        Ok(())
                    })
                    .map_err(|e| e.to_string())
                });
                let _ = reply.send(result);
            }
            InferenceRequest::CountTokens { id, text, reply } => {
                let result = sessions
                    .get(&id)
                    .ok_or_else(|| format!("invalid session handle {id}"))
                    .and_then(|sess| {
                        sess.count_tokens(&text)
                            .map(|n| n as i64)
                            .map_err(|e| e.to_string())
                    });
                let _ = reply.send(result);
            }
            InferenceRequest::Reset { id, reply } => {
                let result = with_session_mut_local(&mut sessions, id, |sess| {
                    sess.reset();
                    Ok(())
                });
                let _ = reply.send(result);
            }
        }
    }
}

fn with_session_mut_local<F, R>(
    sessions: &mut HashMap<u64, LlmSession>,
    id: u64,
    f: F,
) -> Result<R, String>
where
    F: FnOnce(&mut LlmSession) -> Result<R, String>,
{
    let sess = sessions
        .get_mut(&id)
        .ok_or_else(|| format!("invalid session handle {id}"))?;
    f(sess)
}

fn sse_delta_line(delta: &str) -> String {
    format!(
        "data: {}\n\n",
        serde_json::json!({"type": "delta", "content": delta})
    )
}

pub fn alloc_session(path: PathBuf, opts: LoadOptions) -> Result<u64, String> {
    with_worker(|w| w.request(|reply| InferenceRequest::Load { path, opts, reply }))?
}

pub fn free_session(id: u64) {
    let _ = with_worker(|w| w.request(|reply| InferenceRequest::Unload { id, reply }));
}

pub fn session_ready(id: u64) -> bool {
    with_worker(|w| w.request(|reply| InferenceRequest::Ready { id, reply })).unwrap_or(false)
}

pub fn chat_session(
    id: u64,
    messages: Vec<ChatMessage>,
    opts: GenerateOptions,
    span: Span,
) -> Result<String, RuntimeError> {
    let result: Result<String, String> =
        with_worker(|w| w.request(|reply| InferenceRequest::Chat { id, messages, opts, reply }))
            .map_err(|e| RuntimeError::at(span, codes::E1986_NLLM_ERROR, format!("nllm_chat(): {e}")))?;
    result.map_err(|e| RuntimeError::at(span, codes::E1986_NLLM_ERROR, format!("nllm_chat(): {e}")))
}

pub fn chat_stream_session(
    id: u64,
    messages: Vec<ChatMessage>,
    opts: GenerateOptions,
    sse_handle: Option<u64>,
    span: Span,
) -> Result<String, RuntimeError> {
    let result: Result<String, String> = with_worker(|w| {
        w.request(|reply| InferenceRequest::ChatStream {
            id,
            messages,
            opts,
            on_delta: sse_handle,
            reply,
        })
    })
    .map_err(|e| RuntimeError::at(span, codes::E1986_NLLM_ERROR, format!("nllm_chat_stream(): {e}")))?;
    result.map_err(|e| RuntimeError::at(span, codes::E1986_NLLM_ERROR, format!("nllm_chat_stream(): {e}")))
}

pub fn count_tokens_session(id: u64, text: String, span: Span) -> Result<i64, RuntimeError> {
    let result: Result<i64, String> = with_worker(|w| {
        w.request(|reply| InferenceRequest::CountTokens { id, text, reply })
    })
    .map_err(|e| RuntimeError::at(span, codes::E1986_NLLM_ERROR, format!("nllm_count_tokens(): {e}")))?;
    result.map_err(|e| RuntimeError::at(span, codes::E1986_NLLM_ERROR, format!("nllm_count_tokens(): {e}")))
}

pub fn backend_session(id: u64, span: Span) -> Result<String, RuntimeError> {
    let result: Result<String, String> =
        with_worker(|w| w.request(|reply| InferenceRequest::Backend { id, reply }))
            .map_err(|e| RuntimeError::at(span, codes::E1986_NLLM_ERROR, format!("nllm_backend(): {e}")))?;
    result.map_err(|e| RuntimeError::at(span, codes::E1986_NLLM_ERROR, format!("nllm_backend(): {e}")))
}

pub fn reset_session(id: u64, span: Span) -> Result<(), RuntimeError> {
    let result: Result<(), String> =
        with_worker(|w| w.request(|reply| InferenceRequest::Reset { id, reply }))
            .map_err(|e| RuntimeError::at(span, codes::E1986_NLLM_ERROR, format!("nllm_reset(): {e}")))?;
    result.map_err(|e| RuntimeError::at(span, codes::E1986_NLLM_ERROR, format!("nllm_reset(): {e}")))
}
