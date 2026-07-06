//! Ahiru 0.3.0 builtins — state, groups, cache, jobs, streaming, etc.

use super::app::{self, make_neko_handler};
use super::common::*;
use crate::{NativeFn, NekoResult, Value, ValueRef};
use ahiru_core::{
    mount_health_live, mount_health_ready, resource_paths, AhiruResponse, HttpMethod,
    MiddlewareEntry, MiddlewareScope, RouteMeta,
};
use neko_ast::Span;
use std::collections::HashMap;
use std::rc::Rc;

fn ahiru_app_set_state(args: &[ValueRef], span: Span) -> NekoResult<ValueRef> {
    arity_range(args, 3, 3, "ahiru_app_set_state", span)?;
    let app_id = int_arg(args, 0, "ahiru_app_set_state", span)? as u64;
    let key = string_arg(args, 1, "ahiru_app_set_state", span)?;
    let value = match &*args[2].borrow() {
        Value::String(s) => s.clone(),
        other => other.to_string(),
    };
    app::with_app_mut(app_id, "ahiru_app_set_state", span, |state| {
        state.app.state().set_string(key, value);
        Ok(())
    })?;
    ok_nil()
}

fn ahiru_app_group(args: &[ValueRef], span: Span) -> NekoResult<ValueRef> {
    arity_range(args, 2, 3, "ahiru_app_group", span)?;
    let app_id = int_arg(args, 0, "ahiru_app_group", span)? as u64;
    let prefix = string_arg(args, 1, "ahiru_app_group", span)?;
    let opts = if args.len() == 3 {
        object_arg(args, 2, "ahiru_app_group", span)?
    } else {
        HashMap::new()
    };
    let auth = opts.get("auth").and_then(|v| match &*v.borrow() {
        Value::String(s) => Some(s.clone()),
        _ => None,
    });
    let scope_id = app::with_app_mut(app_id, "ahiru_app_group", span, |state| {
        Ok(state.app.scopes_mut().create(
            prefix,
            Vec::new(),
            RouteMeta::default(),
            auth,
        ))
    })?;
    Ok(Value::Int(scope_id as i64).ref_cell())
}

fn ahiru_app_static(args: &[ValueRef], span: Span) -> NekoResult<ValueRef> {
    arity_range(args, 3, 3, "ahiru_app_static", span)?;
    let app_id = int_arg(args, 0, "ahiru_app_static", span)? as u64;
    let url_prefix = string_arg(args, 1, "ahiru_app_static", span)?;
    let dir = string_arg(args, 2, "ahiru_app_static", span)?;
    app::with_app_mut(app_id, "ahiru_app_static", span, |state| {
        state.app.mount_static(url_prefix, dir);
        Ok(())
    })?;
    ok_nil()
}

fn ahiru_app_on_error(args: &[ValueRef], span: Span) -> NekoResult<ValueRef> {
    arity_range(args, 2, 2, "ahiru_app_on_error", span)?;
    let app_id = int_arg(args, 0, "ahiru_app_on_error", span)? as u64;
    let handler = make_neko_handler(args[1].clone(), span);
    app::with_app_mut(app_id, "ahiru_app_on_error", span, |state| {
        state.app.set_error_handler(handler);
        Ok(())
    })?;
    ok_nil()
}

fn ahiru_app_not_found(args: &[ValueRef], span: Span) -> NekoResult<ValueRef> {
    arity_range(args, 2, 2, "ahiru_app_not_found", span)?;
    let app_id = int_arg(args, 0, "ahiru_app_not_found", span)? as u64;
    let handler = make_neko_handler(args[1].clone(), span);
    app::with_app_mut(app_id, "ahiru_app_not_found", span, |state| {
        state.app.set_not_found_handler(handler);
        Ok(())
    })?;
    ok_nil()
}

