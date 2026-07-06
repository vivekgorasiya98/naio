//! Shared Tokio runtime for blocking on MongoDB async driver calls.

use std::sync::OnceLock;

static RUNTIME: OnceLock<tokio::runtime::Runtime> = OnceLock::new();

pub fn runtime() -> &'static tokio::runtime::Runtime {
    RUNTIME.get_or_init(|| {
        tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .thread_name("nmongo")
            .build()
            .expect("failed to create nmongo tokio runtime")
    })
}

pub fn block_on<F: std::future::Future>(f: F) -> F::Output {
    runtime().block_on(f)
}
