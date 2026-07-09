//! Native networking standard library — HTTP, TCP/UDP, TLS, DNS, WebSocket, SMTP, FTP.
//!
//! Import with `import "net"` (or `import "std/net"`).

mod dns;
mod ftp;
mod handles;
mod http_client;
mod http_server;
mod socket;
mod smtp;
mod tls;
mod url;
mod websocket;

use crate::async_tasks::{
    cancel_task, spawn_async, task_done, task_result_value, task_wait_loop, with_task, AsyncValue,
};
use crate::{error_value, NativeFn, NiaoResult, RuntimeError, Value, ValueRef};
use niao_ast::Span;
use niao_errors::codes;
use std::collections::HashMap;
use std::rc::Rc;

pub type NetResult = NiaoResult<ValueRef>;

pub(crate) fn type_err(span: Span, msg: impl Into<String>) -> RuntimeError {
    RuntimeError::TypeError {
        message: msg.into(),
        line: span.line,
        col: span.col,
    }
}

pub(crate) fn arity(args: &[ValueRef], n: usize, name: &str, span: Span) -> NiaoResult<()> {
    if args.len() != n {
        return Err(RuntimeError::at(
            span,
            codes::E1400_NET_ARITY,
            format!("{name}() expects {n} argument(s), got {}", args.len()),
        ));
    }
    Ok(())
}

pub(crate) fn arity_range(
    args: &[ValueRef],
    min: usize,
    max: usize,
    name: &str,
    span: Span,
) -> NiaoResult<()> {
    if args.len() < min || args.len() > max {
        return Err(RuntimeError::at(
            span,
            codes::E1400_NET_ARITY,
            format!("{name}() expects {min}..={max} argument(s), got {}", args.len()),
        ));
    }
    Ok(())
}

pub(crate) fn string_arg(args: &[ValueRef], idx: usize, name: &str, span: Span) -> NiaoResult<String> {
    match &*args[idx].borrow() {
        Value::String(s) => Ok(s.clone()),
        other => Err(type_err(
            span,
            format!(
                "{name}() expects a string as argument {}, got {}",
                idx + 1,
                other.type_name()
            ),
        )),
    }
}

pub(crate) fn int_arg(args: &[ValueRef], idx: usize, name: &str, span: Span) -> NiaoResult<i64> {
    match &*args[idx].borrow() {
        Value::Int(n) => Ok(*n),
        other => Err(type_err(
            span,
            format!(
                "{name}() expects an int as argument {}, got {}",
                idx + 1,
                other.type_name()
            ),
        )),
    }
}

pub(crate) fn bool_arg(args: &[ValueRef], idx: usize, name: &str, span: Span) -> NiaoResult<bool> {
    match &*args[idx].borrow() {
        Value::Bool(b) => Ok(*b),
        other => Err(type_err(
            span,
            format!(
                "{name}() expects a bool as argument {}, got {}",
                idx + 1,
                other.type_name()
            ),
        )),
    }
}

pub(crate) fn size_arg(args: &[ValueRef], idx: usize, name: &str, span: Span) -> NiaoResult<usize> {
    let n = int_arg(args, idx, name, span)?;
    if n < 0 {
        return Err(type_err(
            span,
            format!("{name}() expects a non-negative int as argument {}", idx + 1),
        ));
    }
    Ok(n as usize)
}

pub(crate) fn port_arg(args: &[ValueRef], idx: usize, name: &str, span: Span) -> NiaoResult<u16> {
    let n = int_arg(args, idx, name, span)?;
    if !(0..=65535).contains(&n) {
        return Err(type_err(span, format!("{name}() port must be 0..=65535")));
    }
    Ok(n as u16)
}

pub(crate) fn handle_arg(args: &[ValueRef], idx: usize, name: &str, span: Span) -> NiaoResult<u64> {
    let id = int_arg(args, idx, name, span)?;
    if id <= 0 {
        return Err(type_err(
            span,
            format!("{name}() expects a positive handle as argument {}", idx + 1),
        ));
    }
    Ok(id as u64)
}

