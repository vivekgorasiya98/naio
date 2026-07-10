use crate::auth::{AuthConfig, AuthMode};
use crate::config::{AuthConfigFile, LoggingConfig, SecurityConfig};
use crate::context::RequestContext;
use crate::glob::path_matches_scopes;
use crate::handler::HandlerFn;
use crate::response::AhiruResponse;
use dashmap::DashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Default)]
pub struct LoggingOptions {
    pub enabled: bool,
    pub json: Option<bool>,
    pub skip_paths: Vec<String>,
}

#[derive(Debug, Clone, Default)]
pub struct MiddlewareScope {
    pub only: Vec<String>,
    pub except: Vec<String>,
}

#[derive(Debug, Clone)]
pub enum CompressionAlgo {
    Gzip,
    Brotli,
}

#[derive(Debug, Clone)]
pub enum MiddlewareKind {
    RequestId { enabled: bool },
    Logging(LoggingOptions),
    SecureHeaders { csp_policy: Option<String> },
    Cors(Vec<String>),
    RateLimit { rps: u32 },
    BodyLimitMb(u64),
    Auth(AuthConfig),
    Compression(CompressionAlgo),
    Etag,
    IpFilter { allow: Vec<String>, deny: Vec<String> },
    Csrf,
    Custom,
}

#[derive(Clone)]
pub struct MiddlewareEntry {
    pub kind: MiddlewareKind,
    pub order: i32,
    pub scope: MiddlewareScope,
    pub custom_handler: Option<HandlerFn>,
}

impl MiddlewareEntry {
    pub fn builtin(kind: MiddlewareKind) -> Self {
        Self {
            kind,
            order: 0,
            scope: MiddlewareScope::default(),
            custom_handler: None,
        }
    }

    pub fn custom(handler: HandlerFn, order: i32, scope: MiddlewareScope) -> Self {
        Self {
            kind: MiddlewareKind::Custom,
            order,
            scope,
            custom_handler: Some(handler),
        }
    }

    pub fn applies_to(&self, path: &str) -> bool {
        path_matches_scopes(path, &self.scope.only, &self.scope.except)
    }
}

pub struct RateLimiter {
    buckets: DashMap<String, (u32, Instant)>,
    rps: u32,
    lockouts: DashMap<String, (u32, Instant)>,
    last_sweep: std::sync::Mutex<Instant>,
}

impl RateLimiter {
    pub fn new(rps: u32) -> Self {
        Self {
            buckets: DashMap::new(),
            rps: rps.max(1),
            lockouts: DashMap::new(),
            last_sweep: std::sync::Mutex::new(Instant::now()),
        }
    }

    fn maybe_sweep(&self) {
        let mut last = self.last_sweep.lock().unwrap();
        if last.elapsed() < Duration::from_secs(60) {
            return;
        }
        *last = Instant::now();
        let cutoff = Instant::now() - Duration::from_secs(120);
        self.buckets.retain(|_, (_, start)| *start > cutoff);
        self.lockouts.retain(|_, (_, start)| *start > cutoff);
    }

    pub fn check(&self, key: &str) -> bool {
        self.maybe_sweep();
        if let Some(entry) = self.lockouts.get(key) {
            if entry.0 >= 5 && entry.1.elapsed() < Duration::from_secs(300) {
                return false;
            }
        }
        let now = Instant::now();
        let mut entry = self.buckets.entry(key.to_string()).or_insert((0, now));
        let (count, window_start) = entry.value_mut();
        if now.duration_since(*window_start) >= Duration::from_secs(1) {
            *count = 0;
            *window_start = now;
        }
        if *count >= self.rps {
            let mut lock = self.lockouts.entry(key.to_string()).or_insert((0, now));
            lock.0 += 1;
            lock.1 = now;
            return false;
        }
        *count += 1;
        true
    }

    pub fn record_failed_auth(&self, key: &str) {
        let now = Instant::now();
        let mut lock = self.lockouts.entry(key.to_string()).or_insert((0, now));
        lock.0 += 1;
        lock.1 = now;
    }
}

pub fn sort_middleware(chain: &mut [MiddlewareEntry]) {
    chain.sort_by_key(|e| e.order);
}

