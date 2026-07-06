//! Connection string parsing, TLS, and client open helpers.

use native_tls::TlsConnector;
use postgres::config::{Config, SslMode};
use postgres::tls::NoTls;
use postgres::Client;
use postgres_native_tls::MakeTlsConnector;
use r2d2_postgres::PostgresConnectionManager;
use std::collections::HashMap;
use std::time::Duration;

use super::handles::redact_conninfo;
use crate::{RuntimeError, Value, ValueRef};
use neko_ast::Span;
use neko_errors::codes;

pub fn parse_ssl_mode(s: &str) -> Result<SslMode, String> {
    match s.to_lowercase().as_str() {
        "disable" => Ok(SslMode::Disable),
        "prefer" => Ok(SslMode::Prefer),
        "require" | "verify-ca" | "verify_ca" | "verify-full" | "verify_full" => Ok(SslMode::Require),
        other => Err(format!("unknown sslmode \"{other}\"")),
    }
}

fn make_native_tls() -> Result<MakeTlsConnector, String> {
    let connector = TlsConnector::builder().build().map_err(|e| e.to_string())?;
    Ok(MakeTlsConnector::new(connector))
}

pub fn connect_config(config: &Config) -> Result<Client, String> {
    match config.get_ssl_mode() {
        SslMode::Disable => config.connect(NoTls).map_err(|e| e.to_string()),
        _ => {
            let tls = make_native_tls()?;
            config.connect(tls).map_err(|e| e.to_string())
        }
    }
}

pub fn connect_url(url: &str) -> Result<Client, String> {
    let config = url.parse::<Config>().map_err(|e| e.to_string())?;
    connect_config(&config)
}

pub fn config_from_opts(opts: &HashMap<String, ValueRef>) -> Result<(Config, String), String> {
    let mut config = Config::new();
    let mut display_parts = Vec::new();

    if let Some(url_ref) = opts.get("url").or_else(|| opts.get("connection_string")) {
        let url = match &*url_ref.borrow() {
            Value::String(s) => s.clone(),
            other => return Err(format!("url must be string, got {}", other.type_name())),
        };
        let parsed = url.parse::<Config>().map_err(|e| e.to_string())?;
        return Ok((parsed, redact_conninfo(&url)));
    }

    let host = opts
        .get("host")
        .map(|v| match &*v.borrow() {
            Value::String(s) => Ok(s.clone()),
            other => Err(format!("host must be string, got {}", other.type_name())),
        })
        .transpose()?
        .unwrap_or_else(|| "localhost".to_string());
    config.host(&host);
    display_parts.push(format!("host={host}"));

    let port = opts
        .get("port")
        .map(|v| match &*v.borrow() {
            Value::Int(n) => Ok(*n as u16),
            other => Err(format!("port must be int, got {}", other.type_name())),
        })
        .transpose()?
        .unwrap_or(5432);
    config.port(port);
    display_parts.push(format!("port={port}"));

    if let Some(user_ref) = opts.get("user") {
        let user = match &*user_ref.borrow() {
            Value::String(s) => s.clone(),
            other => return Err(format!("user must be string, got {}", other.type_name())),
        };
        config.user(&user);
        display_parts.push(format!("user={user}"));
    }

    if let Some(pw_ref) = opts.get("password") {
        let pw = match &*pw_ref.borrow() {
            Value::String(s) => s.clone(),
            other => return Err(format!("password must be string, got {}", other.type_name())),
        };
        config.password(&pw);
        display_parts.push("password=***".to_string());
    }

    if let Some(db_ref) = opts.get("database").or_else(|| opts.get("dbname")) {
        let db = match &*db_ref.borrow() {
            Value::String(s) => s.clone(),
            other => return Err(format!("database must be string, got {}", other.type_name())),
        };
        config.dbname(&db);
        display_parts.push(format!("database={db}"));
    }

    let sslmode = opts
        .get("sslmode")
        .map(|v| match &*v.borrow() {
            Value::String(s) => parse_ssl_mode(s),
            other => Err(format!("sslmode must be string, got {}", other.type_name())),
        })
        .transpose()?
        .unwrap_or(SslMode::Prefer);
    config.ssl_mode(sslmode);
    display_parts.push(format!("sslmode={sslmode:?}"));

    if let Some(ct_ref) = opts.get("connect_timeout") {
        let secs = match &*ct_ref.borrow() {
            Value::Int(n) if *n > 0 => *n as u64,
            other => {
                return Err(format!(
                    "connect_timeout must be positive int, got {}",
                    other.type_name()
                ));
            }
        };
        config.connect_timeout(Duration::from_secs(secs));
        display_parts.push(format!("connect_timeout={secs}"));
    }

    if let Some(app_ref) = opts.get("application_name") {
        let app = match &*app_ref.borrow() {
            Value::String(s) => s.clone(),
            other => {
                return Err(format!(
                    "application_name must be string, got {}",
                    other.type_name()
                ));
            }
        };
        config.application_name(&app);
        display_parts.push(format!("application_name={app}"));
    }

    Ok((config, display_parts.join(" ")))
}

