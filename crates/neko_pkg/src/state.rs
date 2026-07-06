use crate::catalog::{LibKind, LibSpec};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstalledLib {
    pub name: String,
    pub version: String,
    pub kind: LibKind,
    pub description: String,
    pub import_paths: Vec<String>,
    pub builtin_count: usize,
    pub installed_at: String,
}

impl From<&LibSpec> for InstalledLib {
    fn from(spec: &LibSpec) -> Self {
        Self {
            name: spec.name.clone(),
            version: spec.version.clone(),
            kind: spec.kind,
            description: spec.description.clone(),
            import_paths: spec.import_paths.clone(),
            builtin_count: spec.builtin_count,
            installed_at: chrono_now(),
        }
    }
}

impl InstalledLib {
    pub fn from_lib_package(pkg: &crate::package::LibPackage) -> Self {
        Self {
            name: pkg.name.clone(),
            version: pkg.version.clone(),
            kind: pkg.kind,
            description: pkg.description.clone(),
            import_paths: pkg.import_paths.clone(),
            builtin_count: pkg.builtin_count,
            installed_at: chrono_now(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstallState {
    pub neko_version: String,
    pub mode: String,
    pub installed_at: String,
    pub root: String,
    #[serde(default)]
    pub source_root: String,
    pub libs: BTreeMap<String, InstalledLib>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LibsCatalog {
    pub neko_version: String,
    pub updated_at: String,
    pub libs: BTreeMap<String, InstalledLib>,
}

impl LibsCatalog {
    pub fn from_specs(specs: &[LibSpec]) -> Self {
        let mut libs = BTreeMap::new();
        for spec in specs {
            libs.insert(spec.name.clone(), InstalledLib::from(spec));
        }
        Self {
            neko_version: crate::catalog::NEKO_TOOLCHAIN_VERSION.to_string(),
            updated_at: chrono_now(),
            libs,
        }
    }
}

fn chrono_now() -> String {
    chrono_now_public()
}

pub fn chrono_now_public() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    ms.to_string()
}
