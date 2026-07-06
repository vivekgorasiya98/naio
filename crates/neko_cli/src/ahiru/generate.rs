//! `neko ahiru generate resource <name>`

use std::fs;
use std::path::Path;

pub fn run_generate_resource(project: &Path, name: &str) -> Result<(), Box<dyn std::error::Error>> {
    let plural = format!("{name}s");
    let handler_path = project.join(format!("src/routes/{plural}.neko"));
    let migration_path = project.join(format!("migrations/010_create_{plural}.sql"));

    if let Some(parent) = handler_path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::create_dir_all(project.join("migrations"))?;

    let handler = format!(
        r#"// Generated resource: {name}
fn mount_{plural}(app) {{
    ahiru_app_resource(app, "/{plural}", {{
        index: {name}_index,
        show: {name}_show,
        create: {name}_create,
        update: {name}_update,
        destroy: {name}_destroy
    }})
}}

fn {name}_index(ctx) {{
    return ahiru_json_response(200, "{{\"{plural}\":[]}}")
}}

fn {name}_show(ctx) {{
    let id = ctx.params.id
    return ahiru_json_response(200, "{{\"id\":\"" + id + "\"}}")
}}

fn {name}_create(ctx) {{
    return ahiru_json_response(201, "{{\"created\":true}}")
}}

fn {name}_update(ctx) {{
    return ahiru_json_response(200, "{{\"updated\":true}}")
}}

fn {name}_destroy(ctx) {{
    return ahiru_json_response(204, "")
}}
"#
    );

    let migration = format!(
        "CREATE TABLE IF NOT EXISTS {plural} (\n  id INTEGER PRIMARY KEY AUTOINCREMENT,\n  created_at TEXT DEFAULT (datetime('now'))\n);\n"
    );

    fs::write(&handler_path, handler)?;
    fs::write(&migration_path, migration)?;
    println!("generated:");
    println!("  {}", handler_path.display());
    println!("  {}", migration_path.display());
    Ok(())
}
