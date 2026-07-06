//! ahiru-server native builtins — high-performance HTTP/WebSocket framework.

mod app;
mod common;
pub mod ctx_bridge;
mod options;
mod pool;
pub(crate) mod stream;
mod v3_builtins;
mod vm_pool;
mod vm_serve;

pub use app::{
    ahiru_serve_blocking, finalize_vm_handlers, handler_vm_index, mount_native_health,
    mount_native_ping, queue_pending_serve, response_from_neko, start_pending_server, AppHandle,
};
pub use vm_pool::{clear_vm_pool, install_vm_pool, vm_pool_active, VmPoolDispatchFn};
pub use vm_serve::{clear_vm_serve, set_vm_serve_active, vm_serve_active};

pub use options::{apply_cli_port, mark_explicit_port, serve_options, set_serve_options};

use crate::{NativeFn, NekoResult, Value, ValueRef};
use common::*;
use neko_ast::Span;
use std::collections::HashMap;
use std::rc::Rc;

pub const MODULE_NAME: &str = "ahiru";
pub const MODULE_PATHS: &[&str] = &["ahiru", "std/ahiru"];

fn ahiru_app_new(args: &[ValueRef], span: Span) -> NekoResult<ValueRef> {
    arity_range(args, 0, 1, "ahiru_app_new", span)?;
    let config = if args.is_empty() {
        ahiru_core::AhiruConfig::default()
    } else {
        match &*args[0].borrow() {
            Value::Object(map) => object_to_config(map, span)?,
            Value::String(path) => {
                ahiru_core::AhiruConfig::from_file(std::path::Path::new(path))
                    .map_err(|e| runtime_err(span, &e))?
            }
            other => {
                return Err(type_err(
                    span,
                    format!(
                        "ahiru_app_new() expects config object or path string, got {}",
                        other.type_name()
                    ),
                ));
            }
        }
    };
    options::set_native_routes(config.server.native_routes);
    let id = app::alloc_app(ahiru_core::AhiruApp::new(config));
    Ok(Value::Int(id as i64).ref_cell())
}

fn ahiru_app_from_config(args: &[ValueRef], span: Span) -> NekoResult<ValueRef> {
    arity(args, 1, "ahiru_app_from_config", span)?;
    let path = string_arg(args, 0, "ahiru_app_from_config", span)?;
    let config = ahiru_core::AhiruConfig::from_file(std::path::Path::new(&path))
        .map_err(|e| runtime_err(span, &e))?;
    options::set_native_routes(config.server.native_routes);
    let id = app::alloc_app(ahiru_core::AhiruApp::new(config));
    Ok(Value::Int(id as i64).ref_cell())
}

fn register_route(
    args: &[ValueRef],
    span: Span,
    method: ahiru_core::HttpMethod,
    name: &str,
) -> NekoResult<ValueRef> {
    arity_range(args, 3, 4, name, span)?;
    let app_id = int_arg(args, 0, name, span)? as u64;
    let path = string_arg(args, 1, name, span)?;
    let handler = args[2].clone();
    let mut meta = if args.len() == 4 {
        route_meta_from_opts(&args[3], span)?
    } else {
        ahiru_core::RouteMeta::default()
    };
    if args.len() == 4 {
        if let Value::Object(map) = &*args[3].borrow() {
            if let Some(schema) = map.get("schema") {
                meta.schema = Some(app::make_neko_handler(schema.clone(), span));
            }
            if let Some(guard) = map.get("auth").or_else(|| map.get("guard")) {
                if matches!(&*guard.borrow(), Value::Function(_)) {
                    meta.guard = Some(app::make_neko_handler(guard.clone(), span));
                }
            }
        }
    }
    let handler_fn = app::make_neko_handler(handler, span);
    app::with_app_mut(app_id, name, span, |state| {
        state
            .app
            .route(method, &path, handler_fn, meta);
        Ok(())
    })?;
    ok_nil()
}

fn ahiru_app_get(args: &[ValueRef], span: Span) -> NekoResult<ValueRef> {
    register_route(args, span, ahiru_core::HttpMethod::Get, "ahiru_app_get")
}

fn ahiru_app_post(args: &[ValueRef], span: Span) -> NekoResult<ValueRef> {
    register_route(args, span, ahiru_core::HttpMethod::Post, "ahiru_app_post")
}

fn ahiru_app_put(args: &[ValueRef], span: Span) -> NekoResult<ValueRef> {
    register_route(args, span, ahiru_core::HttpMethod::Put, "ahiru_app_put")
}

fn ahiru_app_delete(args: &[ValueRef], span: Span) -> NekoResult<ValueRef> {
    register_route(args, span, ahiru_core::HttpMethod::Delete, "ahiru_app_delete")
}

