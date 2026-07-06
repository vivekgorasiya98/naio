use ahiru_core::{AhiruConfig, DatabaseConfig, SecurityConfig};

fn db_cfg(
    name: &str,
    driver: &str,
    url: &str,
    pool_size: u32,
    migrations_dir: Option<String>,
) -> DatabaseConfig {
    DatabaseConfig {
        name: name.into(),
        driver: driver.into(),
        url: url.into(),
        pool_size,
        migrations_dir,
        role: None,
        max_connections: None,
        idle_timeout_secs: None,
        connect_timeout_ms: None,
    }
}
use std::fs;
use std::io::{self, Write};
use std::path::Path;

pub struct WizardAnswers {
    pub name: String,
    pub config: AhiruConfig,
    pub auth_mode: String,
    pub ws_mode: String,
    pub include_users_api: bool,
    pub include_openapi: bool,
}

pub fn run_create(name: &str, yes: bool) -> Result<(), Box<dyn std::error::Error>> {
    let answers = if yes {
        default_answers(name)
    } else {
        interactive_wizard(name)?
    };
    scaffold_project(&answers)?;
    println!("Created ahiru project '{}'", answers.name);
    println!("  cd {}", answers.name);
    println!("  neko ahiru serve");
    Ok(())
}

fn default_answers(name: &str) -> WizardAnswers {
    let mut config = AhiruConfig::default();
    config.server.port = 3000;
    config.databases = vec![db_cfg(
        "primary",
        "sqlite",
        "sqlite://data/app.db",
        10,
        Some("migrations".into()),
    )];
    config.auth.mode = "none".into();
    config.websocket.mode = "disabled".into();
    WizardAnswers {
        name: name.to_string(),
        config,
        auth_mode: "none".into(),
        ws_mode: "disabled".into(),
        include_users_api: false,
        include_openapi: false,
    }
}

fn interactive_wizard(name: &str) -> Result<WizardAnswers, Box<dyn std::error::Error>> {
    let mut config = AhiruConfig::default();
    config.server.host = prompt_default("Server host", &config.server.host)?;
    config.server.port = prompt_u16_default("Server port", config.server.port)?;
    config.server.workers = prompt_usize_default("Worker threads", config.server.workers)?;

    let db_choice = prompt_choice(
        "Database",
        &["none", "sqlite", "postgres", "mysql", "multiple"],
        1,
    )?;
    config.databases = match db_choice.as_str() {
        "none" => vec![],
        "sqlite" => vec![db_cfg(
            "primary",
            "sqlite",
            &prompt_default("SQLite path", "sqlite://data/app.db")?,
            10,
            Some("migrations".into()),
        )],
        "postgres" => vec![db_cfg(
            "primary",
            "postgres",
            &prompt_default(
                "PostgreSQL URL",
                "postgres://user:pass@localhost:5432/app",
            )?,
            prompt_u32_default("Pool size", 10)?,
            Some("migrations".into()),
        )],
        "mysql" => vec![db_cfg(
            "primary",
            "mysql",
            &prompt_default("MySQL URL", "mysql://user:pass@localhost:3306/app")?,
            prompt_u32_default("Pool size", 10)?,
            Some("migrations".into()),
        )],
        "multiple" => {
            let mut dbs = vec![db_cfg(
                "primary",
                "sqlite",
                &prompt_default("Primary SQLite URL", "sqlite://data/app.db")?,
                10,
                Some("migrations".into()),
            )];
            if prompt_yes_no("Add PostgreSQL replica/analytics DB?", false)? {
                dbs.push(db_cfg(
                    "analytics",
                    "postgres",
                    &prompt_default(
                        "Analytics PostgreSQL URL",
                        "postgres://user:pass@localhost:5432/analytics",
                    )?,
                    5,
                    None,
                ));
            }
            dbs
        }
        _ => vec![],
    };

    let auth_mode = prompt_choice(
        "Authentication",
        &["none", "jwt", "session", "api_key", "rbac"],
        0,
    )?;
    config.auth.mode = auth_mode.clone();
    config.auth.scope = prompt_choice("Auth scope", &["global", "per-route", "profile"], 0)?;
    if auth_mode == "jwt" || auth_mode == "rbac" {
        config.auth.jwt_secret = Some(prompt_default("JWT secret", "change-me-in-production")?);
        config.auth.rbac_enabled = auth_mode == "rbac";
    }
    if auth_mode == "session" {
        config.auth.session_secret =
            Some(prompt_default("Session secret", "session-change-me")?);
    }
    if auth_mode == "api_key" {
        let key = prompt_default("API key", "dev-api-key")?;
        config.auth.api_keys = vec![key];
    }

    let ws_mode = prompt_choice(
        "WebSocket",
        &["disabled", "global", "per_route"],
        0,
    )?;
    config.websocket.mode = ws_mode.clone();

    config.security = SecurityConfig {
        cors_origins: vec![prompt_default("CORS origins (* for all)", "*")?],
        rate_limit_rps: prompt_u32_default("Rate limit (req/s)", 100)?,
        secure_headers: prompt_yes_no("Enable secure headers?", true)?,
        csrf: auth_mode == "session" && prompt_yes_no("Enable CSRF protection?", false)?,
        ..Default::default()
    };

    config.logging.request_id = prompt_yes_no("Request ID header?", true)?;
    config.logging.access_log = prompt_yes_no("Access log (request lines)?", true)?;
    config.logging.startup_banner = prompt_yes_no("Startup banner?", true)?;
    config.logging.json_logs = prompt_yes_no("JSON access logs?", false)?;
    config.logging.quiet_handlers = prompt_yes_no("Suppress print() in handlers?", false)?;
    config.logging.level = prompt_default("Log level", "info")?;

    let include_users_api = prompt_yes_no("Generate REST users API module?", true)?;
    let include_openapi = prompt_yes_no("Generate OpenAPI stub?", false)?;

    Ok(WizardAnswers {
        name: name.to_string(),
        config,
        auth_mode,
        ws_mode,
        include_users_api,
        include_openapi,
    })
}