fn ahiru_redirect(args: &[ValueRef], span: Span) -> NekoResult<ValueRef> {
    arity_range(args, 1, 2, "ahiru_redirect", span)?;
    let url = string_arg(args, 0, "ahiru_redirect", span)?;
    let permanent = args.len() == 2
        && matches!(&*args[1].borrow(), Value::Bool(true));
    let resp = AhiruResponse::redirect(url, permanent);
    let mut map = HashMap::new();
    map.insert("status".into(), Value::Int(resp.status as i64).ref_cell());
    map.insert(
        "content_type".into(),
        Value::String(resp.content_type).ref_cell(),
    );
    map.insert("body".into(), Value::String(String::new()).ref_cell());
    map.insert(
        "redirect".into(),
        Value::String(resp.redirect_url.unwrap_or_default()).ref_cell(),
    );
    Ok(Value::Object(map).ref_cell())
}

fn ahiru_app_init_cache(args: &[ValueRef], span: Span) -> NekoResult<ValueRef> {
    arity(args, 1, "ahiru_app_init_cache", span)?;
    let app_id = int_arg(args, 0, "ahiru_app_init_cache", span)? as u64;
    app::init_cache_async(app_id, span)?;
    ok_nil()
}

fn ahiru_cache_get(args: &[ValueRef], span: Span) -> NekoResult<ValueRef> {
    arity_range(args, 2, 2, "ahiru_cache_get", span)?;
    let app_id = int_arg(args, 0, "ahiru_cache_get", span)? as u64;
    let key = string_arg(args, 1, "ahiru_cache_get", span)?;
    let cache = app::with_app(app_id, "ahiru_cache_get", span, |state| {
        state
            .cache
            .clone()
            .ok_or_else(|| "cache not initialized".to_string())
    })?;
    let rt = tokio::runtime::Runtime::new().map_err(|e| runtime_err(span, &e.to_string()))?;
    let result = rt
        .block_on(async { cache.get(&key).await })
        .unwrap_or_default();
    Ok(Value::String(result).ref_cell())
}

fn ahiru_cache_set(args: &[ValueRef], span: Span) -> NekoResult<ValueRef> {
    arity_range(args, 3, 3, "ahiru_cache_set", span)?;
    let app_id = int_arg(args, 0, "ahiru_cache_set", span)? as u64;
    let key = string_arg(args, 1, "ahiru_cache_set", span)?;
    let value = string_arg(args, 2, "ahiru_cache_set", span)?;
    let cache = app::with_app(app_id, "ahiru_cache_set", span, |state| {
        state
            .cache
            .clone()
            .ok_or_else(|| "cache not initialized".to_string())
    })?;
    let rt = tokio::runtime::Runtime::new().map_err(|e| runtime_err(span, &e.to_string()))?;
    rt.block_on(async { cache.set(&key, &value).await })
        .map_err(|e| runtime_err(span, &e))?;
    ok_nil()
}

fn ahiru_cache_incr(args: &[ValueRef], span: Span) -> NekoResult<ValueRef> {
    arity_range(args, 2, 2, "ahiru_cache_incr", span)?;
    let app_id = int_arg(args, 0, "ahiru_cache_incr", span)? as u64;
    let key = string_arg(args, 1, "ahiru_cache_incr", span)?;
    let cache = app::with_app(app_id, "ahiru_cache_incr", span, |state| {
        state
            .cache
            .clone()
            .ok_or_else(|| "cache not initialized".to_string())
    })?;
    let rt = tokio::runtime::Runtime::new().map_err(|e| runtime_err(span, &e.to_string()))?;
    let n = rt
        .block_on(async { cache.incr(&key).await })
        .map_err(|e| runtime_err(span, &e))?;
    Ok(Value::Int(n).ref_cell())
}

fn ahiru_job_enqueue(args: &[ValueRef], span: Span) -> NekoResult<ValueRef> {
    arity_range(args, 3, 3, "ahiru_job_enqueue", span)?;
    let app_id = int_arg(args, 0, "ahiru_job_enqueue", span)? as u64;
    let name = string_arg(args, 1, "ahiru_job_enqueue", span)?;
    let payload = string_arg(args, 2, "ahiru_job_enqueue", span)?;
    app::with_app(app_id, "ahiru_job_enqueue", span, |state| {
        state
            .app
            .jobs()
            .enqueue(name, payload)
            .map_err(|e| format!("E2200: {e}"))
    })?;
    ok_nil()
}

