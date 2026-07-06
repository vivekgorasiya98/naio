use crate::auth::AuthConfig;
use crate::cache::{CacheManager, SharedCacheManager};
use crate::config::AhiruConfig;
use crate::context::{CookieOpts, RequestContext};
use crate::db::{DbManager, SharedDbManager};
use crate::groups::ScopeRegistry;
use crate::handler::{HandlerFn, WsHandlerFn};
use crate::jobs::{CronScheduler, JobQueue, SharedCronScheduler, SharedJobQueue};
use crate::logging::LogController;
use crate::metrics;
use crate::middleware::{
    apply_pre_middleware, check_permission, logging_enabled_in_chain, middleware_from_config,
    request_id_enabled_in_chain, sort_middleware, MiddlewareEntry, MiddlewareKind, RateLimiter,
};
use crate::native::{native_health_handler, native_ping_handler};
use crate::port::{bind_listener, PortBindError, PortBindPolicy};
use crate::response::{AhiruResponse, ResponseBody};
use crate::router::{
    extract_path_params, HttpMethod, RouteEntry, RouteInfo, RouteMeta, to_axum_path,
};
use crate::shutdown::{shutdown_receiver, wait_for_shutdown};
use crate::state::AppStateStore;
use crate::validation::run_validation;
use crate::ws::{handle_websocket, WsMode};
use crate::ws_hub::{SharedWsHub, WsHub};
use axum::body::Body;
use axum::extract::{Request, WebSocketUpgrade};
use axum::http::{HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, get, patch, post, put};
use axum::Router;
use futures_util::Future;
use futures_util::stream;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use tower_http::compression::CompressionLayer;
use tower_http::cors::{Any, CorsLayer};
use tower_http::limit::RequestBodyLimitLayer;
use tower_http::services::ServeDir;
use tower_http::set_header::SetResponseHeaderLayer;
use uuid::Uuid;

#[derive(Debug)]
pub enum ServeError {
    Io(std::io::Error),
    Config(String),
    Tls(String),
    Port(PortBindError),
}

