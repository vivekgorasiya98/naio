//! Programmatic HTTP server via `tiny_http`.

use super::{net_error, object_field, ok_nil, string_arg, NetResult};
use crate::call_niao_function;
use niao_ast::Span;
use niao_errors::codes;
use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use tiny_http::{Header, Request, Response, Server, StatusCode};

#[derive(Clone)]
pub struct RouteEntry {
    pub method: String,
    pub path: String,
    pub handler: crate::ValueRef,
}

pub struct HttpServerState {
    pub server: Arc<Server>,
    pub routes: Vec<RouteEntry>,
    pub fallback: Option<crate::ValueRef>,
    pub stop: Arc<Mutex<bool>>,
}

thread_local! {
    static HTTP_SERVERS: RefCell<HashMap<u64, HttpServerState>> = RefCell::new(HashMap::new());
    static NEXT_HTTP_SERVER: std::cell::Cell<u64> = const { std::cell::Cell::new(1) };
}

fn alloc_server(state: HttpServerState) -> u64 {
    let id = NEXT_HTTP_SERVER.with(|n| {
        let id = n.get();
        n.set(id + 1);
        id
    });
    HTTP_SERVERS.with(|m| m.borrow_mut().insert(id, state));
    id
}

fn with_server_mut<F, R>(id: u64, name: &str, span: Span, f: F) -> Result<R, crate::RuntimeError>
where
    F: FnOnce(&mut HttpServerState) -> Result<R, String>,
{
    HTTP_SERVERS.with(|m| {
        let mut guard = m.borrow_mut();
        let state = guard.get_mut(&id).ok_or_else(|| {
            crate::RuntimeError::at(
                span,
                codes::E1402_NET_INVALID_HANDLE,
                format!("{name}(): invalid http server handle {id}"),
            )
        })?;
        f(state).map_err(|msg| {
            crate::RuntimeError::at(span, codes::E1401_NET_ERROR, format!("{name}(): {msg}"))
        })
    })
}

fn build_request_object(req: &mut Request) -> crate::ValueRef {
    let mut headers = HashMap::new();
    for h in req.headers() {
        headers.insert(
            h.field.as_str().as_str().to_lowercase(),
            crate::Value::String(h.value.as_str().to_string()).ref_cell(),
        );
    }
    let mut body_bytes = Vec::new();
    let _ = req.as_reader().read_to_end(&mut body_bytes);
    let body = String::from_utf8_lossy(&body_bytes).into_owned();
    let path = req.url().to_string();
    let query = req
        .url()
        .split('?')
        .nth(1)
        .unwrap_or("")
        .to_string();
    let path_only = path.split('?').next().unwrap_or(&path).to_string();

    let mut map = HashMap::new();
    map.insert(
        "method".into(),
        crate::Value::String(req.method().as_str().to_string()).ref_cell(),
    );
    map.insert("path".into(), crate::Value::String(path_only).ref_cell());
    map.insert("query".into(), crate::Value::String(query).ref_cell());
    map.insert("body".into(), crate::Value::String(body).ref_cell());
    map.insert(
        "body_bytes".into(),
        crate::Value::IntArray(body_bytes.into_iter().map(|b| b as i64).collect()).ref_cell(),
    );
    map.insert("headers".into(), crate::Value::Object(headers).ref_cell());
    crate::Value::Object(map).ref_cell()
}

fn response_from_value(val: &crate::Value) -> Result<Response<std::io::Cursor<Vec<u8>>>, String> {
    let status = object_field(val, "status")
        .and_then(|v| match &*v.borrow() {
            crate::Value::Int(n) => Some(*n as u16),
            _ => None,
        })
        .ok_or_else(|| "response.status must be int".to_string())?;
    let content_type = object_field(val, "content_type")
        .and_then(|v| match &*v.borrow() {
            crate::Value::String(s) => Some(s.clone()),
            _ => None,
        })
        .unwrap_or_else(|| "text/plain".into());
    let body = object_field(val, "body")
        .and_then(|v| match &*v.borrow() {
            crate::Value::String(s) => Some(s.as_bytes().to_vec()),
            _ => None,
        })
        .ok_or_else(|| "response.body must be string".to_string())?;
    let mut resp = Response::from_data(body).with_status_code(StatusCode(status));
    if let Ok(header) = Header::from_bytes(b"Content-Type", content_type.as_bytes()) {
        resp.add_header(header);
    }
    Ok(resp)
}