fn ahiru_app_cron(args: &[ValueRef], span: Span) -> NekoResult<ValueRef> {
    arity_range(args, 3, 3, "ahiru_app_cron", span)?;
    let app_id = int_arg(args, 0, "ahiru_app_cron", span)? as u64;
    let schedule = string_arg(args, 1, "ahiru_app_cron", span)?;
    let handler = make_neko_handler(args[2].clone(), span);
    app::with_app_mut(app_id, "ahiru_app_cron", span, |state| {
        state
            .app
            .cron()
            .register(schedule, handler)
            .map_err(|e| e.to_string())
    })?;
    ok_nil()
}

fn ahiru_ws_broadcast(args: &[ValueRef], span: Span) -> NekoResult<ValueRef> {
    arity_range(args, 3, 3, "ahiru_ws_broadcast", span)?;
    let app_id = int_arg(args, 0, "ahiru_ws_broadcast", span)? as u64;
    let room = string_arg(args, 1, "ahiru_ws_broadcast", span)?;
    let msg = string_arg(args, 2, "ahiru_ws_broadcast", span)?;
    let count = app::with_app(app_id, "ahiru_ws_broadcast", span, |state| {
        Ok(state.app.ws_hub().broadcast(&room, &msg))
    })?;
    Ok(Value::Int(count as i64).ref_cell())
}

fn ahiru_v3_mount_metrics(args: &[ValueRef], span: Span) -> NekoResult<ValueRef> {
    arity_range(args, 1, 2, "ahiru_v3_mount_metrics", span)?;
    let app_id = int_arg(args, 0, "ahiru_v3_mount_metrics", span)? as u64;
    let path = if args.len() == 2 {
        string_arg(args, 1, "ahiru_v3_mount_metrics", span)?
    } else {
        "/metrics".into()
    };
    app::with_app_mut(app_id, "ahiru_v3_mount_metrics", span, |state| {
        state.app.mount_metrics(path);
        Ok(())
    })?;
    ok_nil()
}

fn ahiru_v3_mount_health(args: &[ValueRef], span: Span) -> NekoResult<ValueRef> {
    arity_range(args, 1, 2, "ahiru_v3_mount_health", span)?;
    let app_id = int_arg(args, 0, "ahiru_v3_mount_health", span)? as u64;
    let live_path = if args.len() == 2 {
        string_arg(args, 1, "ahiru_v3_mount_health", span)?
    } else {
        "/health".into()
    };
    let ready_path = format!("{live_path}/ready");
    let live = format!("{live_path}/live");
    app::with_app_mut(app_id, "ahiru_v3_mount_health", span, |state| {
        let db = state.db.clone();
        let cache = state.cache.clone();
        state.app.route(
            HttpMethod::Get,
            &live,
            mount_health_live(),
            RouteMeta {
                public: true,
                ..Default::default()
            },
        );
        state.app.route(
            HttpMethod::Get,
            &ready_path,
            mount_health_ready(db, cache),
            RouteMeta {
                public: true,
                ..Default::default()
            },
        );
        Ok(())
    })?;
    ok_nil()
}

fn ahiru_app_resource(args: &[ValueRef], span: Span) -> NekoResult<ValueRef> {
    arity_range(args, 3, 3, "ahiru_app_resource", span)?;
    let app_id = int_arg(args, 0, "ahiru_app_resource", span)? as u64;
    let base = string_arg(args, 1, "ahiru_app_resource", span)?;
    let handlers = object_arg(args, 2, "ahiru_app_resource", span)?;
    let (collection, member) = resource_paths(&base);
    let register = |key: &str, method: HttpMethod, path: &str| -> NekoResult<()> {
        if let Some(h) = handlers.get(key) {
            let handler = make_neko_handler(h.clone(), span);
            app::with_app_mut(app_id, "ahiru_app_resource", span, |state| {
                state.app.route(method, path, handler, RouteMeta::default());
                Ok(())
            })?;
        }
        Ok(())
    };
    register("index", HttpMethod::Get, &collection)?;
    register("show", HttpMethod::Get, &member)?;
    register("create", HttpMethod::Post, &collection)?;
    register("update", HttpMethod::Put, &member)?;
    register("destroy", HttpMethod::Delete, &member)?;
    ok_nil()
}

