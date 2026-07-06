use crate::catalog::{standard_libs, NEKO_TOOLCHAIN_VERSION};
use crate::error::{PkgError, PkgResult};
use crate::package::{find_project_package, read_project_package, LibPackage};
use crate::paths::{
    global_catalog_path, global_install_state_path, lib_manifest_dir, neko_bin_dir, neko_home,
    neko_libs_dir, project_venv_dir, venv_catalog_path, venv_install_state_path, venv_libs_dir,
    InstallMode,
};
use crate::source::{
    all_standard_from_source, find_source_root_or_err, install_specs_from_source,
    latest_lib_version, load_lib_from_source, toolchain_version_from_source,
};
use crate::state::{InstallState, InstalledLib, LibsCatalog};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct InstallOptions {
    pub mode: InstallMode,
    pub project_dir: Option<PathBuf>,
    pub neko_bin: Option<PathBuf>,
    pub nm_bin: Option<PathBuf>,
    pub force: bool,
    pub source_root: Option<PathBuf>,
}

impl Default for InstallOptions {
    fn default() -> Self {
        Self {
            mode: InstallMode::Global,
            project_dir: None,
            neko_bin: None,
            nm_bin: None,
            force: false,
            source_root: None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct InstallReport {
    pub mode: InstallMode,
    pub root: PathBuf,
    pub neko_version: String,
    pub libs_installed: Vec<String>,
    pub neko_bin: Option<PathBuf>,
    pub nm_bin: Option<PathBuf>,
    pub source_root: PathBuf,
}

#[derive(Debug, Clone)]
pub struct UpdateReport {
    pub mode: InstallMode,
    pub root: PathBuf,
    pub source_root: PathBuf,
    /// (name, from_version, to_version)
    pub upgraded: Vec<(String, String, String)>,
    pub up_to_date: Vec<String>,
    pub not_installed: Vec<String>,
    pub neko_bin: Option<PathBuf>,
    pub nm_bin: Option<PathBuf>,
}

pub fn install_global(opts: &InstallOptions) -> PkgResult<InstallReport> {
    let source = resolve_source_root(opts)?;
    let names: Vec<String> = standard_libs().into_iter().map(|s| s.name).collect();
    install_with_roots(
        InstallMode::Global,
        neko_home(),
        neko_libs_dir(),
        global_install_state_path(),
        global_catalog_path(),
        &names,
        opts,
        &source,
        true,
        None,
    )
}

pub fn install_venv(project: &Path, opts: &InstallOptions) -> PkgResult<InstallReport> {
    let source = resolve_source_root(opts)?;
    let names: Vec<String> = standard_libs().into_iter().map(|s| s.name).collect();
    let root = project_venv_dir(project);
    install_with_roots(
        InstallMode::Venv,
        root.clone(),
        venv_libs_dir(project),
        venv_install_state_path(project),
        venv_catalog_path(project),
        &names,
        opts,
        &source,
        true,
        None,
    )
}

/// Install one or more libraries by name from the Neko source tree.
pub fn install_libs(
    lib_names: &[String],
    opts: &InstallOptions,
) -> PkgResult<InstallReport> {
    let source = resolve_source_root(opts)?;
    let (libs_root, state_path, catalog_path, root, mode) = install_targets(opts)?;
    install_with_roots(
        mode,
        root,
        libs_root,
        state_path,
        catalog_path,
        lib_names,
        opts,
        &source,
        false,
        None,
    )
}

/// Update installed libraries to the latest version available in the source tree.
pub fn update_libs(lib_names: &[String], opts: &InstallOptions) -> PkgResult<UpdateReport> {
    let source = resolve_source_root(opts)?;
    let (libs_root, state_path, catalog_path, root, mode) = install_targets(opts)?;

    if !catalog_path.exists() && lib_names.is_empty() {
        return Err(PkgError::Message(
            "no libraries installed — run `nm install --global` first".into(),
        ));
    }

    let catalog = if catalog_path.exists() {
        load_catalog(&catalog_path)?
    } else {
        LibsCatalog::from_specs(&[])
    };

    let names: Vec<String> = if lib_names.is_empty() {
        catalog.libs.keys().cloned().collect()
    } else {
        lib_names.to_vec()
    };

    if names.is_empty() {
        return Err(PkgError::Message("no libraries to update".into()));
    }

    fs::create_dir_all(&libs_root)?;

    let mut upgraded = Vec::new();
    let mut up_to_date = Vec::new();
    let mut not_installed = Vec::new();
    let mut catalog = catalog;

    for name in &names {
        let latest = match latest_lib_version(&source, name) {
            Ok(v) => v,
            Err(PkgError::NotFound(_)) => {
                not_installed.push(name.clone());
                continue;
            }
            Err(e) => return Err(e),
        };

        let installed_version = catalog.libs.get(name).map(|l| l.version.as_str());

        let needs_update = match installed_version {
            None => true,
            Some(_) if opts.force => true,
            Some(current) => crate::package::compare_versions(current, &latest).is_lt(),
        };

        if !needs_update {
            prune_old_lib_versions(&libs_root, name, &latest)?;
            up_to_date.push(name.clone());
            continue;
        }

        let from_version = installed_version.unwrap_or("none").to_string();
        let (lib_pkg, src_version_dir) = load_lib_from_source(&source, name, "latest")?;
        let dest_dir = lib_manifest_dir(&libs_root, &lib_pkg.name, &lib_pkg.version);
        install_lib_files(&source, &lib_pkg, &src_version_dir, &dest_dir)?;
        prune_old_lib_versions(&libs_root, name, &lib_pkg.version)?;

        let installed = InstalledLib::from_lib_package(&lib_pkg);
        catalog.libs.insert(name.clone(), installed);
        upgraded.push((name.clone(), from_version, lib_pkg.version));
    }

    if upgraded.is_empty() && not_installed.is_empty() && !up_to_date.is_empty() {
        let (neko_bin, nm_bin) = refresh_global_binaries(&mode, opts)?;
        return Ok(UpdateReport {
            mode,
            root,
            source_root: source.clone(),
            upgraded,
            up_to_date,
            not_installed,
            neko_bin,
            nm_bin,
        });
    }

    catalog.neko_version = toolchain_version_from_source(&source);
    catalog.updated_at = crate::state::chrono_now_public();
    write_json(&catalog_path, &catalog)?;

    let mut state = if state_path.exists() {
        load_install_state(&state_path)?
    } else {
        InstallState {
            neko_version: catalog.neko_version.clone(),
            mode: match mode {
                InstallMode::Global => "global".into(),
                InstallMode::Venv => "venv".into(),
            },
            installed_at: catalog.updated_at.clone(),
            root: root.display().to_string(),
            source_root: String::new(),
            libs: catalog.libs.clone(),
        }
    };
    state.neko_version = catalog.neko_version.clone();
    state.installed_at = catalog.updated_at.clone();
    state.source_root = source.display().to_string();
    for (name, lib) in &catalog.libs {
        state.libs.insert(name.clone(), lib.clone());
    }
    write_json(&state_path, &state)?;

    let (neko_bin, nm_bin) = refresh_global_binaries(&mode, opts)?;

    Ok(UpdateReport {
        mode,
        root,
        source_root: source,
        upgraded,
        up_to_date,
        not_installed,
        neko_bin,
        nm_bin,
    })
}

/// Install dependencies listed in a project `package.json`.
pub fn install_from_package_json(project: &Path, opts: &InstallOptions) -> PkgResult<InstallReport> {
    let package_path = find_project_package(project).ok_or_else(|| {
        PkgError::NotFound(format!(
            "package.json not found in {}",
            project.display()
        ))
    })?;
    let pkg = read_project_package(&package_path)?;
    if pkg.dependencies.is_empty() {
        return Err(PkgError::Message(
            "package.json has no dependencies to install".into(),
        ));
    }
    let mut names: Vec<String> = pkg.dependencies.keys().cloned().collect();
    names.sort();
    let source = resolve_source_root(opts)?;
    let (libs_root, state_path, catalog_path, root, mode) = install_targets(opts)?;
    install_with_roots(
        mode,
        root,
        libs_root,
        state_path,
        catalog_path,
        &names,
        opts,
        &source,
        false,
        Some(&pkg.dependencies),
    )
}

fn install_targets(opts: &InstallOptions) -> PkgResult<(PathBuf, PathBuf, PathBuf, PathBuf, InstallMode)> {
    match opts.mode {
        InstallMode::Global => Ok((
            neko_libs_dir(),
            global_install_state_path(),
            global_catalog_path(),
            neko_home(),
            InstallMode::Global,
        )),
        InstallMode::Venv => {
            let project = opts
                .project_dir
                .as_deref()
                .ok_or_else(|| PkgError::Message("project directory required for venv install".into()))?;
            Ok((
                venv_libs_dir(project),
                venv_install_state_path(project),
                venv_catalog_path(project),
                project_venv_dir(project),
                InstallMode::Venv,
            ))
        }
    }
}

fn resolve_source_root(opts: &InstallOptions) -> PkgResult<PathBuf> {
    if let Some(root) = &opts.source_root {
        if !is_valid_source_root(root) {
            return Err(PkgError::Message(format!(
                "invalid --source path (need neko_libs/ and Cargo.toml): {}",
                root.display()
            )));
        }
        return Ok(root.clone());
    }
    find_source_root_or_err()
}

fn is_valid_source_root(root: &Path) -> bool {
    root.join("neko_libs").is_dir() && root.join("Cargo.toml").is_file()
}

fn install_with_roots(
    mode: InstallMode,
    root: PathBuf,
    libs_root: PathBuf,
    state_path: PathBuf,
    catalog_path: PathBuf,
    lib_names: &[String],
    opts: &InstallOptions,
    source_root: &Path,
    full_toolchain: bool,
    version_overrides: Option<&std::collections::BTreeMap<String, String>>,
) -> PkgResult<InstallReport> {
    if full_toolchain && state_path.exists() && !opts.force {
        return Err(PkgError::AlreadyInstalled(format!(
            "{} install already exists at {} (use --force)",
            if mode == InstallMode::Global {
                "global"
            } else {
                "venv"
            },
            state_path.display()
        )));
    }

    fs::create_dir_all(&libs_root)?;
    if mode == InstallMode::Global {
        fs::create_dir_all(neko_bin_dir())?;
    } else {
        fs::create_dir_all(&root)?;
    }

    let specs = if full_toolchain {
        all_standard_from_source(source_root)?
    } else {
        install_specs_from_source(source_root, lib_names, "*", version_overrides)?
    };

    let mut catalog = if catalog_path.exists() {
        load_catalog(&catalog_path)?
    } else {
        LibsCatalog::from_specs(&[])
    };

    let mut libs_installed = Vec::new();
    for (lib_pkg, src_version_dir) in specs {
        let dest_dir = lib_manifest_dir(&libs_root, &lib_pkg.name, &lib_pkg.version);
        if dest_dir.exists() && !opts.force {
            libs_installed.push(lib_pkg.name.clone());
            if let Some(existing) = catalog.libs.get(&lib_pkg.name) {
                if existing.version == lib_pkg.version {
                    continue;
                }
            }
        }
        install_lib_files(source_root, &lib_pkg, &src_version_dir, &dest_dir)?;
        let installed = InstalledLib::from_lib_package(&lib_pkg);
        catalog.libs.insert(lib_pkg.name.clone(), installed);
        libs_installed.push(lib_pkg.name.clone());
    }

    libs_installed.sort();
    libs_installed.dedup();

    catalog.neko_version = toolchain_version_from_source(source_root);
    catalog.updated_at = crate::state::chrono_now_public();

    write_json(&catalog_path, &catalog)?;

    let mut state = if state_path.exists() {
        load_install_state(&state_path)?
    } else {
        InstallState {
            neko_version: NEKO_TOOLCHAIN_VERSION.to_string(),
            mode: match mode {
                InstallMode::Global => "global".into(),
                InstallMode::Venv => "venv".into(),
            },
            installed_at: catalog.updated_at.clone(),
            root: root.display().to_string(),
            source_root: String::new(),
            libs: catalog.libs.clone(),
        }
    };
    state.neko_version = catalog.neko_version.clone();
    state.installed_at = catalog.updated_at.clone();
    state.source_root = source_root.display().to_string();
    for (name, lib) in &catalog.libs {
        state.libs.insert(name.clone(), lib.clone());
    }
    write_json(&state_path, &state)?;

    let neko_dest = if full_toolchain && mode == InstallMode::Global {
        copy_tool_binary(opts.neko_bin.as_deref(), &neko_bin_dir().join(exe_name("neko")))?
    } else {
        None
    };
    let nm_dest = if full_toolchain && mode == InstallMode::Global {
        copy_tool_binary(opts.nm_bin.as_deref(), &neko_bin_dir().join(exe_name("nm")))?
    } else {
        None
    };

    if mode == InstallMode::Venv {
        write_venv_config(&root, &state)?;
    }

    Ok(InstallReport {
        mode,
        root,
        neko_version: state.neko_version,
        libs_installed,
        neko_bin: neko_dest,
        nm_bin: nm_dest,
        source_root: source_root.to_path_buf(),
    })
}

fn install_lib_files(
    source_root: &Path,
    lib_pkg: &LibPackage,
    src_version_dir: &Path,
    dest_dir: &Path,
) -> PkgResult<()> {
    fs::create_dir_all(dest_dir)?;
    let src_lib = source_root.join("neko_libs").join(&lib_pkg.name);

    if src_version_dir.is_dir() {
        copy_dir_contents(src_version_dir, dest_dir)?;
    }

    let manifest = InstalledLib::from_lib_package(lib_pkg);
    write_json(&dest_dir.join("lib.json"), &manifest)?;

    let package_src = src_lib.join("package.json");
    if package_src.is_file() {
        let dest_pkg = dest_dir.parent().unwrap().join("package.json");
        fs::copy(&package_src, &dest_pkg)?;
    }

    for rel in &lib_pkg.sources {
        let from = src_version_dir.join(rel);
        let to = dest_dir.join(rel);
        if from.is_file() {
            if let Some(parent) = to.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::copy(&from, &to)?;
        }
    }
    Ok(())
}

/// Remove stale version directories after upgrading a library.
fn prune_old_lib_versions(libs_root: &Path, name: &str, keep_version: &str) -> PkgResult<()> {
    let lib_parent = libs_root.join(name);
    if !lib_parent.is_dir() {
        return Ok(());
    }
    for entry in fs::read_dir(&lib_parent)? {
        let entry = entry?;
        if !entry.file_type()?.is_dir() {
            continue;
        }
        let version_name = entry.file_name();
        let Some(version) = version_name.to_str() else {
            continue;
        };
        if version == keep_version {
            continue;
        }
        fs::remove_dir_all(entry.path())?;
    }
    Ok(())
}

fn copy_dir_contents(src: &Path, dest: &Path) -> PkgResult<()> {
    if !src.is_dir() {
        return Ok(());
    }
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let file_type = entry.file_type()?;
        let name = entry.file_name();
        let to = dest.join(&name);
        if file_type.is_dir() {
            fs::create_dir_all(&to)?;
            copy_dir_contents(&entry.path(), &to)?;
        } else if file_type.is_file() {
            fs::copy(entry.path(), &to)?;
        }
    }
    Ok(())
}

fn refresh_global_binaries(
    mode: &InstallMode,
    opts: &InstallOptions,
) -> PkgResult<(Option<PathBuf>, Option<PathBuf>)> {
    if *mode != InstallMode::Global {
        return Ok((None, None));
    }
    let neko_src = opts
        .neko_bin
        .as_deref()
        .filter(|path| path.is_file());
    let nm_src = opts
        .nm_bin
        .as_deref()
        .filter(|path| path.is_file());
    let neko_dest = copy_tool_binary(neko_src, &neko_bin_dir().join(exe_name("neko")))?;
    let nm_dest = copy_tool_binary(nm_src, &neko_bin_dir().join(exe_name("nm")))?;
    Ok((neko_dest, nm_dest))
}

fn copy_tool_binary(src: Option<&Path>, dest: &Path) -> PkgResult<Option<PathBuf>> {
    let Some(src) = src else {
        return Ok(None);
    };
    if !src.is_file() {
        return Err(PkgError::Message(format!(
            "binary not found: {}",
            src.display()
        )));
    }
    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::copy(src, dest)?;
    Ok(Some(dest.to_path_buf()))
}

fn write_venv_config(root: &Path, state: &InstallState) -> PkgResult<()> {
    let config = format!(
        "# Neko project venv — generated by nm\nneko_version = \"{}\"\nmode = \"venv\"\nroot = \"{}\"\nsource_root = \"{}\"\n",
        state.neko_version,
        state.root.replace('\\', "/"),
        state.source_root.replace('\\', "/")
    );
    fs::write(root.join("venv.toml"), config)?;
    Ok(())
}

fn write_json<T: serde::Serialize>(path: &Path, value: &T) -> PkgResult<()> {
    let data = serde_json::to_string_pretty(value)?;
    fs::write(path, data)?;
    Ok(())
}

fn exe_name(base: &str) -> String {
    if cfg!(windows) {
        format!("{base}.exe")
    } else {
        base.to_string()
    }
}

pub fn load_install_state(path: &Path) -> PkgResult<InstallState> {
    let data = crate::package::read_json_text(path)?;
    serde_json::from_str(&data).map_err(|e| {
        PkgError::Message(format!("parse {}: {e}", path.display()))
    })
}

pub fn load_catalog(path: &Path) -> PkgResult<LibsCatalog> {
    let data = crate::package::read_json_text(path)?;
    serde_json::from_str(&data).map_err(|e| {
        PkgError::Message(format!("parse {}: {e}", path.display()))
    })
}

pub fn default_global_state() -> PkgResult<InstallState> {
    load_install_state(&global_install_state_path())
}

pub fn default_global_catalog() -> PkgResult<LibsCatalog> {
    load_catalog(&global_catalog_path())
}
