//! `neko ahiru doctor` — validate config, DB, port.

use super::db::config_path;
use ahiru_core::AhiruConfig;
use std::net::TcpListener;
use std::path::Path;

pub fn run_doctor(project: &Path) -> Result<(), Box<dyn std::error::Error>> {
    let cfg_path = config_path(project);
    if !cfg_path.exists() {
        eprintln!("FAIL: missing {}", cfg_path.display());
        std::process::exit(1);
    }
    let config = match AhiruConfig::load_with_env(&cfg_path) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("FAIL: config parse: {e}");
            std::process::exit(1);
        }
    };
    if let Err(errs) = config.validate() {
        for e in errs {
            eprintln!("FAIL: {} — {}", e.field, e.message);
        }
        std::process::exit(1);
    }
    println!("OK: config valid");

    let port = config.server.port;
    match TcpListener::bind(("127.0.0.1", port)) {
        Ok(_) => println!("OK: port {port} available"),
        Err(_) => eprintln!("WARN: port {port} in use"),
    }

    if !config.databases.is_empty() {
        let rt = tokio::runtime::Runtime::new()?;
        rt.block_on(async {
            match ahiru_core::DbManager::connect_all(&config.databases).await {
                Ok(db) => {
                    if db.ping().await.is_ok() {
                        println!("OK: database reachable");
                    } else {
                        eprintln!("WARN: database ping failed");
                    }
                }
                Err(e) => eprintln!("FAIL: database: {e}"),
            }
        });
    }

    println!("doctor complete");
    Ok(())
}