fn ahiru_app_patch(args: &[ValueRef], span: Span) -> NekoResult<ValueRef> {
    register_route(args, span, ahiru_core::HttpMethod::Patch, "ahiru_app_patch")
}

fn ahiru_app_ws(args: &[ValueRef], span: Span) -> NekoResult<ValueRef> {
    arity_range(args, 3, 4, "ahiru_app_ws", span)?;
    let app_id = int_arg(args, 0, "ahiru_app_ws", span)? as u64;
    let path = string_arg(args, 1, "ahiru_app_ws", span)?;
    let handler = args[2].clone();
    let meta = if args.len() == 4 {
        route_meta_from_opts(&args[3], span)?
    } else {
        ahiru_core::RouteMeta {
            ws: true,
            ..Default::default()
        }
    };
    let ws_fn = app::make_neko_ws_handler(handler, span);
    app::with_app_mut(app_id, "ahiru_app_ws", span, |state| {
        state.app.route_ws(&path, ws_fn, meta);
        Ok(())
    })?;
    ok_nil()
}

fn ahiru_app_use(args: &[ValueRef], span: Span) -> NekoResult<ValueRef> {
  if args.len() >= 2 {
    if let Value::Function(_) = &*args[1].borrow() {
      return v3_builtins::ahiru_app_use_custom(args, span);
    }
  }
    arity_range(args, 2, 3, "ahiru_app_use", span)?;
    let app_id = int_arg(args, 0, "ahiru_app_use", span)? as u64;
    let mw_name = string_arg(args, 1, "ahiru_app_use", span)?;
    let opts = if args.len() == 3 {
        object_arg(args, 2, "ahiru_app_use", span)?
    } else {
        HashMap::new()
    };
    let mw = middleware_from_name(&mw_name, &opts, span)?;
    app::with_app_mut(app_id, "ahiru_app_use", span, |state| {
        state.app.use_middleware(ahiru_core::MiddlewareEntry::builtin(mw));
        Ok(())
    })?;
    ok_nil()
}

fn ahiru_app_listen(args: &[ValueRef], span: Span) -> NekoResult<ValueRef> {
    arity_range(args, 1, 3, "ahiru_app_listen", span)?;
    let app_id = int_arg(args, 0, "ahiru_app_listen", span)? as u64;
    app::with_app_mut(app_id, "ahiru_app_listen", span, |state| {
        let opts = options::serve_options();
        if let Some(p) = opts.cli_port {
            state.app.config.server.port = p;
        }
        if args.len() >= 2 {
            if let Value::String(h) = &*args[1].borrow() {
                state.app.config.server.host = h.clone();
            }
        }
        if args.len() >= 3 {
            if let Value::Int(p) = &*args[2].borrow() {
                state.app.config.server.port = *p as u16;
                options::mark_explicit_port();
            }
        }
        Ok(())
    })?;
    app::queue_pending_serve(app_id, span);
    ok_nil()
}

fn ahiru_app_routes(args: &[ValueRef], span: Span) -> NekoResult<ValueRef> {
    arity(args, 1, "ahiru_app_routes", span)?;
    let app_id = int_arg(args, 0, "ahiru_app_routes", span)? as u64;
    let routes = app::with_app(app_id, "ahiru_app_routes", span, |state| {
        Ok(state.app.list_routes())
    })?;
    let items: Vec<ValueRef> = routes
        .iter()
        .map(|r| {
            let mut m = HashMap::new();
            m.insert(
                "method".into(),
                Value::String(r.method.clone()).ref_cell(),
            );
            m.insert("path".into(), Value::String(r.path.clone()).ref_cell());
            if let Some(p) = &r.permission {
                m.insert("permission".into(), Value::String(p.clone()).ref_cell());
            }
            m.insert(
                "websocket".into(),
                Value::Bool(r.websocket).ref_cell(),
            );
            Value::Object(m).ref_cell()
        })
        .collect();
    Ok(Value::Array(items).ref_cell())
}

fn ahiru_response(args: &[ValueRef], span: Span) -> NekoResult<ValueRef> {
    arity_range(args, 2, 3, "ahiru_response", span)?;
    let status = int_arg(args, 0, "ahiru_response", span)? as u16;
    let content_type = string_arg(args, 1, "ahiru_response", span)?;
    let body = if args.len() == 3 {
        match &*args[2].borrow() {
            Value::String(s) => s.clone(),
            other => other.to_string(),
        }
    } else {
        String::new()
    };
    let mut map = HashMap::new();
    map.insert("status".into(), Value::Int(status as i64).ref_cell());
    map.insert(
        "content_type".into(),
        Value::String(content_type).ref_cell(),
    );
    map.insert("body".into(), Value::String(body).ref_cell());
    Ok(Value::Object(map).ref_cell())
}

