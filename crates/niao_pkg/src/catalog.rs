use serde::{Deserialize, Serialize};

/// ahiru-server library version (may differ from toolchain for native lib updates).
pub const AHIRU_LIB_VERSION: &str = "0.3.0";

/// Niao toolchain version (matches workspace `Cargo.toml`).
pub const NIAO_TOOLCHAIN_VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum LibKind {
    #[default]
    Native,
    Source,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LibSpec {
    pub name: String,
    pub version: String,
    pub kind: LibKind,
    pub description: String,
    pub import_paths: Vec<String>,
    pub builtin_count: usize,
}

impl LibSpec {
    pub fn manifest_json(&self) -> String {
        serde_json::to_string_pretty(self).unwrap_or_default()
    }
}

fn native_lib(
    name: &str,
    version: &str,
    description: &str,
    import_paths: &[&str],
    builtin_count: usize,
) -> LibSpec {
    LibSpec {
        name: name.to_string(),
        version: version.to_string(),
        kind: LibKind::Native,
        description: description.to_string(),
        import_paths: import_paths.iter().map(|s| s.to_string()).collect(),
        builtin_count,
    }
}

/// Built-in standard libraries shipped with the Niao toolchain.
pub fn standard_libs() -> Vec<LibSpec> {
    vec![
        native_lib(
            "core",
            NIAO_TOOLCHAIN_VERSION,
            "Core builtins: print, len, type, assert, errors, timing, arrays",
            &[],
            17,
        ),
        native_lib(
            "dsa",
            NIAO_TOOLCHAIN_VERSION,
            "Data structures and algorithms: list, stack, queue, heap, map, graph, sort",
            &["dsa", "std/dsa"],
            90,
        ),
        native_lib(
            "json",
            NIAO_TOOLCHAIN_VERSION,
            "JSON parse, stringify, and object utilities",
            &["json", "std/json"],
            15,
        ),
        native_lib(
            "io",
            NIAO_TOOLCHAIN_VERSION,
            "File I/O, paths, streaming handles, async background tasks",
            &["io", "std/io"],
            55,
        ),
        native_lib(
            "re",
            NIAO_TOOLCHAIN_VERSION,
            "Regular expressions: match, find, replace, split",
            &["re", "std/re"],
            22,
        ),
        native_lib(
            "net",
            NIAO_TOOLCHAIN_VERSION,
            "Networking: HTTP, TCP/UDP, DNS, TLS, WebSocket, SMTP, FTP",
            &["net", "std/net"],
            55,
        ),
        native_lib(
            "parallel",
            NIAO_TOOLCHAIN_VERSION,
            "Threading, mutexes, channels, worker pools, and cooperative poll",
            &["parallel", "std/parallel"],
            38,
        ),
        native_lib(
            "time",
            NIAO_TOOLCHAIN_VERSION,
            "Wall clock, formatting, parsing, time zones, and date arithmetic",
            &["time", "std/time"],
            32,
        ),
        native_lib(
            "nsqlite",
            NIAO_TOOLCHAIN_VERSION,
            "Fast SQLite: schema, migrations, prepared statements, transactions, async",
            &["nsqlite", "std/nsqlite"],
            39,
        ),
        native_lib(
            "npg",
            NIAO_TOOLCHAIN_VERSION,
            "Fast PostgreSQL: pools, migrations, prepared statements, transactions, async",
            &["npg", "std/npg"],
            52,
        ),
        native_lib(
            "nmongo",
            NIAO_TOOLCHAIN_VERSION,
            "Fast MongoDB: CRUD, aggregation, indexes, transactions, GridFS, change streams, async",
            &["nmongo", "std/nmongo"],
            45,
        ),
        native_lib(
            "nos",
            NIAO_TOOLCHAIN_VERSION,
            "OS interface: process, platform constants, lightweight filesystem",
            &["nos", "std/nos"],
            23,
        ),
        native_lib(
            "nenv",
            NIAO_TOOLCHAIN_VERSION,
            "Environment variables, .env loading, typed accessors, validation, and stores",
            &["nenv", "std/nenv"],
            26,
        ),
        native_lib(
            "ncl",
            NIAO_TOOLCHAIN_VERSION,
            "Niao Column Library: ndarray, Series, DataFrame, vectorized math, groupby, CSV, nsqlite bridge",
            &["ncl", "std/ncl"],
            62,
        ),
        native_lib(
            "nml",
            NIAO_TOOLCHAIN_VERSION,
            "Niao Machine Learning: tensors, autograd, training, data pipelines, GNN, classic ML",
            &["nml", "std/nml"],
            67,
        ),
        native_lib(
            "nvis",
            NIAO_TOOLCHAIN_VERSION,
            "Niao visualization: line, histogram, scatter, heatmap, bar charts (SVG + ASCII)",
            &["nvis", "std/nvis"],
            8,
        ),
        native_lib(
            "ahiru",
            AHIRU_LIB_VERSION,
            "ahiru-server 0.3.0: state, custom middleware, groups, cache, jobs, metrics, CLI toolkit",
            &["ahiru", "std/ahiru"],
            36,
        ),
    ]
}

/// Libraries installed from the online registry (not bundled with core Niao).
pub const REMOTE_LIBS: &[&str] = &["nllm", "nrag"];

pub fn remote_libs() -> &'static [&'static str] {
    REMOTE_LIBS
}

/// Alias used by install code.
pub const STANDARD_LIBS: &[&str] = &[
    "core", "dsa", "json", "io", "re", "net", "parallel", "time", "nsqlite", "npg", "nmongo", "nos", "nenv", "ncl", "nml", "nvis", "ahiru",
];

/// Map user-facing names (e.g. `ahiru-server`) to catalog lib names (`ahiru`).
pub fn resolve_lib_name(name: &str) -> String {
    let name = name.trim();
    if standard_libs().iter().any(|s| s.name == name) {
        return name.to_string();
    }
    for spec in standard_libs() {
        if spec.import_paths.iter().any(|p| p == name) {
            return spec.name.clone();
        }
    }
    match name {
        "ahiru-server" => "ahiru".to_string(),
        _ => name.to_string(),
    }
}
