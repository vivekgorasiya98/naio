//! Native OS standard library — process, platform constants, and lightweight
//! filesystem helpers (Python `os`-style API). Environment variables live in `nenv`.
//!
//! Registered as prefixed builtins (`nos_getpid`, `nos_getcwd`, ...).
//! Import with `import "nos"` (or `import "std/nos"`) for the namespace API.
//!
//! File streaming and heavy I/O live in the `io` module; `nos` focuses on
//! process/OS surface area with thin Rust wrappers for speed.

use crate::{error_value, NativeFn, NiaoResult, RuntimeError, StringArray, Value, ValueRef};
use niao_ast::Span;
use niao_errors::codes;
use std::collections::HashMap;
use std::env;
use std::fs::{self, Metadata};
use std::path::PathBuf;
use std::process::{self, Command};
use std::rc::Rc;
use std::sync::{Mutex, OnceLock};
use std::time::UNIX_EPOCH;

// ---------------------------------------------------------------------------
// Program arguments (set by the CLI before running a script)
// ---------------------------------------------------------------------------

static PROGRAM_ARGS: OnceLock<Mutex<Vec<String>>> = OnceLock::new();

fn program_args_slot() -> &'static Mutex<Vec<String>> {
    PROGRAM_ARGS.get_or_init(|| Mutex::new(Vec::new()))
}

/// Script arguments passed after the file path on the CLI (`niao run app.niao a b`).
pub fn set_program_args(args: Vec<String>) {
    if let Ok(mut slot) = program_args_slot().lock() {
        *slot = args;
    }
}

fn program_args() -> Vec<String> {
    program_args_slot()
        .lock()
        .map(|v| v.clone())
        .unwrap_or_default()
}

// ---------------------------------------------------------------------------
// Platform constants
// ---------------------------------------------------------------------------

#[cfg(windows)]
const OS_NAME: &str = "nt";
#[cfg(not(windows))]
const OS_NAME: &str = "posix";

#[cfg(windows)]
const PATH_SEP: &str = "\\";
#[cfg(not(windows))]
const PATH_SEP: &str = "/";

#[cfg(windows)]
const ALT_SEP: &str = "/";
#[cfg(not(windows))]
const ALT_SEP: &str = "";

#[cfg(windows)]
const PATH_LIST_SEP: &str = ";";
#[cfg(not(windows))]
const PATH_LIST_SEP: &str = ":";

#[cfg(windows)]
const LINE_SEP: &str = "\r\n";
#[cfg(not(windows))]
const LINE_SEP: &str = "\n";

#[cfg(windows)]
const DEV_NULL: &str = "NUL";
#[cfg(not(windows))]
const DEV_NULL: &str = "/dev/null";

// ---------------------------------------------------------------------------
// Argument helpers
// ---------------------------------------------------------------------------

fn type_err(span: Span, msg: impl Into<String>) -> RuntimeError {
    RuntimeError::TypeError {
        message: msg.into(),
        line: span.line,
        col: span.col,
    }
}

fn arity(args: &[ValueRef], n: usize, name: &str, span: Span) -> NiaoResult<()> {
    if args.len() != n {
        return Err(RuntimeError::at(
            span,
            codes::E1800_NOS_ARITY,
            format!("{name}() expects {n} argument(s), got {}", args.len()),
        ));
    }
    Ok(())
}

fn arity_range(args: &[ValueRef], min: usize, max: usize, name: &str, span: Span) -> NiaoResult<()> {
    if args.len() < min || args.len() > max {
        return Err(RuntimeError::at(
            span,
            codes::E1800_NOS_ARITY,
            format!("{name}() expects {min}..={max} argument(s), got {}", args.len()),
        ));
    }
    Ok(())
}

fn string_arg(args: &[ValueRef], idx: usize, name: &str, span: Span) -> NiaoResult<String> {
    match &*args[idx].borrow() {
        Value::String(s) => Ok(s.clone()),
        other => Err(type_err(
            span,
            format!(
                "{name}() expects a string as argument {}, got {}",
                idx + 1,
                other.type_name()
            ),
        )),
    }
}