pub(crate) fn object_arg(
    args: &[ValueRef],
    idx: usize,
    name: &str,
    span: Span,
) -> NiaoResult<HashMap<String, ValueRef>> {
    match &*args[idx].borrow() {
        Value::Object(map) => Ok(map.clone()),
        other => Err(type_err(
            span,
            format!(
                "{name}() expects an object as argument {}, got {}",
                idx + 1,
                other.type_name()
            ),
        )),
    }
}

pub(crate) fn object_string_field(
    map: &HashMap<String, ValueRef>,
    field: &str,
    span: Span,
) -> NiaoResult<String> {
    match map.get(field) {
        Some(v) => match &*v.borrow() {
            Value::String(s) => Ok(s.clone()),
            other => Err(type_err(
                span,
                format!("field '{field}' must be string, got {}", other.type_name()),
            )),
        },
        None => Err(type_err(span, format!("missing field '{field}'"))),
    }
}

pub(crate) fn object_int_field(
    map: &HashMap<String, ValueRef>,
    field: &str,
    span: Span,
) -> NiaoResult<i64> {
    match map.get(field) {
        Some(v) => match &*v.borrow() {
            Value::Int(n) => Ok(*n),
            other => Err(type_err(
                span,
                format!("field '{field}' must be int, got {}", other.type_name()),
            )),
        },
        None => Err(type_err(span, format!("missing field '{field}'"))),
    }
}

pub(crate) fn ok_string(s: String) -> ValueRef {
    Value::String(s).ref_cell()
}

pub(crate) fn ok_nil() -> ValueRef {
    Value::Nil.ref_cell()
}

pub(crate) fn ok_int(n: i64) -> ValueRef {
    Value::Int(n).ref_cell()
}

pub(crate) fn net_error(span: Span, code: u32, kind: &str, msg: impl Into<String>) -> ValueRef {
    error_value(code, kind, msg.into(), span)
}

pub(crate) fn bytes_to_int_array(data: Vec<u8>) -> ValueRef {
    Value::IntArray(data.into_iter().map(|b| b as i64).collect()).ref_cell()
}

pub(crate) fn int_array_arg(args: &[ValueRef], idx: usize, name: &str, span: Span) -> NiaoResult<Vec<u8>> {
    match &*args[idx].borrow() {
        Value::IntArray(items) => {
            let mut out = Vec::with_capacity(items.len());
            for &n in items {
                if !(0..=255).contains(&n) {
                    return Err(type_err(span, format!("{name}() byte values must be 0..=255")));
                }
                out.push(n as u8);
            }
            Ok(out)
        }
        Value::Array(items) => {
            let mut out = Vec::with_capacity(items.len());
            for item in items {
                match &*item.borrow() {
                    Value::Int(n) if (0..=255).contains(n) => out.push(*n as u8),
                    other => {
                        return Err(type_err(
                            span,
                            format!(
                                "{name}() expects byte array, got {} in array",
                                other.type_name()
                            ),
                        ));
                    }
                }
            }
            Ok(out)
        }
        Value::String(s) => Ok(s.as_bytes().to_vec()),
        other => Err(type_err(
            span,
            format!(
                "{name}() expects string or byte array as argument {}, got {}",
                idx + 1,
                other.type_name()
            ),
        )),
    }
}

pub(crate) fn payload_arg(args: &[ValueRef], idx: usize, name: &str, span: Span) -> NiaoResult<Vec<u8>> {
    int_array_arg(args, idx, name, span)
}

pub(crate) fn object_field<'a>(val: &'a Value, field: &str) -> Option<ValueRef> {
    match val {
        Value::Object(map) => map.get(field).cloned(),
        _ => None,
    }
}

#[derive(Default, Clone)]
pub struct HttpOpts {
    pub headers: HashMap<String, String>,
    pub body: Option<String>,
    pub body_bytes: Option<Vec<i64>>,
    pub timeout_ms: Option<u64>,
    pub follow_redirects: bool,
    pub user_agent: Option<String>,
    pub auth: Option<(String, String)>,
}

