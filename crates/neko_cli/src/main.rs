use clap::{Parser, Subcommand};
mod cache;
mod ahiru;

use cache::{build_to_cache, default_cache_dir, load_or_compile};
use neko_bytecode::compile_to_bytecode;
use neko_docs::generate_docs;
use neko_format::format_source;
use neko_interpreter::Interpreter;
use neko_lint::lint_source;
use neko_parser::parse;
use neko_runtime::QuietGuard;
use neko_vm::{run_timed, Vm};
use neko_web::serve_web;
use std::error::Error;
use std::fs;
use std::path::{Path, PathBuf};

type Result<T = ()> = std::result::Result<T, Box<dyn Error>>;

fn err(msg: impl std::fmt::Display) -> Box<dyn Error> {
    msg.to_string().into()
}

const VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Parser)]
#[command(
    name = "neko",
    version = VERSION,
    about = "Neko programming language CLI",
    arg_required_else_help = true
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

const SUBCOMMANDS: &[&str] = &[
    "run", "version", "new", "test", "format", "lint", "docs", "build", "serve", "bench", "ahiru", "help",
];

/// Append `.neko` when the path has no extension and the `.neko` variant exists.
fn resolve_neko_path(file: PathBuf) -> PathBuf {
    if !file.is_file() && file.extension().is_none() {
        let with_ext = file.with_extension("neko");
        if with_ext.is_file() {
            return with_ext;
        }
    }
    file
}

/// `neko file.neko` → `neko run file.neko`
/// `neko file` → `neko run file.neko` (when file.neko exists)
/// `neko file.neko time` → `neko run file.neko --time`
fn normalize_args(args: Vec<String>) -> Vec<String> {
    if args.len() < 2 {
        return args;
    }
    let first = args[1].as_str();
    if SUBCOMMANDS.contains(&first) || first.starts_with('-') {
        return args;
    }
    let path = Path::new(first);
    if path.extension().is_some_and(|e| e == "neko")
        || path.is_file()
        || (path.extension().is_none() && path.with_extension("neko").is_file())
    {
        let program = args[0].clone();
        let mut rest: Vec<String> = args.into_iter().skip(1).collect();
        let show_time = rest.len() > 1 && rest.last().is_some_and(|a| a == "time");
        if show_time {
            rest.pop();
        }
        let mut out = vec![program, "run".to_string()];
        out.extend(rest);
        if show_time {
            out.push("--time".to_string());
        }
        return out;
    }
    args
}

#[derive(Subcommand)]
enum Commands {
    /// Run a .neko file
    Run {
        file: PathBuf,
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
        #[arg(long, default_value = "vm")]
        mode: String,
        /// Print execution time after the program finishes
        #[arg(long, short = 't')]
        time: bool,
    },
    /// Print version
    Version,
    /// Create a new project
    New { name: String },
    /// Run tests in tests/ directory
    Test {
        #[arg(default_value = "tests")]
        dir: PathBuf,
    },
    /// Format a .neko file
    Format {
        file: PathBuf,
        #[arg(long)]
        write: bool,
    },
    /// Lint a .neko file
    Lint { file: PathBuf },
    /// Generate HTML documentation
    Docs {
        file: PathBuf,
        #[arg(short, long, default_value = "docs-output")]
        output: PathBuf,
    },
    /// Compile to bytecode
    Build {
        file: PathBuf,
        #[arg(short, long, default_value = ".neko-build")]
        output: PathBuf,
    },
    /// Run web server DSL
    Serve {
        file: PathBuf,
        #[arg(short, long, default_value_t = 3000)]
        port: u16,
    },
    /// Benchmark VM execution (build release binary first: cargo build --release)
    Bench {
        file: PathBuf,
        #[arg(short, long, default_value_t = 5)]
        runs: u32,
    },
    /// ahiru-server backend framework
    Ahiru {
        #[command(subcommand)]
        command: AhiruCommands,
    },
}