pub fn apply_pre_middleware(
    ctx: &mut RequestContext,
    middleware: &[MiddlewareEntry],
    limiter: &Option<Arc<RateLimiter>>,
    auth: &AuthConfig,
    is_public: bool,
) -> Option<AhiruResponse> {
    let path = ctx.path.clone();
    for mw in middleware {
        if !mw.applies_to(&path) {
            continue;
        }
        match &mw.kind {
            MiddlewareKind::RateLimit { .. } => {
                if let Some(lim) = limiter {
                    let key = ctx
                        .header("x-forwarded-for")
                        .or_else(|| ctx.header("x-real-ip"))
                        .unwrap_or("unknown")
                        .to_string();
                    if !lim.check(&key) {
                        return Some(AhiruResponse::json(
                            429,
                            r#"{"error":"rate limit exceeded","code":"E2101"}"#,
                        ));
                    }
                }
            }
            MiddlewareKind::IpFilter { allow, deny } => {
                let ip = ctx
                    .header("x-forwarded-for")
                    .or_else(|| ctx.header("x-real-ip"))
                    .unwrap_or("")
                    .split(',')
                    .next()
                    .unwrap_or("")
                    .trim();
                if !deny.is_empty() && deny.iter().any(|d| d == ip || d == "*") {
                    return Some(AhiruResponse::json(403, r#"{"error":"ip denied"}"#));
                }
                if !allow.is_empty() && !allow.iter().any(|a| a == ip || a == "*") {
                    return Some(AhiruResponse::json(403, r#"{"error":"ip not allowed"}"#));
                }
            }
            MiddlewareKind::Auth(cfg) if cfg.mode != AuthMode::None && !is_public => {
                match auth.authenticate(ctx) {
                    Ok(Some(user)) => ctx.user = Some(user),
                    Ok(None) if cfg.scope == "global" => {
                        if let Some(lim) = limiter {
                            let key = format!(
                                "auth:{}",
                                ctx.header("x-forwarded-for").unwrap_or("unknown")
                            );
                            lim.record_failed_auth(&key);
                        }
                        return Some(AhiruResponse::unauthorized("authentication required"));
                    }
                    Err(msg) => return Some(AhiruResponse::unauthorized(msg)),
                    _ => {}
                }
            }
            MiddlewareKind::Custom => {
                if let Some(handler) = &mw.custom_handler {
                    let ctx_clone = ctx.clone();
                    let result = tokio::task::block_in_place(|| {
                        tokio::runtime::Handle::current().block_on(async { handler(ctx_clone).await })
                    });
                    match result {
                        Ok(resp) if resp.status >= 400 => return Some(resp),
                        Err(e) => {
                            return Some(AhiruResponse::internal(e));
                        }
                        _ => {}
                    }
                }
            }
            _ => {}
        }
    }
    None
}

pub fn check_permission(ctx: &RequestContext, permission: &str, rbac: bool) -> Option<AhiruResponse> {
    if !rbac {
        return None;
    }
    let user = ctx.user.as_ref()?;
    if user.permissions.iter().any(|p| p == permission || p == "*") {
        return None;
    }
    if user.roles.iter().any(|r| r == "admin") {
        return None;
    }
    Some(AhiruResponse::forbidden("insufficient permissions"))
}

pub fn middleware_from_config(
    security: &SecurityConfig,
    auth_file: &AuthConfigFile,
    logging: &LoggingConfig,
) -> (Vec<MiddlewareEntry>, AuthConfig) {
    let mut chain = vec![MiddlewareEntry::builtin(MiddlewareKind::RequestId {
        enabled: logging.request_id,
    })];
    if logging.access_log {
        chain.push(MiddlewareEntry::builtin(MiddlewareKind::Logging(LoggingOptions {
            enabled: true,
            json: Some(logging.json_logs),
            skip_paths: logging.skip_paths.clone(),
        })));
    }
    if security.secure_headers {
        chain.push(MiddlewareEntry::builtin(MiddlewareKind::SecureHeaders {
            csp_policy: security.csp_policy.clone(),
        }));
    }
    if !security.cors_origins.is_empty() {
        chain.push(MiddlewareEntry::builtin(MiddlewareKind::Cors(
            security.cors_origins.clone(),
        )));
    }
    if security.compression {
        let algo = if security.brotli {
            CompressionAlgo::Brotli
        } else {
            CompressionAlgo::Gzip
        };
        chain.push(MiddlewareEntry::builtin(MiddlewareKind::Compression(algo)));
    }
    if security.etag {
        chain.push(MiddlewareEntry::builtin(MiddlewareKind::Etag));
    }
    if security.rate_limit_rps > 0 {
        chain.push(MiddlewareEntry::builtin(MiddlewareKind::RateLimit {
            rps: security.rate_limit_rps,
        }));
    }
    if security.csrf {
        chain.push(MiddlewareEntry::builtin(MiddlewareKind::Csrf));
    }
    if !security.ip_allow.is_empty() || !security.ip_deny.is_empty() {
        chain.push(MiddlewareEntry::builtin(MiddlewareKind::IpFilter {
            allow: security.ip_allow.clone(),
            deny: security.ip_deny.clone(),
        }));
    }
    let auth = AuthConfig::from_file(auth_file);
    if auth.mode != AuthMode::None {
        chain.push(MiddlewareEntry::builtin(MiddlewareKind::Auth(auth.clone())));
    }
    (chain, auth)
}

pub fn logging_enabled_in_chain(middleware: &[MiddlewareEntry]) -> bool {
    middleware.iter().any(|e| match &e.kind {
        MiddlewareKind::Logging(opts) => opts.enabled,
        _ => false,
    })
}

pub fn request_id_enabled_in_chain(middleware: &[MiddlewareEntry]) -> bool {
    middleware.iter().any(|e| match &e.kind {
        MiddlewareKind::RequestId { enabled } => *enabled,
        _ => false,
    })
}