pub(crate) fn parse_http_opts(opts: ValueRef, _span: Span) -> HttpOpts {
    let mut out = HttpOpts {
        follow_redirects: true,
        ..Default::default()
    };
    let map = match &*opts.borrow() {
        Value::Object(m) => m.clone(),
        _ => return out,
    };
    if let Some(h) = map.get("headers") {
        if let Value::Object(hdrs) = &*h.borrow() {
            for (k, v) in hdrs {
                if let Value::String(s) = &*v.borrow() {
                    out.headers.insert(k.clone(), s.clone());
                }
            }
        }
    }
    if let Some(b) = map.get("body") {
        if let Value::String(s) = &*b.borrow() {
            out.body = Some(s.clone());
        }
    }
    if let Some(b) = map.get("body_bytes") {
        if let Value::IntArray(arr) = &*b.borrow() {
            out.body_bytes = Some(arr.clone());
        }
    }
    if let Some(t) = map.get("timeout_ms") {
        if let Value::Int(n) = &*t.borrow() {
            if *n >= 0 {
                out.timeout_ms = Some(*n as u64);
            }
        }
    }
    if let Some(f) = map.get("follow_redirects") {
        if let Value::Bool(b) = &*f.borrow() {
            out.follow_redirects = *b;
        }
    }
    if let Some(ua) = map.get("user_agent") {
        if let Value::String(s) = &*ua.borrow() {
            out.user_agent = Some(s.clone());
        }
    }
    if let Some(a) = map.get("auth") {
        if let Value::Array(pair) = &*a.borrow() {
            if pair.len() >= 2 {
                if let (Value::String(u), Value::String(p)) =
                    (&*pair[0].borrow(), &*pair[1].borrow())
                {
                    out.auth = Some((u.clone(), p.clone()));
                }
            }
        }
    }
    out
}

pub(crate) fn response_body_bytes(val: &Value) -> Vec<u8> {
    if let Some(b) = object_field(val, "body_bytes") {
        if let Value::IntArray(arr) = &*b.borrow() {
            return arr.iter().map(|&n| n as u8).collect();
        }
    }
    if let Some(b) = object_field(val, "body") {
        if let Value::String(s) = &*b.borrow() {
            return s.as_bytes().to_vec();
        }
    }
    Vec::new()
}

pub(crate) fn value_to_async_response(val: &Value) -> AsyncValue {
    let mut map = HashMap::new();
    if let Value::Object(obj) = val {
        for (k, v) in obj {
            map.insert(k.clone(), value_to_async_leaf(&v.borrow()));
        }
    }
    AsyncValue::Object(map)
}

fn value_to_async_leaf(val: &Value) -> AsyncValue {
    match val {
        Value::Nil => AsyncValue::Nil,
        Value::Int(n) => AsyncValue::Int(*n),
        Value::Bool(b) => AsyncValue::Bool(*b),
        Value::String(s) => AsyncValue::String(s.clone()),
        Value::IntArray(v) => AsyncValue::IntArray(v.clone()),
        Value::Object(m) => {
            let mut out = HashMap::new();
            for (k, v) in m {
                out.insert(k.clone(), value_to_async_leaf(&v.borrow()));
            }
            AsyncValue::Object(out)
        }
        _ => AsyncValue::String(val.to_string()),
    }
}

fn net_async_error(span: Span, msg: String) -> ValueRef {
    net_error(span, codes::E1401_NET_ERROR, "net_error", msg)
}

pub fn net_http_response(args: &[ValueRef], span: Span) -> NetResult {
    arity(args, 3, "net_http_response", span)?;
    let status = int_arg(args, 0, "net_http_response", span)?;
    let content_type = string_arg(args, 1, "net_http_response", span)?;
    let body = string_arg(args, 2, "net_http_response", span)?;
    let mut map = HashMap::new();
    map.insert("status".into(), Value::Int(status).ref_cell());
    map.insert("content_type".into(), ok_string(content_type));
    map.insert("body".into(), ok_string(body));
    Ok(Value::Object(map).ref_cell())
}

fn net_async_http_get(args: &[ValueRef], span: Span) -> NetResult {
    arity_range(args, 1, 2, "net_async_http_get", span)?;
    let url = string_arg(args, 0, "net_async_http_get", span)?;
    let opts = if args.len() == 2 {
        parse_http_opts(args[1].clone(), span)
    } else {
        HttpOpts::default()
    };
    let id = spawn_async(move || {
        match http_client::http_request("GET", &url, opts, span) {
            Ok(v) => Ok(http_client::response_to_async(v)),
            Err(e) => {
                let msg = match &*e.borrow() {
                    Value::Error(err) => err.message.clone(),
                    other => other.to_string(),
                };
                Err(msg)
            }
        }
    });
    Ok(ok_int(id as i64))
}

