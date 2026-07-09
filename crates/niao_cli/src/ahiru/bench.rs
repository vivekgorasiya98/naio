//! `niao ahiru bench` — measure handler dispatch throughput.

use ahiru_core::{AhiruResponse, RequestContext};
use std::collections::HashMap;
use std::time::Instant;

pub fn run_bench(routes: &[String], concurrency: usize, iterations: usize) -> Result<(), Box<dyn std::error::Error>> {
    let rt = tokio::runtime::Runtime::new()?;
    for route in routes {
        let handler = match route.as_str() {
            "health" => ahiru_core::native_health_handler(),
            "ping" => ahiru_core::native_ping_handler(),
            other => {
                eprintln!("unknown route kind: {other} (use health, ping)");
                continue;
            }
        };
        let path = if route == "ping" { "/ping" } else { "/health" };
        let started = Instant::now();
        let total = per_task_total(concurrency, iterations);
        rt.block_on(async {
            let mut handles = Vec::new();
            let per_task = iterations / concurrency.max(1);
            for _ in 0..concurrency.max(1) {
                let h = handler.clone();
                let path = path.to_string();
                handles.push(tokio::spawn(async move {
                    for _ in 0..per_task {
                        let ctx = RequestContext::from_parts(
                            axum::http::Method::GET,
                            path.clone(),
                            HashMap::new(),
                            HashMap::new(),
                            axum::http::HeaderMap::new(),
                            axum::body::Bytes::new(),
                            "bench".into(),
                            HashMap::new(),
                        );
                        let _: Result<AhiruResponse, String> = h(ctx).await;
                    }
                }));
            }
            for handle in handles {
                let _ = handle.await;
            }
        });
        let elapsed = started.elapsed();
        let rps = total as f64 / elapsed.as_secs_f64().max(0.001);
        println!("{route}: {total} requests in {elapsed:?} ({rps:.0} req/s)");
    }
    Ok(())
}

fn per_task_total(concurrency: usize, iterations: usize) -> usize {
    let c = concurrency.max(1);
    let per_task = iterations / c;
    per_task * c
}
