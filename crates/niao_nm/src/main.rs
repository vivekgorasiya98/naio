use clap::{Parser, Subcommand};
use niao_pkg::{
    find_project_package, find_source_root, install_from_package_json, install_global, install_libs,
    install_venv, is_remote_lib, load_catalog_optional, niao_bin_dir, niao_home, niao_libs_dir,
    registry_url, release_tool_binary, remote_libs, resolve_lib_name, standard_libs, uninstall_libs,
    update_libs, InstallMode, InstallOptions, NIAO_TOOLCHAIN_VERSION,
};
use std::env;
use std::fs;
use std::path::{Path, PathBuf};

const VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Parser)]
#[command(
    name = "nm",
    version = VERSION,
    about = "Niao package manager — install, uninstall, and manage standard libraries",
    arg_required_else_help = true
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Install toolchain, libraries, or package.json dependencies
    #[command(visible_alias = "i", visible_alias = "add")]
    Install {
        /// Library names to install (e.g. io json nos). Omit to use package.json or --global.
        #[arg(value_name = "LIB")]
        libs: Vec<String>,
        /// Install full toolchain + all standard libraries to ~/.niao
        #[arg(long)]
        global: bool,
        /// Install to project .niao/ venv
        #[arg(long)]
        venv: bool,
        /// Project directory (default: current directory)
        #[arg(long, default_value = ".")]
        project: PathBuf,
        /// Overwrite existing installs
        #[arg(long)]
        force: bool,
        /// Niao source tree (default: auto-detect repo / NIAO_SOURCE)
        #[arg(long)]
        source: Option<PathBuf>,
        /// Path to niao binary to copy (with --global)
        #[arg(long)]
        niao_bin: Option<PathBuf>,
        /// Path to nm binary to copy (with --global)
        #[arg(long)]
        nm_bin: Option<PathBuf>,
        /// Online package registry API (default: NIAO_REGISTRY or https://nms.taurus-tech.in)
        #[arg(long)]
        registry: Option<String>,
    },
    /// Uninstall one or more libraries
    #[command(visible_alias = "rm", visible_alias = "remove", visible_alias = "un")]
    Uninstall {
        /// Library names to remove (e.g. nos re)
        #[arg(value_name = "LIB", required = true)]
        libs: Vec<String>,
        #[arg(long)]
        venv: bool,
        #[arg(long, default_value = ".")]
        project: PathBuf,
        /// Allow uninstalling protected libraries (e.g. core)
        #[arg(long)]
        force: bool,
    },
    /// List installed libraries (or all standard libs with status)
    #[command(visible_alias = "ls")]
    List {
        /// Show only installed libraries
        #[arg(long)]
        installed: bool,
        /// Show only libraries not yet installed
        #[arg(long)]
        available: bool,
        #[arg(long)]
        venv: bool,
        #[arg(long, default_value = ".")]
        project: PathBuf,
    },
    /// Search the standard library catalog
    #[command(visible_alias = "find")]
    Search {
        /// Name or keyword filter (omit to list all)
        query: Option<String>,
        #[arg(long)]
        venv: bool,
        #[arg(long, default_value = ".")]
        project: PathBuf,
    },
    /// Update installed libraries to the latest version from source
    #[command(visible_alias = "up")]
    Update {
        /// Libraries to update (omit for all installed)
        #[arg(value_name = "LIB")]
        libs: Vec<String>,
        #[arg(long)]
        venv: bool,
        #[arg(long, default_value = ".")]
        project: PathBuf,
        /// Reinstall even when already at the latest version
        #[arg(long)]
        force: bool,
        #[arg(long)]
        source: Option<PathBuf>,
        /// Online package registry API (default: NIAO_REGISTRY or https://nms.taurus-tech.in)
        #[arg(long)]
        registry: Option<String>,
    },
    Version {
        #[arg(long)]
        venv: bool,
        #[arg(long, default_value = ".")]
        project: PathBuf,
    },
    /// Show details for one library (`nm info <name>` or `nm --info <name>`)
    Info {
        name: String,
        #[arg(long)]
        venv: bool,
        #[arg(long, default_value = ".")]
        project: PathBuf,
    },
    /// Initialize a project venv (.niao/)
    Venv {
        #[arg(long, default_value = ".")]
        project: PathBuf,
        #[arg(long)]
        force: bool,
    },
    /// Print niao home directory
    Home,
    /// Print detected Niao source root
    Source,
}

fn main() {
    if let Err(e) = run() {
        eprintln!("nm error: {e}");
        std::process::exit(1);
    }
}

