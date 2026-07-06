use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AhiruConfig {
    #[serde(default)]
    pub server: ServerConfig,
    #[serde(default)]
    pub databases: Vec<DatabaseConfig>,
    #[serde(default)]
    pub caches: Vec<CacheConfig>,
    #[serde(default)]
    pub auth: AuthConfigFile,
    #[serde(default)]
    pub websocket: WsConfigFile,
    #[serde(default)]
    pub security: SecurityConfig,
    #[serde(default)]
    pub logging: LoggingConfig,
}

impl Default for AhiruConfig {
    fn default() -> Self {
        Self {
            server: ServerConfig::default(),
            databases: Vec::new(),
            caches: Vec::new(),
            auth: AuthConfigFile::default(),
            websocket: WsConfigFile::default(),
            security: SecurityConfig::default(),
            logging: LoggingConfig::default(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerConfig {
    #[serde(default = "default_host")]
    pub host: String,
    #[serde(default = "default_port")]
    pub port: u16,
    #[serde(default = "default_workers")]
    pub workers: usize,
    #[serde(default)]
    pub tls_cert: Option<String>,
    #[serde(default)]
    pub tls_key: Option<String>,
    #[serde(default)]
    pub body_limit_mb: u64,
    #[serde(default = "default_native_routes")]
    pub native_routes: bool,
    #[serde(default)]
    pub connect_timeout_ms: Option<u64>,
    #[serde(default)]
    pub idle_timeout_secs: Option<u64>,
    #[serde(default)]
    pub shutdown_drain_secs: Option<u64>,
}

fn default_host() -> String {
    "0.0.0.0".into()
}
fn default_port() -> u16 {
    3000
}
fn default_workers() -> usize {
    num_cpus()
}

fn num_cpus() -> usize {
    std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(4)
        .max(1)
}

fn default_native_routes() -> bool {
    true
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            host: default_host(),
            port: default_port(),
            workers: default_workers(),
            tls_cert: None,
            tls_key: None,
            body_limit_mb: 10,
            native_routes: true,
            connect_timeout_ms: None,
            idle_timeout_secs: None,
            shutdown_drain_secs: Some(30),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DatabaseConfig {
    pub name: String,
    pub driver: String,
    pub url: String,
    #[serde(default = "default_pool_size")]
    pub pool_size: u32,
    #[serde(default)]
    pub migrations_dir: Option<String>,
    #[serde(default)]
    pub role: Option<String>,
    #[serde(default)]
    pub max_connections: Option<u32>,
    #[serde(default)]
    pub idle_timeout_secs: Option<u64>,
    #[serde(default)]
    pub connect_timeout_ms: Option<u64>,
}

fn default_pool_size() -> u32 {
    10
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CacheConfig {
    pub name: String,
    #[serde(default = "default_cache_driver")]
    pub driver: String,
    #[serde(default)]
    pub url: Option<String>,
}

fn default_cache_driver() -> String {
    "memory".into()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiKeyEntry {
    pub key: String,
    #[serde(default)]
    pub scopes: Vec<String>,
    #[serde(default)]
    pub expires_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct OAuthProviderConfig {
    pub client_id: String,
    pub client_secret: String,
    #[serde(default)]
    pub redirect_uri: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AuthConfigFile {
    #[serde(default)]
    pub mode: String,
    #[serde(default)]
    pub scope: String,
    #[serde(default)]
    pub jwt_secret: Option<String>,
    #[serde(default)]
    pub session_secret: Option<String>,
    #[serde(default)]
    pub api_keys: Vec<String>,
    #[serde(default)]
    pub api_key_entries: Vec<ApiKeyEntry>,
    #[serde(default)]
    pub rbac_enabled: bool,
    #[serde(default)]
    pub oauth: HashMap<String, OAuthProviderConfig>,
    #[serde(default)]
    pub refresh_token_enabled: bool,
    #[serde(default)]
    pub totp_enabled: bool,
    #[serde(default)]
    pub magic_link_enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WsConfigFile {
    #[serde(default)]
    pub mode: String,
    #[serde(default)]
    pub heartbeat_secs: Option<u64>,
}

impl Default for WsConfigFile {
    fn default() -> Self {
        Self {
            mode: "disabled".into(),
            heartbeat_secs: Some(30),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SecurityConfig {
    #[serde(default)]
    pub cors_origins: Vec<String>,
    #[serde(default = "default_rate_limit")]
    pub rate_limit_rps: u32,
    #[serde(default)]
    pub secure_headers: bool,
    #[serde(default)]
    pub csrf: bool,
    #[serde(default)]
    pub compression: bool,
    #[serde(default)]
    pub brotli: bool,
    #[serde(default)]
    pub etag: bool,
    #[serde(default)]
    pub csp_policy: Option<String>,
    #[serde(default)]
    pub ip_allow: Vec<String>,
    #[serde(default)]
    pub ip_deny: Vec<String>,
}

fn default_rate_limit() -> u32 {
    100
}

impl Default for SecurityConfig {
    fn default() -> Self {
        Self {
            cors_origins: vec!["*".into()],
            rate_limit_rps: default_rate_limit(),
            secure_headers: true,
            csrf: false,
            compression: false,
            brotli: false,
            etag: false,
            csp_policy: None,
            ip_allow: Vec::new(),
            ip_deny: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoggingConfig {
    #[serde(default = "default_log_level")]
    pub level: String,
    #[serde(default = "default_true")]
    pub request_id: bool,
    #[serde(default)]
    pub json_logs: bool,
    #[serde(default = "default_true")]
    pub access_log: bool,
    #[serde(default = "default_true")]
    pub startup_banner: bool,
    #[serde(default)]
    pub quiet_handlers: bool,
    #[serde(default)]
    pub slow_request_ms: f64,
    #[serde(default)]
    pub skip_paths: Vec<String>,
    #[serde(default)]
    pub file: Option<String>,
    #[serde(default)]
    pub rotate_mb: Option<u64>,
    #[serde(default)]
    pub remote_url: Option<String>,
    #[serde(default)]
    pub redact_fields: Vec<String>,
}

fn default_log_level() -> String {
    "info".into()
}
fn default_true() -> bool {
    true
}

impl Default for LoggingConfig {
    fn default() -> Self {
        Self {
            level: default_log_level(),
            request_id: true,
            json_logs: false,
            access_log: true,
            startup_banner: true,
            quiet_handlers: false,
            slow_request_ms: 0.0,
            skip_paths: Vec::new(),
            file: None,
            rotate_mb: None,
            remote_url: None,
            redact_fields: vec!["password".into(), "secret".into(), "token".into()],
        }
    }
}

#[derive(Debug, Clone)]
pub struct ConfigError {
    pub field: String,
    pub message: String,
}

impl AhiruConfig {
    pub fn from_toml(s: &str) -> Result<Self, String> {
        toml::from_str(s).map_err(|e| e.to_string())
    }

    pub fn from_file(path: &std::path::Path) -> Result<Self, String> {
        let s = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
        Self::from_toml(&s)
    }

    pub fn load_with_env(base_path: &std::path::Path) -> Result<Self, String> {
        let _ = dotenvy::dotenv();
        let mut config = Self::from_file(base_path)?;
        if let Ok(env) = std::env::var("AHIRU_ENV") {
            let overlay = base_path.with_file_name(format!(
                "ahiru.config.{env}.toml"
            ));
            if overlay.exists() {
                let overlay_cfg = Self::from_file(&overlay)?;
                config = merge_config(config, overlay_cfg);
            }
        }
        Ok(config)
    }

    pub fn databases_map(&self) -> HashMap<String, &DatabaseConfig> {
        self.databases.iter().map(|d| (d.name.clone(), d)).collect()
    }

    pub fn validate(&self) -> Result<(), Vec<ConfigError>> {
        let mut errors = Vec::new();
        if self.server.port == 0 {
            errors.push(ConfigError {
                field: "server.port".into(),
                message: "port must be > 0".into(),
            });
        }
        for db in &self.databases {
            if !["sqlite", "postgres", "mysql"].contains(&db.driver.as_str()) {
                errors.push(ConfigError {
                    field: format!("databases.{}", db.name),
                    message: format!("unsupported driver: {}", db.driver),
                });
            }
            if let Some(role) = &db.role {
                if role != "read" && role != "write" {
                    errors.push(ConfigError {
                        field: format!("databases.{}.role", db.name),
                        message: "role must be read or write".into(),
                    });
                }
            }
        }
        for cache in &self.caches {
            if cache.driver != "memory" && cache.driver != "redis" {
                errors.push(ConfigError {
                    field: format!("caches.{}", cache.name),
                    message: format!("unsupported cache driver: {}", cache.driver),
                });
            }
            if cache.driver == "redis" && cache.url.is_none() {
                errors.push(ConfigError {
                    field: format!("caches.{}.url", cache.name),
                    message: "redis cache requires url".into(),
                });
            }
        }
        if !self.auth.mode.is_empty()
            && !["none", "jwt", "session", "api_key", "rbac", "totp", "magic_link"]
                .contains(&self.auth.mode.as_str())
        {
            errors.push(ConfigError {
                field: "auth.mode".into(),
                message: format!("unknown auth mode: {}", self.auth.mode),
            });
        }
        if self.auth.mode == "jwt" && self.auth.jwt_secret.is_none() {
            errors.push(ConfigError {
                field: "auth.jwt_secret".into(),
                message: "jwt mode requires jwt_secret".into(),
            });
        }
        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors)
        }
    }
}

fn merge_config(mut base: AhiruConfig, overlay: AhiruConfig) -> AhiruConfig {
    if overlay.server.host != "0.0.0.0" {
        base.server.host = overlay.server.host;
    }
    if overlay.server.port != 3000 {
        base.server.port = overlay.server.port;
    }
    if !overlay.databases.is_empty() {
        base.databases = overlay.databases;
    }
    if !overlay.caches.is_empty() {
        base.caches = overlay.caches;
    }
    base.auth = overlay.auth;
    base.websocket = overlay.websocket;
    base.security = overlay.security;
    base.logging = overlay.logging;
    base
}
