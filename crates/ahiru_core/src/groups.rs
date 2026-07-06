use crate::handler::HandlerFn;
use crate::middleware::MiddlewareEntry;
use crate::router::RouteMeta;
use std::sync::Arc;

/// Scoped route registration handle — prefix and inherited options.
#[derive(Clone)]
pub struct RouteScope {
    pub id: u64,
    pub prefix: String,
    pub middleware: Vec<MiddlewareEntry>,
    pub meta_defaults: RouteMeta,
    pub auth_override: Option<String>,
}

#[derive(Clone, Default)]
pub struct ScopeRegistry {
    next_id: u64,
    scopes: std::collections::HashMap<u64, RouteScope>,
}

impl ScopeRegistry {
    pub fn new() -> Self {
        Self {
            next_id: 1,
            scopes: std::collections::HashMap::new(),
        }
    }

    pub fn create(
        &mut self,
        prefix: impl Into<String>,
        middleware: Vec<MiddlewareEntry>,
        meta_defaults: RouteMeta,
        auth_override: Option<String>,
    ) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        self.scopes.insert(
            id,
            RouteScope {
                id,
                prefix: prefix.into(),
                middleware,
                meta_defaults,
                auth_override,
            },
        );
        id
    }

    pub fn get(&self, id: u64) -> Option<&RouteScope> {
        self.scopes.get(&id)
    }

    pub fn join_path(prefix: &str, path: &str) -> String {
        let p = prefix.trim_end_matches('/');
        let s = if path.starts_with('/') {
            path.to_string()
        } else {
            format!("/{path}")
        };
        if p.is_empty() {
            s
        } else {
            format!("{p}{s}")
        }
    }
}

/// REST resource handlers.
pub struct ResourceHandlers {
    pub index: Option<HandlerFn>,
    pub show: Option<HandlerFn>,
    pub create: Option<HandlerFn>,
    pub update: Option<HandlerFn>,
    pub destroy: Option<HandlerFn>,
}

impl Default for ResourceHandlers {
    fn default() -> Self {
        Self {
            index: None,
            show: None,
            create: None,
            update: None,
            destroy: None,
        }
    }
}

pub fn resource_paths(base: &str) -> (String, String) {
    let collection = base.trim_end_matches('/').to_string();
    let member = format!("{collection}/:id");
    (collection, member)
}
