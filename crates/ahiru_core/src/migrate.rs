use crate::config::AhiruConfig;
use std::path::Path;

pub struct MigrationReport {
    pub applied: Vec<String>,
    pub skipped: usize,
}

pub fn run_migrations(config: &AhiruConfig, project_dir: &Path) -> Result<MigrationReport, String> {
    let mut applied = Vec::new();
    let mut skipped = 0;

    for db in &config.databases {
        let dir = db
            .migrations_dir
            .as_deref()
            .unwrap_or("migrations");
        let mig_path = project_dir.join(dir);
        if !mig_path.exists() {
            continue;
        }
        let driver = db.driver.to_lowercase();
        if driver != "sqlite" {
            // SQLx/pg/mysql migrations tracked separately; run raw SQL files for now
        }
        let db_path = db
            .url
            .strip_prefix("sqlite://")
            .unwrap_or(&db.url);
        let db_file = project_dir.join(db_path);
        if let Some(parent) = db_file.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        let conn = rusqlite::Connection::open(&db_file).map_err(|e| e.to_string())?;
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS _ahiru_migrations (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                name TEXT NOT NULL UNIQUE,
                applied_at TEXT NOT NULL DEFAULT (datetime('now'))
            );",
        )
        .map_err(|e| e.to_string())?;

        let mut files: Vec<_> = std::fs::read_dir(&mig_path)
            .map_err(|e| e.to_string())?
            .filter_map(|e| e.ok())
            .filter(|e| e.path().extension().is_some_and(|x| x == "sql"))
            .collect();
        files.sort_by_key(|e| e.file_name());

        for entry in files {
            let name = entry.file_name().to_string_lossy().into_owned();
            let already: bool = conn
                .query_row(
                    "SELECT 1 FROM _ahiru_migrations WHERE name = ?1",
                    [&name],
                    |_| Ok(true),
                )
                .unwrap_or(false);
            if already {
                skipped += 1;
                continue;
            }
            let sql = std::fs::read_to_string(entry.path()).map_err(|e| e.to_string())?;
            conn.execute_batch(&sql).map_err(|e| format!("{name}: {e}"))?;
            conn.execute(
                "INSERT INTO _ahiru_migrations (name) VALUES (?1)",
                [&name],
            )
            .map_err(|e| e.to_string())?;
            applied.push(format!("{}:{}", db.name, name));
        }
    }

    Ok(MigrationReport { applied, skipped })
}

pub struct MigrationStatus {
    pub applied: Vec<String>,
    pub pending: Vec<String>,
}

pub fn migration_status(config: &AhiruConfig, project_dir: &Path) -> Result<Vec<MigrationStatus>, String> {
    let mut out = Vec::new();
    for db in &config.databases {
        let dir = db.migrations_dir.as_deref().unwrap_or("migrations");
        let mig_path = project_dir.join(dir);
        if !mig_path.exists() {
            continue;
        }
        let driver = db.driver.to_lowercase();
        if driver != "sqlite" {
            continue;
        }
        let db_path = db.url.strip_prefix("sqlite://").unwrap_or(&db.url);
        let db_file = project_dir.join(db_path);
        let conn = rusqlite::Connection::open(&db_file).map_err(|e| e.to_string())?;
        let mut applied = Vec::new();
        let mut pending = Vec::new();
        let mut files: Vec<_> = std::fs::read_dir(&mig_path)
            .map_err(|e| e.to_string())?
            .filter_map(|e| e.ok())
            .filter(|e| {
                e.path().extension().is_some_and(|x| x == "sql")
                    && !e.file_name().to_string_lossy().ends_with(".down.sql")
            })
            .collect();
        files.sort_by_key(|e| e.file_name());
        for entry in files {
            let name = entry.file_name().to_string_lossy().into_owned();
            let done: bool = conn
                .query_row(
                    "SELECT 1 FROM _ahiru_migrations WHERE name = ?1",
                    [&name],
                    |_| Ok(true),
                )
                .unwrap_or(false);
            if done {
                applied.push(name);
            } else {
                pending.push(name);
            }
        }
        out.push(MigrationStatus { applied, pending });
    }
    Ok(out)
}

pub fn rollback_last(config: &AhiruConfig, project_dir: &Path) -> Result<Option<String>, String> {
    for db in &config.databases {
        if db.driver.to_lowercase() != "sqlite" {
            continue;
        }
        let dir = db.migrations_dir.as_deref().unwrap_or("migrations");
        let mig_path = project_dir.join(dir);
        let db_path = db.url.strip_prefix("sqlite://").unwrap_or(&db.url);
        let db_file = project_dir.join(db_path);
        let conn = rusqlite::Connection::open(&db_file).map_err(|e| e.to_string())?;
        let last: Option<String> = conn
            .query_row(
                "SELECT name FROM _ahiru_migrations ORDER BY id DESC LIMIT 1",
                [],
                |r| r.get(0),
            )
            .ok();
        let Some(name) = last else { return Ok(None) };
        let down_path = mig_path.join(name.replace(".sql", ".down.sql"));
        if down_path.exists() {
            let sql = std::fs::read_to_string(&down_path).map_err(|e| e.to_string())?;
            conn.execute_batch(&sql).map_err(|e| e.to_string())?;
        }
        conn.execute("DELETE FROM _ahiru_migrations WHERE name = ?1", [&name])
            .map_err(|e| e.to_string())?;
        return Ok(Some(name));
    }
    Ok(None)
}

#[allow(dead_code)]
pub fn list_routes_from_config(_config: &AhiruConfig) -> Vec<String> {
    vec![]
}
