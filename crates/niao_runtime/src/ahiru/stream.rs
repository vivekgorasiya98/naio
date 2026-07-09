//! SSE stream handles for incremental HTTP responses.
//!
//! Handlers run synchronously and return the stream object when done, so chunks are
//! buffered here and flushed as the final HTTP response (avoids dropping the mpsc
//! receiver before any writes).

use ahiru_core::{AhiruResponse, ResponseBody};
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

struct SseStream {
    status: u16,
    chunks: Vec<Vec<u8>>,
}

static STREAMS: OnceLock<Mutex<HashMap<u64, SseStream>>> = OnceLock::new();
static NEXT_STREAM: OnceLock<Mutex<u64>> = OnceLock::new();

fn streams() -> &'static Mutex<HashMap<u64, SseStream>> {
    STREAMS.get_or_init(|| Mutex::new(HashMap::new()))
}

fn next_id() -> u64 {
    let mut id = NEXT_STREAM.get_or_init(|| Mutex::new(1)).lock().unwrap();
    let n = *id;
    *id = n + 1;
    n
}

pub fn sse_start(status: u16) -> u64 {
    let id = next_id();
    streams().lock().unwrap().insert(
        id,
        SseStream {
            status,
            chunks: Vec::new(),
        },
    );
    id
}

pub fn sse_write(id: u64, chunk: &str) -> Result<(), String> {
    let mut guard = streams().lock().map_err(|e| e.to_string())?;
    let state = guard
        .get_mut(&id)
        .ok_or_else(|| format!("invalid stream handle {id}"))?;
    state.chunks.push(chunk.as_bytes().to_vec());
    Ok(())
}

pub fn take_response(id: u64) -> Option<AhiruResponse> {
    let state = streams().lock().ok()?.remove(&id)?;
    let body: Vec<u8> = state.chunks.into_iter().flatten().collect();
    let mut resp = AhiruResponse {
        status: state.status,
        content_type: "text/event-stream".into(),
        body: ResponseBody::Buffered(body),
        headers: HashMap::new(),
        redirect_url: None,
    };
    resp.headers
        .insert("cache-control".into(), "no-cache".into());
    Some(resp)
}

pub fn is_stream_handle(val: &crate::Value) -> Option<u64> {
    use crate::Value;
    match val {
        Value::Object(map) => map.get("stream_handle").and_then(|v| match &*v.borrow() {
            Value::Int(n) if *n > 0 => Some(*n as u64),
            _ => None,
        }),
        _ => None,
    }
}
