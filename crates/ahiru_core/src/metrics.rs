use std::sync::atomic::{AtomicU64, Ordering};

static REQUESTS: AtomicU64 = AtomicU64::new(0);
static ERRORS: AtomicU64 = AtomicU64::new(0);

pub fn record_request(status: u16) {
    REQUESTS.fetch_add(1, Ordering::Relaxed);
    if status >= 500 {
        ERRORS.fetch_add(1, Ordering::Relaxed);
    }
}

pub fn prometheus_text() -> String {
    let reqs = REQUESTS.load(Ordering::Relaxed);
    let errs = ERRORS.load(Ordering::Relaxed);
    format!(
        "# HELP ahiru_http_requests_total Total HTTP requests\n\
         # TYPE ahiru_http_requests_total counter\n\
         ahiru_http_requests_total {reqs}\n\
         # HELP ahiru_http_errors_total Total 5xx responses\n\
         # TYPE ahiru_http_errors_total counter\n\
         ahiru_http_errors_total {errs}\n"
    )
}

pub fn mount_metrics_path() -> &'static str {
    "/metrics"
}