fn dispatch(state: &HttpServerState, mut request: Request, span: Span) {
    let method = request.method().as_str().to_string();
    let path = request.url().split('?').next().unwrap_or("").to_string();
    let req_obj = build_request_object(&mut request);

    let mut response = Response::from_string("not found").with_status_code(StatusCode(404));

    for route in &state.routes {
        if route.method.eq_ignore_ascii_case(&method) && route.path == path {
            if let Ok(resp_val) = call_niao_function(route.handler.clone(), &[req_obj.clone()], span)
            {
                if let Ok(resp) = response_from_value(&resp_val.borrow()) {
                    response = resp;
                    break;
                }
            }
        }
    }

    if response.status_code() == StatusCode(404) {
        if let Some(handler) = &state.fallback {
            if let Ok(resp_val) = call_niao_function(handler.clone(), &[req_obj], span) {
                if let Ok(resp) = response_from_value(&resp_val.borrow()) {
                    response = resp;
                }
            }
        }
    }

    let _ = request.respond(response);
}

pub fn net_http_listen(args: &[crate::ValueRef], span: Span) -> NetResult {
    super::arity_range(args, 1, 2, "net_http_listen", span)?;
    let port = super::port_arg(args, 0, "net_http_listen", span)?;
    let host = if args.len() == 2 {
        match &*args[1].borrow() {
            crate::Value::String(s) => s.clone(),
            crate::Value::Object(map) => map
                .get("host")
                .and_then(|v| match &*v.borrow() {
                    crate::Value::String(s) => Some(s.clone()),
                    _ => None,
                })
                .unwrap_or_else(|| "0.0.0.0".into()),
            _ => "0.0.0.0".into(),
        }
    } else {
        "0.0.0.0".into()
    };
    let addr = format!("{host}:{port}");
    match Server::http(&addr) {
        Ok(server) => {
            let state = HttpServerState {
                server: Arc::new(server),
                routes: Vec::new(),
                fallback: None,
                stop: Arc::new(Mutex::new(false)),
            };
            Ok(crate::Value::Int(alloc_server(state) as i64).ref_cell())
        }
        Err(e) => Ok(net_error(
            span,
            codes::E1401_NET_ERROR,
            "net_error",
            e.to_string(),
        )),
    }
}

pub fn net_http_route(args: &[crate::ValueRef], span: Span) -> NetResult {
    super::arity(args, 4, "net_http_route", span)?;
    let id = super::int_arg(args, 0, "net_http_route", span)? as u64;
    let method = string_arg(args, 1, "net_http_route", span)?;
    let path = string_arg(args, 2, "net_http_route", span)?;
    let handler = args[3].clone();
    if !matches!(&*handler.borrow(), crate::Value::Function(_)) {
        return Err(super::type_err(span, "net_http_route() handler must be a function"));
    }
    with_server_mut(id, "net_http_route", span, |state| {
        state.routes.push(RouteEntry {
            method,
            path,
            handler,
        });
        Ok(ok_nil())
    })
}

pub fn net_http_on_request(args: &[crate::ValueRef], span: Span) -> NetResult {
    super::arity(args, 2, "net_http_on_request", span)?;
    let id = super::int_arg(args, 0, "net_http_on_request", span)? as u64;
    let handler = args[1].clone();
    if !matches!(&*handler.borrow(), crate::Value::Function(_)) {
        return Err(super::type_err(
            span,
            "net_http_on_request() handler must be a function",
        ));
    }
    with_server_mut(id, "net_http_on_request", span, |state| {
        state.fallback = Some(handler);
        Ok(ok_nil())
    })
}

