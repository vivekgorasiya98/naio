use dashmap::DashMap;
use std::any::Any;
use std::collections::HashMap;
use std::sync::Arc;

/// Shared application state — string values round-trip to Niao; native handles for Rust subsystems.
#[derive(Clone, Default)]
pub struct AppStateStore {
    pub values: Arc<DashMap<String, String>>,
    native: Arc<DashMap<String, Arc<dyn Any + Send + Sync>>>,
}

impl AppStateStore {
    pub fn new() -> Self {
        Self {
            values: Arc::new(DashMap::new()),
            native: Arc::new(DashMap::new()),
        }
    }

    pub fn set_string(&self, key: impl Into<String>, value: impl Into<String>) {
        self.values.insert(key.into(), value.into());
    }

    pub fn get_string(&self, key: &str) -> Option<String> {
        self.values.get(key).map(|v| v.clone())
    }

    pub fn set_native<T: Any + Send + Sync>(&self, key: impl Into<String>, value: T) {
        self.native.insert(key.into(), Arc::new(value));
    }

    pub fn get_native<T: Any + Send + Sync>(&self, key: &str) -> Option<Arc<T>> {
        self.native
            .get(key)
            .and_then(|v| v.clone().downcast::<T>().ok())
    }

    pub fn snapshot(&self) -> HashMap<String, String> {
        self.values.iter().map(|e| (e.key().clone(), e.value().clone())).collect()
    }
}