/// `nm --info libname` → `nm info libname`
fn normalize_args(args: Vec<String>) -> Vec<String> {
    if args.len() >= 3 && args[1] == "--info" {
        let mut out = vec![args[0].clone(), "info".to_string()];
        out.extend(args.into_iter().skip(2));
        return out;
    }
    args
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse_from(normalize_args(std::env::args().collect()));
    match cli.command {
        Commands::Install {
            libs,
            global,
            venv,
            project,
            force,
            source,
            niao_bin,
            nm_bin,
            registry,
        } => cmd_install(libs, global, venv, &project, force, source, niao_bin, nm_bin, registry),
        Commands::Uninstall {
            libs,
            venv,
            project,
            force,
        } => cmd_uninstall(libs, venv, &project, force),
        Commands::List {
            installed,
            available,
            venv,
            project,
        } => cmd_list(installed, available, venv, &project),
        Commands::Search {
            query,
            venv,
            project,
        } => cmd_search(query.as_deref(), venv, &project),
        Commands::Update {
            libs,
            venv,
            project,
            force,
            source,
            registry,
        } => cmd_update(libs, venv, &project, force, source, registry),
        Commands::Version { venv, project } => cmd_version(venv, &project),
        Commands::Info { name, venv, project } => cmd_info(&name, venv, &project),
        Commands::Venv { project, force } => cmd_venv(&project, force),
        Commands::Home => {
            println!("{}", niao_home().display());
            Ok(())
        }
        Commands::Source => {
            match find_source_root() {
                Some(p) => println!("{}", p.display()),
                None => {
                    eprintln!("Niao source not found. Set NIAO_SOURCE or run from the repo.");
                    std::process::exit(1);
                }
            }
            Ok(())
        }
    }
}

fn cmd_install(
    libs: Vec<String>,
    global: bool,
    venv: bool,
    project: &Path,
    force: bool,
    source: Option<PathBuf>,
    niao_bin: Option<PathBuf>,
    nm_bin: Option<PathBuf>,
    registry: Option<String>,
) -> Result<(), Box<dyn std::error::Error>> {
    let source_for_bins = source.clone().or_else(find_source_root);
    let (niao_bin, nm_bin) = if let Some(ref src) = source_for_bins {
        resolve_tool_binaries(src, niao_bin, nm_bin)
    } else {
        (
            niao_bin.or_else(|| find_sibling_binary("niao")),
            nm_bin.or_else(|| find_sibling_binary("nm")),
        )
    };

    let has_package_json = find_project_package(project).is_some();
    let use_venv = venv || (!global && libs.is_empty() && has_package_json);
    let use_global = global || (libs.is_empty() && !use_venv && !has_package_json);

    let opts = install_opts(use_venv, project, force, source, niao_bin.clone(), nm_bin.clone(), registry);

    let report = if !libs.is_empty() {
        install_libs(&libs, &opts)?
    } else if has_package_json && !use_global {
        install_from_package_json(project, &opts)?
    } else if use_global {
        install_global(&opts)?
    } else {
        return Err(
            "nothing to install — use `nm install nos`, `nm install --global`, or add package.json"
                .into(),
        );
    };

    print_install_report(&report, &niao_bin, &nm_bin)?;
    Ok(())
}

fn cmd_uninstall(
    libs: Vec<String>,
    venv: bool,
    project: &Path,
    force: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let opts = InstallOptions {
        mode: if venv {
            InstallMode::Venv
        } else {
            InstallMode::Global
        },
        project_dir: if venv {
            Some(project.to_path_buf())
        } else {
            None
        },
        niao_bin: None,
        nm_bin: None,
        force,
        source_root: None,
        registry_url: None,
    };

    let report = uninstall_libs(&libs, &opts)?;
    let mode_label = if report.mode == InstallMode::Global {
        "global"
    } else {
        "venv"
    };
    println!("nm uninstall ({mode_label}) complete");
    println!("  root:      {}", report.root.display());
    println!("  removed:   {}", report.libs_removed.join(", "));
    if !report.not_installed.is_empty() {
        println!("  skipped:   {} (not installed)", report.not_installed.join(", "));
    }
    Ok(())
}

