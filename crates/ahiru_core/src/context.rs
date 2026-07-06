use crate::auth::UserContext;
use axum::body::Bytes;
use axum::http::{HeaderMap, Method};
use std::collections::HashMap;

#[derive(Debug, Clone)]
pub struct UploadedFile {
    pub name: String,
    pub filename: String,
    pub content_type: String,
    pub data: Vec<u8>,
}

#[derive(Debug, Clone)]
pub struct RequestContext {
    pub method: String,
    pub path: String,
    pub query: HashMap<String, String>,
    pub params: HashMap<String, String>,
    pub headers: HashMap<String, String>,
    body_bytes: Bytes,
    body_str: Option<String>,
    pub request_id: String,
    pub user: Option<UserContext>,
    pub extra: HashMap<String, String>,
    pub state: HashMap<String, String>,
    pub files: Vec<UploadedFile>,
    pub cookies: HashMap<String, String>,
    pub response_cookies: Vec<(String, String, CookieOpts)>,
    pub ws_room: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct CookieOpts {
    pub max_age_secs: Option<u64>,
    pub http_only: bool,
    pub secure: bool,
    pub path: Option<String>,
}

impl RequestContext {
    pub fn from_parts(
        method: Method,
        path: String,
        query: HashMap<String, String>,
        params: HashMap<String, String>,
        headers: HeaderMap,
        body: Bytes,
        request_id: String,
        state: HashMap<String, String>,
    ) -> Self {
        let hdr_count = headers.len();
        let mut hdrs = HashMap::with_capacity(hdr_count);
        let mut cookies = HashMap::new();
        for (k, v) in headers.iter() {
            if let Ok(s) = v.to_str() {
                let key = k.as_str().to_lowercase();
                if key == "cookie" {
                    for part in s.split(';') {
                        let part = part.trim();
                        if let Some((name, value)) = part.split_once('=') {
                            cookies.insert(name.trim().to_string(), value.trim().to_string());
                        }
                    }
                }
                hdrs.insert(key, s.to_string());
            }
        }
        Self {
            method: method.to_string(),
            path,
            query,
            params,
            headers: hdrs,
            body_bytes: body,
            body_str: None,
            request_id,
            user: None,
            extra: HashMap::new(),
            state,
            files: Vec::new(),
            cookies,
            response_cookies: Vec::new(),
            ws_room: None,
        }
    }

    pub fn body_bytes(&self) -> &Bytes {
        &self.body_bytes
    }

    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers.get(&name.to_lowercase()).map(|s| s.as_str())
    }

    pub fn param(&self, name: &str) -> Option<&str> {
        self.params.get(name).map(|s| s.as_str())
    }

    pub fn query_param(&self, name: &str) -> Option<&str> {
        self.query.get(name).map(|s| s.as_str())
    }

    pub fn set_cookie(
        &mut self,
        name: impl Into<String>,
        value: impl Into<String>,
        opts: CookieOpts,
    ) {
        self.response_cookies
            .push((name.into(), value.into(), opts));
    }
}

pub fn body_for_bridge(ctx: &mut RequestContext) -> String {
    if let Some(s) = &ctx.body_str {
        return s.clone();
    }
    let s = String::from_utf8_lossy(&ctx.body_bytes).into_owned();
    ctx.body_str = Some(s.clone());
    s
}

pub fn body_for_dispatch(ctx: &RequestContext) -> String {
    if let Some(s) = &ctx.body_str {
        return s.clone();
    }
    String::from_utf8_lossy(&ctx.body_bytes).into_owned()
}
