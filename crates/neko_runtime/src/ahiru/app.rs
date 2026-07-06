//! App handle table and Neko handler bridge.

use super::common::runtime_err;
use super::ctx_bridge;
use super::options::serve_options;
use super::pool::{clear_pool, global_pool, install_pool, HandlerWorkerPool};
use super::vm_pool::{clear_vm_pool, vm_pool_active, vm_pool_dispatch};
use crate::{call_neko_function, quiet_output, resolve_neko_function_by_name, set_quiet_output, Value, ValueRef};
use ahiru_core::{native_health_handler, native_ping_handler, AhiruApp, AhiruResponse, HandlerFn, HttpMethod,
    RequestContext, ResponseBody, RouteMeta, SharedDbManager, WsHandlerFn};
use neko_ast::Span;
use neko_errors::codes;
use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};

thread_local! {
    static APPS: RefCell<HashMap<u64, AppState>> = RefCell::new(HashMap::new());
    static NEXT_APP: RefCell<u64> = const { RefCell::new(1) };
    static HANDLER_REGISTRY: RefCell<HandlerRegistry> = RefCell::new(HandlerRegistry::new());
}

static GLOBAL_HANDLER_META: OnceLock<Mutex<HashMap<u64, (String, Span)>>> = OnceLock::new();

fn global_handler_meta() -> &'static Mutex<HashMap<u64, (String, Span)>> {
    GLOBAL_HANDLER_META.get_or_init(|| Mutex::new(HashMap::new()))
}

struct HandlerEntry {
    handler: ValueRef,
    span: Span,
    fn_name: String,
    vm_index: Option<u32>,
}

struct HandlerRegistry {
    next: u64,
    handlers: HashMap<u64, HandlerEntry>,
}

impl HandlerRegistry {
    fn new() -> Self {
        Self {
            next: 1,
            handlers: HashMap::new(),
        }
    }

    fn register(&mut self, handler: ValueRef, span: Span) -> u64 {
        let fn_name = match &*handler.borrow() {
            Value::Function(f) => f.def.name.clone(),
            _ => String::new(),
        };
        let id = self.next;
        self.next += 1;
        self.handlers.insert(
            id,
            HandlerEntry {
                handler,
                span,
                fn_name: fn_name.clone(),
                vm_index: None,
            },
        );
        if !fn_name.is_empty() {
            global_handler_meta()
                .lock()
                .unwrap()
                .insert(id, (fn_name, span));
        }
        id
    }

    fn get(&self, id: u64) -> Option<(ValueRef, Span)> {
        self.handlers
            .get(&id)
            .map(|e| (e.handler.clone(), e.span))
    }

    fn vm_index(&self, id: u64) -> Option<u32> {
        self.handlers.get(&id).and_then(|e| e.vm_index)
    }

    fn apply_vm_indices(&mut self, resolve: &dyn Fn(&str) -> Option<u32>) {
        for entry in self.handlers.values_mut() {
            if !entry.fn_name.is_empty() {
                entry.vm_index = resolve(&entry.fn_name);
            }
        }
    }
}

fn with_handlers_mut<F, R>(f: F) -> R
where
    F: FnOnce(&mut HandlerRegistry) -> R,
{
    HANDLER_REGISTRY.with(|r| f(&mut r.borrow_mut()))
}

fn with_handlers<F, R>(f: F) -> R
where
    F: FnOnce(&HandlerRegistry) -> R,
{
    HANDLER_REGISTRY.with(|r| f(&r.borrow()))
}

fn resolve_handler(handler_id: u64) -> Result<(ValueRef, Span), String> {
    if let Some(pair) = with_handlers(|r| r.get(handler_id)) {
        return Ok(pair);
    }
    let (name, span) = global_handler_meta()
        .lock()
        .unwrap()
        .get(&handler_id)
        .cloned()
        .ok_or_else(|| "handler not found".to_string())?;
    let handler = resolve_neko_function_by_name(&name)
        .ok_or_else(|| format!("handler {name} not resolved"))?;
    Ok((handler, span))
}

pub struct AppState {
    pub app: AhiruApp,
    pub db: Option<SharedDbManager>,
    pub cache: Option<ahiru_core::SharedCacheManager>,
}

pub type AppHandle = u64;

pub fn alloc_app(app: AhiruApp) -> AppHandle {
    let id = NEXT_APP.with(|n| {
        let mut next = n.borrow_mut();
        let id = *next;
        *next = id + 1;
        id
    });
    APPS.with(|m| {
        m.borrow_mut().insert(
            id,
            AppState {
                app,
                db: None,
                cache: None,
            },
        );
    });
    id
}

pub fn take_app(id: AppHandle) -> Option<AppState> {
    APPS.with(|m| m.borrow_mut().remove(&id))
}

