//! HTTP/HTTPS client via `ureq`.

use super::{net_error, ok_string, parse_http_opts, string_arg, HttpOpts, NetResult};
use niao_ast::Span;
use niao_errors::codes;
use std::collections::HashMap;
use std::fs;

fn apply_opts(mut request: ureq::Request, opts: &HttpOpts) -> ureq::Request {
    if let Some(ms) = opts.timeout_ms {
        request = request.timeout(std::time::Duration::from_millis(ms));
    }
    if let Some(ua) = &opts.user_agent {
        request = request.set("User-Agent", ua);
    }
    if let Some((user, pass)) = &opts.auth {
        request = request.set(
            "Authorization",
            &format!("Basic {}", base64_encode(&format!("{user}:{pass}"))),
        );
    }
    for (k, v) in &opts.headers {
        request = request.set(k, v);
    }
    request
}

pub fn http_request(
    method: &str,
    url: &str,
    opts: HttpOpts,
    span: Span,
) -> Result<crate::ValueRef, crate::ValueRef> {
    let result = match method.to_uppercase().as_str() {
        "GET" => {
            if opts.body.is_some() || opts.body_bytes.is_some() {
                return Err(net_error(
                    span,
                    codes::E1404_NET_HTTP,
                    "net_http_error",
                    "GET cannot include a body",
                ));
            }
            apply_opts(ureq::get(url), &opts).call()
        }
        "HEAD" => apply_opts(ureq::head(url), &opts).call(),
        "POST" => {
            let req = apply_opts(ureq::post(url), &opts);
            send_body(req, &opts)
        }
        "PUT" => {
            let req = apply_opts(ureq::put(url), &opts);
            send_body(req, &opts)
        }
        "DELETE" => {
            let req = apply_opts(ureq::delete(url), &opts);
            send_body(req, &opts)
        }
        "PATCH" => {
            let req = apply_opts(ureq::request("PATCH", url), &opts);
            send_body(req, &opts)
        }
        other => {
            return Err(net_error(
                span,
                codes::E1404_NET_HTTP,
                "net_http_error",
                format!("unsupported HTTP method: {other}"),
            ))
        }
    };

    match result {
        Ok(resp) => Ok(response_to_value(resp, url)),
        Err(ureq::Error::Status(_code, resp)) => Ok(response_to_value(resp, url)),
        Err(e) => Err(net_error(
            span,
            codes::E1401_NET_ERROR,
            "net_error",
            e.to_string(),
        )),
    }
}

fn send_body(request: ureq::Request, opts: &HttpOpts) -> Result<ureq::Response, ureq::Error> {
    if let Some(body) = &opts.body {
        request.send_string(body)
    } else if let Some(bytes) = &opts.body_bytes {
        let data: Vec<u8> = bytes.iter().map(|&b| b as u8).collect();
        request.send_bytes(&data)
    } else {
        request.call()
    }
}

fn base64_encode(input: &str) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let bytes = input.as_bytes();
    let mut out = String::new();
    for chunk in bytes.chunks(3) {
        let b0 = chunk[0] as u32;
        let b1 = chunk.get(1).copied().unwrap_or(0) as u32;
        let b2 = chunk.get(2).copied().unwrap_or(0) as u32;
        let n = (b0 << 16) | (b1 << 8) | b2;
        out.push(TABLE[((n >> 18) & 63) as usize] as char);
        out.push(TABLE[((n >> 12) & 63) as usize] as char);
        if chunk.len() > 1 {
            out.push(TABLE[((n >> 6) & 63) as usize] as char);
        } else {
            out.push('=');
        }
        if chunk.len() > 2 {
            out.push(TABLE[(n & 63) as usize] as char);
        } else {
            out.push('=');
        }
    }
    out
}

fn response_to_value(resp: ureq::Response, url: &str) -> crate::ValueRef {
    let status = resp.status() as i64;
    let final_url = resp.get_url().to_string();
    let mut headers = HashMap::new();
    for name in resp.headers_names() {
        if let Some(v) = resp.header(&name) {
            headers.insert(name.to_lowercase(), ok_string(v.to_string()));
        }
    }
    let body_bytes = resp.into_string().map(|s| s.into_bytes()).unwrap_or_default();
    let body = String::from_utf8_lossy(&body_bytes).into_owned();
    let ok = (200..300).contains(&(status as u16));
    let mut map = HashMap::new();
    map.insert("status".into(), crate::Value::Int(status).ref_cell());
    map.insert("ok".into(), crate::Value::Bool(ok).ref_cell());
    map.insert(
        "url".into(),
        ok_string(if final_url.is_empty() {
            url.into()
        } else {
            final_url
        }),
    );
    map.insert("body".into(), ok_string(body));
    map.insert(
        "body_bytes".into(),
        crate::Value::IntArray(body_bytes.into_iter().map(|b| b as i64).collect()).ref_cell(),
    );
    map.insert("headers".into(), crate::Value::Object(headers).ref_cell());
    crate::Value::Object(map).ref_cell()
}

