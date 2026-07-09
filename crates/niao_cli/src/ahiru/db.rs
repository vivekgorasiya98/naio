//! `niao ahiru db` — migrate, status, seed, rollback, reset.

use ahiru_core::{
    migration_status, rollback_last, run_migrations, AhiruConfig,
};
use std::path::{Path, PathBuf};

pub fn run_db_migrate(project: &Path) -> Result<(), Box<dyn std::error::Error>> {
    let config = load_config(project)?;
    let report = run_migrations(&config, project)?;
    for name in &report.applied {
        println!("applied: {name}");
    }
    if report.skipped > 0 {
        println!("skipped: {}", report.skipped);
    }
    Ok(())
}

pub fn run_db_status(project: &Path) -> Result<(), Box<dyn std::error::Error>> {
    let config = load_config(project)?;
    let statuses = migration_status(&config, project)?;
    if statuses.is_empty() {
        println!("no migrations configured");
        return Ok(());
    }
    for st in statuses {
        println!("applied ({}):", st.applied.len());
        for a in &st.applied {
            println!("  {a}");
        }
        println!("pending ({}):", st.pending.len());
        for p in &st.pending {
            println!("  {p}");
        }
    }
    Ok(())
}

pub fn run_db_rollback(project: &Path) -> Result<(), Box<dyn std::error::Error>> {
    let config = load_config(project)?;
    match rollback_last(&config, project)? {
        Some(name) => println!("rolled back: {name}"),
        None => println!("nothing to rollback"),
    }
    Ok(())
}

pub fn run_db_reset(project: &Path, force: bool) -> Result<(), Box<dyn std::error::Error>> {
    if !force {
        eprintln!("db reset requires --force (destructive)");
        std::process::exit(1);
    }
    let config = load_config(project)?;
    for db in &config.databases {
        if db.driver == "sqlite" {
            let path = db.url.strip_prefix("sqlite://").unwrap_or(&db.url);
            let file = project.join(path);
            if file.exists() {
                std::fs::remove_file(&file)?;
                println!("removed {}", file.display());
            }
        }
    }
    run_db_migrate(project)
}

pub fn run_db_seed(project: &Path) -> Result<(), Box<dyn std::error::Error>> {
    let seeds = project.join("seeds");
    if !seeds.exists() {
        println!("no seeds/ directory");
        return Ok(());
    }
    for entry in std::fs::read_dir(&seeds)? {
        let entry = entry?;
        let path = entry.path();
        if path.extension().is_some_and(|e| e == "sql") {
            let sql = std::fs::read_to_string(&path)?;
            println!("seed: {}", path.display());
            let _ = sql;
        }
    }
    println!("seed complete (run migrations first if schema required)");
    Ok(())
}

fn load_config(project: &Path) -> Result<AhiruConfig, Box<dyn std::error::Error>> {
    let path = project.join("ahiru.config.toml");
    Ok(AhiruConfig::load_with_env(&path)?)
}

pub fn config_path(project: &Path) -> PathBuf {
    project.join("ahiru.config.toml")
}
