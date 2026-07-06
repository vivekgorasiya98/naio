//! Zero-cost native handlers for monitoring routes.

use crate::{AhiruResponse, HandlerFn};
use std::sync::Arc;

pub fn native_health_handler() -> HandlerFn {
    Arc::new(|_ctx| {
        Box::pin(async { Ok(AhiruResponse::json(200, r#"{"status":"ok"}"#)) })
    })
}

pub fn native_ping_handler() -> HandlerFn {
    Arc::new(|_ctx| {
        Box::pin(async { Ok(AhiruResponse::json(200, r#"{"pong":true}"#)) })
    })
}
