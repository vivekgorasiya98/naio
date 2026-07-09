use ahiru_core::ServeRuntimeOptions;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

static NATIVE_ROUTES: AtomicBool = AtomicBool::new(true);

static SERVE_OPTIONS: Mutex<ServeRuntimeOptions> = Mutex::new(ServeRuntimeOptions {
    dev: false,
    network: false,
    cli_port: None,
    explicit_port: false,
});

pub fn set_serve_options(opts: ServeRuntimeOptions) {
    *SERVE_OPTIONS.lock().unwrap() = opts;
}

pub fn serve_options() -> ServeRuntimeOptions {
    SERVE_OPTIONS.lock().unwrap().clone()
}

pub fn mark_explicit_port() {
    SERVE_OPTIONS.lock().unwrap().explicit_port = true;
}

pub fn apply_cli_port(port: u16) {
    let mut opts = SERVE_OPTIONS.lock().unwrap();
    opts.cli_port = Some(port);
    opts.explicit_port = true;
}

pub fn set_native_routes(enabled: bool) {
    NATIVE_ROUTES.store(enabled, Ordering::Relaxed);
}

pub fn native_routes_enabled() -> bool {
    NATIVE_ROUTES.load(Ordering::Relaxed)
}
