use crate::config::LoggingConfig;
use crate::router::RouteInfo;
use serde::Serialize;
use std::collections::HashSet;
use std::sync::{Arc, RwLock};

#[derive(Debug, Clone)]
pub struct LogController {
    inner: Arc<RwLock<LogState>>,
}

#[derive(Debug, Clone)]
struct LogState {
    access_log: bool,
    startup_banner: bool,
    json_logs: bool,
    request_id_header: bool,
    quiet_handlers: bool,
    slow_request_ms: f64,
    skip_paths: HashSet<String>,
}

impl LogController {
    pub fn from_config(config: &LoggingConfig) -> Self {
        let mut skip_paths: HashSet<String> = config.skip_paths.iter().cloned().collect();
        if std::env::var("AHIRU_QUIET").ok().as_deref() == Some("1") {
            skip_paths.insert("*".into());
        }
        let access_log = config.access_log && std::env::var("AHIRU_QUIET").ok().as_deref() != Some("1");
        Self {
            inner: Arc::new(RwLock::new(LogState {
                access_log,
                startup_banner: config.startup_banner
                    && std::env::var("AHIRU_QUIET").ok().as_deref() != Some("1"),
                json_logs: config.json_logs,
                request_id_header: config.request_id,
                quiet_handlers: config.quiet_handlers,
                slow_request_ms: config.slow_request_ms,
                skip_paths,
            })),
        }
    }

    pub fn access_log_enabled(&self) -> bool {
        self.inner.read().unwrap().access_log
    }

    pub fn needs_timing(&self) -> bool {
        let s = self.inner.read().unwrap();
        s.access_log || s.slow_request_ms > 0.0
    }

    pub fn request_id_header_enabled(&self) -> bool {
        self.inner.read().unwrap().request_id_header
    }

    pub fn quiet_handlers(&self) -> bool {
        self.inner.read().unwrap().quiet_handlers
    }

    pub fn should_log_request(&self, path: &str, ms: f64) -> bool {
        let s = self.inner.read().unwrap();
        if !s.access_log {
            return false;
        }
        if s.skip_paths.contains("*") || s.skip_paths.contains(path) {
            return false;
        }
        if s.slow_request_ms > 0.0 && ms < s.slow_request_ms {
            return false;
        }
        true
    }

    pub fn log_access(&self, request_id: &str, method: &str, path: &str, status: u16, ms: f64) {
        if !self.should_log_request(path, ms) {
            return;
        }
        let s = self.inner.read().unwrap();
        if s.json_logs {
            let entry = AccessLogEntry {
                request_id: request_id.to_string(),
                method: method.to_string(),
                path: path.to_string(),
                status,
                duration_ms: ms,
            };
            if let Ok(line) = serde_json::to_string(&entry) {
                eprintln!("{line}");
            }
        } else {
            eprintln!("[{request_id}] {method} {path} → {status} ({ms:.1}ms)");
        }
    }

    pub fn print_startup_banner(
        &self,
        host: &str,
        port: u16,
        routes: &[RouteInfo],
        dev: bool,
        network: bool,
    ) {
        if !self.inner.read().unwrap().startup_banner {
            return;
        }
        crate::banner::print_startup_banner(host, port, routes, dev, network);
    }

    pub fn apply_runtime_opts(
        &self,
        access_log: Option<bool>,
        json_logs: Option<bool>,
        startup_banner: Option<bool>,
        quiet_handlers: Option<bool>,
        request_id: Option<bool>,
        slow_request_ms: Option<f64>,
        skip_paths: Option<Vec<String>>,
    ) {
        let mut s = self.inner.write().unwrap();
        if let Some(v) = access_log {
            s.access_log = v;
        }
        if let Some(v) = json_logs {
            s.json_logs = v;
        }
        if let Some(v) = startup_banner {
            s.startup_banner = v;
        }
        if let Some(v) = quiet_handlers {
            s.quiet_handlers = v;
        }
        if let Some(v) = request_id {
            s.request_id_header = v;
        }
        if let Some(v) = slow_request_ms {
            s.slow_request_ms = v;
        }
        if let Some(paths) = skip_paths {
            s.skip_paths = paths.into_iter().collect();
        }
    }
}

#[derive(Serialize)]
struct AccessLogEntry {
    request_id: String,
    method: String,
    path: String,
    status: u16,
    duration_ms: f64,
}
