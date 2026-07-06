use ahiru_core::{
    AhiruApp, AhiruConfig, AhiruResponse, HandlerFn, HttpMethod, LogController, RouteMeta,
};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::runtime::Runtime;

fn native_health_handler() -> HandlerFn {
    Arc::new(|_ctx| Box::pin(async { Ok(AhiruResponse::json(200, r#"{"status":"ok"}"#)) }))
}

fn bridge_echo_handler() -> HandlerFn {
    Arc::new(|ctx| {
        let body = ahiru_core::body_for_dispatch(&ctx);
        Box::pin(async move {
            Ok(AhiruResponse::json(
                200,
                &format!(r#"{{"echo":"{body}"}}"#),
            ))
        })
    })
}

fn bench_label(name: &str, elapsed: Duration, total: usize) {
    let rps = total as f64 / elapsed.as_secs_f64().max(0.001);
    eprintln!("{name}: {total} requests in {elapsed:?} ({rps:.0} req/s)");
}

#[test]
fn bench_simple_native_health() {
    let mut config = AhiruConfig::default();
    config.logging.access_log = false;
    config.logging.startup_banner = false;
    let mut app = AhiruApp::new(config);
    app.route(
        HttpMethod::Get,
        "/health",
        native_health_handler(),
        RouteMeta {
            public: true,
            ..Default::default()
        },
    );
    let _ = app;

    let h = native_health_handler();
    let rt = Runtime::new().unwrap();
    let started = Instant::now();
    let total = 5000;
    rt.block_on(async {
        for _ in 0..total {
            let ctx = ahiru_core::RequestContext::from_parts(
                axum::http::Method::GET,
                "/health".into(),
                HashMap::new(),
                HashMap::new(),
                axum::http::HeaderMap::new(),
                axum::body::Bytes::new(),
                "bench".into(),
                HashMap::new(),
            );
            let _ = h(ctx).await;
        }
    });
    bench_label("simple_native", started.elapsed(), total);
}

#[test]
fn bench_bridge_echo() {
    let h = bridge_echo_handler();
    let rt = Runtime::new().unwrap();
    let started = Instant::now();
    let total = 5000;
    rt.block_on(async {
        for _ in 0..total {
            let ctx = ahiru_core::RequestContext::from_parts(
                axum::http::Method::GET,
                "/echo".into(),
                HashMap::new(),
                HashMap::new(),
                axum::http::HeaderMap::new(),
                axum::body::Bytes::from_static(b"ping"),
                "bench".into(),
                HashMap::new(),
            );
            let _ = h(ctx).await;
        }
    });
    bench_label("bridge_echo", started.elapsed(), total);
}

#[test]
fn bench_logging_overhead() {
    let mut config = AhiruConfig::default();
    config.logging.access_log = false;
    let log_off = LogController::from_config(&config.logging);
    assert!(!log_off.needs_timing());

    config.logging.access_log = true;
    let log_on = LogController::from_config(&config.logging);
    assert!(log_on.needs_timing());
    assert!(log_on.should_log_request("/api/users", 12.0));
    assert!(log_on.should_log_request("/health", 12.0));
}

#[test]
fn bench_worker_interpreter_pool() {
    use ahiru_core::WorkerInterpreterPool;
    let pool = Arc::new(WorkerInterpreterPool {
        invoke: Arc::new(|fields| {
            let body = fields.get("body").cloned().unwrap_or_default();
            Ok(AhiruResponse::json(200, &format!(r#"{{"pool":"{body}"}}"#)))
        }),
    });
    let handler = pool.handler_fn();
    let rt = Runtime::new().unwrap();
    let total = 1000;
    let started = Instant::now();
    rt.block_on(async {
        for _ in 0..total {
            let ctx = ahiru_core::RequestContext::from_parts(
                axum::http::Method::GET,
                "/pool".into(),
                HashMap::new(),
                HashMap::new(),
                axum::http::HeaderMap::new(),
                axum::body::Bytes::from_static(b"ok"),
                "bench".into(),
                HashMap::new(),
            );
            let _ = handler(ctx).await;
        }
    });
    bench_label("worker_pool", started.elapsed(), total);
}

#[test]
fn bench_native_ping() {
    let h = ahiru_core::native_ping_handler();
    let rt = Runtime::new().unwrap();
    let started = Instant::now();
    let total = 5000;
    rt.block_on(async {
        for _ in 0..total {
            let ctx = ahiru_core::RequestContext::from_parts(
                axum::http::Method::GET,
                "/ping".into(),
                HashMap::new(),
                HashMap::new(),
                axum::http::HeaderMap::new(),
                axum::body::Bytes::new(),
                "bench".into(),
                HashMap::new(),
            );
            let _ = h(ctx).await;
        }
    });
    bench_label("native_ping", started.elapsed(), total);
}

#[test]
fn bench_log_controller_skip_paths() {
    let mut config = AhiruConfig::default();
    config.logging.skip_paths = vec!["/health".into(), "/ping".into()];
    let log = LogController::from_config(&config.logging);
    assert!(!log.should_log_request("/health", 1.0));
    assert!(log.should_log_request("/api", 1.0));
}