fn cmd_list(
    installed_only: bool,
    available_only: bool,
    venv: bool,
    project: &Path,
) -> Result<(), Box<dyn std::error::Error>> {
    let catalog = load_catalog_optional(venv, project);
    let has_install = !catalog.libs.is_empty();

    if has_install {
        println!(
            "niao {} — {} installed",
            catalog.niao_version,
            catalog.libs.len()
        );
    } else {
        println!("niao {NIAO_TOOLCHAIN_VERSION} — no libraries installed yet");
        println!("  run `nm install --global` or `nm install <lib>` to get started");
    }

    for spec in standard_libs() {
        print_lib_line(
            &catalog,
            installed_only,
            available_only,
            &spec.name,
            &spec.version,
            &spec.description,
            &format!("{:?}", spec.kind),
            spec.builtin_count,
        );
    }

    for name in remote_libs() {
        if standard_libs().iter().any(|s| s.name == *name) {
            continue;
        }
        let description = format!("online library — install via registry ({})", registry_url());
        print_lib_line(
            &catalog,
            installed_only,
            available_only,
            name,
            NIAO_TOOLCHAIN_VERSION,
            &description,
            "native",
            0,
        );
    }

    Ok(())
}

fn print_lib_line(
    catalog: &niao_pkg::LibsCatalog,
    installed_only: bool,
    available_only: bool,
    name: &str,
    version: &str,
    description: &str,
    kind: &str,
    builtin_count: usize,
) {
    let is_installed = catalog.libs.contains_key(name);
    if installed_only && !is_installed {
        return;
    }
    if available_only && is_installed {
        return;
    }

    if let Some(lib) = catalog.libs.get(name) {
        println!(
            "  [installed] {} {} — {} ({}, {} builtins)",
            lib.name, lib.version, lib.description, kind, lib.builtin_count
        );
    } else {
        let tag = if is_remote_lib(name) { "remote" } else { "available" };
        println!("  [{tag}] {} {} — {}", name, version, description);
        if is_remote_lib(name) {
            println!("      registry: {}", registry_url());
        }
        let _ = builtin_count;
    }
}

fn cmd_search(
    query: Option<&str>,
    venv: bool,
    project: &Path,
) -> Result<(), Box<dyn std::error::Error>> {
    let catalog = load_catalog_optional(venv, project);
    let q = query.unwrap_or("").trim().to_lowercase();
    let mut matches = 0usize;

    for spec in standard_libs() {
        let hay = format!("{} {}", spec.name, spec.description).to_lowercase();
        if !q.is_empty() && !hay.contains(&q) {
            continue;
        }
        matches += 1;
        let status = if catalog.libs.contains_key(&spec.name) {
            "installed"
        } else {
            "available"
        };
        println!(
            "  {} {} {} — {} [{}]",
            if status == "installed" { "+" } else { "-" },
            spec.name,
            spec.version,
            spec.description,
            status
        );
        if !spec.import_paths.is_empty() {
            println!("      import: {}", spec.import_paths.join(", "));
        }
    }

    if matches == 0 {
        if let Some(q) = query.filter(|s| !s.trim().is_empty()) {
            println!("no libraries match '{q}'");
        }
        println!("try: nm search io   or   nm search os");
    } else {
        println!("\n{matches} librar{} found", if matches == 1 { "y" } else { "ies" });
        println!("install: nm install <name>   uninstall: nm uninstall <name>");
    }
    Ok(())
}

fn cmd_update(
    libs: Vec<String>,
    venv: bool,
    project: &Path,
    force: bool,
    source: Option<PathBuf>,
    registry: Option<String>,
) -> Result<(), Box<dyn std::error::Error>> {
    let names: Vec<String> = libs
        .into_iter()
        .map(|n| resolve_lib_name(&n))
        .collect();

    let mut opts = install_opts(venv, project, force, source, None, None, registry);
    opts.force = force;
    if let Some(src) = opts.source_root.clone().or_else(find_source_root) {
        let (niao_bin, nm_bin) = resolve_tool_binaries(&src, None, None);
        opts.niao_bin = niao_bin;
        opts.nm_bin = nm_bin;
    }

    let report = update_libs(&names, &opts)?;

    let mode_label = if report.mode == InstallMode::Global {
        "global"
    } else {
        "venv"
    };
    println!("nm update ({mode_label})");
    println!("  source: {}", report.source_root.display());
    println!("  root:   {}", report.root.display());

    if !report.upgraded.is_empty() {
        println!("  updated:");
        for (name, from, to) in &report.upgraded {
            if from == "none" {
                println!("    {name} -> {to} (installed)");
            } else {
                println!("    {name} {from} -> {to}");
            }
        }
    }
    if !report.up_to_date.is_empty() {
        println!("  up to date: {}", report.up_to_date.join(", "));
    }
    if !report.not_installed.is_empty() {
        println!(
            "  skipped (not in source): {}",
            report.not_installed.join(", ")
        );
    }
    if report.upgraded.is_empty() && report.not_installed.is_empty() {
        println!("  all libraries are already at the latest version");
    }
    if let Some(p) = &report.niao_bin {
        println!("  niao binary:  {}", p.display());
    }
    if let Some(p) = &report.nm_bin {
        println!("  nm binary:    {}", p.display());
    }
    if report.mode == InstallMode::Global && (report.niao_bin.is_some() || report.nm_bin.is_some())
    {
        copy_to_cargo_bin(&report.niao_bin, &report.nm_bin)?;
        print_path_hint();
    }
    Ok(())
}

