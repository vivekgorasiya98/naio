//! URL parsing and encoding utilities.

use super::{net_error, ok_string, string_arg, NetResult};
use niao_ast::Span;
use niao_errors::codes;
use std::collections::HashMap;
use url::form_urlencoded;
use url::Url;

pub fn net_url_parse(args: &[crate::ValueRef], span: Span) -> NetResult {
    super::arity(args, 1, "net_url_parse", span)?;
    let raw = string_arg(args, 0, "net_url_parse", span)?;
    match Url::parse(&raw) {
        Ok(u) => {
            let mut map = HashMap::new();
            map.insert("scheme".into(), ok_string(u.scheme().into()));
            map.insert(
                "host".into(),
                ok_string(u.host_str().unwrap_or("").into()),
            );
            map.insert(
                "port".into(),
                crate::Value::Int(u.port().unwrap_or(default_port(u.scheme())) as i64).ref_cell(),
            );
            map.insert("path".into(), ok_string(u.path().into()));
            map.insert(
                "query".into(),
                ok_string(u.query().unwrap_or("").into()),
            );
            map.insert(
                "fragment".into(),
                ok_string(u.fragment().unwrap_or("").into()),
            );
            map.insert(
                "user".into(),
                ok_string(u.username().into()),
            );
            map.insert(
                "password".into(),
                ok_string(u.password().unwrap_or("").into()),
            );
            Ok(crate::Value::Object(map).ref_cell())
        }
        Err(e) => Ok(net_error(
            span,
            codes::E1403_NET_URL,
            "net_url_error",
            e.to_string(),
        )),
    }
}

fn default_port(scheme: &str) -> u16 {
    match scheme {
        "http" => 80,
        "https" => 443,
        "ws" => 80,
        "wss" => 443,
        "ftp" => 21,
        _ => 0,
    }
}

pub fn net_url_encode(args: &[crate::ValueRef], span: Span) -> NetResult {
    super::arity(args, 1, "net_url_encode", span)?;
    let s = string_arg(args, 0, "net_url_encode", span)?;
    let encoded: String = form_urlencoded::byte_serialize(s.as_bytes()).collect();
    Ok(ok_string(encoded))
}

pub fn net_url_decode(args: &[crate::ValueRef], span: Span) -> NetResult {
    super::arity(args, 1, "net_url_decode", span)?;
    let s = string_arg(args, 0, "net_url_decode", span)?;
    match form_urlencoded::parse(s.as_bytes()).next() {
        Some((decoded, _)) => Ok(ok_string(decoded.into_owned())),
        None => Ok(ok_string(String::new())),
    }
}

pub fn net_url_join(args: &[crate::ValueRef], span: Span) -> NetResult {
    super::arity(args, 2, "net_url_join", span)?;
    let base = string_arg(args, 0, "net_url_join", span)?;
    let reference = string_arg(args, 1, "net_url_join", span)?;
    match Url::parse(&base).and_then(|b| b.join(&reference)) {
        Ok(u) => Ok(ok_string(u.into())),
        Err(e) => Ok(net_error(
            span,
            codes::E1403_NET_URL,
            "net_url_error",
            e.to_string(),
        )),
    }
}

pub fn net_url_build(args: &[crate::ValueRef], span: Span) -> NetResult {
    super::arity(args, 1, "net_url_build", span)?;
    let parts = super::object_arg(args, 0, "net_url_build", span)?;
    let scheme = super::object_string_field(&parts, "scheme", span)?;
    let host = super::object_string_field(&parts, "host", span)?;
    let path = super::object_string_field(&parts, "path", span).unwrap_or_else(|_| "/".into());
    let query = super::object_string_field(&parts, "query", span).unwrap_or_default();
    let fragment = super::object_string_field(&parts, "fragment", span).unwrap_or_default();
    let port = super::object_int_field(&parts, "port", span).ok();

    let mut url = format!("{scheme}://{host}");
    if let Some(p) = port {
        if p > 0 {
            url.push(':');
            url.push_str(&p.to_string());
        }
    }
    if !path.starts_with('/') {
        url.push('/');
    }
    url.push_str(&path);
    if !query.is_empty() {
        url.push('?');
        url.push_str(&query);
    }
    if !fragment.is_empty() {
        url.push('#');
        url.push_str(&fragment);
    }
    match Url::parse(&url) {
        Ok(u) => Ok(ok_string(u.into())),
        Err(e) => Ok(net_error(
            span,
            codes::E1403_NET_URL,
            "net_url_error",
            e.to_string(),
        )),
    }
}