#[derive(Subcommand)]
enum AhiruCommands {
    /// Create a new ahiru-server project (interactive wizard)
    Create {
        name: String,
        /// Use defaults without prompts
        #[arg(long)]
        yes: bool,
    },
    /// Run ahiru project (VM mode by default; use --mode interp for interpreter)
    Serve {
        #[arg(long, default_value = ".")]
        project: PathBuf,
        #[arg(long)]
        file: Option<PathBuf>,
        /// Handler runtime: vm (default) or interp
        #[arg(long, default_value = "vm")]
        mode: String,
        /// Auto-reload when .neko or config files change
        #[arg(long)]
        dev: bool,
        /// Bind 0.0.0.0 and show network URL
        #[arg(long)]
        net: bool,
        /// Listen port (prompts if busy when explicitly set)
        #[arg(short, long)]
        port: Option<u16>,
    },
    /// Benchmark ahiru handler throughput
    Bench {
        #[arg(long, value_delimiter = ',', default_value = "health")]
        routes: Vec<String>,
        #[arg(long, default_value_t = 32)]
        concurrency: usize,
        #[arg(long, default_value_t = 5000)]
        iterations: usize,
    },
    /// Run SQL migrations from ahiru.config.toml
    Migrate {
        #[arg(long, default_value = ".")]
        project: PathBuf,
    },
    /// Show configured routes and databases
    Routes {
        #[arg(long, default_value = ".")]
        project: PathBuf,
    },
    /// Database operations (migrate, status, seed, rollback, reset)
    Db {
        #[command(subcommand)]
        command: AhiruDbCommands,
    },
    /// Validate config, DB connectivity, and port
    Doctor {
        #[arg(long, default_value = ".")]
        project: PathBuf,
    },
    /// Add auth/db/websocket/cache to existing project
    Add {
        feature: String,
        #[arg(long, default_value = ".")]
        project: PathBuf,
    },
    /// Scaffold REST resource (handler + migration)
    Generate {
        #[command(subcommand)]
        command: AhiruGenerateCommands,
    },
    /// Interactive REPL with project loaded
    Console {
        #[arg(long, default_value = ".")]
        project: PathBuf,
    },
    /// Emit OpenAPI spec from project
    Openapi {
        #[arg(long, default_value = ".")]
        project: PathBuf,
        #[arg(long)]
        serve: bool,
    },
    /// Run tests/**/*.neko in project
    Test {
        #[arg(long, default_value = ".")]
        project: PathBuf,
    },
    /// Run background job worker
    Worker {
        #[arg(long, default_value = ".")]
        project: PathBuf,
    },
}

#[derive(Subcommand)]
enum AhiruDbCommands {
    Migrate {
        #[arg(long, default_value = ".")]
        project: PathBuf,
    },
    Status {
        #[arg(long, default_value = ".")]
        project: PathBuf,
    },
    Seed {
        #[arg(long, default_value = ".")]
        project: PathBuf,
    },
    Rollback {
        #[arg(long, default_value = ".")]
        project: PathBuf,
    },
    Reset {
        #[arg(long, default_value = ".")]
        project: PathBuf,
        #[arg(long)]
        force: bool,
    },
}

#[derive(Subcommand)]
enum AhiruGenerateCommands {
    Resource {
        name: String,
        #[arg(long, default_value = ".")]
        project: PathBuf,
    },
}