pub fn parse_connect_opts(opts_ref: &ValueRef, span: Span) -> Result<(Config, String), RuntimeError> {
    let opts = match &*opts_ref.borrow() {
        Value::Object(map) => map.clone(),
        other => {
            return Err(RuntimeError::at(
                span,
                codes::E1900_NPG_ARITY,
                format!(
                    "npg.connect_opts() expects options object, got {}",
                    other.type_name()
                ),
            ));
        }
    };
    config_from_opts(&opts).map_err(|msg| RuntimeError::at(span, codes::E1907_NPG_TLS, msg))
}

pub fn pool_manager_plain(config: &Config) -> PostgresConnectionManager<NoTls> {
    PostgresConnectionManager::new(config.clone(), NoTls)
}

pub fn pool_manager_tls(config: &Config) -> Result<PostgresConnectionManager<MakeTlsConnector>, String> {
  let tls = make_native_tls()?;
  Ok(PostgresConnectionManager::new(config.clone(), tls))
}

pub fn pool_opts_from_map(
    opts: &HashMap<String, ValueRef>,
) -> Result<(Config, String, u32, u32, Option<Duration>, Duration), String> {
    let (config, display) = config_from_opts(opts)?;
    let max_size = opts
        .get("max_size")
        .map(|v| match &*v.borrow() {
            Value::Int(n) if *n > 0 => Ok(*n as u32),
            other => Err(format!("max_size must be positive int, got {}", other.type_name())),
        })
        .transpose()?
        .unwrap_or(10);
    let min_idle = opts
        .get("min_idle")
        .map(|v| match &*v.borrow() {
            Value::Int(n) if *n >= 0 => Ok(*n as u32),
            other => Err(format!("min_idle must be non-negative int, got {}", other.type_name())),
        })
        .transpose()?
        .unwrap_or(0);
    let max_lifetime = opts
        .get("max_lifetime_secs")
        .map(|v| match &*v.borrow() {
            Value::Int(n) if *n > 0 => Ok(Duration::from_secs(*n as u64)),
            other => {
                Err(format!(
                    "max_lifetime_secs must be positive int, got {}",
                    other.type_name()
                ))
            }
        })
        .transpose()?;
    let connection_timeout = opts
        .get("connection_timeout_secs")
        .map(|v| match &*v.borrow() {
            Value::Int(n) if *n > 0 => Ok(Duration::from_secs(*n as u64)),
            other => {
                Err(format!(
                    "connection_timeout_secs must be positive int, got {}",
                    other.type_name()
                ))
            }
        })
        .transpose()?
        .unwrap_or(Duration::from_secs(30));
    Ok((config, display, max_size, min_idle, max_lifetime, connection_timeout))
}
