//! Neko package manager core — catalog, install layout, global/venv support.

mod catalog;
mod error;
mod install;
mod package;
mod paths;
mod source;
mod state;
mod uninstall;

pub use catalog::{
    resolve_lib_name, standard_libs, LibKind, LibSpec, AHIRU_LIB_VERSION, NEKO_TOOLCHAIN_VERSION,
    STANDARD_LIBS,
};
pub use error::PkgError;
pub use install::{
    default_global_catalog, default_global_state, install_from_package_json, install_global,
    install_libs, install_venv, load_catalog, load_install_state, update_libs, InstallOptions,
    InstallReport, UpdateReport,
};
pub use package::{find_project_package, read_project_package, ProjectPackage};
pub use paths::{
    global_catalog_path, global_install_state_path, lib_manifest_dir, neko_bin_dir, neko_home,
    neko_libs_dir, project_venv_dir, venv_catalog_path, venv_install_state_path, venv_libs_dir,
    InstallMode,
};
pub use source::{find_source_root, latest_lib_version, list_source_lib_names, release_tool_binary};
pub use state::{InstalledLib, InstallState, LibsCatalog};
pub use uninstall::{load_catalog_optional, uninstall_libs, UninstallReport};