fn serve_loop(id: u64, span: Span) -> Result<(), String> {
    let (server, routes, fallback, stop) = with_server_mut(id, "net_http_serve", span, |state| {
        Ok((
            Arc::clone(&state.server),
            state.routes.clone(),
            state.fallback.clone(),
            Arc::clone(&state.stop),
        ))
    })
    .map_err(|e| e.to_string())?;

    let state = HttpServerState {
        server,
        routes,
        fallback,
        stop,
    };

    loop {
        if *state.stop.lock().unwrap() {
            break;
        }
        match state.server.recv() {
            Ok(request) => dispatch(&state, request, span),
            Err(e) => return Err(e.to_string()),
        }
    }
    Ok(())
}

pub fn net_http_poll(args: &[crate::ValueRef], span: Span) -> NetResult {
    super::arity(args, 1, "net_http_poll", span)?;
    let id = super::int_arg(args, 0, "net_http_poll", span)? as u64;
    let (server, routes, fallback) = HTTP_SERVERS.with(|m| {
        let guard = m.borrow();
        let state = guard.get(&id).ok_or_else(|| {
            crate::RuntimeError::at(
                span,
                codes::E1402_NET_INVALID_HANDLE,
                format!("net_http_poll(): invalid http server handle {id}"),
            )
        })?;
        Ok((
            Arc::clone(&state.server),
            state.routes.clone(),
            state.fallback.clone(),
        ))
    })?;
    match server.try_recv() {
        Ok(Some(request)) => {
            let state = HttpServerState {
                server,
                routes,
                fallback,
                stop: Arc::new(Mutex::new(false)),
            };
            dispatch(&state, request, span);
        }
        Ok(None) => {}
        Err(e) => {
            return Ok(net_error(
                span,
                codes::E1404_NET_HTTP,
                "net_http_error",
                e.to_string(),
            ));
        }
    }
    Ok(ok_nil())
}

pub fn net_http_serve(args: &[crate::ValueRef], span: Span) -> NetResult {
    super::arity(args, 1, "net_http_serve", span)?;
    let id = super::int_arg(args, 0, "net_http_serve", span)? as u64;
    if let Err(e) = serve_loop(id, span) {
        return Ok(net_error(
            span,
            codes::E1404_NET_HTTP,
            "net_http_error",
            e,
        ));
    }
    Ok(ok_nil())
}

pub fn net_http_serve_async(args: &[crate::ValueRef], span: Span) -> NetResult {
    super::arity(args, 1, "net_http_serve_async", span)?;
    let id = super::int_arg(args, 0, "net_http_serve_async", span)? as u64;
    let task = crate::async_tasks::spawn_async(move || {
        serve_loop(id, span).map(|_| crate::async_tasks::AsyncValue::nil())
    });
    Ok(crate::Value::Int(task as i64).ref_cell())
}

pub fn net_http_stop(args: &[crate::ValueRef], span: Span) -> NetResult {
    super::arity(args, 1, "net_http_stop", span)?;
    let id = super::int_arg(args, 0, "net_http_stop", span)? as u64;
    with_server_mut(id, "net_http_stop", span, |state| {
        *state.stop.lock().unwrap() = true;
        Ok(ok_nil())
    })
}

pub fn net_request_field(args: &[crate::ValueRef], span: Span) -> NetResult {
    super::arity(args, 2, "net_request_field", span)?;
    let field = string_arg(args, 1, "net_request_field", span)?;
    let borrowed = args[0].borrow();
    match object_field(&borrowed, &field) {
        Some(v) => Ok(v),
        None => Err(crate::RuntimeError::at(
            span,
            codes::E1404_NET_HTTP,
            format!("unknown request field '{field}'"),
        )),
    }
}

pub fn net_response_field(args: &[crate::ValueRef], span: Span) -> NetResult {
    super::arity(args, 2, "net_response_field", span)?;
    let field = string_arg(args, 1, "net_response_field", span)?;
    let borrowed = args[0].borrow();
    match object_field(&borrowed, &field) {
        Some(v) => Ok(v),
        None => Err(crate::RuntimeError::at(
            span,
            codes::E1404_NET_HTTP,
            format!("unknown response field '{field}'"),
        )),
    }
}