fn cmd_version(venv: bool, project: &Path) -> Result<(), Box<dyn std::error::Error>> {
    println!("nm (package manager) {VERSION}");
    println!("niao toolchain       {NIAO_TOOLCHAIN_VERSION}");

    if let Ok(state) = load_state_for(venv, project) {
        println!("installed niao       {}", state.niao_version);
        println!("install mode         {}", state.mode);
        println!("install root         {}", state.root);
        if !state.source_root.is_empty() {
            println!("source root          {}", state.source_root);
        }
        println!("installed libs       {}", state.libs.len());
    } else {
        println!("install state        (not installed — run `nm install --global`)");
    }

    if let Some(src) = find_source_root() {
        println!("detected source      {}", src.display());
    }

    println!("\nstandard libraries ({}):", standard_libs().len());
    for spec in standard_libs() {
        println!("  {} {}", spec.name, spec.version);
    }
    Ok(())
}

fn cmd_info(name: &str, venv: bool, project: &Path) -> Result<(), Box<dyn std::error::Error>> {
    let name = resolve_lib_name(name);
    let catalog = load_catalog_optional(venv, project);

    if let Some(lib) = catalog.libs.get(&name) {
        print_lib_details(lib, true);
        return Ok(());
    }

    if let Some(spec) = standard_libs().into_iter().find(|s| s.name == name) {
        println!("name:         {}", spec.name);
        println!("version:      {}", spec.version);
        println!("kind:         {:?}", spec.kind);
        println!("description:  {}", spec.description);
        println!("builtins:     {}", spec.builtin_count);
        if !spec.import_paths.is_empty() {
            println!("import paths: {}", spec.import_paths.join(", "));
        }
        println!("status:       not installed — run `nm install {name}`");
        return Ok(());
    }

    Err(format!("unknown library '{name}' — run `nm search` to list available libraries").into())
}

fn print_lib_details(lib: &niao_pkg::InstalledLib, installed: bool) {
    println!("name:         {}", lib.name);
    println!("version:      {}", lib.version);
    println!("kind:         {:?}", lib.kind);
    println!("description:  {}", lib.description);
    println!("builtins:     {}", lib.builtin_count);
    if !lib.import_paths.is_empty() {
        println!("import paths: {}", lib.import_paths.join(", "));
    }
    if installed {
        println!("installed_at: {}", lib.installed_at);
        println!("status:       installed");
    }
}

fn cmd_venv(project: &Path, force: bool) -> Result<(), Box<dyn std::error::Error>> {
    let opts = InstallOptions {
        mode: InstallMode::Venv,
        project_dir: Some(project.to_path_buf()),
        niao_bin: None,
        nm_bin: None,
        force,
        source_root: None,
        registry_url: None,
    };
    let report = install_venv(project, &opts)?;
    println!("venv created at {}", report.root.display());
    println!("libraries: {}", report.libs_installed.join(", "));
    Ok(())
}

fn install_opts(
    venv: bool,
    project: &Path,
    force: bool,
    source: Option<PathBuf>,
    niao_bin: Option<PathBuf>,
    nm_bin: Option<PathBuf>,
    registry: Option<String>,
) -> InstallOptions {
    InstallOptions {
        mode: if venv {
            InstallMode::Venv
        } else {
            InstallMode::Global
        },
        project_dir: if venv {
            Some(project.to_path_buf())
        } else {
            None
        },
        niao_bin,
        nm_bin,
        force,
        source_root: source,
        registry_url: registry,
    }
}

fn print_install_report(
    report: &niao_pkg::InstallReport,
    niao_bin: &Option<PathBuf>,
    nm_bin: &Option<PathBuf>,
) -> Result<(), Box<dyn std::error::Error>> {
    let mode_label = if report.mode == InstallMode::Global {
        "Global"
    } else {
        "Venv"
    };
    println!("nm install ({mode_label}) complete");
    println!("  niao version: {}", report.niao_version);
    println!("  source:       {}", report.source_root.display());
    println!("  root:         {}", report.root.display());
    println!("  niao_libs:    {}", install_libs_display(report));
    println!("  libraries:    {}", report.libs_installed.join(", "));

    if let Some(p) = &report.niao_bin {
        println!("  niao binary:  {}", p.display());
        print_path_hint();
    }
    if let Some(p) = &report.nm_bin {
        println!("  nm binary:    {}", p.display());
    }

    if report.mode == InstallMode::Global && (report.niao_bin.is_some() || report.nm_bin.is_some())
    {
        copy_to_cargo_bin(niao_bin, nm_bin)?;
    }
    if report.libs_installed.iter().any(|n| is_remote_lib(n)) {
        println!("  registry:     {}", registry_url());
    }
    Ok(())
}

