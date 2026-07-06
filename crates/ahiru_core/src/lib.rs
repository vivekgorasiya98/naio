//! ahiru-server core — Tokio/Axum HTTP engine with middleware, auth, DB, and WebSocket.

mod auth;
mod banner;
mod cache;
mod config;
mod glob;
mod groups;
mod jobs;
mod logging;
mod context;
mod db;
mod handler;
mod metrics;
mod middleware;
mod migrate;
mod native;
mod port;
mod response;
mod router;
mod server;
mod shutdown;
mod state;
mod test_client;
mod validation;
mod ws;
mod ws_hub;

pub use auth::{AuthConfig, AuthMode, UserContext};
pub use banner::{log_request, print_startup_banner};
pub use cache::{CacheManager, SharedCacheManager};
pub use config::{
    AhiruConfig, CacheConfig, ConfigError, DatabaseConfig, LoggingConfig, SecurityConfig,
    ServerConfig,
};
pub use logging::LogController;
pub use migrate::{migration_status, rollback_last, run_migrations, MigrationReport, MigrationStatus};
pub use native::{native_health_handler, native_ping_handler};
pub use port::{bind_listener, PortBindPolicy, PortBindError};
pub use shutdown::{reset_shutdown, trigger_shutdown};
pub use context::RequestContext;
pub use context::{body_for_bridge, body_for_dispatch};
pub use db::{DbManager, DbPool, SharedDbManager};
pub use groups::{resource_paths, ResourceHandlers, RouteScope, ScopeRegistry};
pub use handler::{HandlerFn, HandlerResult, WsHandlerFn, WsSink};
pub use jobs::{CronScheduler, JobQueue, SharedCronScheduler, SharedJobQueue};
pub use middleware::{
    CompressionAlgo, LoggingOptions, MiddlewareEntry, MiddlewareKind, MiddlewareScope,
};
pub use metrics::prometheus_text;
pub use response::{AhiruResponse, ResponseBody};
pub use router::{HttpMethod, RouteInfo, RouteMeta};
pub use server::{
    mount_health_live, mount_health_ready, AhiruApp, ServeError, ServeRuntimeOptions,
    WorkerInterpreterPool,
};
pub use state::AppStateStore;
pub use test_client::test_request;
pub use validation::validation_response;
pub use ws::WsMode;
pub use ws_hub::{SharedWsHub, WsHub};