fn main() -> Result {
    let cli = Cli::parse_from(normalize_args(std::env::args().collect()));
    match cli.command {
        Commands::Run { file, args, mode, time } => {
            run_file(&resolve_neko_path(file), &args, &mode, time)?
        }
        Commands::Version => println!("neko {VERSION}"),
        Commands::New { name } => new_project(&name)?,
        Commands::Test { dir } => run_tests(&dir)?,
        Commands::Format { file, write } => format_file(&resolve_neko_path(file), write)?,
        Commands::Lint { file } => lint_file(&resolve_neko_path(file))?,
        Commands::Docs { file, output } => docs_file(&resolve_neko_path(file), &output)?,
        Commands::Build { file, output } => build_file(&resolve_neko_path(file), &output)?,
        Commands::Serve { file, port } => serve_file(&resolve_neko_path(file), port)?,
        Commands::Bench { file, runs } => bench_file(&resolve_neko_path(file), runs)?,
        Commands::Ahiru { command } => match command {
            AhiruCommands::Create { name, yes } => ahiru::run_create(&name, yes)?,
            AhiruCommands::Serve {
                project,
                file,
                mode,
                dev,
                net,
                port,
            } => ahiru::run_serve(
                &project,
                file.as_deref(),
                ahiru::ServeFlags {
                    dev,
                    net,
                    port,
                    mode,
                },
            )?,
            AhiruCommands::Bench {
                routes,
                concurrency,
                iterations,
            } => ahiru::run_bench(&routes, concurrency, iterations)?,
            AhiruCommands::Migrate { project } => ahiru::run_migrate(&project)?,
            AhiruCommands::Routes { project } => ahiru::run_routes(&project)?,
            AhiruCommands::Db { command } => match command {
                AhiruDbCommands::Migrate { project } => ahiru::run_db_migrate(&project)?,
                AhiruDbCommands::Status { project } => ahiru::run_db_status(&project)?,
                AhiruDbCommands::Seed { project } => ahiru::run_db_seed(&project)?,
                AhiruDbCommands::Rollback { project } => ahiru::run_db_rollback(&project)?,
                AhiruDbCommands::Reset { project, force } => ahiru::run_db_reset(&project, force)?,
            },
            AhiruCommands::Doctor { project } => ahiru::run_doctor(&project)?,
            AhiruCommands::Add { feature, project } => ahiru::run_add(&project, &feature)?,
            AhiruCommands::Generate { command } => match command {
                AhiruGenerateCommands::Resource { name, project } => {
                    ahiru::run_generate_resource(&project, &name)?
                }
            },
            AhiruCommands::Console { project } => ahiru::run_console(&project)?,
            AhiruCommands::Openapi { project, serve } => ahiru::run_openapi(&project, serve)?,
            AhiruCommands::Test { project } => ahiru::run_ahiru_test(&project)?,
            AhiruCommands::Worker { project } => ahiru::run_worker(&project)?,
        },
    }
    Ok(())
}

/// True when the program imports another .neko file. The bytecode VM has no
/// module loader, so those programs must run on the interpreter.
fn uses_ahiru_server(file: &Path) -> bool {
    let Ok(source) = fs::read_to_string(file) else {
        return false;
    };
    source.contains("ahiru_app_") || source.contains("import \"ahiru\"")
}

fn has_file_imports(file: &Path) -> bool {
    let native = neko_runtime::native_module_paths();
    let Ok(source) = fs::read_to_string(file) else {
        return false;
    };
    let Ok(program) = parse(&source) else {
        return false;
    };
    program.items.iter().any(|item| match item {
        neko_ast::TopLevel::Import(imp) => {
            !native.contains(&imp.path.trim_matches('"'))
        }
        _ => false,
    })
}

fn run_file(file: &Path, script_args: &[String], mode: &str, show_time: bool) -> Result {
    neko_runtime::set_program_args(script_args.to_vec());
    if mode == "interp" || has_file_imports(file) {
        let start = std::time::Instant::now();
        let mut interp = Interpreter::new().with_base_dir(
            file.parent().unwrap_or(Path::new(".")).to_path_buf(),
        );
        interp.run_file(file).map_err(|e| err(e))?;
        if show_time {
            let ms = start.elapsed().as_secs_f64() * 1000.0;
            eprintln!("finished in {ms:.2} ms");
        }
        return Ok(());
    }

    let base_dir = file.parent().unwrap_or(Path::new("."));
    let (bytecode, compile_time) = load_or_compile(file, &default_cache_dir())?;
    let run_start = std::time::Instant::now();
    if let Some(path) = bytecode.fast_path {
        neko_vm::execute_fast_path(path);
    } else if uses_ahiru_server(file) && mode == "vm" {
        let mut vm = Vm::new();
        neko_vm::call_bridge::run_with_handler_hook(&mut vm, &bytecode, base_dir).map_err(err)?;
    } else {
        let mut vm = Vm::new();
        vm.run(&bytecode, base_dir).map_err(err)?;
    }
    if show_time {
        let compile_ms = compile_time.as_secs_f64() * 1000.0;
        let run_ms = run_start.elapsed().as_secs_f64() * 1000.0;
        eprintln!("compile: {compile_ms:.2} ms, run: {run_ms:.2} ms");
    }
    Ok(())
}