fn ahiru_sse_start(args: &[ValueRef], span: Span) -> NekoResult<ValueRef> {
    arity_range(args, 0, 1, "ahiru_sse_start", span)?;
    let status = if args.is_empty() {
        200
    } else {
        int_arg(args, 0, "ahiru_sse_start", span)? as u16
    };
    let handle = stream::sse_start(status);
    let mut map = HashMap::new();
    map.insert("status".into(), Value::Int(status as i64).ref_cell());
    map.insert(
        "content_type".into(),
        Value::String("text/event-stream".into()).ref_cell(),
    );
    map.insert("stream_handle".into(), Value::Int(handle as i64).ref_cell());
    map.insert("body".into(), Value::String(String::new()).ref_cell());
    Ok(Value::Object(map).ref_cell())
}

fn ahiru_sse_write(args: &[ValueRef], span: Span) -> NekoResult<ValueRef> {
    arity(args, 2, "ahiru_sse_write", span)?;
    let handle = match &*args[0].borrow() {
        Value::Int(n) => *n as u64,
        Value::Object(map) => map
            .get("stream_handle")
            .and_then(|v| match &*v.borrow() {
                Value::Int(n) => Some(*n as u64),
                _ => None,
            })
            .ok_or_else(|| {
                runtime_err(span, "ahiru_sse_write(): expected stream handle or response object")
            })?,
        other => {
            return Err(type_err(
                span,
                format!(
                    "ahiru_sse_write() expects stream handle or object, got {}",
                    other.type_name()
                ),
            ))
        }
    };
    let chunk = string_arg(args, 1, "ahiru_sse_write", span)?;
    stream::sse_write(handle, &chunk).map_err(|e| runtime_err(span, &e))?;
    Ok(Value::Bool(true).ref_cell())
}

fn ahiru_json_response(args: &[ValueRef], span: Span) -> NekoResult<ValueRef> {
    arity_range(args, 1, 2, "ahiru_json_response", span)?;
    let status = if args.len() == 2 {
        int_arg(args, 0, "ahiru_json_response", span)? as u16
    } else {
        200
    };
    let json_idx = if args.len() == 2 { 1 } else { 0 };
    let body = match &*args[json_idx].borrow() {
        Value::String(s) => s.clone(),
        other => other.to_string(),
    };
    let mut map = HashMap::new();
    map.insert("status".into(), Value::Int(status as i64).ref_cell());
    map.insert(
        "content_type".into(),
        Value::String("application/json".into()).ref_cell(),
    );
    map.insert("body".into(), Value::String(body).ref_cell());
    Ok(Value::Object(map).ref_cell())
}

fn ahiru_db_exec(args: &[ValueRef], span: Span) -> NekoResult<ValueRef> {
    arity_range(args, 2, 3, "ahiru_db_exec", span)?;
    let app_id = int_arg(args, 0, "ahiru_db_exec", span)? as u64;
    let db_name = string_arg(args, 1, "ahiru_db_exec", span)?;
    let sql = string_arg(args, 2, "ahiru_db_exec", span)?;
    let rows = app::with_app(app_id, "ahiru_db_exec", span, |state| {
        let db = state
            .db
            .as_ref()
            .ok_or_else(|| "database not initialized — call ahiru_app_init_db first".to_string())?;
        db.exec_sqlite(&db_name, &sql)
    })?;
    Ok(Value::Int(rows as i64).ref_cell())
}

fn ahiru_app_set_logging(args: &[ValueRef], span: Span) -> NekoResult<ValueRef> {
    arity_range(args, 1, 2, "ahiru_app_set_logging", span)?;
    let app_id = int_arg(args, 0, "ahiru_app_set_logging", span)? as u64;
    let opts = if args.len() == 2 {
        object_arg(args, 1, "ahiru_app_set_logging", span)?
    } else {
        HashMap::new()
    };
    let access_log = opts.get("access_log").and_then(|v| match &*v.borrow() {
        Value::Bool(b) => Some(*b),
        _ => None,
    });
    let json_logs = opts.get("json_logs").or_else(|| opts.get("json")).and_then(|v| match &*v.borrow() {
        Value::Bool(b) => Some(*b),
        _ => None,
    });
    let startup_banner = opts.get("startup_banner").and_then(|v| match &*v.borrow() {
        Value::Bool(b) => Some(*b),
        _ => None,
    });
    let quiet_handlers = opts.get("quiet_handlers").or_else(|| opts.get("quiet")).and_then(|v| match &*v.borrow() {
        Value::Bool(b) => Some(*b),
        _ => None,
    });
    let request_id = opts.get("request_id").and_then(|v| match &*v.borrow() {
        Value::Bool(b) => Some(*b),
        _ => None,
    });
    let slow_request_ms = opts.get("slow_request_ms").and_then(|v| match &*v.borrow() {
        Value::Int(n) => Some(*n as f64),
        _ => None,
    });
    let skip_paths = opts.get("skip_paths").or_else(|| opts.get("skip")).and_then(|v| match &*v.borrow() {
        Value::Array(items) => Some(
            items
                .iter()
                .filter_map(|i| match &*i.borrow() {
                    Value::String(s) => Some(s.clone()),
                    _ => None,
                })
                .collect::<Vec<_>>(),
        ),
        _ => None,
    });
    app::with_app_mut(app_id, "ahiru_app_set_logging", span, |state| {
        state.app.logging().apply_runtime_opts(
            access_log,
            json_logs,
            startup_banner,
            quiet_handlers,
            request_id,
            slow_request_ms,
            skip_paths,
        );
        Ok(())
    })?;
    ok_nil()
}