fn net_async_http_request(args: &[ValueRef], span: Span) -> NetResult {
    arity_range(args, 2, 3, "net_async_http_request", span)?;
    let method = string_arg(args, 0, "net_async_http_request", span)?;
    let url = string_arg(args, 1, "net_async_http_request", span)?;
    let opts = if args.len() == 3 {
        parse_http_opts(args[2].clone(), span)
    } else {
        HttpOpts::default()
    };
    let id = spawn_async(move || {
        match http_client::http_request(&method, &url, opts, span) {
            Ok(v) => Ok(http_client::response_to_async(v)),
            Err(e) => {
                let msg = match &*e.borrow() {
                    Value::Error(err) => err.message.clone(),
                    other => other.to_string(),
                };
                Err(msg)
            }
        }
    });
    Ok(ok_int(id as i64))
}

fn net_async_tcp_connect(args: &[ValueRef], span: Span) -> NetResult {
    arity(args, 2, "net_async_tcp_connect", span)?;
    let host = string_arg(args, 0, "net_async_tcp_connect", span)?;
    let port = port_arg(args, 1, "net_async_tcp_connect", span)?;
    let id = spawn_async(move || {
        match std::net::TcpStream::connect(format!("{host}:{port}")) {
            Ok(stream) => {
                let handle = handles::alloc_handle(socket::tcp_handle(stream));
                Ok(AsyncValue::Int(handle as i64))
            }
            Err(e) => Err(e.to_string()),
        }
    });
    Ok(ok_int(id as i64))
}

fn net_task_done(args: &[ValueRef], span: Span) -> NetResult {
    arity(args, 1, "net_task_done", span)?;
    let id = int_arg(args, 0, "net_task_done", span)? as u64;
    with_task(
        id,
        "net_task_done",
        span,
        codes::E1406_NET_TASK_NOT_FOUND,
        "async task cancelled",
        net_async_error,
        |state| Ok(Value::Bool(task_done(state)).ref_cell()),
    )
}

fn net_task_poll(args: &[ValueRef], span: Span) -> NetResult {
    arity(args, 1, "net_task_poll", span)?;
    let id = int_arg(args, 0, "net_task_poll", span)? as u64;
    with_task(
        id,
        "net_task_poll",
        span,
        codes::E1406_NET_TASK_NOT_FOUND,
        "async task cancelled",
        net_async_error,
        |state| Ok(task_result_value(state, span, "async task cancelled", net_async_error)),
    )
}

fn net_task_wait(args: &[ValueRef], span: Span) -> NetResult {
    arity(args, 1, "net_task_wait", span)?;
    let id = int_arg(args, 0, "net_task_wait", span)? as u64;
    task_wait_loop(id);
    with_task(
        id,
        "net_task_wait",
        span,
        codes::E1406_NET_TASK_NOT_FOUND,
        "async task cancelled",
        net_async_error,
        |state| Ok(task_result_value(state, span, "async task cancelled", net_async_error)),
    )
}

fn net_task_cancel(args: &[ValueRef], span: Span) -> NetResult {
    arity(args, 1, "net_task_cancel", span)?;
    let id = int_arg(args, 0, "net_task_cancel", span)? as u64;
    let cancelled = cancel_task(id, span, codes::E1406_NET_TASK_NOT_FOUND)?;
    Ok(Value::Bool(cancelled).ref_cell())
}

