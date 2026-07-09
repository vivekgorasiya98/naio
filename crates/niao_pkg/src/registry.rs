use crate::catalog::{remote_libs, NIAO_TOOLCHAIN_VERSION};
use crate::error::{PkgError, PkgResult};
use crate::package::{
    is_latest_request, latest_version, pick_version, read_lib_package, version_matches, LibPackage,
};
use crate::paths::niao_home;
use flate2::read::GzDecoder;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use tar::Archive;

/// Default online registry for optional Niao libraries.
pub const DEFAULT_REGISTRY_URL: &str = "https://nms.taurus-tech.in";

#[derive(Debug, Clone, Deserialize)]
pub struct RegistryCatalog {
    #[serde(default)]
    pub niao_version: String,
    #[serde(default)]
    pub remote_libs: Vec<String>,
    #[serde(default)]
    pub libs: std::collections::BTreeMap<String, RegistryLibEntry>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RegistryLibEntry {
    pub name: String,
    pub version: String,
    #[serde(default)]
    pub versions: Vec<String>,
    #[serde(default)]
    pub installable_versions: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RegistryVersionMeta {
    pub name: String,
    pub version: String,
    pub package: LibPackage,
    pub dist: RegistryDist,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RegistryDist {
    pub tarball: String,
    pub shasum: String,
    #[serde(default)]
    pub size: u64,
}

pub fn registry_url() -> String {
    std::env::var("NIAO_REGISTRY")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| DEFAULT_REGISTRY_URL.to_string())
        .trim_end_matches('/')
        .to_string()
}

pub fn is_remote_lib(name: &str) -> bool {
    remote_libs().iter().any(|n| *n == name)
}

pub fn registry_cache_dir() -> PathBuf {
    niao_home().join("registry-cache")
}

fn fetch_text(url: &str) -> PkgResult<String> {
    let response = ureq::get(url)
        .call()
        .map_err(|e| PkgError::Message(format!("registry request failed: {e}")))?;
    if !(200..300).contains(&response.status()) {
        return Err(PkgError::Message(format!(
            "registry HTTP {} for {}",
            response.status(),
            url
        )));
    }
    response
        .into_string()
        .map_err(|e| PkgError::Message(format!("registry read failed: {e}")))
}

fn fetch_bytes(url: &str) -> PkgResult<Vec<u8>> {
    let response = ureq::get(url)
        .call()
        .map_err(|e| PkgError::Message(format!("registry download failed: {e}")))?;
    if !(200..300).contains(&response.status()) {
        return Err(PkgError::Message(format!(
            "registry HTTP {} for {}",
            response.status(),
            url
        )));
    }
    let mut buf = Vec::new();
    response
        .into_reader()
        .read_to_end(&mut buf)
        .map_err(|e| PkgError::Message(format!("registry download read failed: {e}")))?;
    Ok(buf)
}

pub fn fetch_catalog(base: &str) -> PkgResult<RegistryCatalog> {
    let url = format!("{base}/v1/catalog");
    let text = fetch_text(&url)?;
    serde_json::from_str(&text).map_err(|e| PkgError::Message(format!("parse registry catalog: {e}")))
}

pub fn fetch_package_versions(base: &str, name: &str) -> PkgResult<Vec<String>> {
    let url = format!("{base}/v1/packages/{name}");
    if let Ok(text) = fetch_text(&url) {
        #[derive(Deserialize)]
        struct Pkg {
            #[serde(default)]
            versions: Vec<String>,
            #[serde(default)]
            installable_versions: Vec<String>,
            version: String,
        }
        let pkg: Pkg = serde_json::from_str(&text)
            .map_err(|e| PkgError::Message(format!("parse registry package: {e}")))?;
        if !pkg.installable_versions.is_empty() {
            return Ok(pkg.installable_versions);
        }
        if !pkg.versions.is_empty() {
            return Ok(pkg.versions);
        }
        return Ok(vec![pkg.version]);
    }

    let catalog = fetch_catalog(base)?;
    let entry = catalog
        .libs
        .get(name)
        .ok_or_else(|| PkgError::NotFound(format!("no versions on registry for '{name}'")))?;
    if !entry.installable_versions.is_empty() {
        return Ok(entry.installable_versions.clone());
    }
    if !entry.versions.is_empty() {
        return Ok(entry.versions.clone());
    }
    Ok(vec![entry.version.clone()])
}

pub fn fetch_version_meta(base: &str, name: &str, version: &str) -> PkgResult<RegistryVersionMeta> {
    let url = format!("{base}/v1/packages/{name}/{version}");
    let text = fetch_text(&url)?;
    serde_json::from_str(&text)
        .map_err(|e| PkgError::Message(format!("parse registry version meta: {e}")))
}

fn pick_registry_version(requested: &str, available: &[String]) -> PkgResult<String> {
    if available.is_empty() {
        return Err(PkgError::NotFound(format!(
            "no versions on registry for requested '{requested}'"
        )));
    }
    if is_latest_request(requested) {
        return latest_version(available).ok_or_else(|| {
            PkgError::NotFound(format!(
                "no versions on registry for requested '{requested}'"
            ))
        });
    }
    if let Some(v) = available.iter().find(|v| version_matches(requested, v)) {
        return Ok(v.clone());
    }
    pick_version(requested, available)
}

fn sha256_hex(data: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(data);
    format!("{:x}", hasher.finalize())
}

fn extract_tarball(bytes: &[u8], dest_root: &Path, expected_name: &str) -> PkgResult<()> {
    if dest_root.exists() {
        fs::remove_dir_all(dest_root)?;
    }
    fs::create_dir_all(dest_root)?;

    let decoder = GzDecoder::new(bytes);
    let mut archive = Archive::new(decoder);
    archive
        .unpack(dest_root)
        .map_err(|e| PkgError::Message(format!("extract registry tarball: {e}")))?;

    let pkg_root = dest_root.join(expected_name);
    if !pkg_root.is_dir() {
        return Err(PkgError::Message(format!(
            "tarball missing package directory '{expected_name}'"
        )));
    }
    Ok(())
}

/// Download and cache a library from the online registry.
pub fn load_lib_from_registry(
    name: &str,
    version_req: &str,
    registry_base: Option<&str>,
) -> PkgResult<(LibPackage, PathBuf)> {
    let base = registry_base
        .map(|s| s.trim_end_matches('/').to_string())
        .unwrap_or_else(registry_url);

    let versions = fetch_package_versions(&base, name)?;
    let version = pick_registry_version(version_req, &versions)?;
    let meta = fetch_version_meta(&base, name, &version)?;

    let cache_lib = registry_cache_dir().join(name);
    let version_dir = cache_lib.join(&version);
    let package_path = cache_lib.join("package.json");

    if version_dir.join("lib.json").is_file() && package_path.is_file() {
        let pkg = read_lib_package(&package_path)?;
        return Ok((pkg, version_dir));
    }

    let tarball_url = if meta.dist.tarball.starts_with("http") {
        meta.dist.tarball.clone()
    } else {
        format!("{base}{}", meta.dist.tarball)
    };

    let bytes = fetch_bytes(&tarball_url)?;
    if meta.dist.shasum.len() == 64 {
        let actual = sha256_hex(&bytes);
        if actual != meta.dist.shasum {
            return Err(PkgError::Message(format!(
                "registry checksum mismatch for {name}@{version}"
            )));
        }
    }

    extract_tarball(&bytes, &registry_cache_dir(), name)?;

    let pkg = read_lib_package(&package_path)?;
    if pkg.name != name {
        return Err(PkgError::Message(format!(
            "registry package name mismatch: expected '{name}', found '{}'",
            pkg.name
        )));
    }
    Ok((pkg, version_dir))
}

pub fn latest_lib_version_registry(
    name: &str,
    registry_base: Option<&str>,
) -> PkgResult<String> {
    let base = registry_base
        .map(|s| s.trim_end_matches('/').to_string())
        .unwrap_or_else(registry_url);
    let versions = fetch_package_versions(&base, name)?;
    latest_version(&versions).ok_or_else(|| {
        PkgError::NotFound(format!("no versions on registry for '{name}'"))
    })
}

/// Resolve a library from local source, falling back to the online registry.
pub fn load_lib(
    source_root: Option<&Path>,
    name: &str,
    version_req: &str,
    registry_base: Option<&str>,
) -> PkgResult<(LibPackage, PathBuf)> {
    if let Some(root) = source_root {
        if let Ok(result) = crate::source::load_lib_from_source(root, name, version_req) {
            return Ok(result);
        }
    }

    if is_remote_lib(name) || source_root.is_none() {
        return load_lib_from_registry(name, version_req, registry_base);
    }

    if let Some(root) = source_root {
        return crate::source::load_lib_from_source(root, name, version_req);
    }

    Err(PkgError::Message(format!(
        "library '{name}' not found locally and registry fallback unavailable"
    )))
}

pub fn latest_lib_version(
    source_root: Option<&Path>,
    name: &str,
    registry_base: Option<&str>,
) -> PkgResult<String> {
    if let Some(root) = source_root {
        if let Ok(v) = crate::source::latest_lib_version(root, name) {
            return Ok(v);
        }
    }
    if is_remote_lib(name) {
        return latest_lib_version_registry(name, registry_base);
    }
    if let Some(root) = source_root {
        return crate::source::latest_lib_version(root, name);
    }
    Err(PkgError::NotFound(format!("library '{name}' not found")))
}

pub fn toolchain_version(_source_root: Option<&Path>) -> String {
    NIAO_TOOLCHAIN_VERSION.to_string()
}
