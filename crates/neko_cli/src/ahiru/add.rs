//! `neko ahiru add <feature>` — bolt features onto existing projects.

use std::fs;
use std::path::Path;

pub fn run_add(project: &Path, feature: &str) -> Result<(), Box<dyn std::error::Error>> {
    match feature {
        "auth" => write_snippet(project, "src/routes/auth.neko", AUTH_SNIPPET)?,
        "db" => {
            write_snippet(project, "migrations/001_init.sql", DB_MIGRATION)?;
            write_snippet(project, "src/routes/db.neko", DB_SNIPPET)?;
        }
        "websocket" => write_snippet(project, "src/routes/ws.neko", WS_SNIPPET)?,
        "cache" => write_snippet(project, "src/routes/cache.neko", CACHE_SNIPPET)?,
        other => {
            eprintln!("unknown feature: {other} (try auth, db, websocket, cache)");
            std::process::exit(1);
        }
    }
    println!("added feature: {feature}");
    Ok(())
}

fn write_snippet(project: &Path, rel: &str, content: &str) -> Result<(), Box<dyn std::error::Error>> {
    let path = project.join(rel);
    if path.exists() {
        println!("skip (exists): {}", path.display());
        return Ok(());
    }
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(&path, content)?;
    println!("wrote {}", path.display());
    Ok(())
}

const AUTH_SNIPPET: &str = r#"// Auth routes — import from main.neko
fn mount_auth(app) {
    ahiru_app_post(app, "/auth/login", login_handler, { is_public: true })
}
"#;

const DB_MIGRATION: &str = "CREATE TABLE IF NOT EXISTS items (id INTEGER PRIMARY KEY, name TEXT);\n";

const DB_SNIPPET: &str = r#"fn mount_db_routes(app) {
    ahiru_app_get(app, "/items", list_items)
}
"#;

const WS_SNIPPET: &str = r#"fn mount_ws(app) {
    ahiru_app_ws(app, "/ws", ws_handler)
}
"#;

const CACHE_SNIPPET: &str = r#"fn mount_cache_demo(app) {
    ahiru_app_get(app, "/cache-demo", cache_demo_handler, { is_public: true })
}
"#;
