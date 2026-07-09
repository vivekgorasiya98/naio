use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InstallMode {
    Global,
    Venv,
}

/// User-wide Niao home: `%USERPROFILE%/.niao` on Windows, `~/.niao` elsewhere.
pub fn niao_home() -> PathBuf {
    if let Ok(dir) = std::env::var("NIAO_HOME") {
        return PathBuf::from(dir);
    }
    #[cfg(windows)]
    {
        if let Ok(profile) = std::env::var("USERPROFILE") {
            return PathBuf::from(profile).join(".niao");
        }
    }
    if let Ok(home) = std::env::var("HOME") {
        return PathBuf::from(home).join(".niao");
    }
    PathBuf::from(".niao")
}

pub fn niao_bin_dir() -> PathBuf {
    niao_home().join("bin")
}

pub fn niao_libs_dir() -> PathBuf {
    niao_home().join("niao_libs")
}

pub fn global_install_state_path() -> PathBuf {
    niao_home().join("install.json")
}

pub fn global_catalog_path() -> PathBuf {
    niao_libs_dir().join("catalog.json")
}

/// Project-local venv root: `<project>/.niao`
pub fn project_venv_dir(project: &Path) -> PathBuf {
    project.join(".niao")
}

pub fn venv_libs_dir(project: &Path) -> PathBuf {
    project_venv_dir(project).join("niao_libs")
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