fn ahiru_db_query(args: &[ValueRef], span: Span) -> NekoResult<ValueRef> {
    arity_range(args, 3, 3, "ahiru_db_query", span)?;
    let app_id = int_arg(args, 0, "ahiru_db_query", span)? as u64;
    let db_name = string_arg(args, 1, "ahiru_db_query", span)?;
    let sql = string_arg(args, 2, "ahiru_db_query", span)?;
    let rows = app::with_app(app_id, "ahiru_db_query", span, |state| {
        let db = state
            .db
            .as_ref()
            .ok_or_else(|| "database not initialized".to_string())?;
        db.query_sqlite(&db_name, &sql)
    })?;
    let items: Vec<ValueRef> = rows
        .iter()
        .map(|row| {
            let mut m = HashMap::new();
            for (k, v) in row {
                m.insert(k.clone(), Value::String(v.clone()).ref_cell());
            }
            Value::Object(m).ref_cell()
        })
        .collect();
    Ok(Value::Array(items).ref_cell())
}

pub fn ahiru_app_use_custom(args: &[ValueRef], span: Span) -> NekoResult<ValueRef> {
    arity_range(args, 2, 3, "ahiru_app_use", span)?;
    let app_id = int_arg(args, 0, "ahiru_app_use", span)? as u64;
    if let Value::Function(_) = &*args[1].borrow() {
        let handler = make_neko_handler(args[1].clone(), span);
        let opts = if args.len() == 3 {
            object_arg(args, 2, "ahiru_app_use", span)?
        } else {
            HashMap::new()
        };
        let order = opts
            .get("order")
            .and_then(|v| match &*v.borrow() {
                Value::Int(n) => Some(*n as i32),
                _ => None,
            })
            .unwrap_or(100);
        let mut scope = MiddlewareScope::default();
        if let Some(only) = opts.get("only") {
            if let Value::Array(items) = &*only.borrow() {
                scope.only = items
                    .iter()
                    .filter_map(|i| match &*i.borrow() {
                        Value::String(s) => Some(s.clone()),
                        _ => None,
                    })
                    .collect();
            }
        }
        app::with_app_mut(app_id, "ahiru_app_use", span, |state| {
            state
                .app
                .use_middleware(MiddlewareEntry::custom(handler, order, scope));
            Ok(())
        })?;
        return ok_nil();
    }
    Err(runtime_err(
        span,
        "use ahiru_app_use with string name for built-in middleware",
    ))
}

pub fn v3_builtins() -> Vec<(&'static str, NativeFn)> {
    vec![
        ("ahiru_app_set_state", Rc::new(ahiru_app_set_state)),
        ("ahiru_app_group", Rc::new(ahiru_app_group)),
        ("ahiru_app_static", Rc::new(ahiru_app_static)),
        ("ahiru_app_on_error", Rc::new(ahiru_app_on_error)),
        ("ahiru_app_not_found", Rc::new(ahiru_app_not_found)),
        ("ahiru_redirect", Rc::new(ahiru_redirect)),
        ("ahiru_app_init_cache", Rc::new(ahiru_app_init_cache)),
        ("ahiru_cache_get", Rc::new(ahiru_cache_get)),
        ("ahiru_cache_set", Rc::new(ahiru_cache_set)),
        ("ahiru_cache_incr", Rc::new(ahiru_cache_incr)),
        ("ahiru_job_enqueue", Rc::new(ahiru_job_enqueue)),
        ("ahiru_app_cron", Rc::new(ahiru_app_cron)),
        ("ahiru_ws_broadcast", Rc::new(ahiru_ws_broadcast)),
        ("ahiru_v3_mount_metrics", Rc::new(ahiru_v3_mount_metrics)),
        ("ahiru_v3_mount_health", Rc::new(ahiru_v3_mount_health)),
        ("ahiru_app_resource", Rc::new(ahiru_app_resource)),
        ("ahiru_db_query", Rc::new(ahiru_db_query)),
    ]
}
