use crate::server::AhiruApp;
use crate::ServeRuntimeOptions;
use axum::body::Body;
use axum::http::Request;
use tower::ServiceExt;

pub async fn test_request(
    app: &AhiruApp,
    method: &str,
    path: &str,
    body: Option<&str>,
) -> Result<(u16, String), String> {
    let router = app.build_test_router();
    let mut req = Request::builder()
        .method(method)
        .uri(path)
        .header("content-type", "application/json");
    let req = if let Some(b) = body {
        req.body(Body::from(b.to_string()))
    } else {
        req.body(Body::empty())
    }
    .map_err(|e| e.to_string())?;
    let resp = router.oneshot(req).await.map_err(|e| e.to_string())?;
    let status = resp.status().as_u16();
    let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .map_err(|e| e.to_string())?;
    Ok((status, String::from_utf8_lossy(&bytes).into_owned()))
}
