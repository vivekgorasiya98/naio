use crate::handler::{HandlerFn, WsHandlerFn};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum HttpMethod {
    Get,
    Post,
    Put,
    Delete,
    Patch,
    Options,
    Head,
}

impl HttpMethod {
    pub fn as_str(&self) -> &'static str {
        match self {
            HttpMethod::Get => "GET",
            HttpMethod::Post => "POST",
            HttpMethod::Put => "PUT",
            HttpMethod::Delete => "DELETE",
            HttpMethod::Patch => "PATCH",
            HttpMethod::Options => "OPTIONS",
            HttpMethod::Head => "HEAD",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_uppercase().as_str() {
            "GET" => Some(HttpMethod::Get),
            "POST" => Some(HttpMethod::Post),
            "PUT" => Some(HttpMethod::Put),
            "DELETE" => Some(HttpMethod::Delete),
            "PATCH" => Some(HttpMethod::Patch),
            "OPTIONS" => Some(HttpMethod::Options),
            "HEAD" => Some(HttpMethod::Head),
            _ => None,
        }
    }
}

#[derive(Clone, Default)]
pub struct RouteMeta {
    pub permission: Option<String>,
    pub ws: bool,
    pub public: bool,
    pub body_limit_mb: Option<u64>,
    pub timeout_ms: Option<u64>,
    pub stream: bool,
    pub schema: Option<HandlerFn>,
    pub guard: Option<HandlerFn>,
}

#[derive(Clone)]
pub struct RouteEntry {
    pub method: HttpMethod,
    pub path: String,
    pub axum_path: String,
    pub handler: HandlerFn,
    pub ws_handler: Option<WsHandlerFn>,
    pub meta: RouteMeta,
}

#[derive(Debug, Clone, Serialize)]
pub struct RouteInfo {
    pub method: String,
    pub path: String,
    pub permission: Option<String>,
    pub websocket: bool,
}

impl RouteEntry {
    pub fn info(&self) -> RouteInfo {
        RouteInfo {
            method: self.method.as_str().into(),
            path: self.path.clone(),
            permission: self.meta.permission.clone(),
            websocket: self.meta.ws,
        }
    }
}

/// Convert `/users/:id/posts/:postId` → `/users/{id}/posts/{postId}` for Axum.
pub fn to_axum_path(path: &str) -> String {
    let mut out = String::new();
    for part in path.split('/') {
        if part.is_empty() {
            continue;
        }
        out.push('/');
        if let Some(name) = part.strip_prefix(':') {
            out.push('{');
            out.push_str(name);
            out.push('}');
        } else {
            out.push_str(part);
        }
    }
    if out.is_empty() {
        "/".into()
    } else {
        out
    }
}

pub fn extract_path_params(template: &str, actual: &str) -> std::collections::HashMap<String, String> {
    let mut params = std::collections::HashMap::new();
    let t_parts: Vec<&str> = template.split('/').filter(|s| !s.is_empty()).collect();
    let a_parts: Vec<&str> = actual.split('/').filter(|s| !s.is_empty()).collect();
    if t_parts.len() != a_parts.len() {
        return params;
    }
    for (t, a) in t_parts.iter().zip(a_parts.iter()) {
        if let Some(name) = t.strip_prefix(':') {
            params.insert(name.to_string(), urlencoding_decode(a));
        }
    }
    params
}

pub fn parse_query(query: &str) -> std::collections::HashMap<String, String> {
    let mut map = std::collections::HashMap::new();
    for pair in query.split('&') {
        if pair.is_empty() {
            continue;
        }
        let mut it = pair.splitn(2, '=');
        if let Some(k) = it.next() {
            let v = it.next().unwrap_or("");
            map.insert(
                urlencoding_decode(k),
                urlencoding_decode(v),
            );
        }
    }
    map
}

fn urlencoding_decode(s: &str) -> String {
    let s = s.replace('+', " ");
    percent_decode(&s)
}

fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let Ok(v) = u8::from_str_radix(
                std::str::from_utf8(&bytes[i + 1..i + 3]).unwrap_or(""),
                16,
            ) {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}