fn new_project(name: &str) -> Result {
    let project_dir = PathBuf::from(name);
    fs::create_dir_all(project_dir.join("src"))?;
    fs::create_dir_all(project_dir.join("tests"))?;
    fs::create_dir_all(project_dir.join("public"))?;

    fs::write(
        project_dir.join("neko.config"),
        format!("name = \"{name}\"\nversion = \"0.1.0\"\nentry = \"src/main.neko\"\n"),
    )?;

    fs::write(
        project_dir.join("src/main.neko"),
        r#"fn main() {
    print("Hello from Neko!")
}
"#,
    )?;

    fs::write(
        project_dir.join("tests/basic.neko"),
        r#"fn main() {
    assert(1 + 1 == 2, "math works")
    print("all tests passed")
}
"#,
    )?;

    println!("Created project '{name}'");
    println!("  cd {name}");
    println!("  neko run src/main.neko");
    Ok(())
}

fn run_tests(dir: &Path) -> Result {
    if !dir.exists() {
        return Err(err(format!("test directory not found: {}", dir.display())));
    }

    let mut passed = 0;
    let mut failed = 0;

    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.extension().is_some_and(|e| e == "neko") {
            print!("test {} ... ", path.display());
            neko_runtime::set_program_args(vec![]);
            let mut interp = Interpreter::new().with_base_dir(
                path.parent().unwrap_or(Path::new(".")).to_path_buf(),
            );
            match interp.run_file(&path) {
                Ok(_) => {
                    println!("ok");
                    passed += 1;
                }
                Err(e) => {
                    println!("FAILED");
                    eprintln!("  {e}");
                    failed += 1;
                }
            }
        }
    }

    println!("\n{passed} passed, {failed} failed");
    if failed > 0 {
        return Err(err("tests failed"));
    }
    Ok(())
}

fn format_file(file: &Path, write: bool) -> Result {
    let source = fs::read_to_string(file)?;
    let formatted = format_source(&source).map_err(err)?;
    if write {
        fs::write(file, &formatted)?;
        println!("formatted {}", file.display());
    } else {
        print!("{formatted}");
    }
    Ok(())
}

fn lint_file(file: &Path) -> Result {
    let source = fs::read_to_string(file)?;
    let issues = lint_source(&source).map_err(err)?;
    if issues.is_empty() {
        println!("no issues found");
    } else {
        for issue in &issues {
            println!(
                "{}:{}: {} - {}",
                issue.line, issue.col, issue.code, issue.message
            );
        }
        return Err(err(format!("{} lint issue(s) found", issues.len())));
    }
    Ok(())
}

fn docs_file(file: &Path, output: &Path) -> Result {
    let source = fs::read_to_string(file)?;
    let html = generate_docs(&source, file).map_err(err)?;
    fs::create_dir_all(output)?;
    let out_path = output.join("index.html");
    fs::write(&out_path, html)?;
    println!("docs generated at {}", out_path.display());
    Ok(())
}

fn build_file(file: &Path, output: &Path) -> Result {
    let out_path = build_to_cache(file, output)?;
    println!("built {}", out_path.display());
    Ok(())
}

fn bench_file(file: &Path, runs: u32) -> Result {
    let base_dir = file.parent().unwrap_or(Path::new("."));
    let (bytecode, compile_time) = load_or_compile(file, &default_cache_dir())?;
    eprintln!(
        "compile/cache: {:.2} ms",
        compile_time.as_secs_f64() * 1000.0
    );

    let (best_ms, avg_ms) = {
        let _quiet = QuietGuard::new();

        run_timed(&bytecode, base_dir).map_err(err)?; // warmup

        let mut best = std::time::Duration::MAX;
        let mut total = std::time::Duration::ZERO;
        for _ in 0..runs {
            let elapsed = run_timed(&bytecode, base_dir).map_err(err)?;
            total += elapsed;
            best = best.min(elapsed);
        }

        (
            best.as_secs_f64() * 1000.0,
            total.as_secs_f64() * 1000.0 / f64::from(runs),
        )
    };

    println!(
        "{}: best {:.2} ms, avg {:.2} ms ({} runs)",
        file.display(),
        best_ms,
        avg_ms,
        runs
    );
    Ok(())
}

fn serve_file(file: &Path, port: u16) -> Result {
    let rt = tokio::runtime::Runtime::new()?;
    rt.block_on(serve_web(file, port)).map_err(err)?;
    Ok(())
}
