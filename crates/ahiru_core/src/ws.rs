use crate::handler::WsHandlerFn;
use axum::extract::ws::{Message, WebSocket};
use futures_util::StreamExt;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WsMode {
    Disabled,
    Global,
    PerRoute,
}

impl WsMode {
    pub fn from_str(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "global" => WsMode::Global,
            "per_route" | "per-route" | "route" => WsMode::PerRoute,
            _ => WsMode::Disabled,
        }
    }
}

pub async fn handle_websocket(socket: WebSocket, handler: WsHandlerFn, ctx: crate::context::RequestContext) {
    let send = std::sync::Arc::new(tokio::sync::Mutex::new(socket));
    let send_fn: std::sync::Arc<
        dyn Fn(String) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<(), String>> + Send>>
            + Send
            + Sync,
    > = {
        let send = std::sync::Arc::clone(&send);
        std::sync::Arc::new(move |msg: String| {
            let send = std::sync::Arc::clone(&send);
            Box::pin(async move {
                let mut guard = send.lock().await;
                guard
                    .send(Message::Text(msg.into()))
                    .await
                    .map_err(|e| e.to_string())
            })
        })
    };
    let sink = crate::handler::WsSink { send: send_fn };
    (handler)(ctx, sink).await;

    loop {
        let mut guard = send.lock().await;
        match guard.next().await {
            Some(Ok(Message::Close(_))) | None => break,
            Some(Err(_)) => break,
            _ => {}
        }
    }
}