impl std::fmt::Display for ServeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ServeError::Io(e) => write!(f, "io error: {e}"),
            ServeError::Config(e) => write!(f, "config error: {e}"),
            ServeError::Tls(e) => write!(f, "tls error: {e}"),
            ServeError::Port(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for ServeError {}

impl From<std::io::Error> for ServeError {
    fn from(e: std::io::Error) -> Self {
        ServeError::Io(e)
    }
}

impl From<PortBindError> for ServeError {
    fn from(e: PortBindError) -> Self {
        ServeError::Port(e)
    }
}

#[derive(Debug, Clone, Default)]
pub struct ServeRuntimeOptions {
    pub dev: bool,
    pub network: bool,
    pub cli_port: Option<u16>,
    pub explicit_port: bool,
}

#[derive(Clone)]
pub struct RouteDispatch {
    pub entry: RouteEntry,
    pub middleware: Arc<Vec<MiddlewareEntry>>,
    pub auth: Arc<AuthConfig>,
    pub limiter: Option<Arc<RateLimiter>>,
    pub logging: Arc<LogController>,
    pub state_snapshot: HashMap<String, String>,
    pub schema: Option<HandlerFn>,
    pub guard: Option<HandlerFn>,
    pub error_handler: Option<HandlerFn>,
    pub timeout: Option<Duration>,
}

pub struct AhiruApp {
    pub config: AhiruConfig,
    routes: Vec<RouteEntry>,
    middleware: Vec<MiddlewareEntry>,
    auth: AuthConfig,
    limiter: Option<Arc<RateLimiter>>,
    db: Option<SharedDbManager>,
    cache: Option<SharedCacheManager>,
    jobs: SharedJobQueue,
    cron: SharedCronScheduler,
    ws_hub: SharedWsHub,
    scopes: ScopeRegistry,
    state: AppStateStore,
    static_mounts: Vec<(String, PathBuf)>,
    error_handler: Option<HandlerFn>,
    not_found_handler: Option<HandlerFn>,
    metrics_path: Option<String>,
    _ws_mode: WsMode,
    logging: LogController,
}

impl AhiruApp {
    pub fn new(config: AhiruConfig) -> Self {
        let logging = LogController::from_config(&config.logging);
        let (middleware, auth) =
            middleware_from_config(&config.security, &config.auth, &config.logging);
        let limiter = middleware.iter().find_map(|m| match &m.kind {
            MiddlewareKind::RateLimit { rps } => Some(Arc::new(RateLimiter::new(*rps))),
            _ => None,
        });
        let ws_mode = WsMode::from_str(&config.websocket.mode);
        Self {
            config,
            routes: Vec::new(),
            middleware,
            auth,
            limiter,
            db: None,
            cache: None,
            jobs: Arc::new(JobQueue::new()),
            cron: Arc::new(CronScheduler::new()),
            ws_hub: Arc::new(WsHub::new()),
            scopes: ScopeRegistry::new(),
            state: AppStateStore::new(),
            static_mounts: Vec::new(),
            error_handler: None,
            not_found_handler: None,
            metrics_path: None,
            _ws_mode: ws_mode,
            logging,
        }
    }

    pub fn from_config_file(path: &std::path::Path) -> Result<Self, String> {
        let config = AhiruConfig::load_with_env(path)?;
        config.validate().map_err(|errs| {
            errs.iter()
                .map(|e| format!("{}: {}", e.field, e.message))
                .collect::<Vec<_>>()
                .join("; ")
        })?;
        Ok(Self::new(config))
    }

    pub fn state(&self) -> &AppStateStore {
        &self.state
    }

    pub fn jobs(&self) -> SharedJobQueue {
        Arc::clone(&self.jobs)
    }

    pub fn cron(&self) -> SharedCronScheduler {
        Arc::clone(&self.cron)
    }

    pub fn ws_hub(&self) -> SharedWsHub {
        Arc::clone(&self.ws_hub)
    }

    pub fn scopes(&self) -> &ScopeRegistry {
        &self.scopes
    }

    pub fn scopes_mut(&mut self) -> &mut ScopeRegistry {
        &mut self.scopes
    }

    pub fn logging(&self) -> &LogController {
        &self.logging
    }

    pub fn set_logging(&mut self, logging: LogController) {
        self.logging = logging;
    }

    pub fn set_error_handler(&mut self, handler: HandlerFn) {
        self.error_handler = Some(handler);
    }

    pub fn set_not_found_handler(&mut self, handler: HandlerFn) {
        self.not_found_handler = Some(handler);
    }

    pub fn mount_metrics(&mut self, path: impl Into<String>) {
        self.metrics_path = Some(path.into());
    }

    pub fn mount_static(&mut self, url_prefix: impl Into<String>, dir: impl Into<PathBuf>) {
        self.static_mounts.push((url_prefix.into(), dir.into()));
    }

    pub fn route(
        &mut self,
        method: HttpMethod,
        path: &str,
        handler: HandlerFn,
        meta: RouteMeta,
    ) -> &mut Self {
        let axum_path = to_axum_path(path);
        self.routes.push(RouteEntry {
            method,
            path: path.into(),
            axum_path,
            handler,
            ws_handler: None,
            meta,
        });
        self
    }

    pub fn route_with_scope(
        &mut self,
        scope_id: u64,
        method: HttpMethod,
        path: &str,
        handler: HandlerFn,
        mut meta: RouteMeta,
    ) -> &mut Self {
        if let Some(scope) = self.scopes.get(scope_id) {
            let full_path = ScopeRegistry::join_path(&scope.prefix, path);
            if meta.permission.is_none() {
                meta.permission = scope.meta_defaults.permission.clone();
            }
            if !meta.public {
                meta.public = scope.meta_defaults.public;
            }
            self.route(method, &full_path, handler, meta);
        }
        self
    }

    pub fn route_ws(&mut self, path: &str, handler: WsHandlerFn, meta: RouteMeta) -> &mut Self {
        let axum_path = to_axum_path(path);
        let http_handler: HandlerFn = Arc::new(|_ctx| {
            Box::pin(async { Ok(AhiruResponse::text(426, "use WebSocket upgrade")) })
        });
        self.routes.push(RouteEntry {
            method: HttpMethod::Get,
            path: path.into(),
            axum_path,
            handler: http_handler,
            ws_handler: Some(handler),
            meta: RouteMeta {
                ws: true,
                ..meta
            },
        });
        self
    }

    pub fn use_middleware(&mut self, mw: MiddlewareEntry) -> &mut Self {
        self.middleware.push(mw);
        sort_middleware(&mut self.middleware);
        self
    }

    pub fn db(&self) -> Option<SharedDbManager> {
        self.db.clone()
    }

    pub fn cache(&self) -> Option<SharedCacheManager> {
        self.cache.clone()
    }

    pub fn set_db_internal(&mut self, db: SharedDbManager) {
        self.db = Some(db);
    }

    pub fn set_cache_internal(&mut self, cache: SharedCacheManager) {
        self.cache = Some(cache);
    }

    pub fn list_routes(&self) -> Vec<RouteInfo> {
        self.routes.iter().map(|r| r.info()).collect()
    }

    pub fn worker_count(&self) -> usize {
        self.config.server.workers.max(1)
    }

    pub async fn init_db(&mut self) -> Result<(), String> {
        if self.config.databases.is_empty() {
            return Ok(());
        }
        let db = DbManager::connect_all(&self.config.databases).await?;
        self.db = Some(Arc::new(db));
        Ok(())
    }

    pub async fn init_cache(&mut self) -> Result<(), String> {
        if self.config.caches.is_empty() {
            self.cache = Some(Arc::new(CacheManager::memory()));
            return Ok(());
        }
        let cfg = &self.config.caches[0];
        let mgr = match cfg.driver.as_str() {
            "memory" => CacheManager::memory(),
            "redis" => {
                #[cfg(feature = "redis")]
                {
                    CacheManager::connect_redis(cfg.url.as_deref().unwrap_or("redis://127.0.0.1"))
                        .await?
                }
                #[cfg(not(feature = "redis"))]
                {
                    return Err("redis feature not enabled (E2301)".into());
                }
            }
            other => return Err(format!("unsupported cache driver: {other}")),
        };
        self.cache = Some(Arc::new(mgr));
        Ok(())
    }

    fn make_dispatch(&self, entry: RouteEntry) -> RouteDispatch {
        let timeout = entry.meta.timeout_ms.map(Duration::from_millis);
        RouteDispatch {
            entry: entry.clone(),
            middleware: Arc::new(self.middleware.clone()),
            auth: Arc::new(self.auth.clone()),
            limiter: self.limiter.clone(),
            logging: Arc::new(self.logging.clone()),
            state_snapshot: self.state.snapshot(),
            schema: entry.meta.schema.clone(),
            guard: entry.meta.guard.clone(),
            error_handler: self.error_handler.clone(),
            timeout,
        }
    }

    pub fn build_test_router(&self) -> Router {
        self.build_router()
    }

    fn build_router(&self) -> Router {
        let mut router = Router::new();

        if let Some(path) = &self.metrics_path {
            let p = path.clone();
            router = router.route(
                &p,
                get(|| async move {
                    metrics::prometheus_text().into_response()
                }),
            );
        }

        for (prefix, dir) in &self.static_mounts {
            let service = ServeDir::new(dir);
            router = router.nest_service(prefix, service);
        }

        for route in &self.routes {
            let dispatch = self.make_dispatch(route.clone());
            let path = route.axum_path.clone();

            router = match (&route.method, route.ws_handler.is_some()) {
                (_, true) => {
                    let d = dispatch.clone();
                    router.route(
                        &path,
                        get(move |ws: WebSocketUpgrade, req: Request| async move {
                            handle_http_or_ws(d, req, Some(ws)).await
                        }),
                    )
                }
                (HttpMethod::Get, _) => {
                    let d = dispatch.clone();
                    router.route(&path, get(move |req: Request| async move {
                        handle_http_or_ws(d, req, None).await
                    }))
                }
                (HttpMethod::Post, _) => {
                    let d = dispatch.clone();
                    router.route(&path, post(move |req: Request| async move {
                        handle_http_or_ws(d, req, None).await
                    }))
                }
                (HttpMethod::Put, _) => {
                    let d = dispatch.clone();
                    router.route(&path, put(move |req: Request| async move {
                        handle_http_or_ws(d, req, None).await
                    }))
                }
                (HttpMethod::Delete, _) => {
                    let d = dispatch.clone();
                    router.route(&path, delete(move |req: Request| async move {
                        handle_http_or_ws(d, req, None).await
                    }))
                }
                (HttpMethod::Patch, _) => {
                    let d = dispatch.clone();
                    router.route(&path, patch(move |req: Request| async move {
                        handle_http_or_ws(d, req, None).await
                    }))
                }
                _ => router,
            };
        }

        let body_limit = self.config.server.body_limit_mb.saturating_mul(1024 * 1024);
        router = router.layer(RequestBodyLimitLayer::new(body_limit as usize));

        for mw in &self.middleware {
            if let MiddlewareKind::SecureHeaders { csp_policy } = &mw.kind {
                router = router
                    .layer(SetResponseHeaderLayer::if_not_present(
                        axum::http::header::X_CONTENT_TYPE_OPTIONS,
                        HeaderValue::from_static("nosniff"),
                    ))
                    .layer(SetResponseHeaderLayer::if_not_present(
                        axum::http::header::X_FRAME_OPTIONS,
                        HeaderValue::from_static("DENY"),
                    ));
                if let Some(csp) = csp_policy {
                    if let Ok(v) = HeaderValue::from_str(csp) {
                        router = router.layer(SetResponseHeaderLayer::if_not_present(
                            axum::http::header::CONTENT_SECURITY_POLICY,
                            v,
                        ));
                    }
                }
            }
            if let MiddlewareKind::Compression(_) = &mw.kind {
                router = router.layer(CompressionLayer::new());
            }
            if let MiddlewareKind::Cors(origins) = &mw.kind {
                let layer = if origins.iter().any(|o| o == "*") {
                    CorsLayer::new()
                        .allow_origin(Any)
                        .allow_methods(Any)
                        .allow_headers(Any)
                } else {
                    let mut layer = CorsLayer::new().allow_methods(Any).allow_headers(Any);
                    for o in origins {
                        if let Ok(v) = o.parse::<HeaderValue>() {
                            layer = layer.allow_origin(v);
                        }
                    }
                    layer
                };
                router = router.layer(layer);
            }
        }

        router
    }

    pub async fn serve(self, opts: ServeRuntimeOptions) -> Result<(), ServeError> {
        let mut host = self.config.server.host.clone();
        let mut port = opts.cli_port.unwrap_or(self.config.server.port);
        if opts.network {
            host = "0.0.0.0".into();
        }

        let _ = tracing_subscriber::fmt()
            .with_env_filter(
                tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| {
                    tracing_subscriber::EnvFilter::new(&self.config.logging.level)
                }),
            )
            .try_init();

        let policy = if opts.explicit_port {
            PortBindPolicy::Prompt
        } else {
            PortBindPolicy::AutoNext
        };

        let (listener, bound_port) = bind_listener(&host, port, policy)
            .await
            .map_err(ServeError::from)?;
        port = bound_port;

        let routes = self.list_routes();
        self.logging
            .print_startup_banner(&host, port, &routes, opts.dev, opts.network);

        let router = self.build_router();
        let shutdown_rx = shutdown_receiver();
        let drain = self
            .config
            .server
            .shutdown_drain_secs
            .unwrap_or(30);
        axum::serve(listener, router)
            .with_graceful_shutdown(wait_for_shutdown(shutdown_rx, drain))
            .await?;
        Ok(())
    }
}

