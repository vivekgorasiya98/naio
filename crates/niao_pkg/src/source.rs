use crate::catalog::{standard_libs, LibSpec, NIAO_TOOLCHAIN_VERSION};
use crate::error::{PkgError, PkgResult};
use crate::package::{is_latest_request, latest_version, list_lib_versions, pick_version, read_lib_package, LibPackage};
use std::env;
use std::fs;
use std::path::{Path, PathBuf};

/// Locate the Niao source tree that contains `niao_libs/`.
pub fn find_source_root() -> Option<PathBuf> {
    if let Ok(dir) = env::var("NIAO_SOURCE") {
        let p = PathBuf::from(dir);
        if is_source_root(&p) {
            return Some(p);
        }
    }

    if let Ok(cwd) = env::current_dir() {
        if let Some(root) = walk_up_for_source(&cwd) {
            return Some(root);
        }
    }

    if let Ok(exe) = env::current_exe() {
        let mut dir = exe.parent()?.to_path_buf();
        for _ in 0..8 {
            if is_source_root(&dir) {
                return Some(dir);
            }
            if !dir.pop() {
                break;
            }
        }
    }

    if let Ok(data) = crate::package::read_json_text(&crate::paths::global_install_state_path()) {
        if let Ok(state) = serde_json::from_str::<crate::state::InstallState>(&data) {
            let root = PathBuf::from(&state.source_root);
            if !state.source_root.is_empty() && is_source_root(&root) {
                return Some(root);
            }
        }
    }

    None
}

pub fn find_source_root_or_err() -> PkgResult<PathBuf> {
    find_source_root().ok_or_else(|| {
        PkgError::Message(
            "Niao source root not found. Set NIAO_SOURCE, run from the Niao repo, or pass --source <path>".into(),
        )
    })
}

pub fn source_libs_dir(source_root: &Path) -> PathBuf {
    source_root.join("niao_libs")
}

fn is_source_root(path: &Path) -> bool {
    path.join("niao_libs").is_dir() && path.join("Cargo.toml").is_file()
}

fn walk_up_for_source(start: &Path) -> Option<PathBuf> {
    let mut dir = start.to_path_buf();
    loop {
        if is_source_root(&dir) {
            return Some(dir);
        }
        if !dir.pop() {
            break;
        }
    }
    None
}

pub fn load_lib_from_source(source_root: &Path, name: &str, version_req: &str) -> PkgResult<(LibPackage, PathBuf)> {
    let libs = source_libs_dir(source_root);
    let package_path = libs.join(name).join("package.json");
    if package_path.is_file() {
        let pkg = read_lib_package(&package_path)?;
        if pkg.name != name {
            return Err(PkgError::Message(format!(
                "package name mismatch: expected '{name}', found '{}'",
                pkg.name
            )));
        }
        let version = if is_latest_request(version_req) {
            let available = list_lib_versions(&libs, name).unwrap_or_default();
            if let Some(latest) = latest_version(&available) {
                if compare_versions(&latest, &pkg.version).is_gt() {
                    latest
                } else {
                    pkg.version.clone()
                }
            } else {
                pkg.version.clone()
            }
        } else if version_matches_req(version_req, &pkg.version) {
            pkg.version.clone()
        } else {
            pick_version(version_req, &list_lib_versions(&libs, name)?)?
        };
        let version_dir = libs.join(name).join(&version);
        if !version_dir.is_dir() {
            return Err(PkgError::NotFound(format!(
                "version directory missing: {}",
                version_dir.display()
            )));
        }
        let mut pkg = pkg;
        pkg.version = version.clone();
        return Ok((pkg, version_dir));
    }

    // Fallback: built-in catalog when per-lib package.json is absent.
    let spec = standard_libs()
        .into_iter()
        .find(|s| s.name == name)
        .ok_or_else(|| PkgError::NotFound(format!("library '{name}' not in source catalog")))?;

    let available = list_lib_versions(&libs, name).unwrap_or_default();
    let version = if !available.is_empty() {
        if is_latest_request(version_req) {
            latest_version(&available).unwrap()
        } else if version_matches_req(version_req, &spec.version) {
            spec.version.clone()
        } else {
            pick_version(version_req, &available)?
        }
    } else if is_latest_request(version_req) || version_matches_req(version_req, &spec.version) {
        spec.version.clone()
    } else {
        return Err(PkgError::NotFound(format!(
            "version '{version_req}' not found for '{name}'"
        )));
    };
    let version_dir = libs.join(name).join(&version);
    if version_dir.is_dir() {
        let pkg = lib_package_from_spec(&spec);
        let mut pkg = pkg;
        pkg.version = version.clone();
        return Ok((pkg, version_dir));
    }

    // Last resort: synthesize from catalog (no files on disk yet).
    let mut pkg = lib_package_from_spec(&spec);
    pkg.version = version.clone();
    Ok((pkg, version_dir))
}