fn int_arg(args: &[ValueRef], idx: usize, name: &str, span: Span) -> NiaoResult<i64> {
    match &*args[idx].borrow() {
        Value::Int(n) => Ok(*n),
        other => Err(type_err(
            span,
            format!(
                "{name}() expects an int as argument {}, got {}",
                idx + 1,
                other.type_name()
            ),
        )),
    }
}

fn path_from_arg(args: &[ValueRef], idx: usize, name: &str, span: Span) -> NiaoResult<PathBuf> {
    Ok(PathBuf::from(string_arg(args, idx, name, span)?))
}

fn nos_error(span: Span, msg: impl Into<String>) -> ValueRef {
    error_value(codes::E1801_NOS_ERROR, "nos_error", msg.into(), span)
}

fn ok_nil() -> ValueRef {
    Value::Nil.ref_cell()
}

fn ok_bool(b: bool) -> ValueRef {
    Value::Bool(b).ref_cell()
}

fn ok_int(n: i64) -> ValueRef {
    Value::Int(n).ref_cell()
}

fn ok_string(s: impl Into<String>) -> ValueRef {
    Value::String(s.into()).ref_cell()
}

fn ok_string_array(items: Vec<String>) -> ValueRef {
    Value::StringArray(StringArray::dense(items)).ref_cell()
}

