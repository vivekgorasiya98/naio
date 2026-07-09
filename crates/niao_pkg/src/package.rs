use crate::catalog::LibKind;
use crate::error::{PkgError, PkgResult};
use serde::{Deserialize, Serialize};
use std::cmp::Ordering;
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

/// Project `package.json` (npm-style dependencies).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectPackage {
    pub name: String,
    #[serde(default)]
    pub version: String,
    /// Required Niao toolchain version.
    #[serde(default)]
    pub niao: String,
    #[serde(default)]
    pub dependencies: BTreeMap<String, String>,
}

/// Library package manifest in the Niao source tree (`niao_libs/<name>/package.json`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LibPackage {
    pub name: String,
    pub version: String,
    #[serde(default)]
    pub kind: LibKind,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub import_paths: Vec<String>,
    #[serde(default)]
    pub builtin_count: usize,
    /// Optional `.niao` sources relative to the version directory.
    #[serde(default)]
    pub sources: Vec<String>,
}

pub fn read_project_package(path: &Path) -> PkgResult<ProjectPackage> {
    let data = read_json_text(path)?;
    serde_json::from_str(&data).map_err(|e| {
        PkgError::Message(format!("parse {}: {e}", path.display()))
    })
}

pub fn read_lib_package(path: &Path) -> PkgResult<LibPackage> {
    let data = read_json_text(path)?;
    if data.trim().is_empty() {
        return Err(PkgError::Message(format!("empty json file: {}", path.display())));
    }
    serde_json::from_str(&data).map_err(|e| {
        PkgError::Message(format!("parse {}: {e}", path.display()))
    })
}

pub(crate) fn read_json_text(path: &Path) -> PkgResult<String> {
    let mut data = fs::read(path)?;
    if data.starts_with(&[0xEF, 0xBB, 0xBF]) {
        data = data[3..].to_vec();
    }
    String::from_utf8(data).map_err(|e| PkgError::Message(format!("utf8 {}: {e}", path.display())))
}

pub fn find_project_package(project: &Path) -> Option<PathBuf> {
    let path = project.join("package.json");
    if path.is_file() {
        Some(path)
    } else {
        None
    }
}

/// Compare dotted version strings (e.g. `0.2.10` > `0.2.2`).
pub fn compare_versions(a: &str, b: &str) -> Ordering {
    let pa: Vec<u64> = a.split('.').map(|p| p.parse().unwrap_or(0)).collect();
    let pb: Vec<u64> = b.split('.').map(|p| p.parse().unwrap_or(0)).collect();
    let n = pa.len().max(pb.len());
    for i in 0..n {
        let va = *pa.get(i).unwrap_or(&0);
        let vb = *pb.get(i).unwrap_or(&0);
        match va.cmp(&vb) {
            Ordering::Equal => continue,
            o => return o,
        }
    }
    Ordering::Equal
}

/// Highest version in a list (semver-style numeric segments).
pub fn latest_version(available: &[String]) -> Option<String> {
    available
        .iter()
        .max_by(|a, b| compare_versions(a, b))
        .cloned()
}

pub fn is_latest_request(requested: &str) -> bool {
    matches!(requested.trim(), "*" | "latest")
}

/// Match a requested version (`0.1.0`, `^0.1.0`, `*`) against an available version.
pub fn version_matches(requested: &str, available: &str) -> bool {
    let req = requested.trim();
    if is_latest_request(req) {
        return true;
    }
    if req == available {
        return true;
    }
    if let Some(base) = req.strip_prefix('^') {
        let req_parts: Vec<_> = base.split('.').collect();
        let avail_parts: Vec<_> = available.split('.').collect();
        if req_parts.is_empty() || avail_parts.is_empty() {
            return false;
        }
        return req_parts[0] == avail_parts[0]
            && (req_parts.len() < 2 || req_parts[1] == avail_parts.get(1).copied().unwrap_or(""));
    }
    false
}
pub fn pick_version(requested: &str, available: &[String]) -> PkgResult<String> {
    if available.is_empty() {
        return Err(PkgError::NotFound(format!(
            "no versions available for requested '{requested}'"
        )));
    }
    if is_latest_request(requested) {
        return latest_version(available).ok_or_else(|| {
            PkgError::NotFound(format!(
                "no versions available for requested '{requested}'"
            ))
        });
    }
    if let Some(v) = available.iter().find(|v| version_matches(requested, v)) {
        return Ok(v.clone());
    }
    Err(PkgError::NotFound(format!(
        "version '{requested}' not found (available: {})",
        available.join(", ")
    )))
}

pub fn list_lib_versions(source_libs: &Path, name: &str) -> PkgResult<Vec<String>> {
    let lib_root = source_libs.join(name);
    if !lib_root.is_dir() {
        return Err(PkgError::NotFound(format!(
            "library '{name}' not in source at {}",
            lib_root.display()
        )));
    }
    let mut versions = Vec::new();
    for entry in fs::read_dir(&lib_root)? {
        let entry = entry?;
        if !entry.file_type()?.is_dir() {
            continue;
        }
        let file_name = entry.file_name();
        let Some(v) = file_name.to_str() else {
            continue;
        };
        let path = entry.path();
        if path.join("lib.json").is_file() {
            versions.push(v.to_string());
            continue;
        }
        let has_files = fs::read_dir(&path)?.next().is_some();
        if has_files {
            versions.push(v.to_string());
        }
    }
    versions.sort();
    Ok(versions)
}
