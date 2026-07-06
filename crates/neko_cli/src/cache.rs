use neko_bytecode::{compile_to_bytecode, BytecodeModule};
use neko_parser::parse;
use std::error::Error;
use std::fs;
use std::path::{Path, PathBuf};

const CACHE_DIR_NAME: &str = ".neko-build";

/// Default bytecode cache directory: `<cwd>/.neko-build/`.
pub fn default_cache_dir() -> PathBuf {
    std::env::current_dir()
        .unwrap_or_else(|_| PathBuf::from("."))
        .join(CACHE_DIR_NAME)
}

/// Stable cache file path for a source file under `cache_dir`.
pub fn cache_path(source: &Path, cache_dir: &Path) -> PathBuf {
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let rel = source.strip_prefix(&cwd).unwrap_or(source);
    let key = rel
        .to_string_lossy()
        .replace(['/', '\\'], "_")
        .replace(':', "_")
        .replace(".neko", "");
    cache_dir.join(format!("{key}.nekobc"))
}

pub fn cache_is_fresh(source: &Path, cache: &Path) -> bool {
    let Ok(src_meta) = source.metadata() else {
        return false;
    };
    let Ok(cache_meta) = cache.metadata() else {
        return false;
    };
    let Ok(src_mod) = src_meta.modified() else {
        return false;
    };
    let Ok(cache_mod) = cache_meta.modified() else {
        return false;
    };
    cache_mod >= src_mod
}

pub fn try_load_cache(cache: &Path) -> Option<BytecodeModule> {
    let bytes = fs::read(cache).ok()?;
    BytecodeModule::deserialize(&bytes)
}

pub fn write_cache_atomic(cache: &Path, module: &BytecodeModule) -> Result<(), Box<dyn Error>> {
    if let Some(parent) = cache.parent() {
        fs::create_dir_all(parent)?;
    }
    let tmp = cache.with_extension("nekobc.tmp");
    fs::write(&tmp, module.serialize())?;
    fs::rename(&tmp, cache)?;
    Ok(())
}

pub fn write_cache(cache: &Path, module: &BytecodeModule) {
    if let Err(e) = write_cache_atomic(cache, module) {
        eprintln!("warning: failed to write bytecode cache {}: {e}", cache.display());
    }
}

pub fn load_or_compile(
    file: &Path,
    cache_dir: &Path,
) -> Result<(BytecodeModule, std::time::Duration), Box<dyn Error>> {
    let compile_start = std::time::Instant::now();
    let cache = cache_path(file, cache_dir);

    if cache.is_file() && cache_is_fresh(file, &cache) {
        if let Some(mut module) = try_load_cache(&cache) {
            module.ensure_fast_path();
            return Ok((module, compile_start.elapsed()));
        }
    }

    let source = fs::read_to_string(file)?;
    let program = parse(&source).map_err(|e| e.to_string())?;
    let mut bytecode = compile_to_bytecode(&program).map_err(|e| e.to_string())?;
    bytecode.source_path = Some(
        file.canonicalize()
            .unwrap_or_else(|_| file.to_path_buf())
            .to_string_lossy()
            .into_owned(),
    );
    write_cache(&cache, &bytecode);
    Ok((bytecode, compile_start.elapsed()))
}

pub fn build_to_cache(
    file: &Path,
    output: &Path,
) -> Result<PathBuf, Box<dyn Error>> {
    let source = fs::read_to_string(file)?;
    let program = parse(&source).map_err(|e| e.to_string())?;
    let mut bytecode = compile_to_bytecode(&program).map_err(|e| e.to_string())?;
    bytecode.source_path = Some(
        file.canonicalize()
            .unwrap_or_else(|_| file.to_path_buf())
            .to_string_lossy()
            .into_owned(),
    );
    let out_path = cache_path(file, output);
    write_cache_atomic(&out_path, &bytecode)?;
    Ok(out_path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;
    use std::thread;
    use std::time::Duration;

    static CWD_LOCK: Mutex<()> = Mutex::new(());

    fn write_fixture(dir: &Path, rel: &str, body: &str) -> PathBuf {
        let path = dir.join(rel);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(&path, body).unwrap();
        path
    }

    #[test]
    fn cache_written_under_neko_build() {
        let _lock = CWD_LOCK.lock().unwrap();
        let tmp = std::env::temp_dir().join(format!("neko_cache_test_{}", std::process::id()));
        let _ = fs::remove_dir_all(&tmp);
        fs::create_dir_all(&tmp).unwrap();
        let prev = std::env::current_dir().unwrap();
        std::env::set_current_dir(&tmp).unwrap();

        write_fixture(
            &tmp,
            "examples/factorial.neko",
            r#"fn main() {
    print(1)
}
"#,
        );
        let source = Path::new("examples/factorial.neko");
        let cache_dir = default_cache_dir();
        let (module, _) = load_or_compile(source, &cache_dir).unwrap();
        assert!(module.functions.iter().any(|f| f.name == "main"));

        let cache_file = cache_path(source, &cache_dir);
        assert!(cache_file.is_file(), "cache file missing at {}", cache_file.display());

        std::env::set_current_dir(&prev).unwrap();
        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn cache_recompiles_when_source_is_newer() {
        let _lock = CWD_LOCK.lock().unwrap();
        let tmp = std::env::temp_dir().join(format!("neko_cache_fresh_{}", std::process::id()));
        let _ = fs::remove_dir_all(&tmp);
        fs::create_dir_all(&tmp).unwrap();
        let prev = std::env::current_dir().unwrap();
        std::env::set_current_dir(&tmp).unwrap();

        write_fixture(&tmp, "hello.neko", "fn main() { print(1) }\n");
        let source = Path::new("hello.neko");
        let cache_dir = default_cache_dir();
        load_or_compile(source, &cache_dir).unwrap();

        thread::sleep(Duration::from_millis(50));
        fs::write(source, "fn main() { print(1) }\n// touch\n").unwrap();

        let (module, _) = load_or_compile(source, &cache_dir).unwrap();
        assert!(module.source_path.is_some());

        std::env::set_current_dir(&prev).unwrap();
        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn distinct_cache_paths_for_same_stem() {
        let _lock = CWD_LOCK.lock().unwrap();
        let tmp = std::env::temp_dir().join(format!("neko_cache_stem_{}", std::process::id()));
        let _ = fs::remove_dir_all(&tmp);
        fs::create_dir_all(&tmp).unwrap();
        let prev = std::env::current_dir().unwrap();
        std::env::set_current_dir(&tmp).unwrap();

        write_fixture(&tmp, "a/foo.neko", "fn main() { print(1) }\n");
        write_fixture(&tmp, "b/foo.neko", "fn main() { print(2) }\n");
        let cache_dir = default_cache_dir();
        let path_a = cache_path(Path::new("a/foo.neko"), &cache_dir);
        let path_b = cache_path(Path::new("b/foo.neko"), &cache_dir);
        assert_ne!(path_a, path_b);

        std::env::set_current_dir(&prev).unwrap();
        let _ = fs::remove_dir_all(&tmp);
    }
}
