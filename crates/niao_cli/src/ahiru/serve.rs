use ahiru_core::{reset_shutdown, trigger_shutdown, ServeRuntimeOptions};
use niao_interpreter::Interpreter;
use niao_ast::TopLevel;
use niao_parser::parse;
use niao_runtime::ahiru::{
    self, clear_vm_pool, clear_vm_serve, finalize_vm_handlers, install_vm_pool, response_from_niao,
    set_vm_serve_active, VmPoolDispatchFn,
};
use niao_runtime::{quiet_output, set_ahiru_serve_options, set_quiet_output, apply_ahiru_cli_port};
use niao_vm::ahiru_pool::VmHandlerPool;
use niao_vm::call_bridge::{clear_thread_vm_hook, run_with_handler_hook};
use niao_vm::Vm;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use crate::cache::{default_cache_dir, load_or_compile};

#[derive(Debug, Clone)]
pub struct ServeFlags {
    pub dev: bool,
    pub net: bool,
    pub port: Option<u16>,
    /// `vm` (default) or `interp`
    pub mode: String,
}

impl Default for ServeFlags {
    fn default() -> Self {
        Self {
            dev: false,
            net: false,
            port: None,
            mode: "vm".into(),
        }
    }
}

pub fn run_serve(
    project: &Path,
    file: Option<&Path>,
    flags: ServeFlags,
) -> Result<(), Box<dyn std::error::Error>> {
    let opts = ServeRuntimeOptions {
        dev: flags.dev,
        network: flags.net,
        cli_port: flags.port,
        explicit_port: flags.port.is_some(),
    };
    set_ahiru_serve_options(opts.clone());
    if let Some(port) = flags.port {
        apply_ahiru_cli_port(port);
    }

    if flags.dev {
        eprintln!("  [dev] watching for file changes in {}", project.display());
    }

    loop {
        reset_shutdown();
        clear_vm_serve();
        clear_vm_pool();
        set_ahiru_serve_options(opts.clone());
        if let Some(port) = flags.port {
            apply_ahiru_cli_port(port);
        }

        let entry = resolve_entry(project, file)?;
        let base = entry.parent().unwrap_or(project).to_path_buf();

        let watcher = if flags.dev {
            Some(spawn_dev_watcher(project.to_path_buf()))
        } else {
            None
        };

        niao_runtime::set_program_args(vec![]);
        let use_interp = flags.mode == "interp" || has_file_imports(&entry);

        let result = if use_interp {
            run_interp_serve(&entry, &base, project)
        } else {
            run_vm_serve(&entry, &base, project)
        };

        if let Some(handle) = watcher {
            let _ = handle.join();
        }

        match result {
            Ok(()) => {
                if flags.dev {
                    eprintln!("  [dev] reloading…\n");
                    std::thread::sleep(Duration::from_millis(400));
                    continue;
                }
                break;
            }
            Err(e) => {
                if e.contains("server not started") {
                    return Ok(());
                }
                return Err(e.into());
            }
        }
    }
    Ok(())
}

fn run_interp_serve(
    entry: &Path,
    base: &Path,
    _project: &Path,
) -> Result<(), String> {
    let mut interp = Interpreter::new().with_base_dir(base.to_path_buf());
    if let Some(stdlib) = locate_stdlib_from(entry) {
        interp = interp.with_stdlib_dir(stdlib);
    }
    let result = interp.run_file_keep_hook(entry);
    let serve_result = if result.is_ok() {
        niao_runtime::start_ahiru_pending_server().map_err(|e| e.to_string())
    } else {
        Ok(())
    };
    interp.disable_call_hook();
    match (result, serve_result) {
        (Ok(_), Ok(_)) => Ok(()),
        (Err(e), _) => Err(e.to_string()),
        (_, Err(e)) => Err(e),
    }
}

fn run_vm_serve(entry: &Path, base: &Path, project: &Path) -> Result<(), String> {
    let cache_dir = project.join(default_cache_dir().file_name().unwrap_or_default());
    let cache_dir = if cache_dir.exists() {
        cache_dir
    } else {
        default_cache_dir()
    };
    let (bytecode, _) = load_or_compile(entry, &cache_dir).map_err(|e| e.to_string())?;
    let module = Arc::new(bytecode);
    set_vm_serve_active(true);

    let mut vm = Vm::new();
    let run_result = run_with_handler_hook(&mut vm, &module, base);
    finalize_vm_handlers(&|name| vm.function_index(name).map(|i| i as u32));

    let workers = project_workers(project);
    let fields_to_args = Arc::new(|fields: &std::collections::HashMap<String, String>| {
        ahiru::ctx_bridge::fields_to_niao(fields)
    });
    let handler_index = Arc::new(|id: u64| ahiru::handler_vm_index(id));
    let to_response = Arc::new(|val: &niao_runtime::Value| response_from_niao(val));
    let pool = Arc::new(VmHandlerPool::new(
        workers,
        Arc::clone(&module),
        base.to_path_buf(),
        fields_to_args,
        handler_index,
        to_response,
    ));

    let pool_dispatch = Arc::clone(&pool);
    let dispatch: VmPoolDispatchFn = Arc::new(move |handler_id, vm_index, fields, quiet| {
        let prev_quiet = quiet_output();
        if quiet {
            set_quiet_output(true);
        }
        let out = pool_dispatch.dispatch(handler_id, vm_index, fields);
        if quiet {
            set_quiet_output(prev_quiet);
        }
        out
    });
    install_vm_pool(Some(dispatch));
    clear_thread_vm_hook();

    let serve_result = if run_result.is_ok() {
        niao_runtime::start_ahiru_pending_server().map_err(|e| e.to_string())
    } else {
        Ok(())
    };

    clear_vm_pool();
    clear_vm_serve();

    match (run_result, serve_result) {
        (Ok(_), Ok(_)) => Ok(()),
        (Err(e), _) => Err(e.to_string()),
        (_, Err(e)) => Err(e),
    }
}

