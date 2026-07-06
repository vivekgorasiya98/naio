//! `neko ahiru worker` — job queue consumer.

use std::path::Path;

pub fn run_worker(project: &Path) -> Result<(), Box<dyn std::error::Error>> {
    let main = project.join("src/main.neko");
    if !main.exists() {
        eprintln!("missing src/main.neko — worker loads app entry");
        std::process::exit(1);
    }
    println!("ahiru worker starting for {}", project.display());
    println!("run app entry to register job handlers, then poll queue");
    println!("tip: use ahiru_job_enqueue from handlers; worker polls in-process");
    let neko = std::env::current_exe()?;
    let status = std::process::Command::new(neko)
        .args([
            "run",
            "--mode",
            "interp",
            main.to_str().unwrap(),
        ])
        .status()?;
    if !status.success() {
        std::process::exit(status.code().unwrap_or(1));
    }
    Ok(())
}
