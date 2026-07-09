use crate::catalog::{standard_libs, NIAO_TOOLCHAIN_VERSION};
use crate::error::{PkgError, PkgResult};
use crate::package::{find_project_package, read_project_package, LibPackage};
use crate::paths::{
    global_catalog_path, global_install_state_path, lib_manifest_dir, niao_bin_dir, niao_home,
    niao_libs_dir, project_venv_dir, venv_catalog_path, venv_install_state_path, venv_libs_dir,
    InstallMode,
};
use crate::source::{
    all_standard_from_source, find_source_root_or_err,
};
use crate::registry::{self, is_remote_lib};
use crate::state::{InstallState, InstalledLib, LibsCatalog};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct InstallOptions {
    pub mode: InstallMode,
    pub project_dir: Option<PathBuf>,
    pub niao_bin: Option<PathBuf>,
    pub nm_bin: Option<PathBuf>,
    pub force: bool,
    pub source_root: Option<PathBuf>,
    /// Online package registry API (default: NIAO_REGISTRY or https://nms.taurus-tech.in)
    pub registry_url: Option<String>,
}

impl Default for InstallOptions {
    fn default() -> Self {
        Self {
            mode: InstallMode::Global,
            project_dir: None,
            niao_bin: None,
            nm_bin: None,
            force: false,
            source_root: None,
            registry_url: None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct InstallReport {
    pub mode: InstallMode,
    pub root: PathBuf,
    pub niao_version: String,
    pub libs_installed: Vec<String>,
    pub niao_bin: Option<PathBuf>,
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
    pub niao_bin: Option<PathBuf>,
    pub nm_bin: Option<PathBuf>,
}

pub fn install_global(opts: &InstallOptions) -> PkgResult<InstallReport> {
    let source = resolve_source_root(opts)?;
    let names: Vec<String> = standard_libs().into_iter().map(|s| s.name).collect();
    install_with_roots(
        InstallMode::Global,
        niao_home(),
        niao_libs_dir(),
        global_install_state_path(),
        global_catalog_path(),
        &names,
        opts,
        Some(source),
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
        Some(source),
        true,
        None,
    )
}

/// Install one or more libraries by name from local source or the online registry.
pub fn install_libs(
    lib_names: &[String],
    opts: &InstallOptions,
) -> PkgResult<InstallReport> {
    let source = resolve_source_root_optional(opts, lib_names, false)?;
    let (libs_root, state_path, catalog_path, root, mode) = install_targets(opts)?;
    install_with_roots(
        mode,
        root,
        libs_root,
        state_path,
        catalog_path,
        lib_names,
        opts,
        source,
        false,
        None,
    )
}

/// Update installed libraries to the latest version available in the source tree or registry.
pub fn update_libs(lib_names: &[String], opts: &InstallOptions) -> PkgResult<UpdateReport> {
    let source = resolve_source_root_optional(opts, lib_names, false)?;
    let registry_base = opts.registry_url.as_deref();
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
        let latest = match registry::latest_lib_version(source.as_deref(), name, registry_base) {
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
        let (lib_pkg, src_version_dir) =
            registry::load_lib(source.as_deref(), name, "latest", registry_base)?;
        let dest_dir = lib_manifest_dir(&libs_root, &lib_pkg.name, &lib_pkg.version);
        install_lib_files(&lib_pkg, &src_version_dir, &dest_dir)?;
        prune_old_lib_versions(&libs_root, name, &lib_pkg.version)?;

        let installed = InstalledLib::from_lib_package(&lib_pkg);
        catalog.libs.insert(name.clone(), installed);
        upgraded.push((name.clone(), from_version, lib_pkg.version));
    }

    if upgraded.is_empty() && not_installed.is_empty() && !up_to_date.is_empty() {
        let (niao_bin, nm_bin) = refresh_global_binaries(&mode, opts)?;
        return Ok(UpdateReport {
            mode,
            root,
            source_root: source
                .clone()
                .unwrap_or_else(|| niao_home()),
            upgraded,
            up_to_date,
            not_installed,
            niao_bin,
            nm_bin,
        });
    }

    catalog.niao_version = registry::toolchain_version(source.as_deref());
    catalog.updated_at = crate::state::chrono_now_public();
    write_json(&catalog_path, &catalog)?;

    let mut state = if state_path.exists() {
        load_install_state(&state_path)?
    } else {
        InstallState {
            niao_version: catalog.niao_version.clone(),
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
    state.niao_version = catalog.niao_version.clone();
    state.installed_at = catalog.updated_at.clone();
    state.source_root = source
        .as_ref()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|| registry::registry_url());
    for (name, lib) in &catalog.libs {
        state.libs.insert(name.clone(), lib.clone());
    }
    write_json(&state_path, &state)?;

    let (niao_bin, nm_bin) = refresh_global_binaries(&mode, opts)?;

    Ok(UpdateReport {
        mode,
        root,
        source_root: source.unwrap_or_else(|| niao_home()),
        upgraded,
        up_to_date,
        not_installed,
        niao_bin,
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
    let source = resolve_source_root_optional(opts, &names, false)?;
    let (libs_root, state_path, catalog_path, root, mode) = install_targets(opts)?;
    install_with_roots(
        mode,
        root,
        libs_root,
        state_path,
        catalog_path,
        &names,
        opts,
        source,
        false,
        Some(&pkg.dependencies),
    )
}

fn install_targets(opts: &InstallOptions) -> PkgResult<(PathBuf, PathBuf, PathBuf, PathBuf, InstallMode)> {
    match opts.mode {
        InstallMode::Global => Ok((
            niao_libs_dir(),
            global_install_state_path(),
            global_catalog_path(),
            niao_home(),
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

fn resolve_source_root_optional(
    opts: &InstallOptions,
    lib_names: &[String],
    full_toolchain: bool,
) -> PkgResult<Option<PathBuf>> {
    if full_toolchain {
        return Ok(Some(resolve_source_root(opts)?));
    }
    if let Some(root) = &opts.source_root {
        if !is_valid_source_root(root) {
            return Err(PkgError::Message(format!(
                "invalid --source path (need niao_libs/ and Cargo.toml): {}",
                root.display()
            )));
        }
        return Ok(Some(root.clone()));
    }
    if lib_names.iter().all(|n| is_remote_lib(n)) {
        return Ok(crate::source::find_source_root());
    }
    Ok(Some(find_source_root_or_err()?))
}

fn resolve_source_root(opts: &InstallOptions) -> PkgResult<PathBuf> {
    if let Some(root) = &opts.source_root {
        if !is_valid_source_root(root) {
            return Err(PkgError::Message(format!(
                "invalid --source path (need niao_libs/ and Cargo.toml): {}",
                root.display()
            )));
        }
        return Ok(root.clone());
    }
    find_source_root_or_err()
}

fn is_valid_source_root(root: &Path) -> bool {
    root.join("niao_libs").is_dir() && root.join("Cargo.toml").is_file()
}

fn install_specs_resolved(
    source_root: Option<&Path>,
    lib_names: &[String],
    default_version: &str,
    version_overrides: Option<&std::collections::BTreeMap<String, String>>,
    registry_base: Option<&str>,
) -> PkgResult<Vec<(LibPackage, PathBuf)>> {
    let mut out = Vec::new();
    for name in lib_names {
        let req = version_overrides
            .and_then(|m| m.get(name))
            .map(|s| s.as_str())
            .unwrap_or(default_version);
        let (pkg, dir) = registry::load_lib(source_root, name, req, registry_base)?;
        out.push((pkg, dir));
    }
    Ok(out)
}

fn install_with_roots(
    mode: InstallMode,
    root: PathBuf,
    libs_root: PathBuf,
    state_path: PathBuf,
    catalog_path: PathBuf,
    lib_names: &[String],
    opts: &InstallOptions,
    source_root: Option<PathBuf>,
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
        fs::create_dir_all(niao_bin_dir())?;
    } else {
        fs::create_dir_all(&root)?;
    }

    let registry_base = opts.registry_url.as_deref();
    let specs = if full_toolchain {
        let root = source_root
            .as_deref()
            .ok_or_else(|| PkgError::Message("Niao source root required for full toolchain install".into()))?;
        all_standard_from_source(root)?
    } else {
        install_specs_resolved(
            source_root.as_deref(),
            lib_names,
            "*",
            version_overrides,
            registry_base,
        )?
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
        install_lib_files(&lib_pkg, &src_version_dir, &dest_dir)?;
        let installed = InstalledLib::from_lib_package(&lib_pkg);
        catalog.libs.insert(lib_pkg.name.clone(), installed);
        libs_installed.push(lib_pkg.name.clone());
    }

    libs_installed.sort();
    libs_installed.dedup();

    catalog.niao_version = registry::toolchain_version(source_root.as_deref());
    catalog.updated_at = crate::state::chrono_now_public();

    write_json(&catalog_path, &catalog)?;

    let mut state = if state_path.exists() {
        load_install_state(&state_path)?
    } else {
        InstallState {
            niao_version: NIAO_TOOLCHAIN_VERSION.to_string(),
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
    state.niao_version = catalog.niao_version.clone();
    state.installed_at = catalog.updated_at.clone();
    state.source_root = source_root
        .as_ref()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|| registry::registry_url());
    for (name, lib) in &catalog.libs {
        state.libs.insert(name.clone(), lib.clone());
    }
    write_json(&state_path, &state)?;

    let niao_dest = if full_toolchain && mode == InstallMode::Global {
        copy_tool_binary(opts.niao_bin.as_deref(), &niao_bin_dir().join(exe_name("niao")))?
    } else {
        None
    };
    let nm_dest = if full_toolchain && mode == InstallMode::Global {
        copy_tool_binary(opts.nm_bin.as_deref(), &niao_bin_dir().join(exe_name("nm")))?
    } else {
        None
    };

    if mode == InstallMode::Venv {
        write_venv_config(&root, &state)?;
    }

    Ok(InstallReport {
        mode,
        root,
        niao_version: state.niao_version,
        libs_installed,
        niao_bin: niao_dest,
        nm_bin: nm_dest,
        source_root: source_root.unwrap_or_else(|| niao_home()),
    })
}

fn install_lib_files(
    lib_pkg: &LibPackage,
    src_version_dir: &Path,
    dest_dir: &Path,
) -> PkgResult<()> {
    fs::create_dir_all(dest_dir)?;

    if src_version_dir.is_dir() {
        copy_dir_contents(src_version_dir, dest_dir)?;
    }

    let manifest = InstalledLib::from_lib_package(lib_pkg);
    write_json(&dest_dir.join("lib.json"), &manifest)?;

    let package_src = src_version_dir
        .parent()
        .map(|p| p.join("package.json"))
        .filter(|p| p.is_file());
    if let Some(package_src) = package_src {
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
    let niao_src = opts
        .niao_bin
        .as_deref()
        .filter(|path| path.is_file());
    let nm_src = opts
        .nm_bin
        .as_deref()
        .filter(|path| path.is_file());
    let niao_dest = copy_tool_binary(niao_src, &niao_bin_dir().join(exe_name("niao")))?;
    let nm_dest = copy_tool_binary(nm_src, &niao_bin_dir().join(exe_name("nm")))?;
    Ok((niao_dest, nm_dest))
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
        "# Niao project venv — generated by nm\nniao_version = \"{}\"\nmode = \"venv\"\nroot = \"{}\"\nsource_root = \"{}\"\n",
        state.niao_version,
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
