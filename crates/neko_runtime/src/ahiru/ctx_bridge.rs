//! Fast request-context bridge for handler dispatch (avoids per-request HashMap churn).

use crate::{Value, ValueRef};
use ahiru_core::{body_for_bridge, body_for_dispatch, RequestContext};
use std::cell::RefCell;
use std::collections::HashMap;

thread_local! {
    static CTX_FIELD_BUF: RefCell<HashMap<String, String>> = RefCell::new(HashMap::new());
}

/// Build a Neko request object from a live HTTP context (interpreter / single-thread VM path).
pub fn ctx_to_neko(ctx: &mut RequestContext) -> ValueRef {
    let body = body_for_bridge(ctx);
    build_neko_request(
        &ctx.method,
        &ctx.path,
        &body,
        &ctx.request_id,
        &ctx.params,
        &ctx.query,
        &ctx.headers,
        &ctx.state,
        ctx.user.as_ref().map(|u| (u.id.as_str(), u.roles.as_slice())),
    )
}

/// Build a Neko request object from flat pool-dispatch fields.
pub fn fields_to_neko(fields: &HashMap<String, String>) -> ValueRef {
    let mut params = HashMap::new();
    let mut query = HashMap::new();
    let mut headers = HashMap::new();
    let mut state = HashMap::new();
    for (k, v) in fields {
        if let Some(name) = k.strip_prefix("param_") {
            params.insert(name.to_string(), v.clone());
        } else if let Some(name) = k.strip_prefix("query_") {
            query.insert(name.to_string(), v.clone());
        } else if let Some(name) = k.strip_prefix("header_") {
            headers.insert(name.to_string(), v.clone());
        } else if let Some(name) = k.strip_prefix("state_") {
            state.insert(name.to_string(), v.clone());
        }
    }
    let user = fields
        .get("user_id")
        .map(|uid| (uid.as_str(), &[] as &[String]));
    build_neko_request(
        fields.get("method").map(String::as_str).unwrap_or("GET"),
        fields.get("path").map(String::as_str).unwrap_or("/"),
        fields.get("body").map(String::as_str).unwrap_or(""),
        fields
            .get("request_id")
            .map(String::as_str)
            .unwrap_or(""),
        &params,
        &query,
        &headers,
        &state,
        user,
    )
}

/// Flatten request context to string fields for worker-pool dispatch.
pub fn ctx_to_fields(ctx: &RequestContext) -> HashMap<String, String> {
    CTX_FIELD_BUF.with(|buf| {
        let mut fields = buf.borrow_mut();
        fields.clear();
        fields.insert("method".into(), ctx.method.clone());
        fields.insert("path".into(), ctx.path.clone());
        fields.insert("body".into(), body_for_dispatch(ctx));
        fields.insert("request_id".into(), ctx.request_id.clone());
        for (k, v) in &ctx.params {
            fields.insert(format!("param_{k}"), v.clone());
        }
        for (k, v) in &ctx.query {
            fields.insert(format!("query_{k}"), v.clone());
        }
        for (k, v) in &ctx.headers {
            fields.insert(format!("header_{k}"), v.clone());
        }
        for (k, v) in &ctx.state {
            fields.insert(format!("state_{k}"), v.clone());
        }
        if let Some(user) = &ctx.user {
            fields.insert("user_id".into(), user.id.clone());
        }
        fields.clone()
    })
}

fn build_neko_request(
    method: &str,
    path: &str,
    body: &str,
    request_id: &str,
    params: &HashMap<String, String>,
    query: &HashMap<String, String>,
    headers: &HashMap<String, String>,
    state: &HashMap<String, String>,
    user: Option<(&str, &[String])>,
) -> ValueRef {
    let mut map = HashMap::with_capacity(10);
    map.insert("method".into(), Value::String(method.into()).ref_cell());
    map.insert("path".into(), Value::String(path.into()).ref_cell());
    map.insert("body".into(), Value::String(body.into()).ref_cell());
    map.insert(
        "request_id".into(),
        Value::String(request_id.into()).ref_cell(),
    );

    let mut param_obj = HashMap::with_capacity(params.len());
    for (k, v) in params {
        param_obj.insert(k.clone(), Value::String(v.clone()).ref_cell());
    }
    map.insert("params".into(), Value::Object(param_obj).ref_cell());

    let mut query_obj = HashMap::with_capacity(query.len());
    for (k, v) in query {
        query_obj.insert(k.clone(), Value::String(v.clone()).ref_cell());
    }
    map.insert("query".into(), Value::Object(query_obj).ref_cell());

    let mut header_obj = HashMap::with_capacity(headers.len());
    for (k, v) in headers {
        header_obj.insert(k.clone(), Value::String(v.clone()).ref_cell());
    }
    map.insert("headers".into(), Value::Object(header_obj).ref_cell());

    let mut state_obj = HashMap::with_capacity(state.len());
    for (k, v) in state {
        state_obj.insert(k.clone(), Value::String(v.clone()).ref_cell());
    }
    map.insert("state".into(), Value::Object(state_obj).ref_cell());

    if let Some((uid, roles)) = user {
        let mut user_map = HashMap::new();
        user_map.insert("id".into(), Value::String(uid.into()).ref_cell());
        let role_vals: Vec<ValueRef> = roles
            .iter()
            .map(|r| Value::String(r.clone()).ref_cell())
            .collect();
        user_map.insert("roles".into(), Value::Array(role_vals).ref_cell());
        map.insert("user".into(), Value::Object(user_map).ref_cell());
    }

    Value::Object(map).ref_cell()
}

#[cold]
#[allow(dead_code)]
pub fn ctx_to_neko_slow(ctx: &mut RequestContext) -> ValueRef {
    ctx_to_neko(ctx)
}
