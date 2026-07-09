//! `niao ahiru console` — REPL with project context.

use std::io::{self, Write};
use std::path::Path;

pub fn run_console(project: &Path) -> Result<(), Box<dyn std::error::Error>> {
    let main = project.join("src/main.niao");
    if !main.exists() {
        eprintln!("missing src/main.niao");
        std::process::exit(1);
    }
    println!("ahiru console (type :quit to exit)");
    println!("project: {}", project.display());
    let mut line = String::new();
    loop {
        print!("ahiru> ");
        io::stdout().flush()?;
        line.clear();
        if io::stdin().read_line(&mut line)? == 0 {
            break;
        }
        let trimmed = line.trim();
        if trimmed == ":quit" || trimmed == "exit" {
            break;
        }
        if trimmed.is_empty() {
            continue;
        }
        println!("(console eval not wired — run: niao run --mode interp {})", main.display());
    }
    Ok(())
}