pub fn mount_health_live() -> HandlerFn {
    native_health_handler()
}

pub fn mount_health_ready(db: Option<SharedDbManager>, cache: Option<SharedCacheManager>) -> HandlerFn {
    Arc::new(move |ctx| {
        let db = db.clone();
        let cache = cache.clone();
        Box::pin(async move {
            let mut ok = true;
            if let Some(db) = db {
                if db.ping().await.is_err() {
                    ok = false;
                }
            }
            if let Some(cache) = cache {
                if !cache.ping().await {
                    ok = false;
                }
            }
            let _ = ctx;
            if ok {
                Ok(AhiruResponse::json(200, r#"{"status":"ready"}"#))
            } else {
                Ok(AhiruResponse::json(503, r#"{"status":"not_ready"}"#))
            }
        })
    })
}

async fn handle_http_or_ws(
    dispatch: RouteDispatch,
    req: Request,
    ws_upgrade: Option<WebSocketUpgrade>,
) -> Response {
    let chain_logging = logging_enabled_in_chain(&dispatch.middleware);
    let log_ctrl = &dispatch.logging;
    let needs_timing = chain_logging || log_ctrl.needs_timing();
    let started = needs_timing.then(std::time::Instant::now);

    let (parts, body) = req.into_parts();
    let method = parts.method.as_str().to_string();
    let path = dispatch.entry.path.clone();
    let query_str = parts.uri.query().unwrap_or("");
    let query = crate::router::parse_query(query_str);
    let actual_path = parts.uri.path();
    let path_params = extract_path_params(&path, actual_path);

    let body_bytes = if method == "GET" || method == "HEAD" || method == "OPTIONS" {
        axum::body::Bytes::new()
    } else {
        match axum::body::to_bytes(body, usize::MAX).await {
            Ok(b) => b,
            Err(e) => return AhiruResponse::internal(e.to_string()).into_axum(),
        }
    };

    let request_id = Uuid::new_v4().to_string();
    let mut ctx = RequestContext::from_parts(
        parts.method,
        path.clone(),
        query,
        path_params,
        parts.headers,
        body_bytes,
        request_id.clone(),
        dispatch.state_snapshot.clone(),
    );

    let is_public = dispatch.entry.meta.public;

    if let Some(resp) = apply_pre_middleware(
        &mut ctx,
        &dispatch.middleware,
        &dispatch.limiter,
        &dispatch.auth,
        is_public,
    ) {
        return finish_response(
            resp,
            &request_id,
            &method,
            &path,
            started,
            log_ctrl,
            chain_logging,
            &dispatch.middleware,
            &ctx,
        );
    }

    if let Some(perm) = &dispatch.entry.meta.permission {
        if let Some(resp) = check_permission(&ctx, perm, dispatch.auth.rbac_enabled) {
            return finish_response(
                resp,
                &request_id,
                &method,
                &path,
                started,
                log_ctrl,
                chain_logging,
                &dispatch.middleware,
                &ctx,
            );
        }
    }

    if let Some(schema) = &dispatch.schema {
        if let Err(resp) = run_validation(schema, ctx.clone()).await {
            return finish_response(
                resp,
                &request_id,
                &method,
                &path,
                started,
                log_ctrl,
                chain_logging,
                &dispatch.middleware,
                &ctx,
            );
        }
    }

    if let Some(guard) = &dispatch.guard {
        match guard(ctx.clone()).await {
            Ok(resp) if resp.status < 400 => {}
            Ok(resp) => {
                return finish_response(
                    resp,
                    &request_id,
                    &method,
                    &path,
                    started,
                    log_ctrl,
                    chain_logging,
                    &dispatch.middleware,
                    &ctx,
                );
            }
            Err(e) => {
                return finish_response(
                    AhiruResponse::forbidden(e),
                    &request_id,
                    &method,
                    &path,
                    started,
                    log_ctrl,
                    chain_logging,
                    &dispatch.middleware,
                    &ctx,
                );
            }
        }
    }

    if let (Some(upgrade), Some(ws_handler)) = (ws_upgrade, dispatch.entry.ws_handler.clone()) {
        if chain_logging {
            let ms = started
                .map(|s| s.elapsed().as_secs_f64() * 1000.0)
                .unwrap_or(0.0);
            log_ctrl.log_access(&request_id, &method, &path, 101, ms);
        }
        metrics::record_request(101);
        return upgrade
            .on_upgrade(move |socket| async move {
                handle_websocket(socket, ws_handler, ctx).await;
            })
            .into_response();
    }

    let handler_fut = async {
        if log_ctrl.quiet_handlers() {
            ctx.extra.insert("quiet_handlers".into(), "1".into());
        }
        let result = if let Some(timeout) = dispatch.timeout {
            match tokio::time::timeout(timeout, (dispatch.entry.handler)(ctx.clone())).await {
                Ok(r) => r,
                Err(_) => Err("request timeout".into()),
            }
        } else {
            (dispatch.entry.handler)(ctx.clone()).await
        };
        match result {
            Ok(resp) => resp,
            Err(e) => {
                if let Some(err_handler) = &dispatch.error_handler {
                    match err_handler(ctx.clone()).await {
                        Ok(resp) => resp,
                        Err(_) => AhiruResponse::internal(e),
                    }
                } else {
                    AhiruResponse::internal(e)
                }
            }
        }
    };

    let response = handler_fut.await;

    finish_response(
        response,
        &request_id,
        &method,
        &path,
        started,
        log_ctrl,
        chain_logging,
        &dispatch.middleware,
        &ctx,
    )
}

fn finish_response(
    mut resp: AhiruResponse,
    request_id: &str,
    method: &str,
    path: &str,
    started: Option<std::time::Instant>,
    log_ctrl: &LogController,
    chain_logging: bool,
    middleware: &[MiddlewareEntry],
    ctx: &RequestContext,
) -> Response {
    let add_request_id = log_ctrl.request_id_header_enabled()
        || request_id_enabled_in_chain(middleware);
    if add_request_id {
        resp.headers
            .insert("x-request-id".into(), request_id.to_string());
    }

    for (name, value, opts) in &ctx.response_cookies {
        let mut cookie = format!("{name}={value}");
        if let Some(max_age) = opts.max_age_secs {
            cookie.push_str(&format!("; Max-Age={max_age}"));
        }
        if opts.http_only {
            cookie.push_str("; HttpOnly");
        }
        if opts.secure {
            cookie.push_str("; Secure");
        }
        if let Some(p) = &opts.path {
            cookie.push_str(&format!("; Path={p}"));
        }
        resp.headers
            .insert("set-cookie".into(), cookie);
    }

    let status = resp.status;
    metrics::record_request(status);
    let axum_resp = resp.into_axum();

    if chain_logging {
        let ms = started
            .map(|s| s.elapsed().as_secs_f64() * 1000.0)
            .unwrap_or(0.0);
        let skip = middleware.iter().any(|m| match &m.kind {
            MiddlewareKind::Logging(opts) => {
                !opts.enabled || opts.skip_paths.iter().any(|p| p == path)
            }
            _ => false,
        });
        if !skip {
            log_ctrl.log_access(request_id, method, path, status, ms);
        }
    }

    axum_resp
}

impl AhiruResponse {
    pub(crate) fn into_axum(self) -> Response {
        if let Some(url) = self.redirect_url {
            return Response::builder()
                .status(StatusCode::from_u16(self.status).unwrap_or(StatusCode::FOUND))
                .header("location", url)
                .body(Body::empty())
                .unwrap();
        }

        let mut builder =
            Response::builder().status(StatusCode::from_u16(self.status).unwrap_or(StatusCode::OK));
        builder = builder.header("content-type", self.content_type.as_str());
        for (k, v) in &self.headers {
            builder = builder.header(k.as_str(), v.as_str());
        }

        match self.body {
            ResponseBody::Buffered(bytes) => builder
                .body(Body::from(bytes))
                .unwrap_or_else(|_| {
                    Response::builder()
                        .status(StatusCode::INTERNAL_SERVER_ERROR)
                        .body(Body::from("response build error"))
                        .unwrap()
                }),
            ResponseBody::Stream(sender) => {
                let (tx, mut rx) = tokio::sync::mpsc::channel::<Vec<u8>>(64);
                if let Ok(mut guard) = sender.lock() {
                    *guard = Some(tx);
                }
                let stream = stream::poll_fn(move |cx| {
                    match std::pin::pin!(rx.recv()).poll(cx) {
                        std::task::Poll::Ready(Some(chunk)) => {
                            std::task::Poll::Ready(Some(Ok::<_, std::convert::Infallible>(chunk)))
                        }
                        std::task::Poll::Ready(None) => std::task::Poll::Ready(None),
                        std::task::Poll::Pending => std::task::Poll::Pending,
                    }
                });
                builder
                    .body(Body::from_stream(stream))
                    .unwrap_or_else(|_| {
                        Response::builder()
                            .status(StatusCode::INTERNAL_SERVER_ERROR)
                            .body(Body::from("stream error"))
                            .unwrap()
                    })
            }
        }
    }
}

/// Bridge for shared-per-worker Neko interpreter invocation.
pub struct WorkerInterpreterPool {
    pub invoke: Arc<dyn Fn(HashMap<String, String>) -> Result<AhiruResponse, String> + Send + Sync>,
}

impl WorkerInterpreterPool {
    pub fn handler_fn(self: &Arc<Self>) -> HandlerFn {
        let pool = Arc::clone(self);
        Arc::new(move |ctx| {
            let pool = Arc::clone(&pool);
            let mut fields = HashMap::with_capacity(
                8 + ctx.params.len() + ctx.query.len() + ctx.headers.len() + ctx.state.len(),
            );
            fields.insert("method".into(), ctx.method.clone());
            fields.insert("path".into(), ctx.path.clone());
            fields.insert("body".into(), crate::context::body_for_dispatch(&ctx));
            fields.insert("request_id".into(), ctx.request_id.clone());
            for (k, v) in &ctx.params {
                fields.insert(format!("param_{k}"), v.clone());
            }
            for (k, v) in &ctx.query {
                fields.insert(format!("query_{k}"), v.clone());
            }
            for (k, v) in &ctx.headers {
                fields.insert(format!("header_{k}"), v.clone());
            }
            for (k, v) in &ctx.state {
                fields.insert(format!("state_{k}"), v.clone());
            }
            if let Some(user) = &ctx.user {
                fields.insert("user_id".into(), user.id.clone());
            }
            Box::pin(async move { (pool.invoke)(fields) })
        })
    }
}
