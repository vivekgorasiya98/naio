use clap::{Parser, Subcommand};
use neko_pkg::{
    find_project_package, find_source_root, install_from_package_json, install_global, install_libs,
    install_venv, load_catalog_optional, neko_bin_dir, neko_home, neko_libs_dir, release_tool_binary,
    resolve_lib_name, standard_libs, uninstall_libs, update_libs, InstallMode, InstallOptions,
    NEKO_TOOLCHAIN_VERSION,
};
use std::env;
use std::fs;
use std::path::{Path, PathBuf};

const VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Parser)]
#[command(
    name = "nm",
    version = VERSION,
    about = "Neko package manager — install, uninstall, and manage standard libraries",
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
        /// Install full toolchain + all standard libraries to ~/.neko
        #[arg(long)]
        global: bool,
        /// Install to project .neko/ venv
        #[arg(long)]
        venv: bool,
        /// Project directory (default: current directory)
        #[arg(long, default_value = ".")]
        project: PathBuf,
        /// Overwrite existing installs
        #[arg(long)]
        force: bool,
        /// Neko source tree (default: auto-detect repo / NEKO_SOURCE)
        #[arg(long)]
        source: Option<PathBuf>,
        /// Path to neko binary to copy (with --global)
        #[arg(long)]
        neko_bin: Option<PathBuf>,
        /// Path to nm binary to copy (with --global)
        #[arg(long)]
        nm_bin: Option<PathBuf>,
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
    },
    /// Show Neko and library versions
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
    /// Initialize a project venv (.neko/)
    Venv {
        #[arg(long, default_value = ".")]
        project: PathBuf,
        #[arg(long)]
        force: bool,
    },
    /// Print neko home directory
    Home,
    /// Print detected Neko source root
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
            neko_bin,
            nm_bin,
        } => cmd_install(libs, global, venv, &project, force, source, neko_bin, nm_bin),
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
        } => cmd_update(libs, venv, &project, force, source),
        Commands::Version { venv, project } => cmd_version(venv, &project),
        Commands::Info { name, venv, project } => cmd_info(&name, venv, &project),
        Commands::Venv { project, force } => cmd_venv(&project, force),
        Commands::Home => {
            println!("{}", neko_home().display());
            Ok(())
        }
        Commands::Source => {
            match find_source_root() {
                Some(p) => println!("{}", p.display()),
                None => {
                    eprintln!("Neko source not found. Set NEKO_SOURCE or run from the repo.");
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
    neko_bin: Option<PathBuf>,
    nm_bin: Option<PathBuf>,
) -> Result<(), Box<dyn std::error::Error>> {
    let source_for_bins = source.clone().or_else(find_source_root);
    let (neko_bin, nm_bin) = if let Some(ref src) = source_for_bins {
        resolve_tool_binaries(src, neko_bin, nm_bin)
    } else {
        (
            neko_bin.or_else(|| find_sibling_binary("neko")),
            nm_bin.or_else(|| find_sibling_binary("nm")),
        )
    };

    let has_package_json = find_project_package(project).is_some();
    let use_venv = venv || (!global && libs.is_empty() && has_package_json);
    let use_global = global || (libs.is_empty() && !use_venv && !has_package_json);

    let opts = install_opts(use_venv, project, force, source, neko_bin.clone(), nm_bin.clone());

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

    print_install_report(&report, &neko_bin, &nm_bin)?;
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
        neko_bin: None,
        nm_bin: None,
        force,
        source_root: None,
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
            "neko {} — {} installed",
            catalog.neko_version,
            catalog.libs.len()
        );
    } else {
        println!("neko {NEKO_TOOLCHAIN_VERSION} — no libraries installed yet");
        println!("  run `nm install --global` or `nm install <lib>` to get started");
    }

    for spec in standard_libs() {
        let is_installed = catalog.libs.contains_key(&spec.name);
        if installed_only && !is_installed {
            continue;
        }
        if available_only && is_installed {
            continue;
        }

        if let Some(lib) = catalog.libs.get(&spec.name) {
            println!(
                "  [installed] {} {} — {} ({:?}, {} builtins)",
                lib.name, lib.version, lib.description, lib.kind, lib.builtin_count
            );
        } else {
            println!(
                "  [available] {} {} — {}",
                spec.name, spec.version, spec.description
            );
        }
    }

    Ok(())
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
) -> Result<(), Box<dyn std::error::Error>> {
    let names: Vec<String> = libs
        .into_iter()
        .map(|n| resolve_lib_name(&n))
        .collect();

    let mut opts = install_opts(venv, project, force, source, None, None);
    opts.force = force;
    if let Some(src) = opts.source_root.clone().or_else(find_source_root) {
        let (neko_bin, nm_bin) = resolve_tool_binaries(&src, None, None);
        opts.neko_bin = neko_bin;
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
    if let Some(p) = &report.neko_bin {
        println!("  neko binary:  {}", p.display());
    }
    if let Some(p) = &report.nm_bin {
        println!("  nm binary:    {}", p.display());
    }
    if report.mode == InstallMode::Global && (report.neko_bin.is_some() || report.nm_bin.is_some())
    {
        copy_to_cargo_bin(&report.neko_bin, &report.nm_bin)?;
        print_path_hint();
    }
    Ok(())
}

fn cmd_version(venv: bool, project: &Path) -> Result<(), Box<dyn std::error::Error>> {
    println!("nm (package manager) {VERSION}");
    println!("neko toolchain       {NEKO_TOOLCHAIN_VERSION}");

    if let Ok(state) = load_state_for(venv, project) {
        println!("installed neko       {}", state.neko_version);
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

fn print_lib_details(lib: &neko_pkg::InstalledLib, installed: bool) {
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
        neko_bin: None,
        nm_bin: None,
        force,
        source_root: None,
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
    neko_bin: Option<PathBuf>,
    nm_bin: Option<PathBuf>,
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
        neko_bin,
        nm_bin,
        force,
        source_root: source,
    }
}

fn print_install_report(
    report: &neko_pkg::InstallReport,
    neko_bin: &Option<PathBuf>,
    nm_bin: &Option<PathBuf>,
) -> Result<(), Box<dyn std::error::Error>> {
    let mode_label = if report.mode == InstallMode::Global {
        "Global"
    } else {
        "Venv"
    };
    println!("nm install ({mode_label}) complete");
    println!("  neko version: {}", report.neko_version);
    println!("  source:       {}", report.source_root.display());
    println!("  root:         {}", report.root.display());
    println!("  neko_libs:    {}", install_libs_display(report));
    println!("  libraries:    {}", report.libs_installed.join(", "));

    if let Some(p) = &report.neko_bin {
        println!("  neko binary:  {}", p.display());
        print_path_hint();
    }
    if let Some(p) = &report.nm_bin {
        println!("  nm binary:    {}", p.display());
    }

    if report.mode == InstallMode::Global && (report.neko_bin.is_some() || report.nm_bin.is_some())
    {
        copy_to_cargo_bin(neko_bin, nm_bin)?;
    }
    Ok(())
}

fn install_libs_display(report: &neko_pkg::InstallReport) -> String {
    if report.mode == InstallMode::Global {
        neko_libs_dir().display().to_string()
    } else {
        report.root.join("neko_libs").display().to_string()
    }
}

fn load_state_for(
    venv: bool,
    project: &Path,
) -> Result<neko_pkg::InstallState, Box<dyn std::error::Error>> {
    use neko_pkg::{global_install_state_path, load_install_state, venv_install_state_path};
    let path = if venv {
        venv_install_state_path(project)
    } else {
        global_install_state_path()
    };
    Ok(load_install_state(&path)?)
}

fn resolve_tool_binaries(
    source: &Path,
    neko_bin: Option<PathBuf>,
    nm_bin: Option<PathBuf>,
) -> (Option<PathBuf>, Option<PathBuf>) {
    let neko = neko_bin
        .or_else(|| release_tool_binary(source, "neko"))
        .or_else(|| find_sibling_binary("neko"));
    let nm = nm_bin
        .or_else(|| release_tool_binary(source, "nm"))
        .or_else(|| find_sibling_binary("nm"));
    (neko, nm)
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
    neko_bin: &Option<PathBuf>,
    nm_bin: &Option<PathBuf>,
) -> Result<(), Box<dyn std::error::Error>> {
    let Some(dir) = cargo_bin_dir() else {
        return Ok(());
    };
    fs::create_dir_all(&dir)?;
    if let Some(src) = neko_bin {
        let dest = dir.join(exe_name("neko"));
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
    println!("\nNeko home: {}", neko_home().display());
    println!("Binaries:  {}", neko_bin_dir().display());
}
