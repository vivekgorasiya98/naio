use crate::handler::HandlerFn;
use crate::response::AhiruResponse;
use crate::context::RequestContext;

pub async fn run_validation(
    schema: &HandlerFn,
    ctx: RequestContext,
) -> Result<(), AhiruResponse> {
    match schema(ctx).await {
        Ok(resp) if resp.status == 200 || resp.status == 204 => Ok(()),
        Ok(resp) => Err(resp),
        Err(msg) => Err(AhiruResponse::json(
            422,
            format!(r#"{{"error":"validation failed","message":"{}","code":"E2120"}}"#, msg.replace('"', "\\\"")),
        )),
    }
}

pub fn validation_response(errors: &[String]) -> AhiruResponse {
    let body = serde_json::json!({
        "error": "validation failed",
        "code": "E2120",
        "errors": errors,
    });
    AhiruResponse::json(422, body.to_string())
}
