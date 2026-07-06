use crate::config::DatabaseConfig;
use r2d2::Pool;
use r2d2_postgres::PostgresConnectionManager;
use r2d2_sqlite::SqliteConnectionManager;
use sqlx::mysql::MySqlPoolOptions;
use sqlx::postgres::PgPoolOptions;
use sqlx::{MySql, Pool as SqlxPool, Postgres};
use std::collections::HashMap;
use std::sync::Arc;

#[derive(Clone)]
pub enum DbPool {
    Sqlite(Pool<SqliteConnectionManager>),
    Postgres(Pool<PostgresConnectionManager<postgres::tls::NoTls>>),
    SqlxPostgres(SqlxPool<Postgres>),
    SqlxMysql(SqlxPool<MySql>),
}

pub struct DbManager {
    pools: HashMap<String, DbPool>,
}

impl DbManager {
    pub fn new() -> Self {
        Self {
            pools: HashMap::new(),
        }
    }

    pub async fn connect_all(configs: &[DatabaseConfig]) -> Result<Self, String> {
        let mut mgr = Self::new();
        for cfg in configs {
            let pool = mgr.connect_one(cfg).await?;
            mgr.pools.insert(cfg.name.clone(), pool);
        }
        Ok(mgr)
    }

    async fn connect_one(&self, cfg: &DatabaseConfig) -> Result<DbPool, String> {
        let driver = cfg.driver.to_lowercase();
        match driver.as_str() {
            "sqlite" => {
                let path = cfg
                    .url
                    .strip_prefix("sqlite://")
                    .unwrap_or(&cfg.url)
                    .to_string();
                let manager = SqliteConnectionManager::file(&path);
                let pool = Pool::builder()
                    .max_size(cfg.pool_size)
                    .build(manager)
                    .map_err(|e| e.to_string())?;
                Ok(DbPool::Sqlite(pool))
            }
            "postgres" | "postgresql" => {
                if cfg.url.starts_with("postgres://") || cfg.url.starts_with("postgresql://") {
                    let pool = PgPoolOptions::new()
                        .max_connections(cfg.pool_size)
                        .connect(&cfg.url)
                        .await
                        .map_err(|e| e.to_string())?;
                    return Ok(DbPool::SqlxPostgres(pool));
                }
                let config = cfg.url.parse::<postgres::Config>().map_err(|e| e.to_string())?;
                let manager = PostgresConnectionManager::new(config, postgres::tls::NoTls);
                let pool = Pool::builder()
                    .max_size(cfg.pool_size)
                    .build(manager)
                    .map_err(|e| e.to_string())?;
                Ok(DbPool::Postgres(pool))
            }
            "mysql" => {
                let pool = MySqlPoolOptions::new()
                    .max_connections(cfg.pool_size)
                    .connect(&cfg.url)
                    .await
                    .map_err(|e| e.to_string())?;
                Ok(DbPool::SqlxMysql(pool))
            }
            other => Err(format!("unsupported database driver: {other}")),
        }
    }

    pub fn get(&self, name: &str) -> Option<&DbPool> {
        self.pools.get(name)
    }

    pub fn exec_sqlite(&self, name: &str, sql: &str) -> Result<u64, String> {
        let pool = self.pools.get(name).ok_or_else(|| format!("db '{name}' not found"))?;
        match pool {
            DbPool::Sqlite(p) => {
                let conn = p.get().map_err(|e| e.to_string())?;
                conn.execute_batch(sql).map_err(|e| e.to_string())?;
                Ok(conn.changes() as u64)
            }
            _ => Err("not a sqlite pool".into()),
        }
    }

    pub fn query_sqlite(&self, name: &str, sql: &str) -> Result<Vec<HashMap<String, String>>, String> {
        let pool = self.pools.get(name).ok_or_else(|| format!("db '{name}' not found"))?;
        match pool {
            DbPool::Sqlite(p) => {
                let conn = p.get().map_err(|e| e.to_string())?;
                let mut stmt = conn.prepare(sql).map_err(|e| e.to_string())?;
                let cols: Vec<String> = stmt
                    .column_names()
                    .iter()
                    .map(|s| s.to_string())
                    .collect();
                let mut rows = Vec::new();
                let mut query = stmt.query([]).map_err(|e| e.to_string())?;
                while let Some(row) = query.next().map_err(|e| e.to_string())? {
                    let mut map = HashMap::new();
                    for (i, col) in cols.iter().enumerate() {
                        let val: String = row.get(i).unwrap_or_default();
                        map.insert(col.clone(), val);
                    }
                    rows.push(map);
                }
                Ok(rows)
            }
            _ => Err("not a sqlite pool".into()),
        }
    }

    pub async fn exec_sqlx(&self, name: &str, sql: &str) -> Result<u64, String> {
        let pool = self.pools.get(name).ok_or_else(|| format!("db '{name}' not found"))?;
        match pool {
            DbPool::SqlxPostgres(p) => {
                let r = sqlx::query(sql)
                    .execute(p)
                    .await
                    .map_err(|e| e.to_string())?;
                Ok(r.rows_affected())
            }
            DbPool::SqlxMysql(p) => {
                let r = sqlx::query(sql)
                    .execute(p)
                    .await
                    .map_err(|e| e.to_string())?;
                Ok(r.rows_affected())
            }
            _ => Err("not a sqlx pool".into()),
        }
    }

    pub async fn ping(&self) -> Result<(), String> {
        for (name, pool) in &self.pools {
            match pool {
                DbPool::Sqlite(p) => {
                    let conn = p.get().map_err(|e| e.to_string())?;
                    conn.execute_batch("SELECT 1")
                        .map_err(|e| format!("{name}: {e}"))?;
                }
                DbPool::SqlxPostgres(p) => {
                    sqlx::query("SELECT 1")
                        .execute(p)
                        .await
                        .map_err(|e| format!("{name}: {e}"))?;
                }
                DbPool::SqlxMysql(p) => {
                    sqlx::query("SELECT 1")
                        .execute(p)
                        .await
                        .map_err(|e| format!("{name}: {e}"))?;
                }
                DbPool::Postgres(p) => {
                    let mut conn = p.get().map_err(|e| e.to_string())?;
                    conn.execute("SELECT 1", &[])
                        .map_err(|e| format!("{name}: {e}"))?;
                }
            }
        }
        Ok(())
    }

    pub fn pool_for_role(&self, name: &str, role: &str) -> Option<&DbPool> {
        self.pools.get(name).or_else(|| {
            if role == "read" {
                self.pools.values().next()
            } else {
                None
            }
        })
    }
}

pub type SharedDbManager = Arc<DbManager>;