fn install_libs_display(report: &niao_pkg::InstallReport) -> String {
    if report.mode == InstallMode::Global {
        niao_libs_dir().display().to_string()
    } else {
        report.root.join("niao_libs").display().to_string()
    }
}

fn load_state_for(
    venv: bool,
    project: &Path,
) -> Result<niao_pkg::InstallState, Box<dyn std::error::Error>> {
    use niao_pkg::{global_install_state_path, load_install_state, venv_install_state_path};
    let path = if venv {
        venv_install_state_path(project)
    } else {
        global_install_state_path()
    };
    Ok(load_install_state(&path)?)
}

fn resolve_tool_binaries(
    source: &Path,
    niao_bin: Option<PathBuf>,
    nm_bin: Option<PathBuf>,
) -> (Option<PathBuf>, Option<PathBuf>) {
    let niao = niao_bin
        .or_else(|| release_tool_binary(source, "niao"))
        .or_else(|| find_sibling_binary("niao"));
    let nm = nm_bin
        .or_else(|| release_tool_binary(source, "nm"))
        .or_else(|| find_sibling_binary("nm"));
    (niao, nm)
}

fn find_sibling_binary(name: &str) -> Option<PathBuf> {
    let exe = if cfg!(windows) {
        format!("{name}.exe")
    } else {
        name.to_string()
    };
    if let Ok(current) = env::current_exe() {
        let sibling = current.parent()?.join(&exe);
        if sibling.is_file() {
            return Some(sibling);
        }
    }
    which_in_path(&exe)
}

fn which_in_path(name: &str) -> Option<PathBuf> {
    let path_var = env::var_os("PATH")?;
    for dir in env::split_paths(&path_var) {
        let candidate = dir.join(name);
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}

fn copy_to_cargo_bin(
    niao_bin: &Option<PathBuf>,
    nm_bin: &Option<PathBuf>,
) -> Result<(), Box<dyn std::error::Error>> {
    let Some(dir) = cargo_bin_dir() else {
        return Ok(());
    };
    fs::create_dir_all(&dir)?;
    remove_legacy_neko_shim(&dir);
    if let Some(src) = niao_bin {
        let dest = dir.join(exe_name("niao"));
        fs::copy(src, &dest)?;
        println!("  also installed: {}", dest.display());
    }
    if let Some(src) = nm_bin {
        let dest = dir.join(exe_name("nm"));
        fs::copy(src, &dest)?;
        println!("  also installed: {}", dest.display());
    }
    Ok(())
}

fn remove_legacy_neko_shim(dir: &Path) {
    let legacy = dir.join(exe_name("neko"));
    if legacy.is_file() {
        let _ = fs::remove_file(&legacy);
        println!("  removed legacy shim: {}", legacy.display());
    }
}

fn cargo_bin_dir() -> Option<PathBuf> {
    #[cfg(windows)]
    {
        env::var_os("USERPROFILE").map(|p| PathBuf::from(p).join(".cargo").join("bin"))
    }
    #[cfg(not(windows))]
    {
        env::var_os("HOME").map(|p| PathBuf::from(p).join(".cargo").join("bin"))
    }
}

fn exe_name(base: &str) -> String {
    if cfg!(windows) {
        format!("{base}.exe")
    } else {
        base.to_string()
    }
}

fn print_path_hint() {
    let bin = niao_bin_dir();
    println!("\nNiao home: {}", niao_home().display());
    println!("Binaries:  {}", bin.display());
    if !path_contains(&bin) {
        println!("\nNote: {} is not on your PATH yet.", bin.display());
        #[cfg(windows)]
        println!("  Run: powershell -File install.ps1");
        #[cfg(windows)]
        println!("  Or add manually: {}", bin.display());
        #[cfg(not(windows))]
        println!("  Add to PATH: export PATH=\"{}:$PATH\"", bin.display());
    }
}

fn path_contains(dir: &Path) -> bool {
    let Some(path_var) = env::var_os("PATH") else {
        return false;
    };
    env::split_paths(&path_var).any(|p| p == dir)
}
