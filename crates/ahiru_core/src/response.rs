use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc;

#[derive(Debug, Clone)]
pub enum ResponseBody {
    Buffered(Vec<u8>),
    Stream(Arc<Mutex<Option<mpsc::Sender<Vec<u8>>>>>),
}

#[derive(Debug, Clone)]
pub struct AhiruResponse {
    pub status: u16,
    pub content_type: String,
    pub body: ResponseBody,
    pub headers: HashMap<String, String>,
    pub redirect_url: Option<String>,
}

impl AhiruResponse {
    pub fn text(status: u16, body: impl Into<String>) -> Self {
        Self {
            status,
            content_type: "text/plain; charset=utf-8".into(),
            body: ResponseBody::Buffered(body.into().into_bytes()),
            headers: HashMap::new(),
            redirect_url: None,
        }
    }

    pub fn json(status: u16, body: impl Into<String>) -> Self {
        Self {
            status,
            content_type: "application/json; charset=utf-8".into(),
            body: ResponseBody::Buffered(body.into().into_bytes()),
            headers: HashMap::new(),
            redirect_url: None,
        }
    }

    pub fn html(status: u16, body: impl Into<String>) -> Self {
        Self {
            status,
            content_type: "text/html; charset=utf-8".into(),
            body: ResponseBody::Buffered(body.into().into_bytes()),
            headers: HashMap::new(),
            redirect_url: None,
        }
    }

    pub fn redirect(url: impl Into<String>, permanent: bool) -> Self {
        Self {
            status: if permanent { 301 } else { 302 },
            content_type: "text/plain; charset=utf-8".into(),
            body: ResponseBody::Buffered(Vec::new()),
            headers: HashMap::new(),
            redirect_url: Some(url.into()),
        }
    }

    pub fn stream(content_type: impl Into<String>) -> (Self, mpsc::Receiver<Vec<u8>>) {
        let (tx, rx) = mpsc::channel(64);
        let resp = Self {
            status: 200,
            content_type: content_type.into(),
            body: ResponseBody::Stream(Arc::new(Mutex::new(Some(tx)))),
            headers: HashMap::new(),
            redirect_url: None,
        };
        (resp, rx)
    }

    pub fn sse() -> (Self, mpsc::Receiver<Vec<u8>>) {
        let (mut resp, rx) = Self::stream("text/event-stream");
        resp.headers
            .insert("cache-control".into(), "no-cache".into());
        (resp, rx)
    }

    pub fn write_chunk(body: &ResponseBody, chunk: impl Into<Vec<u8>>) -> Result<(), String> {
        match body {
            ResponseBody::Stream(sender) => {
                let guard = sender.lock().map_err(|e| e.to_string())?;
                if let Some(tx) = guard.as_ref() {
                    tx.try_send(chunk.into())
                        .map_err(|e| format!("stream closed (E2130): {e}"))
                } else {
                    Err("stream closed (E2130)".into())
                }
            }
            ResponseBody::Buffered(_) => Err("not a stream response".into()),
        }
    }

    pub fn body_bytes(&self) -> Option<&[u8]> {
        match &self.body {
            ResponseBody::Buffered(b) => Some(b),
            ResponseBody::Stream(_) => None,
        }
    }

    pub fn with_header(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.headers.insert(key.into(), value.into());
        self
    }

    pub fn not_found(msg: impl Into<String>) -> Self {
        Self::text(404, msg)
    }

    pub fn unauthorized(msg: impl Into<String>) -> Self {
        Self::json(
            401,
            format!(
                r#"{{"error":"{}","code":"E2400"}}"#,
                msg.into().replace('"', "\\\"")
            ),
        )
    }

    pub fn forbidden(msg: impl Into<String>) -> Self {
        Self::json(
            403,
            format!(
                r#"{{"error":"{}","code":"E2400"}}"#,
                msg.into().replace('"', "\\\"")
            ),
        )
    }

    pub fn internal(msg: impl Into<String>) -> Self {
        Self::text(500, msg)
    }
}