fn scaffold_project(answers: &WizardAnswers) -> Result<(), Box<dyn std::error::Error>> {
    let root = Path::new(&answers.name);
    if root.exists() {
        return Err(format!("directory '{}' already exists", answers.name).into());
    }
    fs::create_dir_all(root.join("src/routes/api"))?;
    fs::create_dir_all(root.join("migrations"))?;
    fs::create_dir_all(root.join("public"))?;
    fs::create_dir_all(root.join("tests/integration"))?;
    if answers.config.databases.iter().any(|d| d.driver == "sqlite") {
        fs::create_dir_all(root.join("data"))?;
    }

    let config_toml = toml::to_string_pretty(&answers.config)?;
    fs::write(root.join("ahiru.config.toml"), config_toml)?;

    fs::write(
        root.join("neko.config"),
        format!(
            "name = \"{}\"\nversion = \"0.1.0\"\nentry = \"src/main.neko\"\n",
            answers.name
        ),
    )?;

    let mut deps = vec!["ahiru", "json"];
    if !answers.config.databases.is_empty() {
        deps.push("nsqlite");
    }
    if answers.config.databases.iter().any(|d| d.driver == "postgres") {
        deps.push("npg");
    }
    let deps_json: String = deps
        .iter()
        .map(|d| format!("\"{d}\""))
        .collect::<Vec<_>>()
        .join(", ");
    fs::write(
        root.join("package.json"),
        format!("{{\n  \"dependencies\": [{deps_json}]\n}}\n"),
    )?;

    fs::write(root.join("src/main.neko"), render_main(answers))?;
    fs::write(root.join("src/routes/health.neko"), render_health())?;

    if answers.include_users_api {
        fs::write(root.join("src/routes/api/users.neko"), render_users(answers))?;
    }

    if !answers.config.databases.is_empty() {
        fs::write(
            root.join("migrations/001_init.sql"),
            "-- ahiru migration\nCREATE TABLE IF NOT EXISTS users (\n  id INTEGER PRIMARY KEY AUTOINCREMENT,\n  name TEXT NOT NULL,\n  email TEXT UNIQUE\n);\n",
        )?;
    }

    if answers.auth_mode != "none" {
        fs::create_dir_all(root.join("src/middleware"))?;
        fs::write(root.join("src/middleware/auth.neko"), render_auth_middleware(answers))?;
    }

    fs::write(
        root.join("tests/integration/health.neko"),
        "fn main() {\n    assert(1 == 1, \"placeholder\")\n    print(\"integration placeholder\")\n}\n",
    )?;

    if answers.include_openapi {
        fs::write(
            root.join("public/openapi.json"),
            r#"{"openapi":"3.0.0","info":{"title":"API","version":"0.1.0"},"paths":{}}"#,
        )?;
    }

    Ok(())
}