fn parse_opts(args: &[crate::ValueRef], start: usize, span: Span) -> HttpOpts {
    if args.len() <= start {
        return HttpOpts::default();
    }
    parse_http_opts(args[start].clone(), span)
}

pub fn net_http_get(args: &[crate::ValueRef], span: Span) -> NetResult {
    super::arity_range(args, 1, 2, "net_http_get", span)?;
    let url = string_arg(args, 0, "net_http_get", span)?;
    let opts = parse_opts(args, 1, span);
    match http_request("GET", &url, opts, span) {
        Ok(v) => Ok(v),
        Err(e) => Ok(e),
    }
}

pub fn net_http_post(args: &[crate::ValueRef], span: Span) -> NetResult {
    super::arity_range(args, 2, 3, "net_http_post", span)?;
    let url = string_arg(args, 0, "net_http_post", span)?;
    let body = string_arg(args, 1, "net_http_post", span)?;
    let mut opts = parse_opts(args, 2, span);
    opts.body = Some(body);
    match http_request("POST", &url, opts, span) {
        Ok(v) => Ok(v),
        Err(e) => Ok(e),
    }
}

pub fn net_http_put(args: &[crate::ValueRef], span: Span) -> NetResult {
    super::arity_range(args, 2, 3, "net_http_put", span)?;
    let url = string_arg(args, 0, "net_http_put", span)?;
    let body = string_arg(args, 1, "net_http_put", span)?;
    let mut opts = parse_opts(args, 2, span);
    opts.body = Some(body);
    match http_request("PUT", &url, opts, span) {
        Ok(v) => Ok(v),
        Err(e) => Ok(e),
    }
}

pub fn net_http_delete(args: &[crate::ValueRef], span: Span) -> NetResult {
    super::arity_range(args, 1, 2, "net_http_delete", span)?;
    let url = string_arg(args, 0, "net_http_delete", span)?;
    let opts = parse_opts(args, 1, span);
    match http_request("DELETE", &url, opts, span) {
        Ok(v) => Ok(v),
        Err(e) => Ok(e),
    }
}

pub fn net_http_patch(args: &[crate::ValueRef], span: Span) -> NetResult {
    super::arity_range(args, 2, 3, "net_http_patch", span)?;
    let url = string_arg(args, 0, "net_http_patch", span)?;
    let body = string_arg(args, 1, "net_http_patch", span)?;
    let mut opts = parse_opts(args, 2, span);
    opts.body = Some(body);
    match http_request("PATCH", &url, opts, span) {
        Ok(v) => Ok(v),
        Err(e) => Ok(e),
    }
}

pub fn net_http_head(args: &[crate::ValueRef], span: Span) -> NetResult {
    super::arity_range(args, 1, 2, "net_http_head", span)?;
    let url = string_arg(args, 0, "net_http_head", span)?;
    let opts = parse_opts(args, 1, span);
    match http_request("HEAD", &url, opts, span) {
        Ok(v) => Ok(v),
        Err(e) => Ok(e),
    }
}

pub fn net_http_request(args: &[crate::ValueRef], span: Span) -> NetResult {
    super::arity_range(args, 2, 3, "net_http_request", span)?;
    let method = string_arg(args, 0, "net_http_request", span)?;
    let url = string_arg(args, 1, "net_http_request", span)?;
    let opts = parse_opts(args, 2, span);
    match http_request(&method, &url, opts, span) {
        Ok(v) => Ok(v),
        Err(e) => Ok(e),
    }
}

pub fn net_http_download(args: &[crate::ValueRef], span: Span) -> NetResult {
    super::arity_range(args, 2, 3, "net_http_download", span)?;
    let url = string_arg(args, 0, "net_http_download", span)?;
    let path = string_arg(args, 1, "net_http_download", span)?;
    let opts = parse_opts(args, 2, span);
    match http_request("GET", &url, opts, span) {
        Ok(resp) => {
            let bytes = super::response_body_bytes(&resp.borrow());
            match fs::write(&path, bytes) {
                Ok(()) => Ok(resp),
                Err(e) => Ok(net_error(
                    span,
                    codes::E1401_NET_ERROR,
                    "net_error",
                    e.to_string(),
                )),
            }
        }
        Err(e) => Ok(e),
    }
}

pub fn response_to_async(resp: crate::ValueRef) -> crate::async_tasks::AsyncValue {
    super::value_to_async_response(&resp.borrow())
}
