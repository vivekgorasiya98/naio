use dashmap::DashMap;
use std::sync::Arc;

#[derive(Debug, Clone)]
pub enum CacheDriver {
    Memory,
    #[cfg(feature = "redis")]
    Redis,
}

#[derive(Clone)]
pub struct CacheManager {
    memory: Arc<DashMap<String, String>>,
    #[cfg(feature = "redis")]
    redis: Option<redis::aio::ConnectionManager>,
    default_driver: CacheDriver,
}

impl CacheManager {
    pub fn memory() -> Self {
        Self {
            memory: Arc::new(DashMap::new()),
            #[cfg(feature = "redis")]
            redis: None,
            default_driver: CacheDriver::Memory,
        }
    }

    #[cfg(feature = "redis")]
    pub async fn connect_redis(url: &str) -> Result<Self, String> {
        let client = redis::Client::open(url).map_err(|e| e.to_string())?;
        let conn = client
            .get_connection_manager()
            .await
            .map_err(|e| e.to_string())?;
        Ok(Self {
            memory: Arc::new(DashMap::new()),
            redis: Some(conn),
            default_driver: CacheDriver::Redis,
        })
    }

    pub async fn get(&self, key: &str) -> Option<String> {
        match self.default_driver {
            CacheDriver::Memory => self.memory.get(key).map(|v| v.clone()),
            #[cfg(feature = "redis")]
            CacheDriver::Redis => {
                if let Some(conn) = &self.redis {
                    use redis::AsyncCommands;
                    let mut c = conn.clone();
                    c.get(key).await.ok()
                } else {
                    None
                }
            }
        }
    }

    pub async fn set(&self, key: &str, value: &str) -> Result<(), String> {
        match self.default_driver {
            CacheDriver::Memory => {
                self.memory.insert(key.to_string(), value.to_string());
                Ok(())
            }
            #[cfg(feature = "redis")]
            CacheDriver::Redis => {
                if let Some(conn) = &self.redis {
                    use redis::AsyncCommands;
                    let mut c = conn.clone();
                    c.set(key, value).await.map_err(|e| e.to_string())
                } else {
                    Err("redis unavailable (E2301)".into())
                }
            }
        }
    }

    pub async fn incr(&self, key: &str) -> Result<i64, String> {
        match self.default_driver {
            CacheDriver::Memory => {
                let mut entry = self.memory.entry(key.to_string()).or_insert("0".into());
                let n: i64 = entry.parse().unwrap_or(0) + 1;
                *entry = n.to_string();
                Ok(n)
            }
            #[cfg(feature = "redis")]
            CacheDriver::Redis => {
                if let Some(conn) = &self.redis {
                    use redis::AsyncCommands;
                    let mut c = conn.clone();
                    c.incr(key, 1i64).await.map_err(|e| e.to_string())
                } else {
                    Err("redis unavailable (E2301)".into())
                }
            }
        }
    }

    pub async fn del(&self, key: &str) -> Result<(), String> {
        match self.default_driver {
            CacheDriver::Memory => {
                self.memory.remove(key);
                Ok(())
            }
            #[cfg(feature = "redis")]
            CacheDriver::Redis => {
                if let Some(conn) = &self.redis {
                    use redis::AsyncCommands;
                    let mut c = conn.clone();
                    c.del(key).await.map_err(|e| e.to_string())
                } else {
                    Err("redis unavailable (E2301)".into())
                }
            }
        }
    }

    pub async fn ping(&self) -> bool {
        match self.default_driver {
            CacheDriver::Memory => true,
            #[cfg(feature = "redis")]
            CacheDriver::Redis => {
                if let Some(conn) = &self.redis {
                    use redis::AsyncCommands;
                    let mut c = conn.clone();
                    redis::cmd("PING")
                        .query_async::<String>(&mut c)
                        .await
                        .is_ok()
                } else {
                    false
                }
            }
        }
    }
}

pub type SharedCacheManager = Arc<CacheManager>;
