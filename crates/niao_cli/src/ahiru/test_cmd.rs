//! `niao ahiru test` — discover tests/**/*.niao

use std::path::Path;
use std::process::Command;

pub fn run_ahiru_test(project: &Path) -> Result<(), Box<dyn std::error::Error>> {
    let tests_dir = project.join("tests");
    if !tests_dir.exists() {
        println!("no tests/ directory");
        return Ok(());
    }
    let niao = std::env::current_exe()?;
    let mut count = 0;
    for entry in walkdir_tests(&tests_dir)? {
        println!("running {}", entry.display());
        let status = Command::new(&niao)
            .args(["run", "--mode", "interp", entry.to_str().unwrap()])
            .status()?;
        if !status.success() {
            eprintln!("FAIL: {}", entry.display());
            std::process::exit(1);
        }
        count += 1;
    }
    println!("{count} test file(s) passed");
    Ok(())
}

fn walkdir_tests(dir: &Path) -> Result<Vec<std::path::PathBuf>, Box<dyn std::error::Error>> {
    let mut out = Vec::new();
    if !dir.is_dir() {
        return Ok(out);
    }
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            out.extend(walkdir_tests(&path)?);
        } else if path.extension().is_some_and(|e| e == "niao") {
            out.push(path);
        }
    }
    Ok(out)
}
