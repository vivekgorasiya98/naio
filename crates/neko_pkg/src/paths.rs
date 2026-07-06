use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InstallMode {
    Global,
    Venv,
}

/// User-wide Neko home: `%USERPROFILE%/.neko` on Windows, `~/.neko` elsewhere.
pub fn neko_home() -> PathBuf {
    if let Ok(dir) = std::env::var("NEKO_HOME") {
        return PathBuf::from(dir);
    }
    #[cfg(windows)]
    {
        if let Ok(profile) = std::env::var("USERPROFILE") {
            return PathBuf::from(profile).join(".neko");
        }
    }
    if let Ok(home) = std::env::var("HOME") {
        return PathBuf::from(home).join(".neko");
    }
    PathBuf::from(".neko")
}

pub fn neko_bin_dir() -> PathBuf {
    neko_home().join("bin")
}

pub fn neko_libs_dir() -> PathBuf {
    neko_home().join("neko_libs")
}

pub fn global_install_state_path() -> PathBuf {
    neko_home().join("install.json")
}

pub fn global_catalog_path() -> PathBuf {
    neko_libs_dir().join("catalog.json")
}

/// Project-local venv root: `<project>/.neko`
pub fn project_venv_dir(project: &Path) -> PathBuf {
    project.join(".neko")
}

pub fn venv_libs_dir(project: &Path) -> PathBuf {
    project_venv_dir(project).join("neko_libs")
}

pub fn venv_install_state_path(project: &Path) -> PathBuf {
    project_venv_dir(project).join("install.json")
}

pub fn venv_catalog_path(project: &Path) -> PathBuf {
    venv_libs_dir(project).join("catalog.json")
}

pub fn lib_manifest_dir(base: &Path, name: &str, version: &str) -> PathBuf {
    base.join(name).join(version)
}