fn ahiru_app_init_db(args: &[ValueRef], span: Span) -> NekoResult<ValueRef> {
    arity(args, 1, "ahiru_app_init_db", span)?;
    let app_id = int_arg(args, 0, "ahiru_app_init_db", span)? as u64;
    app::init_db_async(app_id, span)?;
    ok_nil()
}

fn ahiru_native_routes(_args: &[ValueRef], _span: Span) -> NekoResult<ValueRef> {
    Ok(Value::Bool(options::native_routes_enabled()).ref_cell())
}

fn ahiru_native_mount_health(args: &[ValueRef], span: Span) -> NekoResult<ValueRef> {
    arity_range(args, 2, 3, "ahiru_native_mount_health", span)?;
    let app_id = int_arg(args, 0, "ahiru_native_mount_health", span)? as u64;
    let path = string_arg(args, 1, "ahiru_native_mount_health", span)?;
    let path = if path.is_empty() { "/health".into() } else { path };
    app::mount_native_health(app_id, &path, span)?;
    ok_nil()
}

fn ahiru_native_mount_ping(args: &[ValueRef], span: Span) -> NekoResult<ValueRef> {
    arity_range(args, 2, 3, "ahiru_native_mount_ping", span)?;
    let app_id = int_arg(args, 0, "ahiru_native_mount_ping", span)? as u64;
    let path = string_arg(args, 1, "ahiru_native_mount_ping", span)?;
    let path = if path.is_empty() { "/ping".into() } else { path };
    app::mount_native_ping(app_id, &path, span)?;
    ok_nil()
}

pub fn builtins() -> Vec<(&'static str, NativeFn)> {
    let mut list: Vec<(&'static str, NativeFn)> = vec![
        ("ahiru_app_new", Rc::new(ahiru_app_new)),
        ("ahiru_app_from_config", Rc::new(ahiru_app_from_config)),
        ("ahiru_app_get", Rc::new(ahiru_app_get)),
        ("ahiru_app_post", Rc::new(ahiru_app_post)),
        ("ahiru_app_put", Rc::new(ahiru_app_put)),
        ("ahiru_app_delete", Rc::new(ahiru_app_delete)),
        ("ahiru_app_patch", Rc::new(ahiru_app_patch)),
        ("ahiru_app_ws", Rc::new(ahiru_app_ws)),
        ("ahiru_app_use", Rc::new(ahiru_app_use)),
        ("ahiru_app_set_logging", Rc::new(ahiru_app_set_logging)),
        ("ahiru_app_listen", Rc::new(ahiru_app_listen)),
        ("ahiru_app_routes", Rc::new(ahiru_app_routes)),
        ("ahiru_app_init_db", Rc::new(ahiru_app_init_db)),
        ("ahiru_native_routes", Rc::new(ahiru_native_routes)),
        ("ahiru_native_mount_health", Rc::new(ahiru_native_mount_health)),
        ("ahiru_native_mount_ping", Rc::new(ahiru_native_mount_ping)),
        ("ahiru_response", Rc::new(ahiru_response)),
        ("ahiru_sse_start", Rc::new(ahiru_sse_start)),
        ("ahiru_sse_write", Rc::new(ahiru_sse_write)),
        ("ahiru_json_response", Rc::new(ahiru_json_response)),
        ("ahiru_db_exec", Rc::new(ahiru_db_exec)),
    ];
    list.extend(v3_builtins::v3_builtins().into_iter());
    list
}

pub fn namespace() -> Value {
    let mut map = HashMap::new();
    for (name, func) in builtins() {
        map.insert(
            name.strip_prefix("ahiru_").unwrap_or(name).into(),
            Value::NativeFunction(func).ref_cell(),
        );
    }
    Value::Object(map)
}