fn render_main(answers: &WizardAnswers) -> String {
    let mut s = String::from("import \"ahiru\"\nimport \"std/ahiru/v3\"\n");
    if answers.include_users_api {
        s.push_str("import \"routes/api/users\"\n");
    }
    s.push_str(
        r#"
fn main() {
    let app = ahiru_v3_create_app_from_config("ahiru.config.toml")
    ahiru_v3_use_standard_middleware(app)
"#,
    );
    if answers.auth_mode != "none" {
        s.push_str("    ahiru_app_use(app, \"secure_headers\")\n");
    }
    if !answers.config.databases.is_empty() {
        s.push_str("    ahiru_app_init_db(app)\n");
        s.push_str("    ahiru_app_init_cache(app)\n");
    }
    s.push_str("    ahiru_v3_setup_health(app, \"/health\")\n");
    s.push_str("    ahiru_native_mount_ping(app, \"/ping\")\n");
    if answers.include_users_api {
        s.push_str("    register_users_routes(app)\n");
    }
    if answers.ws_mode != "disabled" {
        s.push_str("    ahiru_app_ws(app, \"/ws\", ws_handler)\n");
    }
    s.push_str("    ahiru_app_listen(app)\n}\n");
    if answers.ws_mode != "disabled" {
        s.push_str(
            r#"
fn ws_handler(ctx) {
    return ahiru_json_response(200, "{\"ok\":true}")
}
"#,
        );
    }
    s
}

fn render_health() -> String {
    render_health_handlers()
}

fn render_health_handlers() -> String {
    r#"fn health_handler(ctx) {
    return ahiru_json_response(200, "{\"status\":\"ok\"}")
}
"#
    .to_string()
}

fn render_users(answers: &WizardAnswers) -> String {
    let perm_read = if answers.auth_mode == "rbac" {
        ", permission: \"users.read\""
    } else {
        ""
    };
    let perm_write = if answers.auth_mode == "rbac" {
        ", permission: \"users.write\""
    } else {
        ""
    };
    format!(
        r#"fn register_users_routes(app) {{
    ahiru_app_get(app, "/api/users", list_users{perm_read})
    ahiru_app_post(app, "/api/users", create_user{perm_write})
}}

fn list_users(ctx) {{
    return ahiru_json_response(200, "[]")
}}

fn create_user(ctx) {{
    return ahiru_json_response(201, "{{\"created\":true}}")
}}
"#
    )
}

fn render_auth_middleware(answers: &WizardAnswers) -> String {
    format!(
        "// Auth mode: {}\n// Enforced by ahiru.config.toml middleware chain\n",
        answers.auth_mode
    )
}

fn prompt_default(label: &str, default: &str) -> Result<String, Box<dyn std::error::Error>> {
    print!("{label} [{default}]: ");
    io::stdout().flush()?;
    let mut line = String::new();
    io::stdin().read_line(&mut line)?;
    let line = line.trim();
    if line.is_empty() {
        Ok(default.to_string())
    } else {
        Ok(line.to_string())
    }
}

fn prompt_u16_default(label: &str, default: u16) -> Result<u16, Box<dyn std::error::Error>> {
    let s = prompt_default(label, &default.to_string())?;
    Ok(s.parse().unwrap_or(default))
}

fn prompt_u32_default(label: &str, default: u32) -> Result<u32, Box<dyn std::error::Error>> {
    let s = prompt_default(label, &default.to_string())?;
    Ok(s.parse().unwrap_or(default))
}

fn prompt_usize_default(label: &str, default: usize) -> Result<usize, Box<dyn std::error::Error>> {
    let s = prompt_default(label, &default.to_string())?;
    Ok(s.parse().unwrap_or(default))
}

fn prompt_yes_no(label: &str, default: bool) -> Result<bool, Box<dyn std::error::Error>> {
    let def = if default { "Y/n" } else { "y/N" };
    let s = prompt_default(label, def)?;
    match s.to_lowercase().as_str() {
        "y" | "yes" => Ok(true),
        "n" | "no" => Ok(false),
        _ if s == def => Ok(default),
        _ => Ok(default),
    }
}

fn prompt_choice(
    label: &str,
    options: &[&str],
    default_idx: usize,
) -> Result<String, Box<dyn std::error::Error>> {
    println!("{label}:");
    for (i, opt) in options.iter().enumerate() {
        let mark = if i == default_idx { "*" } else { " " };
        println!("  {mark} {i}: {opt}");
    }
    let s = prompt_default("Choice", &default_idx.to_string())?;
    let idx: usize = s.parse().unwrap_or(default_idx);
    Ok(options.get(idx).unwrap_or(&options[default_idx]).to_string())
}