fn metadata_ms(meta: &Metadata, field: &str) -> i64 {
    let time = match field {
        "modified" => meta.modified(),
        "accessed" => meta.accessed(),
        "created" => meta.created(),
        _ => return 0,
    };
    time.ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

fn stat_object(meta: &Metadata) -> Value {
    let mut map = HashMap::new();
    let insert = |map: &mut HashMap<String, ValueRef>, k: &str, v: Value| {
        map.insert(k.to_string(), v.ref_cell());
    };
    insert(&mut map, "size", Value::Int(meta.len() as i64));
    insert(&mut map, "mtime_ms", Value::Int(metadata_ms(meta, "modified")));
    insert(&mut map, "atime_ms", Value::Int(metadata_ms(meta, "accessed")));
    insert(&mut map, "ctime_ms", Value::Int(metadata_ms(meta, "created")));
    insert(&mut map, "is_file", Value::Bool(meta.is_file()));
    insert(&mut map, "is_dir", Value::Bool(meta.is_dir()));
    insert(&mut map, "is_symlink", Value::Bool(meta.is_symlink()));
    insert(&mut map, "readonly", Value::Bool(meta.permissions().readonly()));
    Value::Object(map)
}

// ---------------------------------------------------------------------------
// Process
// ---------------------------------------------------------------------------

fn nos_getpid(_args: &[ValueRef], _span: Span) -> NiaoResult<ValueRef> {
    Ok(ok_int(process::id() as i64))
}

fn nos_getppid(_args: &[ValueRef], _span: Span) -> NiaoResult<ValueRef> {
    // Not available in std; return 0 when unsupported (Python raises on some platforms).
    Ok(ok_int(0))
}

fn nos_exit(args: &[ValueRef], span: Span) -> NiaoResult<ValueRef> {
    arity_range(args, 0, 1, "nos_exit", span)?;
    let code = if args.is_empty() {
        0
    } else {
        int_arg(args, 0, "nos_exit", span)? as i32
    };
    process::exit(code);
}

fn nos_system(args: &[ValueRef], span: Span) -> NiaoResult<ValueRef> {
    arity(args, 1, "nos_system", span)?;
    let cmd = string_arg(args, 0, "nos_system", span)?;
    let status = if cfg!(windows) {
        Command::new("cmd").args(["/C", &cmd]).status()
    } else {
        Command::new("sh").args(["-c", &cmd]).status()
    };
    match status {
        Ok(s) => Ok(ok_int(s.code().unwrap_or(-1) as i64)),
        Err(e) => Ok(nos_error(span, e.to_string())),
    }
}

fn nos_argv(_args: &[ValueRef], _span: Span) -> NiaoResult<ValueRef> {
    let items: Vec<ValueRef> = program_args()
        .into_iter()
        .map(|s| Value::String(s).ref_cell())
        .collect();
    Ok(Value::Array(items).ref_cell())
}

// ---------------------------------------------------------------------------
// Working directory
// ---------------------------------------------------------------------------

fn nos_getcwd(_args: &[ValueRef], span: Span) -> NiaoResult<ValueRef> {
    match env::current_dir() {
        Ok(p) => Ok(ok_string(p.to_string_lossy().into_owned())),
        Err(e) => Ok(nos_error(span, e.to_string())),
    }
}

fn nos_chdir(args: &[ValueRef], span: Span) -> NiaoResult<ValueRef> {
    arity(args, 1, "nos_chdir", span)?;
    let path = path_from_arg(args, 0, "nos_chdir", span)?;
    match env::set_current_dir(&path) {
        Ok(()) => Ok(ok_nil()),
        Err(e) => Ok(nos_error(span, e.to_string())),
    }
}

// ---------------------------------------------------------------------------
// Filesystem (lightweight — whole-path ops only)
// ---------------------------------------------------------------------------

fn nos_listdir(args: &[ValueRef], span: Span) -> NiaoResult<ValueRef> {
    arity_range(args, 0, 1, "nos_listdir", span)?;
    let path = if args.is_empty() {
        env::current_dir().map_err(|e| type_err(span, e.to_string()))?
    } else {
        path_from_arg(args, 0, "nos_listdir", span)?
    };
    match fs::read_dir(&path) {
        Ok(entries) => {
            let mut names = Vec::new();
            for entry in entries {
                match entry {
                    Ok(e) => names.push(e.file_name().to_string_lossy().into_owned()),
                    Err(e) => return Ok(nos_error(span, e.to_string())),
                }
            }
            names.sort_unstable();
            Ok(ok_string_array(names))
        }
        Err(e) => Ok(nos_error(span, e.to_string())),
    }
}

fn nos_mkdir(args: &[ValueRef], span: Span) -> NiaoResult<ValueRef> {
    arity(args, 1, "nos_mkdir", span)?;
    let path = path_from_arg(args, 0, "nos_mkdir", span)?;
    match fs::create_dir(&path) {
        Ok(()) => Ok(ok_nil()),
        Err(e) => Ok(nos_error(span, e.to_string())),
    }
}

fn nos_makedirs(args: &[ValueRef], span: Span) -> NiaoResult<ValueRef> {
    arity(args, 1, "nos_makedirs", span)?;
    let path = path_from_arg(args, 0, "nos_makedirs", span)?;
    match fs::create_dir_all(&path) {
        Ok(()) => Ok(ok_nil()),
        Err(e) => Ok(nos_error(span, e.to_string())),
    }
}

fn nos_rmdir(args: &[ValueRef], span: Span) -> NiaoResult<ValueRef> {
    arity(args, 1, "nos_rmdir", span)?;
    let path = path_from_arg(args, 0, "nos_rmdir", span)?;
    match fs::remove_dir(&path) {
        Ok(()) => Ok(ok_nil()),
        Err(e) => Ok(nos_error(span, e.to_string())),
    }
}

fn nos_remove(args: &[ValueRef], span: Span) -> NiaoResult<ValueRef> {
    arity(args, 1, "nos_remove", span)?;
    let path = path_from_arg(args, 0, "nos_remove", span)?;
    let result = if path.is_dir() {
        fs::remove_dir_all(&path)
    } else {
        fs::remove_file(&path)
    };
    match result {
        Ok(()) => Ok(ok_nil()),
        Err(e) => Ok(nos_error(span, e.to_string())),
    }
}

fn nos_rename(args: &[ValueRef], span: Span) -> NiaoResult<ValueRef> {
    arity(args, 2, "nos_rename", span)?;
    let from = path_from_arg(args, 0, "nos_rename", span)?;
    let to = path_from_arg(args, 1, "nos_rename", span)?;
    match fs::rename(&from, &to) {
        Ok(()) => Ok(ok_nil()),
        Err(e) => Ok(nos_error(span, e.to_string())),
    }
}

fn nos_exists(args: &[ValueRef], span: Span) -> NiaoResult<ValueRef> {
    arity(args, 1, "nos_exists", span)?;
    let path = path_from_arg(args, 0, "nos_exists", span)?;
    Ok(ok_bool(path.exists()))
}

fn nos_isfile(args: &[ValueRef], span: Span) -> NiaoResult<ValueRef> {
    arity(args, 1, "nos_isfile", span)?;
    let path = path_from_arg(args, 0, "nos_isfile", span)?;
    Ok(ok_bool(path.is_file()))
}

fn nos_isdir(args: &[ValueRef], span: Span) -> NiaoResult<ValueRef> {
    arity(args, 1, "nos_isdir", span)?;
    let path = path_from_arg(args, 0, "nos_isdir", span)?;
    Ok(ok_bool(path.is_dir()))
}

fn nos_stat(args: &[ValueRef], span: Span) -> NiaoResult<ValueRef> {
    arity(args, 1, "nos_stat", span)?;
    let path = path_from_arg(args, 0, "nos_stat", span)?;
    match fs::metadata(&path) {
        Ok(meta) => Ok(stat_object(&meta).ref_cell()),
        Err(e) => Ok(nos_error(span, e.to_string())),
    }
}

fn nos_lstat(args: &[ValueRef], span: Span) -> NiaoResult<ValueRef> {
    arity(args, 1, "nos_lstat", span)?;
    let path = path_from_arg(args, 0, "nos_lstat", span)?;
    match fs::symlink_metadata(&path) {
        Ok(meta) => Ok(stat_object(&meta).ref_cell()),
        Err(e) => Ok(nos_error(span, e.to_string())),
    }
}

// ---------------------------------------------------------------------------
// Platform info
// ---------------------------------------------------------------------------

fn nos_cpu_count(_args: &[ValueRef], _span: Span) -> NiaoResult<ValueRef> {
    let n = std::thread::available_parallelism()
        .map(|p| p.get() as i64)
        .unwrap_or(1);
    Ok(ok_int(n))
}

fn nos_hostname(_args: &[ValueRef], span: Span) -> NiaoResult<ValueRef> {
    if let Ok(h) = env::var("COMPUTERNAME").or_else(|_| env::var("HOSTNAME")) {
        return Ok(ok_string(h));
    }
    Ok(nos_error(span, "hostname unavailable"))
}

fn nos_username(_args: &[ValueRef], span: Span) -> NiaoResult<ValueRef> {
    if let Ok(u) = env::var("USER").or_else(|_| env::var("USERNAME")) {
        return Ok(ok_string(u));
    }
    Ok(nos_error(span, "username unavailable"))
}

fn nos_platform(_args: &[ValueRef], _span: Span) -> NiaoResult<ValueRef> {
    #[cfg(windows)]
    return Ok(ok_string("windows"));
    #[cfg(target_os = "macos")]
    return Ok(ok_string("macos"));
    #[cfg(target_os = "linux")]
    return Ok(ok_string("linux"));
    #[cfg(not(any(windows, target_os = "macos", target_os = "linux")))]
    return Ok(ok_string("unknown"));
}

fn nos_arch(_args: &[ValueRef], _span: Span) -> NiaoResult<ValueRef> {
    Ok(ok_string(std::env::consts::ARCH))
}

// ---------------------------------------------------------------------------
// Registration
// ---------------------------------------------------------------------------

fn all_builtins() -> Vec<(&'static str, NativeFn)> {
    vec![
        ("nos_getpid", Rc::new(nos_getpid)),
        ("nos_getppid", Rc::new(nos_getppid)),
        ("nos_exit", Rc::new(nos_exit)),
        ("nos_system", Rc::new(nos_system)),
        ("nos_argv", Rc::new(nos_argv)),
        ("nos_getcwd", Rc::new(nos_getcwd)),
        ("nos_chdir", Rc::new(nos_chdir)),
        ("nos_listdir", Rc::new(nos_listdir)),
        ("nos_mkdir", Rc::new(nos_mkdir)),
        ("nos_makedirs", Rc::new(nos_makedirs)),
        ("nos_rmdir", Rc::new(nos_rmdir)),
        ("nos_remove", Rc::new(nos_remove)),
        ("nos_rename", Rc::new(nos_rename)),
        ("nos_exists", Rc::new(nos_exists)),
        ("nos_isfile", Rc::new(nos_isfile)),
        ("nos_isdir", Rc::new(nos_isdir)),
        ("nos_stat", Rc::new(nos_stat)),
        ("nos_lstat", Rc::new(nos_lstat)),
        ("nos_cpu_count", Rc::new(nos_cpu_count)),
        ("nos_hostname", Rc::new(nos_hostname)),
        ("nos_username", Rc::new(nos_username)),
        ("nos_platform", Rc::new(nos_platform)),
        ("nos_arch", Rc::new(nos_arch)),
    ]
}

/// Namespace object for `nos.getcwd`, `nos.getpid`, etc.
pub fn namespace() -> Value {
    let mut map = HashMap::new();
    let bind = |map: &mut HashMap<String, ValueRef>, name: &str, f: NativeFn| {
        map.insert(name.to_string(), Value::NativeFunction(f).ref_cell());
    };
    let insert_str = |map: &mut HashMap<String, ValueRef>, name: &str, s: &str| {
        map.insert(name.to_string(), Value::String(s.to_string()).ref_cell());
    };

    // Python-style constants
    insert_str(&mut map, "name", OS_NAME);
    insert_str(&mut map, "sep", PATH_SEP);
    insert_str(&mut map, "altsep", ALT_SEP);
    insert_str(&mut map, "pathsep", PATH_LIST_SEP);
    insert_str(&mut map, "linesep", LINE_SEP);
    insert_str(&mut map, "devnull", DEV_NULL);

    bind(&mut map, "getpid", Rc::new(nos_getpid));
    bind(&mut map, "getppid", Rc::new(nos_getppid));
    bind(&mut map, "exit", Rc::new(nos_exit));
    bind(&mut map, "system", Rc::new(nos_system));
    bind(&mut map, "argv", Rc::new(nos_argv));
    bind(&mut map, "getcwd", Rc::new(nos_getcwd));
    bind(&mut map, "chdir", Rc::new(nos_chdir));
    bind(&mut map, "listdir", Rc::new(nos_listdir));
    bind(&mut map, "mkdir", Rc::new(nos_mkdir));
    bind(&mut map, "makedirs", Rc::new(nos_makedirs));
    bind(&mut map, "rmdir", Rc::new(nos_rmdir));
    bind(&mut map, "remove", Rc::new(nos_remove));
    bind(&mut map, "rename", Rc::new(nos_rename));
    bind(&mut map, "exists", Rc::new(nos_exists));
    bind(&mut map, "isfile", Rc::new(nos_isfile));
    bind(&mut map, "isdir", Rc::new(nos_isdir));
    bind(&mut map, "stat", Rc::new(nos_stat));
    bind(&mut map, "lstat", Rc::new(nos_lstat));
    bind(&mut map, "cpu_count", Rc::new(nos_cpu_count));
    bind(&mut map, "hostname", Rc::new(nos_hostname));
    bind(&mut map, "username", Rc::new(nos_username));
    bind(&mut map, "platform", Rc::new(nos_platform));
    bind(&mut map, "arch", Rc::new(nos_arch));
    Value::Object(map)
}

pub const MODULE_NAME: &str = "nos";
pub const MODULE_PATHS: &[&str] = &["nos", "std/nos"];

pub fn builtins() -> Vec<(&'static str, NativeFn)> {
    all_builtins()
}

#[cfg(test)]
mod tests {
    use super::*;
    use niao_ast::Span;

    fn span() -> Span {
        Span::dummy()
    }

    #[test]
    fn getpid_positive() {
        match &*nos_getpid(&[], span()).unwrap().borrow() {
            Value::Int(n) => assert!(*n > 0),
            other => panic!("expected int, got {other:?}"),
        }
    }

    #[test]
    fn namespace_has_constants() {
        match namespace() {
            Value::Object(map) => {
                match &*map["name"].borrow() {
                    Value::String(s) => assert_eq!(s, OS_NAME),
                    other => panic!("expected string name, got {other:?}"),
                }
                match &*map["sep"].borrow() {
                    Value::String(s) => assert_eq!(s, PATH_SEP),
                    other => panic!("expected string sep, got {other:?}"),
                }
            }
            other => panic!("expected object, got {other:?}"),
        }
    }
}