/// All net builtins in registration order.
pub fn builtins() -> Vec<(&'static str, NativeFn)> {
    vec![
        // url
        ("net_url_parse", Rc::new(url::net_url_parse)),
        ("net_url_encode", Rc::new(url::net_url_encode)),
        ("net_url_decode", Rc::new(url::net_url_decode)),
        ("net_url_join", Rc::new(url::net_url_join)),
        ("net_url_build", Rc::new(url::net_url_build)),
        // dns
        ("net_resolve", Rc::new(dns::net_resolve)),
        ("net_hostname", Rc::new(dns::net_hostname)),
        // http client
        ("net_http_get", Rc::new(http_client::net_http_get)),
        ("net_http_post", Rc::new(http_client::net_http_post)),
        ("net_http_put", Rc::new(http_client::net_http_put)),
        ("net_http_delete", Rc::new(http_client::net_http_delete)),
        ("net_http_patch", Rc::new(http_client::net_http_patch)),
        ("net_http_head", Rc::new(http_client::net_http_head)),
        ("net_http_request", Rc::new(http_client::net_http_request)),
        ("net_http_download", Rc::new(http_client::net_http_download)),
        ("net_response_field", Rc::new(http_server::net_response_field)),
        // tcp/udp
        ("net_tcp_socket", Rc::new(socket::net_tcp_socket)),
        ("net_tcp_connect", Rc::new(socket::net_tcp_connect)),
        ("net_tcp_bind", Rc::new(socket::net_tcp_bind)),
        ("net_tcp_listen", Rc::new(socket::net_tcp_listen)),
        ("net_tcp_accept", Rc::new(socket::net_tcp_accept)),
        ("net_tcp_send", Rc::new(socket::net_tcp_send)),
        ("net_tcp_recv", Rc::new(socket::net_tcp_recv)),
        ("net_tcp_close", Rc::new(socket::net_tcp_close)),
        ("net_udp_socket", Rc::new(socket::net_udp_socket)),
        ("net_udp_bind", Rc::new(socket::net_udp_bind)),
        ("net_udp_send", Rc::new(socket::net_udp_send)),
        ("net_udp_recv", Rc::new(socket::net_udp_recv)),
        ("net_set_timeout", Rc::new(socket::net_set_timeout)),
        // tls
        ("net_tls_connect", Rc::new(tls::net_tls_connect)),
        ("net_tls_wrap", Rc::new(tls::net_tls_wrap)),
        ("net_tls_config", Rc::new(tls::net_tls_config)),
        // http server
        ("net_http_listen", Rc::new(http_server::net_http_listen)),
        ("net_http_route", Rc::new(http_server::net_http_route)),
        ("net_http_on_request", Rc::new(http_server::net_http_on_request)),
        ("net_http_serve", Rc::new(http_server::net_http_serve)),
        ("net_http_poll", Rc::new(http_server::net_http_poll)),
        ("net_http_serve_async", Rc::new(http_server::net_http_serve_async)),
        ("net_http_stop", Rc::new(http_server::net_http_stop)),
        ("net_http_response", Rc::new(net_http_response)),
        ("net_request_field", Rc::new(http_server::net_request_field)),
        // websocket
        ("net_ws_connect", Rc::new(websocket::net_ws_connect)),
        ("net_ws_send", Rc::new(websocket::net_ws_send)),
        ("net_ws_recv", Rc::new(websocket::net_ws_recv)),
        ("net_ws_close", Rc::new(websocket::net_ws_close)),
        // smtp / ftp
        ("net_smtp_send", Rc::new(smtp::net_smtp_send)),
        ("net_ftp_connect", Rc::new(ftp::net_ftp_connect)),
        ("net_ftp_login", Rc::new(ftp::net_ftp_login)),
        ("net_ftp_get", Rc::new(ftp::net_ftp_get)),
        ("net_ftp_put", Rc::new(ftp::net_ftp_put)),
        ("net_ftp_close", Rc::new(ftp::net_ftp_close)),
        // async
        ("net_async_http_get", Rc::new(net_async_http_get)),
        ("net_async_http_request", Rc::new(net_async_http_request)),
        ("net_async_tcp_connect", Rc::new(net_async_tcp_connect)),
        ("net_task_done", Rc::new(net_task_done)),
        ("net_task_poll", Rc::new(net_task_poll)),
        ("net_task_wait", Rc::new(net_task_wait)),
        ("net_task_cancel", Rc::new(net_task_cancel)),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use niao_ast::Span;

    #[test]
    fn url_parse_roundtrip() {
        let span = Span::dummy();
        let parsed = url::net_url_parse(
            &[Value::String("https://example.com:443/path?q=1".into()).ref_cell()],
            span,
        )
        .unwrap();
        let obj = match &*parsed.borrow() {
            Value::Object(m) => m.clone(),
            _ => panic!("expected object"),
        };
        assert_eq!(
            object_string_field(&obj, "host", span).unwrap(),
            "example.com"
        );
    }
}
