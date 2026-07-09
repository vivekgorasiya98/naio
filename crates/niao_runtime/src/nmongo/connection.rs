//! Connection open/close, ping, and database listing.

use super::common::*;
use super::handles::{alloc_client, remove_client, warm_parallel_client_pool, with_client};
use super::runtime::block_on;
use crate::{error_value, NiaoResult, RuntimeError, Value, ValueRef};
use mongodb::bson::doc;
use mongodb::options::{ClientOptions, Tls, TlsOptions};
use mongodb::Client;
use niao_ast::Span;
use niao_errors::codes;

fn nmongo_error(span: Span, msg: impl Into<String>) -> ValueRef {
    error_value(codes::E1921_NMONGO_ERROR, "nmongo_error", msg.into(), span)
}

fn ok_int(n: i64) -> ValueRef {
    Value::Int(n).ref_cell()
}

fn ok_bool(b: bool) -> ValueRef {
    Value::Bool(b).ref_cell()
}

fn ok_nil() -> ValueRef {
    Value::Nil.ref_cell()
}

fn str_field(map: &std::collections::HashMap<String, ValueRef>, key: &str) -> Option<String> {
    map.get(key).and_then(|v| match &*v.borrow() {
        Value::String(s) => Some(s.clone()),
        _ => None,
    })
}

fn bool_field(map: &std::collections::HashMap<String, ValueRef>, key: &str, default: bool) -> bool {
    map.get(key)
        .and_then(|v| match &*v.borrow() {
            Value::Bool(b) => Some(*b),
            _ => None,
        })
        .unwrap_or(default)
}

fn int_field(map: &std::collections::HashMap<String, ValueRef>, key: &str) -> Option<i64> {
    map.get(key).and_then(|v| match &*v.borrow() {
        Value::Int(n) => Some(*n),
        _ => None,
    })
}

