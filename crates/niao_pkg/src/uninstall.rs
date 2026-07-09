use crate::error::{PkgError, PkgResult};
use crate::install::{load_catalog, load_install_state, InstallOptions};
use crate::paths::{
    global_catalog_path, global_install_state_path, lib_manifest_dir, niao_home, niao_libs_dir,
    project_venv_dir, venv_catalog_path, venv_install_state_path, venv_libs_dir, InstallMode,
};
use crate::state::{InstallState, LibsCatalog};
use std::fs;
use std::path::PathBuf;

#[derive(Debug, Clone)]
pub struct UninstallReport {
    pub mode: InstallMode,
    pub root: PathBuf,
    pub libs_removed: Vec<String>,
    pub not_installed: Vec<String>,
}

/// Remove one or more installed libraries from global or venv layout.
pub fn uninstall_libs(lib_names: &[String], opts: &InstallOptions) -> PkgResult<UninstallReport> {
    if lib_names.is_empty() {
        return Err(PkgError::Message(
            "specify library name(s) to uninstall, e.g. `nm uninstall nos`".into(),
        ));
    }

    let (libs_root, state_path, catalog_path, root, mode) = uninstall_targets(opts)?;

    if !catalog_path.exists() {
        return Err(PkgError::NotFound(format!(
            "no install at {} — nothing to uninstall",
            catalog_path.display()
        )));
    }

    let mut catalog = load_catalog(&catalog_path)?;
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

    let mut libs_removed = Vec::new();
    let mut not_installed = Vec::new();

    for name in lib_names {
        if name == "core" && !opts.force {
            return Err(PkgError::Message(
                "cannot uninstall core (required by the toolchain); use --force to override".into(),
            ));
        }

        let Some(lib) = catalog.libs.remove(name) else {
            not_installed.push(name.clone());
            continue;
        };

        let dest_dir = lib_manifest_dir(&libs_root, &lib.name, &lib.version);
        if dest_dir.exists() {
            fs::remove_dir_all(&dest_dir)?;
        }

        let lib_parent = libs_root.join(&lib.name);
        if lib_parent.is_dir() && is_dir_empty(&lib_parent)? {
            fs::remove_dir(&lib_parent)?;
        }

        state.libs.remove(name);
        libs_removed.push(name.clone());
    }

    if libs_removed.is_empty() {
        return Err(PkgError::NotFound(format!(
            "none of the requested libraries are installed: {}",
            not_installed.join(", ")
        )));
    }

    catalog.updated_at = crate::state::chrono_now_public();
    write_json(&catalog_path, &catalog)?;
    state.installed_at = catalog.updated_at.clone();
    write_json(&state_path, &state)?;

    libs_removed.sort();

    Ok(UninstallReport {
        mode,
        root,
        libs_removed,
        not_installed,
    })
}

fn uninstall_targets(opts: &InstallOptions) -> PkgResult<(PathBuf, PathBuf, PathBuf, PathBuf, InstallMode)> {
    match opts.mode {
        InstallMode::Global => Ok((
            niao_libs_dir(),
            global_install_state_path(),
            global_catalog_path(),
            niao_home(),
            InstallMode::Global,
        )),
        InstallMode::Venv => {
            let project = opts.project_dir.as_deref().ok_or_else(|| {
                PkgError::Message("project directory required for venv uninstall".into())
            })?;
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

fn is_dir_empty(path: &std::path::Path) -> PkgResult<bool> {
    Ok(fs::read_dir(path)?.next().is_none())
}

fn write_json<T: serde::Serialize>(path: &std::path::Path, value: &T) -> PkgResult<()> {
    let data = serde_json::to_string_pretty(value)?;
    fs::write(path, data)?;
    Ok(())
}

/// Try to load the installed catalog for global or venv; returns empty catalog if missing.
pub fn load_catalog_optional(venv: bool, project: &std::path::Path) -> LibsCatalog {
    let path = if venv {
        venv_catalog_path(project)
    } else {
        global_catalog_path()
    };
    load_catalog(&path).unwrap_or_else(|_| LibsCatalog::from_specs(&[]))
}
