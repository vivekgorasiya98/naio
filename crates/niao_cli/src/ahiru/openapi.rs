//! `niao ahiru openapi` — emit OpenAPI stub from project.

use ahiru_core::AhiruConfig;
use std::fs;
use std::path::Path;

pub fn run_openapi(project: &Path, serve: bool) -> Result<(), Box<dyn std::error::Error>> {
    let config = AhiruConfig::load_with_env(&project.join("ahiru.config.toml"))?;
    let spec = format!(
        r#"{{
  "openapi": "3.1.0",
  "info": {{
    "title": "ahiru API",
    "version": "0.3.0"
  }},
  "servers": [{{ "url": "http://{}:{}" }}],
  "paths": {{
    "/health": {{
      "get": {{ "summary": "Health check", "responses": {{ "200": {{ "description": "OK" }} }} }}
    }}
  }}
}}"#,
        config.server.host, config.server.port
    );
    let out = project.join("public/openapi.json");
    if let Some(parent) = out.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(&out, &spec)?;
    println!("wrote {}", out.display());
    if serve {
        println!("serve with: niao ahiru serve then mount /public via ahiru_app_static");
    }
    Ok(())
}
