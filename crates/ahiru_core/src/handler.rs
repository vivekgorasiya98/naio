use crate::context::RequestContext;
use crate::response::AhiruResponse;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

pub type HandlerResult = Result<AhiruResponse, String>;
pub type HandlerFn = Arc<dyn Fn(RequestContext) -> Pin<Box<dyn Future<Output = HandlerResult> + Send>> + Send + Sync>;

pub type WsHandlerFn = Arc<dyn Fn(RequestContext, WsSink) -> Pin<Box<dyn Future<Output = ()> + Send>> + Send + Sync>;

pub struct WsSink {
    pub send: Arc<dyn Fn(String) -> Pin<Box<dyn Future<Output = Result<(), String>> + Send>> + Send + Sync>,
}
