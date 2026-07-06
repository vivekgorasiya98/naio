//! SSE stream handles for incremental HTTP responses.

use ahiru_core::AhiruResponse;
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

static STREAMS: OnceLock<Mutex<HashMap<u64, AhiruResponse>>> = OnceLock::new();
static NEXT_STREAM: OnceLock<Mutex<u64>> = OnceLock::new();

fn streams() -> &'static Mutex<HashMap<u64, AhiruResponse>> {
    STREAMS.get_or_init(|| Mutex::new(HashMap::new()))
}

fn next_id() -> u64 {
    let mut id = NEXT_STREAM.get_or_init(|| Mutex::new(1)).lock().unwrap();
    let n = *id;
    *id = n + 1;
    n
}

pub fn sse_start(status: u16) -> u64 {
    let (resp, _rx) = AhiruResponse::sse();
    let mut resp = resp;
    resp.status = status;
    let id = next_id();
    streams().lock().unwrap().insert(id, resp);
    id
}

pub fn sse_write(id: u64, chunk: &str) -> Result<(), String> {
    let guard = streams().lock().map_err(|e| e.to_string())?;
    let resp = guard
        .get(&id)
        .ok_or_else(|| format!("invalid stream handle {id}"))?;
    AhiruResponse::write_chunk(&resp.body, chunk.as_bytes().to_vec())
}

pub fn take_response(id: u64) -> Option<AhiruResponse> {
    streams().lock().ok()?.remove(&id)
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