pub fn with_app_mut<F, R>(id: AppHandle, name: &str, span: Span, f: F) -> Result<R, crate::RuntimeError>
where
    F: FnOnce(&mut AppState) -> Result<R, String>,
{
    APPS.with(|m| {
        let mut guard = m.borrow_mut();
        let state = guard.get_mut(&id).ok_or_else(|| {
            crate::RuntimeError::at(
                span,
                codes::E2102_AHIRU_INVALID_HANDLE,
                format!("{name}(): invalid app handle {id}"),
            )
        })?;
        f(state).map_err(|msg| runtime_err(span, &msg))
    })
}

pub fn with_app<F, R>(id: AppHandle, name: &str, span: Span, f: F) -> Result<R, crate::RuntimeError>
where
    F: FnOnce(&AppState) -> Result<R, String>,
{
    APPS.with(|m| {
        let guard = m.borrow();
        let state = guard.get(&id).ok_or_else(|| {
            crate::RuntimeError::at(
                span,
                codes::E2102_AHIRU_INVALID_HANDLE,
                format!("{name}(): invalid app handle {id}"),
            )
        })?;
        f(state).map_err(|msg| runtime_err(span, &msg))
    })
}

pub fn init_db_async(id: AppHandle, span: Span) -> Result<(), crate::RuntimeError> {
    let rt = tokio::runtime::Runtime::new().map_err(|e| runtime_err(span, &e.to_string()))?;
    with_app_mut(id, "ahiru_app_init_db", span, |state| {
        rt.block_on(async {
            state.app.init_db().await?;
            state.db = state.app.db();
            Ok(())
        })
    })
}

pub fn init_cache_async(id: AppHandle, span: Span) -> Result<(), crate::RuntimeError> {
    let rt = tokio::runtime::Runtime::new().map_err(|e| runtime_err(span, &e.to_string()))?;
    with_app_mut(id, "ahiru_app_init_cache", span, |state| {
        rt.block_on(async {
            state.app.init_cache().await?;
            state.cache = state.app.cache();
            Ok(())
        })
    })
}

/// Resolve bytecode function indices after VM startup.
pub fn finalize_vm_handlers(resolve: &dyn Fn(&str) -> Option<u32>) {
    with_handlers_mut(|r| r.apply_vm_indices(resolve));
}

pub fn handler_vm_index(handler_id: u64) -> Option<u32> {
    with_handlers(|r| r.vm_index(handler_id))
}

pub fn response_from_neko(val: &Value) -> Result<AhiruResponse, String> {
    neko_to_response(val)
}

pub fn mount_native_health(app_id: AppHandle, path: &str, span: Span) -> Result<(), crate::RuntimeError> {
    with_app_mut(app_id, "ahiru_native_mount_health", span, |state| {
        state.app.route(
            HttpMethod::Get,
            path,
            native_health_handler(),
            RouteMeta {
                public: true,
                ..Default::default()
            },
        );
        Ok(())
    })
}

pub fn mount_native_ping(app_id: AppHandle, path: &str, span: Span) -> Result<(), crate::RuntimeError> {
    with_app_mut(app_id, "ahiru_native_mount_ping", span, |state| {
        state.app.route(
            HttpMethod::Get,
            path,
            native_ping_handler(),
            RouteMeta {
                public: true,
                ..Default::default()
            },
        );
        Ok(())
    })
}

fn invoke_sync(handler_id: u64, mut ctx: RequestContext, quiet: bool) -> Result<AhiruResponse, String> {
    let (handler, span) = resolve_handler(handler_id)?;
    let prev_quiet = quiet_output();
    if quiet {
        set_quiet_output(true);
    }
    let result = (|| {
        let req = ctx_bridge::ctx_to_neko(&mut ctx);
        let result = call_neko_function(handler, &[req], span).map_err(|e| e.to_string())?;
        let borrowed = result.borrow();
        neko_to_response(&borrowed)
    })();
    if quiet {
        set_quiet_output(prev_quiet);
    }
    result
}

fn invoke_by_id(handler_id: u64, ctx: RequestContext, quiet: bool) -> Result<AhiruResponse, String> {
    if vm_pool_active() {
        let vm_index = with_handlers(|r| r.vm_index(handler_id));
        let fields = ctx_bridge::ctx_to_fields(&ctx);
        return vm_pool_dispatch(handler_id, vm_index, fields, quiet);
    }
    if let Some(pool) = global_pool() {
        if pool.workers() > 1 {
            let fields = ctx_bridge::ctx_to_fields(&ctx);
            return pool.dispatch(handler_id, fields);
        }
    }
    invoke_sync(handler_id, ctx, quiet)
}

pub fn make_neko_handler(handler: ValueRef, span: Span) -> HandlerFn {
    let handler_id = with_handlers_mut(|r| r.register(handler, span));
    Arc::new(move |ctx| {
        let handler_id = handler_id;
        let quiet = ctx.extra.get("quiet_handlers").map(|s| s == "1").unwrap_or(false);
        if vm_pool_active() {
            let vm_index = with_handlers(|r| r.vm_index(handler_id));
            return Box::pin(async move {
                tokio::task::spawn_blocking(move || {
                    std::thread::spawn(move || {
                        let fields = ctx_bridge::ctx_to_fields(&ctx);
                        vm_pool_dispatch(handler_id, vm_index, fields, quiet)
                    })
                    .join()
                    .map_err(|_| "handler thread panicked".to_string())?
                })
                .await
                .map_err(|e| e.to_string())?
            });
        }
        // Interpreter / thread-local handlers: run off the Tokio worker (nmongo block_on).
        Box::pin(async move {
            tokio::task::spawn_blocking(move || {
                std::thread::spawn(move || invoke_by_id(handler_id, ctx, quiet))
                    .join()
                    .map_err(|_| "handler thread panicked".to_string())?
            })
            .await
            .map_err(|e| e.to_string())?
        })
    })
}