fn lib_package_from_spec(spec: &LibSpec) -> LibPackage {
    LibPackage {
        name: spec.name.clone(),
        version: spec.version.clone(),
        kind: spec.kind,
        description: spec.description.clone(),
        import_paths: spec.import_paths.clone(),
        builtin_count: spec.builtin_count,
        sources: Vec::new(),
    }
}

fn version_matches_req(requested: &str, available: &str) -> bool {
    crate::package::version_matches(requested, available)
}

fn compare_versions(a: &str, b: &str) -> std::cmp::Ordering {
    crate::package::compare_versions(a, b)
}

/// Latest published version for a library in the source tree.
pub fn latest_lib_version(source_root: &Path, name: &str) -> PkgResult<String> {
    Ok(load_lib_from_source(source_root, name, "latest")?.0.version)
}

pub fn list_source_lib_names(source_root: &Path) -> PkgResult<Vec<String>> {
    let libs = source_libs_dir(source_root);
    if !libs.is_dir() {
        return Err(PkgError::NotFound(format!(
            "niao_libs missing at {}",
            libs.display()
        )));
    }
    let mut names = Vec::new();
    for entry in fs::read_dir(&libs)? {
        let entry = entry?;
        if entry.file_type()?.is_dir() {
            if let Some(name) = entry.file_name().to_str() {
                if entry.path().join("package.json").is_file()
                    || standard_libs().iter().any(|s| s.name == name)
                {
                    names.push(name.to_string());
                }
            }
        }
    }
    names.sort();
    Ok(names)
}

pub fn all_standard_from_source(source_root: &Path) -> PkgResult<Vec<(LibPackage, PathBuf)>> {
    let names: Vec<String> = crate::catalog::STANDARD_LIBS.iter().map(|s| s.to_string()).collect();
    install_specs_from_source(source_root, &names, "*", None)
}

pub fn install_specs_from_source(
    source_root: &Path,
    names: &[String],
    default_version: &str,
    version_overrides: Option<&std::collections::BTreeMap<String, String>>,
) -> PkgResult<Vec<(LibPackage, PathBuf)>> {
    let mut out = Vec::new();
    for name in names {
        let req = version_overrides
            .and_then(|m| m.get(name))
            .map(|s| s.as_str())
            .unwrap_or(default_version);
        let (pkg, dir) = load_lib_from_source(source_root, name, req)?;
        out.push((pkg, dir));
    }
    Ok(out)
}

pub fn toolchain_version_from_source(_source_root: &Path) -> String {
    NIAO_TOOLCHAIN_VERSION.to_string()
}

/// Fresh `niao` / `nm` from `cargo build --release` under the source tree.
pub fn release_tool_binary(source_root: &Path, name: &str) -> Option<PathBuf> {
    let path = source_root
        .join("target")
        .join("release")
        .join(release_exe_name(name));
    path.is_file().then_some(path)
}

fn release_exe_name(base: &str) -> String {
    if cfg!(windows) {
        format!("{base}.exe")
    } else {
        base.to_string()
    }
}