fn pct_encode(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

fn build_uri(opts: &std::collections::HashMap<String, ValueRef>) -> String {
    let mut hosts: Vec<String> = Vec::new();
    if let Some(hosts_val) = opts.get("hosts") {
        if let Value::Array(items) = &*hosts_val.borrow() {
            for item in items {
                if let Value::String(h) = &*item.borrow() {
                    hosts.push(h.clone());
                }
            }
        }
    }
    if hosts.is_empty() {
        let host = str_field(opts, "host").unwrap_or_else(|| "localhost".to_string());
        let port = int_field(opts, "port").unwrap_or(27017);
        hosts.push(format!("{host}:{port}"));
    }
    let host_part = hosts.join(",");
    if let Some(user) = str_field(opts, "user") {
        let pw = str_field(opts, "password").unwrap_or_default();
        let auth_db = str_field(opts, "auth_source").unwrap_or_else(|| "admin".to_string());
        format!(
            "mongodb://{}:{}@{}?authSource={}",
            pct_encode(&user),
            pct_encode(&pw),
            host_part,
            pct_encode(&auth_db)
        )
    } else {
        format!("mongodb://{host_part}/")
    }
}

fn build_client_options(
    opts: &std::collections::HashMap<String, ValueRef>,
    span: Span,
) -> Result<(ClientOptions, Option<String>), RuntimeError> {
    let password = str_field(opts, "password");
    let uri = build_uri(opts);

    let mut client_opts = block_on(async move { ClientOptions::parse(&uri).await }).map_err(|e| {
        RuntimeError::at(
            span,
            codes::E1921_NMONGO_ERROR,
            redact_secrets(&e.to_string(), password.as_deref()),
        )
    })?;

    if let Some(db) = str_field(opts, "database") {
        validate_name(&db, "database", span)?;
        client_opts.default_database = Some(db);
    }

    if let Some(max) = int_field(opts, "max_pool_size") {
        client_opts.max_pool_size = Some(max as u32);
    }
    if let Some(min) = int_field(opts, "min_pool_size") {
        client_opts.min_pool_size = Some(min as u32);
    }
    if let Some(ms) = int_field(opts, "server_selection_timeout_ms") {
        client_opts.server_selection_timeout = Some(std::time::Duration::from_millis(ms as u64));
    }
    if let Some(ms) = int_field(opts, "connect_timeout_ms") {
        client_opts.connect_timeout = Some(std::time::Duration::from_millis(ms as u64));
    }
    if let Some(name) = str_field(opts, "app_name") {
        client_opts.app_name = Some(name);
    }

    let tls_enabled = if let Some(tls_val) = opts.get("tls") {
        match &*tls_val.borrow() {
            Value::Object(tls_map) => bool_field(tls_map, "enabled", true),
            Value::Bool(b) => *b,
            _ => true,
        }
    } else {
        false
    };

    if tls_enabled {
        let allow_invalid = opts
            .get("tls")
            .and_then(|v| match &*v.borrow() {
                Value::Object(m) => Some(bool_field(m, "allow_invalid_certs", false)),
                _ => None,
            })
            .unwrap_or(false);
        let ca_file = opts.get("tls").and_then(|v| match &*v.borrow() {
            Value::Object(m) => str_field(m, "ca_file"),
            _ => None,
        });
        let tls_opts = match (allow_invalid, ca_file.as_deref()) {
            (true, Some(ca)) => TlsOptions::builder()
                .allow_invalid_certificates(true)
                .ca_file_path(Some(std::path::PathBuf::from(ca)))
                .build(),
            (true, None) => TlsOptions::builder()
                .allow_invalid_certificates(true)
                .build(),
            (false, Some(ca)) => TlsOptions::builder()
                .ca_file_path(Some(std::path::PathBuf::from(ca)))
                .build(),
            (false, None) => TlsOptions::builder().build(),
        };
        client_opts.tls = Some(Tls::Enabled(tls_opts));
    }

    Ok((client_opts, password))
}

pub fn nmongo_connect(args: &[ValueRef], span: Span) -> NiaoResult<ValueRef> {
    arity(args, 1, "nmongo_connect", span)?;
    let opts_map = match &*args[0].borrow() {
        Value::Object(map) => map.clone(),
        other => {
            return Err(RuntimeError::at(
                span,
                codes::E1920_NMONGO_ARITY,
                format!(
                    "nmongo_connect() expects opts object, got {}",
                    other.type_name()
                ),
            ));
        }
    };

    let (client_opts, password) = build_client_options(&opts_map, span)?;
    let opts_clone = client_opts.clone();

    match block_on(async move { Client::with_options(client_opts) }) {
        Ok(client) => {
            let id = alloc_client(client, opts_clone);
            Ok(ok_int(id as i64))
        }
        Err(e) => Ok(nmongo_error(
            span,
            redact_secrets(&e.to_string(), password.as_deref()),
        )),
    }
}

pub fn nmongo_connect_uri(args: &[ValueRef], span: Span) -> NiaoResult<ValueRef> {
    arity(args, 1, "nmongo_connect_uri", span)?;
    let uri = string_arg(args, 0, "nmongo_connect_uri", span)?;

    let mut client_opts = block_on(async move { ClientOptions::parse(&uri).await }).map_err(|e| {
        RuntimeError::at(
            span,
            codes::E1921_NMONGO_ERROR,
            redact_secrets(&e.to_string(), None),
        )
    })?;
    if client_opts.max_pool_size.is_none() {
        client_opts.max_pool_size = Some(200);
    }
    let opts_clone = client_opts.clone();

    match block_on(async move { Client::with_options(client_opts) }) {
        Ok(client) => {
            let id = alloc_client(client, opts_clone);
            Ok(ok_int(id as i64))
        }
        Err(e) => Ok(nmongo_error(
            span,
            redact_secrets(&e.to_string(), None),
        )),
    }
}

pub fn nmongo_close(args: &[ValueRef], span: Span) -> NiaoResult<ValueRef> {
    arity(args, 1, "nmongo_close", span)?;
    let id = client_arg(args, 0, "nmongo_close", span)?;
    remove_client(id);
    Ok(ok_nil())
}

pub fn nmongo_warm_parallel_pool(args: &[ValueRef], span: Span) -> NiaoResult<ValueRef> {
    arity(args, 1, "nmongo_warm_parallel_pool", span)?;
    let id = client_arg(args, 0, "nmongo_warm_parallel_pool", span)?;
    warm_parallel_client_pool(id);
    Ok(ok_nil())
}

pub fn nmongo_ping(args: &[ValueRef], span: Span) -> NiaoResult<ValueRef> {
    arity(args, 1, "nmongo_ping", span)?;
    let id = client_arg(args, 0, "nmongo_ping", span)?;
    with_client(id, "nmongo_ping", span, |client| {
        block_on(async move {
            client
                .database("admin")
                .run_command(doc! {"ping": 1})
                .await
                .map(|_| ())
                .map_err(|e| e.to_string())
        })
    })
    .map(|_| ok_bool(true))
    .or_else(|e| Ok(crate::error_from_runtime(&e)))
}

pub fn nmongo_list_databases(args: &[ValueRef], span: Span) -> NiaoResult<ValueRef> {
    arity(args, 1, "nmongo_list_databases", span)?;
    let id = client_arg(args, 0, "nmongo_list_databases", span)?;
    with_client(id, "nmongo_list_databases", span, |client| {
        block_on(async move {
            let dbs = client.list_database_names().await.map_err(|e| e.to_string())?;
            Ok(dbs)
        })
    })
    .map(|names| {
        Value::Array(names.into_iter().map(|n| Value::String(n).ref_cell()).collect()).ref_cell()
    })
    .or_else(|e| Ok(crate::error_from_runtime(&e)))
}
