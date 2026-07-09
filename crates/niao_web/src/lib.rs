use axum::body::Body;
use axum::http::{Response, StatusCode};
use axum::routing::{delete, get, patch, post, put};
use axum::Router;
use niao_ast::*;
use niao_interpreter::Interpreter;
use niao_parser::parse;
use std::cell::RefCell;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

thread_local! {
    static WEB_INTERP: RefCell<Option<Interpreter>> = const { RefCell::new(None) };
}

fn with_shared_interpreter<F, R>(base_dir: PathBuf, f: F) -> R
where
    F: FnOnce(&mut Interpreter) -> R,
{
    WEB_INTERP.with(|cell| {
        let mut slot = cell.borrow_mut();
        if slot.is_none() {
            *slot = Some(Interpreter::new().with_base_dir(base_dir.clone()));
        }
        let interp = slot.as_mut().expect("interpreter slot");
        interp.set_base_dir(base_dir);
        f(interp)
    })
}

#[derive(Debug)]
pub enum WebError {
    Parse(niao_parser::ParseError),
    Io(std::io::Error),
    Runtime(niao_interpreter::InterpreterError),
    NoRoutes,
}

impl std::fmt::Display for WebError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            WebError::Parse(e) => write!(f, "parse error: {e}"),
            WebError::Io(e) => write!(f, "io error: {e}"),
            WebError::Runtime(e) => write!(f, "runtime error: {e}"),
            WebError::NoRoutes => write!(f, "no server or routes defined"),
        }
    }
}

impl std::error::Error for WebError {}

impl From<niao_parser::ParseError> for WebError {
    fn from(e: niao_parser::ParseError) -> Self {
        WebError::Parse(e)
    }
}

impl From<std::io::Error> for WebError {
    fn from(e: std::io::Error) -> Self {
        WebError::Io(e)
    }
}

impl From<niao_interpreter::InterpreterError> for WebError {
    fn from(e: niao_interpreter::InterpreterError) -> Self {
        WebError::Runtime(e)
    }
}

#[derive(Clone)]
struct RouteHandler {
    source: String,
    base_dir: PathBuf,
}

pub async fn serve_web(file: &Path, default_port: u16) -> Result<(), WebError> {
    let source = fs::read_to_string(file)?;
    let program = parse(&source)?;
    let base_dir = file.parent().unwrap_or(Path::new(".")).to_path_buf();

    let mut port = default_port;
    let mut route_handlers: Vec<(HttpMethod, String, RouteHandler)> = Vec::new();

    for item in &program.items {
        match item {
            TopLevel::Server(s) => {
                for field in &s.fields {
                    if field.name == "port" {
                        if let Expr::Int(p, _) = &field.value {
                            port = *p as u16;
                        }
                    }
                }
            }
            TopLevel::Route(r) => {
                let route_source = block_to_fn_source(&r.body);
                route_handlers.push((
                    r.method,
                    r.path.clone(),
                    RouteHandler {
                        source: route_source,
                        base_dir: base_dir.clone(),
                    },
                ));
            }
            _ => {}
        }
    }

    if route_handlers.is_empty() {
        return Err(WebError::NoRoutes);
    }

    let mut app = Router::new();

    for (method, path, handler) in route_handlers {
        let h = Arc::new(handler);
        let route_path = if path == "/" {
            "/".to_string()
        } else {
            path.clone()
        };

        app = match method {
            HttpMethod::Get => {
                let h = Arc::clone(&h);
                app.route(&route_path, get(move || {
                    let h = Arc::clone(&h);
                    async move { execute_route(&h).await }
                }))
            }
            HttpMethod::Post => {
                let h = Arc::clone(&h);
                app.route(&route_path, post(move || {
                    let h = Arc::clone(&h);
                    async move { execute_route(&h).await }
                }))
            }
            HttpMethod::Put => {
                let h = Arc::clone(&h);
                app.route(&route_path, put(move || {
                    let h = Arc::clone(&h);
                    async move { execute_route(&h).await }
                }))
            }
            HttpMethod::Delete => {
                let h = Arc::clone(&h);
                app.route(&route_path, delete(move || {
                    let h = Arc::clone(&h);
                    async move { execute_route(&h).await }
                }))
            }
            HttpMethod::Patch => {
                let h = Arc::clone(&h);
                app.route(&route_path, patch(move || {
                    let h = Arc::clone(&h);
                    async move { execute_route(&h).await }
                }))
            }
        };
    }

    let addr = format!("0.0.0.0:{port}");
    println!("Niao server listening on http://localhost:{port}");
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    axum::serve(listener, app).await?;
    Ok(())
}

fn block_to_fn_source(block: &Block) -> String {
    let mut stmts = String::new();
    for stmt in &block.stmts {
        stmts.push_str(&stmt_to_source(stmt));
        stmts.push('\n');
    }
    format!("fn main() {{\n{stmts}}}")
}

fn stmt_to_source(stmt: &Stmt) -> String {
    match stmt {
        Stmt::Return { value: Some(expr), .. } => {
            format!("    return {};", expr_to_source(expr))
        }
        Stmt::Return { .. } => "    return;".into(),
        Stmt::Expr(expr) => format!("    {};", expr_to_source(expr)),
        _ => String::new(),
    }
}

fn expr_to_source(expr: &Expr) -> String {
    match expr {
        Expr::String(s, _) => format!("\"{s}\""),
        Expr::Int(v, _) => v.to_string(),
        Expr::Bool(v, _) => v.to_string(),
        Expr::Ident(n, _) => n.clone(),
        _ => "\"\"".into(),
    }
}

async fn execute_route(handler: &RouteHandler) -> Response<Body> {
    match with_shared_interpreter(handler.base_dir.clone(), |interp| {
        interp.with_call_hook(|i| i.run_source(&handler.source))
    }) {
        Ok(val) => {
            let body_text = val.borrow().to_string();
            Response::builder()
                .status(StatusCode::OK)
                .header("content-type", "text/plain")
                .body(Body::from(body_text))
                .unwrap()
        }
        Err(e) => Response::builder()
            .status(StatusCode::INTERNAL_SERVER_ERROR)
            .body(Body::from(e.to_string()))
            .unwrap(),
    }
}