pub fn make_neko_ws_handler(handler: ValueRef, span: Span) -> WsHandlerFn {
    let handler_id = with_handlers_mut(|r| r.register(handler, span));
    Arc::new(move |ctx, sink| {
        let handler_id = handler_id;
        Box::pin(async move {
            if let Ok((handler, span)) = resolve_handler(handler_id) {
                let mut ctx_mut = ctx;
                let req = ctx_bridge::ctx_to_neko(&mut ctx_mut);
                let _ = call_neko_function(handler, &[req], span);
            }
            let _ = (sink.send)("{\"event\":\"connected\"}".into()).await;
        })
    })
}

fn object_field<'a>(val: &'a Value, key: &str) -> Option<&'a ValueRef> {
    match val {
        Value::Object(map) => map.get(key),
        _ => None,
    }
}

fn neko_to_response(val: &Value) -> Result<AhiruResponse, String> {
    if let Some(handle) = super::stream::is_stream_handle(val) {
        if let Some(resp) = super::stream::take_response(handle) {
            return Ok(resp);
        }
        return Err(format!("invalid or closed stream handle {handle}"));
    }

    let status = object_field(val, "status")
        .and_then(|v| match &*v.borrow() {
            Value::Int(n) => Some(*n as u16),
            _ => None,
        })
        .unwrap_or(200);
    let content_type = object_field(val, "content_type")
        .and_then(|v| match &*v.borrow() {
            Value::String(s) => Some(s.clone()),
            _ => None,
        })
        .unwrap_or_else(|| "text/plain; charset=utf-8".into());
    let body = object_field(val, "body")
        .and_then(|v| match &*v.borrow() {
            Value::String(s) => Some(s.as_bytes().to_vec()),
            _ => None,
        })
        .unwrap_or_default();
    Ok(AhiruResponse {
        status,
        content_type,
        body: ResponseBody::Buffered(body),
        headers: HashMap::new(),
        redirect_url: object_field(val, "redirect")
            .or_else(|| object_field(val, "location"))
            .and_then(|v| match &*v.borrow() {
                Value::String(s) => Some(s.clone()),
                _ => None,
            }),
    })
}

static PENDING_SERVE: Mutex<Option<(AppHandle, Span)>> = Mutex::new(None);

pub fn queue_pending_serve(id: AppHandle, span: Span) {
    *PENDING_SERVE.lock().unwrap() = Some((id, span));
}

pub fn start_pending_server() -> Result<(), crate::RuntimeError> {
    let pending = PENDING_SERVE.lock().unwrap().take();
    if let Some((id, span)) = pending {
        ahiru_serve_blocking(id, span)
    } else {
        Ok(())
    }
}

fn pool_invoke(handler_id: u64, fields: HashMap<String, String>) -> Result<AhiruResponse, String> {
    let (handler, span) = resolve_handler(handler_id)?;
    let req = ctx_bridge::fields_to_neko(&fields);
    let result = call_neko_function(handler, &[req], span).map_err(|e| e.to_string())?;
    let borrowed = result.borrow();
    neko_to_response(&borrowed)
}

pub fn ahiru_serve_blocking(id: AppHandle, span: Span) -> Result<(), crate::RuntimeError> {
    let state = take_app(id).ok_or_else(|| {
        crate::RuntimeError::at(
            span,
            codes::E2102_AHIRU_INVALID_HANDLE,
            format!("ahiru_app_listen(): invalid app handle {id}"),
        )
    })?;

    let opts = serve_options();
    let workers = state.app.worker_count();

    if !vm_pool_active() && workers > 1 {
        let invoke: Arc<
            dyn Fn(u64, HashMap<String, String>) -> Result<AhiruResponse, String> + Send + Sync,
        > = Arc::new(pool_invoke);
        let pool = Arc::new(HandlerWorkerPool::new(workers, invoke));
        install_pool(pool);
    }

    let worker_threads = workers.max(2);
    let rt = if vm_pool_active() {
        tokio::runtime::Builder::new_multi_thread()
            .worker_threads(worker_threads)
            .enable_all()
            .build()
    } else {
        // Interpreter handlers and ValueRefs are thread-local to this thread.
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
    }
    .map_err(|e| runtime_err(span, &e.to_string()))?;

    let result = rt.block_on(async move {
        state.app.serve(opts).await.map_err(|e| e.to_string())
    });

    clear_pool();
    clear_vm_pool();
    result.map_err(|e| runtime_err(span, &e))
}