fn project_workers(project: &Path) -> usize {
    let config_path = project.join("ahiru.config.toml");
    if config_path.is_file() {
        if let Ok(text) = fs::read_to_string(&config_path) {
            if let Ok(config) = toml::from_str::<ahiru_core::AhiruConfig>(&text) {
                return config.server.workers.max(1);
            }
        }
    }
    std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(4)
        .max(1)
}

fn has_file_imports(file: &Path) -> bool {
    let native = niao_runtime::native_module_paths();
    let Ok(source) = fs::read_to_string(file) else {
        return false;
    };
    let Ok(program) = parse(&source) else {
        return false;
    };
    program.items.iter().any(|item| match item {
        TopLevel::Import(imp) => !native.contains(&imp.path.trim_matches('"')),
        _ => false,
    })
}

fn locate_stdlib_from(entry: &Path) -> Option<PathBuf> {
    let project = entry.parent().unwrap_or(entry);
    locate_stdlib(project)
}

fn spawn_dev_watcher(project: PathBuf) -> std::thread::JoinHandle<()> {
    std::thread::spawn(move || {
        let mut files = collect_watch_files(&project);
        let mut mtimes = snapshot_mtimes(&files);
        loop {
            std::thread::sleep(Duration::from_millis(800));
            files = collect_watch_files(&project);
            for path in &files {
                let modified = fs::metadata(path).and_then(|m| m.modified()).ok();
                let prev = mtimes.get(path).copied();
                if modified.is_some() && modified != prev {
                    eprintln!(
                        "\n  [dev] change detected: {} — stopping server…",
                        path.strip_prefix(&project)
                            .unwrap_or(path)
                            .display()
                    );
                    trigger_shutdown();
                    return;
                }
                if let Some(m) = modified {
                    mtimes.insert(path.clone(), m);
                }
            }
        }
    })
}

fn collect_watch_files(project: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for name in ["ahiru.config.toml", "niao.config"] {
        let p = project.join(name);
        if p.is_file() {
            out.push(p);
        }
    }
    let src = project.join("src");
    if src.is_dir() {
        walk_niao_files(&src, &mut out);
    }
    out
}

fn walk_niao_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            walk_niao_files(&path, out);
        } else if path.extension().is_some_and(|e| e == "niao") {
            out.push(path);
        }
    }
}

fn snapshot_mtimes(files: &[PathBuf]) -> std::collections::HashMap<PathBuf, SystemTime> {
    let mut map = std::collections::HashMap::new();
    for path in files {
        if let Ok(m) = fs::metadata(path).and_then(|meta| meta.modified()) {
            map.insert(path.clone(), m);
        }
    }
    map
}

pub fn run_migrate(project: &Path) -> Result<(), Box<dyn std::error::Error>> {
    let config_path = project.join("ahiru.config.toml");
    if !config_path.is_file() {
        return Err("ahiru.config.toml not found — run from project root".into());
    }
    let config = ahiru_core::AhiruConfig::from_file(&config_path)?;
    let report = ahiru_core::run_migrations(&config, project)?;
    if report.applied.is_empty() {
        println!("no new migrations ({} skipped)", report.skipped);
    } else {
        for m in &report.applied {
            println!("applied: {m}");
        }
    }
    Ok(())
}

pub fn run_routes(project: &Path) -> Result<(), Box<dyn std::error::Error>> {
    let config_path = project.join("ahiru.config.toml");
    if config_path.is_file() {
        let toml = fs::read_to_string(&config_path)?;
        let config: ahiru_core::AhiruConfig = toml::from_str(&toml).map_err(|e| e.to_string())?;
        println!("server: {}:{}", config.server.host, config.server.port);
        println!("auth: {}", config.auth.mode);
        println!("websocket: {}", config.websocket.mode);
        for db in &config.databases {
            println!("db {}: {} ({})", db.name, db.driver, db.url);
        }
    }
    let entry = resolve_entry(project, None)?;
    println!("\nentry: {}", entry.display());
    println!("(route table populated at runtime — use ahiru_app_routes in code)");
    Ok(())
}

fn resolve_entry(project: &Path, file: Option<&Path>) -> Result<PathBuf, Box<dyn std::error::Error>> {
    if let Some(f) = file {
        return Ok(f.to_path_buf());
    }
    let niao_config = project.join("niao.config");
    if niao_config.is_file() {
        let text = fs::read_to_string(&niao_config)?;
        for line in text.lines() {
            if let Some(rest) = line.strip_prefix("entry") {
                if let Some((_, path)) = rest.split_once('=') {
                    let path = path.trim().trim_matches('"');
                    let entry = project.join(path);
                    if entry.is_file() {
                        return Ok(entry);
                    }
                }
            }
        }
    }
    let default = project.join("src/main.niao");
    if default.is_file() {
        return Ok(default);
    }
    Err("could not find entry .niao file".into())
}

fn locate_stdlib(project: &Path) -> Option<PathBuf> {
    let mut candidates: Vec<PathBuf> = Vec::new();
    if let Ok(env) = std::env::var("NIAO_STDLIB") {
        candidates.push(PathBuf::from(env));
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            candidates.push(dir.join("stdlib"));
            candidates.push(dir.join("../../stdlib"));
        }
    }
    candidates.push(project.join("stdlib"));
    candidates.push(PathBuf::from("stdlib"));
    candidates.push(PathBuf::from("../../stdlib"));
    candidates.push(PathBuf::from("../../../stdlib"));
    candidates.into_iter().find(|p| p.join("ahiru").is_dir())
}
