//! Native nenv standard library — environment variables, `.env` file loading,
//! typed accessors, validation, and isolated env stores.
//!
//! Import with `import "nenv"` (or `import "std/nenv"`).

use crate::{error_value, NativeFn, NekoResult, RuntimeError, Value, ValueRef};
use neko_ast::Span;
use neko_errors::codes;
use std::cell::RefCell;
use std::collections::HashMap;
use std::env;
use std::fs;
use std::io::{BufRead, Cursor};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::atomic::{AtomicI64, Ordering};

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

fn arity(args: &[ValueRef], n: usize, name: &str, span: Span) -> NekoResult<()> {
    if args.len() != n {
        return Err(RuntimeError::at(
            span,
            codes::E1950_NENV_ARITY,
            format!("{name}() expects {n} argument(s), got {}", args.len()),
        ));
    }
    Ok(())
}

fn arity_range(args: &[ValueRef], min: usize, max: usize, name: &str, span: Span) -> NekoResult<()> {
    if args.len() < min || args.len() > max {
        return Err(RuntimeError::at(
            span,
            codes::E1950_NENV_ARITY,
            format!("{name}() expects {min}..={max} argument(s), got {}", args.len()),
        ));
    }
    Ok(())
}

fn string_arg(args: &[ValueRef], idx: usize, name: &str, span: Span) -> NekoResult<String> {
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

fn int_arg(args: &[ValueRef], idx: usize, name: &str, span: Span) -> NekoResult<i64> {
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

fn optional_object_arg(
    args: &[ValueRef],
    idx: usize,
) -> Option<HashMap<String, ValueRef>> {
    if args.len() <= idx {
        return None;
    }
    match &*args[idx].borrow() {
        Value::Object(map) => Some(map.clone()),
        Value::Nil => None,
        _ => None,
    }
}

fn bool_field(map: Option<&HashMap<String, ValueRef>>, key: &str, default: bool) -> bool {
    let Some(map) = map else {
        return default;
    };
    match map.get(key).map(|v| v.borrow().clone()) {
        Some(Value::Bool(b)) => b,
        Some(Value::Int(n)) => n != 0,
        Some(Value::String(s)) => matches!(s.as_str(), "true" | "1" | "yes" | "on"),
        _ => default,
    }
}

fn nenv_error(span: Span, msg: impl Into<String>) -> ValueRef {
    error_value(codes::E1951_NENV_ERROR, "nenv_error", msg.into(), span)
}

fn nenv_not_found(span: Span, key: &str) -> ValueRef {
    error_value(
        codes::E1952_NENV_NOT_FOUND,
        "nenv_error",
        format!("required environment variable not set: {key}"),
        span,
    )
}

fn nenv_invalid_value(span: Span, msg: impl Into<String>) -> ValueRef {
    error_value(codes::E1953_NENV_INVALID_VALUE, "nenv_error", msg.into(), span)
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

fn value_to_string(v: &Value) -> Option<String> {
    match v {
        Value::String(s) => Some(s.clone()),
        Value::Int(n) => Some(n.to_string()),
        Value::Float(f) => Some(f.to_string()),
        Value::Bool(b) => Some(b.to_string()),
        Value::Nil => Some(String::new()),
        _ => None,
    }
}

fn object_from_pairs(pairs: &[(String, String)]) -> ValueRef {
    let mut map = HashMap::new();
    for (k, v) in pairs {
        map.insert(k.clone(), Value::String(v.clone()).ref_cell());
    }
    Value::Object(map).ref_cell()
}

fn parse_dotenv_reader<R: BufRead>(reader: R) -> Result<Vec<(String, String)>, String> {
    let iter = dotenvy::from_read_iter(reader);
    let mut pairs = Vec::new();
    for item in iter {
        let (k, v) = item.map_err(|e| e.to_string())?;
        pairs.push((k, v));
    }
    Ok(pairs)
}

fn parse_dotenv_file(path: &Path) -> Result<Vec<(String, String)>, String> {
    let file = fs::File::open(path).map_err(|e| e.to_string())?;
    parse_dotenv_reader(std::io::BufReader::new(file))
}

fn apply_pairs(pairs: &[(String, String)], override_existing: bool) -> usize {
    let mut count = 0;
    for (k, v) in pairs {
        if override_existing || env::var(k).is_err() {
            env::set_var(k, v);
            count += 1;
        }
    }
    count
}

fn load_opts(args: &[ValueRef], opts_idx: usize) -> bool {
    bool_field(optional_object_arg(args, opts_idx).as_ref(), "override", false)
}

fn parse_bool_str(s: &str) -> Option<bool> {
    match s.trim().to_ascii_lowercase().as_str() {
        "true" | "1" | "yes" | "on" => Some(true),
        "false" | "0" | "no" | "off" => Some(false),
        _ => None,
    }
}

fn expand_text(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let bytes = text.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'$' {
            if i + 1 < bytes.len() && bytes[i + 1] == b'{' {
                if let Some((var, end)) = read_braced_var(text, i + 2) {
                    out.push_str(&lookup_var(&var));
                    i = end + 1;
                    continue;
                }
            } else if let Some((var, end)) = read_bare_var(text, i + 1) {
                out.push_str(&lookup_var(&var));
                i = end;
                continue;
            }
        }
        out.push(bytes[i] as char);
        i += 1;
    }
    out
}

fn read_braced_var(text: &str, start: usize) -> Option<(String, usize)> {
    let rest = &text[start..];
    let end = rest.find('}')?;
    let name = rest[..end].trim();
    if name.is_empty() || !is_var_char(name.chars().next()?) {
        return None;
    }
    if !name.chars().all(is_var_char) {
        return None;
    }
    Some((name.to_string(), start + end))
}

fn read_bare_var(text: &str, start: usize) -> Option<(String, usize)> {
    let rest = text[start..].chars();
    let mut name = String::new();
    let mut end = start;
    for ch in rest {
        if is_var_char(ch) {
            name.push(ch);
            end += ch.len_utf8();
        } else {
            break;
        }
    }
    if name.is_empty() {
        None
    } else {
        Some((name, end))
    }
}

fn is_var_char(ch: char) -> bool {
    ch.is_ascii_alphanumeric() || ch == '_'
}

fn lookup_var(name: &str) -> String {
    env::var(name).unwrap_or_default()
}

// ---------------------------------------------------------------------------
// Env store handles
// ---------------------------------------------------------------------------

struct EnvStore {
    vars: HashMap<String, String>,
    inherit: Option<HashMap<String, String>>,
}

thread_local! {
    static STORES: RefCell<HashMap<i64, EnvStore>> = RefCell::new(HashMap::new());
}

static STORE_COUNTER: AtomicI64 = AtomicI64::new(1);

fn alloc_store(inherit: bool) -> i64 {
    let inherit_map = if inherit {
        Some(env::vars().collect())
    } else {
        None
    };
    let id = STORE_COUNTER.fetch_add(1, Ordering::Relaxed);
    STORES.with(|stores| {
        stores.borrow_mut().insert(
            id,
            EnvStore {
                vars: HashMap::new(),
                inherit: inherit_map,
            },
        );
    });
    id
}

fn with_store<F, R>(id: i64, name: &str, span: Span, f: F) -> Result<R, RuntimeError>
where
    F: FnOnce(&mut EnvStore) -> R,
{
    STORES.with(|stores| {
        let mut guard = stores.borrow_mut();
        let store = guard.get_mut(&id).ok_or_else(|| {
            RuntimeError::at(
                span,
                codes::E1954_NENV_INVALID_HANDLE,
                format!("{name}(): invalid or closed env store handle {id}"),
            )
        })?;
        Ok(f(store))
    })
}

fn remove_store(id: i64) -> bool {
    STORES.with(|stores| stores.borrow_mut().remove(&id).is_some())
}

fn store_lookup(store: &EnvStore, key: &str) -> Option<String> {
    store
        .vars
        .get(key)
        .cloned()
        .or_else(|| store.inherit.as_ref().and_then(|m| m.get(key).cloned()))
}

fn apply_store(store: &EnvStore, override_existing: bool) -> usize {
    let mut count = 0;
    if let Some(inherit) = &store.inherit {
        for (k, v) in inherit {
            if override_existing || env::var(k).is_err() {
                env::set_var(k, v);
                count += 1;
            }
        }
    }
    for (k, v) in &store.vars {
        if override_existing || env::var(k).is_err() {
            env::set_var(k, v);
            count += 1;
        }
    }
    count
}

fn object_to_store_map(obj: &HashMap<String, ValueRef>, span: Span, name: &str) -> Result<HashMap<String, String>, RuntimeError> {
    let mut map = HashMap::new();
    for (k, v) in obj {
        match value_to_string(&*v.borrow()) {
            Some(s) => {
                map.insert(k.clone(), s);
            }
            None => {
                return Err(type_err(
                    span,
                    format!("{name}(): object values must be strings, ints, floats, or bools"),
                ));
            }
        }
    }
    Ok(map)
}

// ---------------------------------------------------------------------------
// Global process-env API
// ---------------------------------------------------------------------------

fn nenv_get(args: &[ValueRef], span: Span) -> NekoResult<ValueRef> {
    arity_range(args, 1, 2, "nenv_get", span)?;
    let key = string_arg(args, 0, "nenv_get", span)?;
    match env::var(&key) {
        Ok(v) => Ok(ok_string(v)),
        Err(_) => {
            if args.len() == 2 {
                Ok(Rc::clone(&args[1]))
            } else {
                Ok(ok_nil())
            }
        }
    }
}

fn nenv_set(args: &[ValueRef], span: Span) -> NekoResult<ValueRef> {
    arity(args, 2, "nenv_set", span)?;
    let key = string_arg(args, 0, "nenv_set", span)?;
    let value = string_arg(args, 1, "nenv_set", span)?;
    env::set_var(&key, &value);
    Ok(ok_nil())
}

fn nenv_unset(args: &[ValueRef], span: Span) -> NekoResult<ValueRef> {
    arity(args, 1, "nenv_unset", span)?;
    let key = string_arg(args, 0, "nenv_unset", span)?;
    env::remove_var(&key);
    Ok(ok_nil())
}

fn nenv_has(args: &[ValueRef], span: Span) -> NekoResult<ValueRef> {
    arity(args, 1, "nenv_has", span)?;
    let key = string_arg(args, 0, "nenv_has", span)?;
    Ok(ok_bool(env::var(&key).is_ok()))
}

fn nenv_all(_args: &[ValueRef], _span: Span) -> NekoResult<ValueRef> {
    let mut map = HashMap::new();
    for (k, v) in env::vars() {
        map.insert(k, Value::String(v).ref_cell());
    }
    Ok(Value::Object(map).ref_cell())
}

fn nenv_require(args: &[ValueRef], span: Span) -> NekoResult<ValueRef> {
    arity(args, 1, "nenv_require", span)?;
    let key = string_arg(args, 0, "nenv_require", span)?;
    match env::var(&key) {
        Ok(v) => Ok(ok_string(v)),
        Err(_) => Ok(nenv_not_found(span, &key)),
    }
}

fn nenv_get_int(args: &[ValueRef], span: Span) -> NekoResult<ValueRef> {
    arity_range(args, 1, 2, "nenv_get_int", span)?;
    let key = string_arg(args, 0, "nenv_get_int", span)?;
    match env::var(&key) {
        Ok(v) => match v.parse::<i64>() {
            Ok(n) => Ok(ok_int(n)),
            Err(_) => {
                if args.len() == 2 {
                    Ok(Rc::clone(&args[1]))
                } else {
                    Ok(nenv_invalid_value(
                        span,
                        format!("nenv_get_int(): {key} is not a valid integer: {v}"),
                    ))
                }
            }
        },
        Err(_) => {
            if args.len() == 2 {
                Ok(Rc::clone(&args[1]))
            } else {
                Ok(nenv_not_found(span, &key))
            }
        }
    }
}

fn nenv_get_bool(args: &[ValueRef], span: Span) -> NekoResult<ValueRef> {
    arity_range(args, 1, 2, "nenv_get_bool", span)?;
    let key = string_arg(args, 0, "nenv_get_bool", span)?;
    match env::var(&key) {
        Ok(v) => match parse_bool_str(&v) {
            Some(b) => Ok(ok_bool(b)),
            None => {
                if args.len() == 2 {
                    Ok(Rc::clone(&args[1]))
                } else {
                    Ok(nenv_invalid_value(
                        span,
                        format!("nenv_get_bool(): {key} is not a valid boolean: {v}"),
                    ))
                }
            }
        },
        Err(_) => {
            if args.len() == 2 {
                Ok(Rc::clone(&args[1]))
            } else {
                Ok(nenv_not_found(span, &key))
            }
        }
    }
}

fn nenv_get_float(args: &[ValueRef], span: Span) -> NekoResult<ValueRef> {
    arity_range(args, 1, 2, "nenv_get_float", span)?;
    let key = string_arg(args, 0, "nenv_get_float", span)?;
    match env::var(&key) {
        Ok(v) => match v.parse::<f64>() {
            Ok(n) => Ok(Value::Float(n).ref_cell()),
            Err(_) => {
                if args.len() == 2 {
                    Ok(Rc::clone(&args[1]))
                } else {
                    Ok(nenv_invalid_value(
                        span,
                        format!("nenv_get_float(): {key} is not a valid float: {v}"),
                    ))
                }
            }
        },
        Err(_) => {
            if args.len() == 2 {
                Ok(Rc::clone(&args[1]))
            } else {
                Ok(nenv_not_found(span, &key))
            }
        }
    }
}

fn nenv_load(args: &[ValueRef], span: Span) -> NekoResult<ValueRef> {
    arity_range(args, 0, 2, "nenv_load", span)?;
    let (path, opts_idx) = match args.len() {
        0 => (".env".to_string(), 1),
        1 => match &*args[0].borrow() {
            Value::Object(_) => (".env".to_string(), 0),
            _ => (string_arg(args, 0, "nenv_load", span)?, 1),
        },
        _ => (string_arg(args, 0, "nenv_load", span)?, 1),
    };
    let override_existing = load_opts(args, opts_idx);
    match parse_dotenv_file(Path::new(&path)) {
        Ok(pairs) => Ok(ok_int(apply_pairs(&pairs, override_existing) as i64)),
        Err(e) => Ok(nenv_error(span, e)),
    }
}

fn nenv_load_many(args: &[ValueRef], span: Span) -> NekoResult<ValueRef> {
    arity_range(args, 1, 2, "nenv_load_many", span)?;
    let paths_val = &*args[0].borrow();
    let paths: Vec<String> = match paths_val {
        Value::StringArray(arr) => arr.dense_vec(),
        Value::Array(items) => {
            let mut out = Vec::new();
            for item in items {
                match &*item.borrow() {
                    Value::String(s) => out.push(s.clone()),
                    other => {
                        return Err(type_err(
                            span,
                            format!(
                                "nenv_load_many() expects an array of strings, got {}",
                                other.type_name()
                            ),
                        ));
                    }
                }
            }
            out
        }
        other => {
            return Err(type_err(
                span,
                format!(
                    "nenv_load_many() expects an array of paths, got {}",
                    other.type_name()
                ),
            ));
        }
    };
    let override_existing = load_opts(args, 1);
    let mut total = 0i64;
    for path in paths {
        match parse_dotenv_file(Path::new(&path)) {
            Ok(pairs) => total += apply_pairs(&pairs, override_existing) as i64,
            Err(e) => return Ok(nenv_error(span, format!("{path}: {e}"))),
        }
    }
    Ok(ok_int(total))
}

fn nenv_load_defaults(args: &[ValueRef], span: Span) -> NekoResult<ValueRef> {
    arity_range(args, 0, 1, "nenv_load_defaults", span)?;
    let override_existing = load_opts(args, 0);
    let candidates = [".env", ".env.local"];
    let mut total = 0i64;
    for path in candidates {
        if Path::new(path).is_file() {
            match parse_dotenv_file(Path::new(path)) {
                Ok(pairs) => total += apply_pairs(&pairs, override_existing) as i64,
                Err(e) => return Ok(nenv_error(span, format!("{path}: {e}"))),
            }
        }
    }
    Ok(ok_int(total))
}

fn nenv_parse(args: &[ValueRef], span: Span) -> NekoResult<ValueRef> {
    arity(args, 1, "nenv_parse", span)?;
    let path = string_arg(args, 0, "nenv_parse", span)?;
    match parse_dotenv_file(Path::new(&path)) {
        Ok(pairs) => Ok(object_from_pairs(&pairs)),
        Err(e) => Ok(nenv_error(span, e)),
    }
}

fn nenv_parse_text(args: &[ValueRef], span: Span) -> NekoResult<ValueRef> {
    arity(args, 1, "nenv_parse_text", span)?;
    let text = string_arg(args, 0, "nenv_parse_text", span)?;
    match parse_dotenv_reader(Cursor::new(text.as_bytes())) {
        Ok(pairs) => Ok(object_from_pairs(&pairs)),
        Err(e) => Ok(nenv_error(span, e)),
    }
}

fn nenv_expand(args: &[ValueRef], span: Span) -> NekoResult<ValueRef> {
    arity(args, 1, "nenv_expand", span)?;
    let text = string_arg(args, 0, "nenv_expand", span)?;
    Ok(ok_string(expand_text(&text)))
}

fn nenv_find_up(args: &[ValueRef], span: Span) -> NekoResult<ValueRef> {
    arity_range(args, 0, 2, "nenv_find_up", span)?;
    let filename = if args.is_empty() {
        ".env".to_string()
    } else {
        string_arg(args, 0, "nenv_find_up", span)?
    };
    let start = if args.len() >= 2 {
        PathBuf::from(string_arg(args, 1, "nenv_find_up", span)?)
    } else {
        env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
    };
    let mut dir = start;
    loop {
        let candidate = dir.join(&filename);
        if candidate.is_file() {
            return Ok(ok_string(candidate.to_string_lossy()));
        }
        if !dir.pop() {
            return Ok(ok_nil());
        }
    }
}

fn validate_type(key: &str, expected: &str, value: &str) -> Option<String> {
    match expected {
        "int" => {
            if value.parse::<i64>().is_err() {
                Some(format!("{key}: expected int, got {value}"))
            } else {
                None
            }
        }
        "float" => {
            if value.parse::<f64>().is_err() {
                Some(format!("{key}: expected float, got {value}"))
            } else {
                None
            }
        }
        "bool" => {
            if parse_bool_str(value).is_none() {
                Some(format!("{key}: expected bool, got {value}"))
            } else {
                None
            }
        }
        "string" => None,
        other => Some(format!("{key}: unknown type {other}")),
    }
}

fn nenv_validate(args: &[ValueRef], span: Span) -> NekoResult<ValueRef> {
    arity(args, 1, "nenv_validate", span)?;
    let schema_val = &*args[0].borrow();
    let schema = match schema_val {
        Value::Object(map) => map,
        other => {
            return Err(type_err(
                span,
                format!(
                    "nenv_validate() expects an object schema, got {}",
                    other.type_name()
                ),
            ));
        }
    };
    let mut problems = Vec::new();

    if let Some(required_ref) = schema.get("required") {
        match &*required_ref.borrow() {
            Value::StringArray(arr) => {
                for key in arr.dense_vec() {
                    if env::var(&key).is_err() {
                        problems.push(format!("missing required variable: {key}"));
                    }
                }
            }
            Value::Array(items) => {
                for item in items {
                    if let Value::String(key) = &*item.borrow() {
                        if env::var(key).is_err() {
                            problems.push(format!("missing required variable: {key}"));
                        }
                    }
                }
            }
            other => {
                return Err(type_err(
                    span,
                    format!(
                        "nenv_validate(): schema.required must be an array, got {}",
                        other.type_name()
                    ),
                ));
            }
        }
    }

    if let Some(types_ref) = schema.get("types") {
        match &*types_ref.borrow() {
            Value::Object(types) => {
                for (key, expected_ref) in types {
                    let expected_type = match &*expected_ref.borrow() {
                        Value::String(s) => s.clone(),
                        other => {
                            return Err(type_err(
                                span,
                                format!(
                                    "nenv_validate(): type for {key} must be a string, got {}",
                                    other.type_name()
                                ),
                            ));
                        }
                    };
                    match env::var(key) {
                        Ok(value) => {
                            if let Some(msg) = validate_type(key, &expected_type, &value) {
                                problems.push(msg);
                            }
                        }
                        Err(_) => {}
                    }
                }
            }
            other => {
                return Err(type_err(
                    span,
                    format!(
                        "nenv_validate(): schema.types must be an object, got {}",
                        other.type_name()
                    ),
                ));
            }
        }
    }

    if problems.is_empty() {
        Ok(ok_nil())
    } else {
        Ok(nenv_error(span, problems.join("; ")))
    }
}

// ---------------------------------------------------------------------------
// Store handle API
// ---------------------------------------------------------------------------

fn nenv_open(args: &[ValueRef], span: Span) -> NekoResult<ValueRef> {
    arity_range(args, 0, 1, "nenv_open", span)?;
    let inherit = bool_field(optional_object_arg(args, 0).as_ref(), "inherit", false);
    Ok(ok_int(alloc_store(inherit)))
}

fn nenv_close(args: &[ValueRef], span: Span) -> NekoResult<ValueRef> {
    arity(args, 1, "nenv_close", span)?;
    let id = int_arg(args, 0, "nenv_close", span)?;
    if remove_store(id) {
        Ok(ok_nil())
    } else {
        Ok(error_value(
            codes::E1954_NENV_INVALID_HANDLE,
            "nenv_error",
            format!("nenv_close(): invalid or closed env store handle {id}"),
            span,
        ))
    }
}

fn store_err(span: Span, e: RuntimeError) -> ValueRef {
    error_value(codes::E1954_NENV_INVALID_HANDLE, "nenv_error", e.message(), span)
}

fn nenv_store_load(args: &[ValueRef], span: Span) -> NekoResult<ValueRef> {
    arity_range(args, 2, 3, "nenv_store_load", span)?;
    let id = int_arg(args, 0, "nenv_store_load", span)?;
    let path = string_arg(args, 1, "nenv_store_load", span)?;
    let override_existing = load_opts(args, 2);
    let pairs = match parse_dotenv_file(Path::new(&path)) {
        Ok(p) => p,
        Err(e) => return Ok(nenv_error(span, e)),
    };
    match with_store(id, "nenv_store_load", span, |store| {
        let mut count = 0i64;
        for (k, v) in pairs {
            if override_existing || !store.vars.contains_key(&k) {
                store.vars.insert(k, v);
                count += 1;
            }
        }
        count
    }) {
        Ok(count) => Ok(ok_int(count)),
        Err(e) => Ok(store_err(span, e)),
    }
}

fn nenv_store_get(args: &[ValueRef], span: Span) -> NekoResult<ValueRef> {
    arity_range(args, 2, 3, "nenv_store_get", span)?;
    let id = int_arg(args, 0, "nenv_store_get", span)?;
    let key = string_arg(args, 1, "nenv_store_get", span)?;
    let default = if args.len() == 3 {
        Some(Rc::clone(&args[2]))
    } else {
        None
    };
    match with_store(id, "nenv_store_get", span, |store| store_lookup(store, &key)) {
        Ok(Some(v)) => Ok(ok_string(v)),
        Ok(None) => {
            if let Some(d) = default {
                Ok(d)
            } else {
                Ok(ok_nil())
            }
        }
        Err(e) => Ok(store_err(span, e)),
    }
}

fn nenv_store_set(args: &[ValueRef], span: Span) -> NekoResult<ValueRef> {
    arity(args, 3, "nenv_store_set", span)?;
    let id = int_arg(args, 0, "nenv_store_set", span)?;
    let key = string_arg(args, 1, "nenv_store_set", span)?;
    let value = string_arg(args, 2, "nenv_store_set", span)?;
    match with_store(id, "nenv_store_set", span, |store| {
        store.vars.insert(key, value);
    }) {
        Ok(()) => Ok(ok_nil()),
        Err(e) => Ok(store_err(span, e)),
    }
}

fn nenv_store_unset(args: &[ValueRef], span: Span) -> NekoResult<ValueRef> {
    arity(args, 2, "nenv_store_unset", span)?;
    let id = int_arg(args, 0, "nenv_store_unset", span)?;
    let key = string_arg(args, 1, "nenv_store_unset", span)?;
    match with_store(id, "nenv_store_unset", span, |store| {
        store.vars.remove(&key);
    }) {
        Ok(()) => Ok(ok_nil()),
        Err(e) => Ok(store_err(span, e)),
    }
}

fn nenv_store_all(args: &[ValueRef], span: Span) -> NekoResult<ValueRef> {
    arity(args, 1, "nenv_store_all", span)?;
    let id = int_arg(args, 0, "nenv_store_all", span)?;
    match with_store(id, "nenv_store_all", span, |store| {
        let mut merged = HashMap::new();
        if let Some(inherit) = &store.inherit {
            for (k, v) in inherit {
                merged.insert(k.clone(), Value::String(v.clone()).ref_cell());
            }
        }
        for (k, v) in &store.vars {
            merged.insert(k.clone(), Value::String(v.clone()).ref_cell());
        }
        merged
    }) {
        Ok(map) => Ok(Value::Object(map).ref_cell()),
        Err(e) => Ok(store_err(span, e)),
    }
}

fn nenv_store_apply(args: &[ValueRef], span: Span) -> NekoResult<ValueRef> {
    arity_range(args, 1, 2, "nenv_store_apply", span)?;
    let id = int_arg(args, 0, "nenv_store_apply", span)?;
    let override_existing = load_opts(args, 1);
    match with_store(id, "nenv_store_apply", span, |store| apply_store(store, override_existing)) {
        Ok(count) => Ok(ok_int(count as i64)),
        Err(e) => Ok(store_err(span, e)),
    }
}

fn nenv_from_object(args: &[ValueRef], span: Span) -> NekoResult<ValueRef> {
    arity(args, 1, "nenv_from_object", span)?;
    let obj = match &*args[0].borrow() {
        Value::Object(map) => map.clone(),
        other => {
            return Err(type_err(
                span,
                format!(
                    "nenv_from_object() expects an object, got {}",
                    other.type_name()
                ),
            ));
        }
    };
    let vars = object_to_store_map(&obj, span, "nenv_from_object")?;
    let id = alloc_store(false);
    STORES.with(|stores| {
        if let Some(store) = stores.borrow_mut().get_mut(&id) {
            store.vars = vars;
        }
    });
    Ok(ok_int(id))
}

// ---------------------------------------------------------------------------
// Registration
// ---------------------------------------------------------------------------

fn all_builtins() -> Vec<(&'static str, NativeFn)> {
    vec![
        ("nenv_load", Rc::new(nenv_load)),
        ("nenv_load_many", Rc::new(nenv_load_many)),
        ("nenv_load_defaults", Rc::new(nenv_load_defaults)),
        ("nenv_parse", Rc::new(nenv_parse)),
        ("nenv_parse_text", Rc::new(nenv_parse_text)),
        ("nenv_get", Rc::new(nenv_get)),
        ("nenv_set", Rc::new(nenv_set)),
        ("nenv_unset", Rc::new(nenv_unset)),
        ("nenv_has", Rc::new(nenv_has)),
        ("nenv_all", Rc::new(nenv_all)),
        ("nenv_require", Rc::new(nenv_require)),
        ("nenv_get_int", Rc::new(nenv_get_int)),
        ("nenv_get_bool", Rc::new(nenv_get_bool)),
        ("nenv_get_float", Rc::new(nenv_get_float)),
        ("nenv_expand", Rc::new(nenv_expand)),
        ("nenv_find_up", Rc::new(nenv_find_up)),
        ("nenv_validate", Rc::new(nenv_validate)),
        ("nenv_open", Rc::new(nenv_open)),
        ("nenv_close", Rc::new(nenv_close)),
        ("nenv_store_load", Rc::new(nenv_store_load)),
        ("nenv_store_get", Rc::new(nenv_store_get)),
        ("nenv_store_set", Rc::new(nenv_store_set)),
        ("nenv_store_unset", Rc::new(nenv_store_unset)),
        ("nenv_store_all", Rc::new(nenv_store_all)),
        ("nenv_store_apply", Rc::new(nenv_store_apply)),
        ("nenv_from_object", Rc::new(nenv_from_object)),
    ]
}

pub fn namespace() -> Value {
    let mut map = HashMap::new();
    let bind = |map: &mut HashMap<String, ValueRef>, name: &str, f: NativeFn| {
        map.insert(name.to_string(), Value::NativeFunction(f).ref_cell());
    };

    bind(&mut map, "load", Rc::new(nenv_load));
    bind(&mut map, "load_many", Rc::new(nenv_load_many));
    bind(&mut map, "load_defaults", Rc::new(nenv_load_defaults));
    bind(&mut map, "parse", Rc::new(nenv_parse));
    bind(&mut map, "parse_text", Rc::new(nenv_parse_text));
    bind(&mut map, "get", Rc::new(nenv_get));
    bind(&mut map, "set", Rc::new(nenv_set));
    bind(&mut map, "unset", Rc::new(nenv_unset));
    bind(&mut map, "has", Rc::new(nenv_has));
    bind(&mut map, "all", Rc::new(nenv_all));
    bind(&mut map, "require", Rc::new(nenv_require));
    bind(&mut map, "get_int", Rc::new(nenv_get_int));
    bind(&mut map, "get_bool", Rc::new(nenv_get_bool));
    bind(&mut map, "get_float", Rc::new(nenv_get_float));
    bind(&mut map, "expand", Rc::new(nenv_expand));
    bind(&mut map, "find_up", Rc::new(nenv_find_up));
    bind(&mut map, "validate", Rc::new(nenv_validate));
    bind(&mut map, "open", Rc::new(nenv_open));
    bind(&mut map, "close", Rc::new(nenv_close));
    bind(&mut map, "store_load", Rc::new(nenv_store_load));
    bind(&mut map, "store_get", Rc::new(nenv_store_get));
    bind(&mut map, "store_set", Rc::new(nenv_store_set));
    bind(&mut map, "store_unset", Rc::new(nenv_store_unset));
    bind(&mut map, "store_all", Rc::new(nenv_store_all));
    bind(&mut map, "store_apply", Rc::new(nenv_store_apply));
    bind(&mut map, "from_object", Rc::new(nenv_from_object));

    Value::Object(map)
}

pub const MODULE_NAME: &str = "nenv";
pub const MODULE_PATHS: &[&str] = &["nenv", "std/nenv"];

pub fn builtins() -> Vec<(&'static str, NativeFn)> {
    all_builtins()
}

#[cfg(test)]
mod tests {
    use super::*;
    use neko_ast::Span;

    fn span() -> Span {
        Span::dummy()
    }

    #[test]
    fn parse_text_basic() {
        let text = "FOO=bar\n# comment\nexport BAZ=qux\n";
        let pairs = parse_dotenv_reader(Cursor::new(text.as_bytes())).unwrap();
        assert_eq!(pairs.len(), 2);
        assert_eq!(pairs[0], ("FOO".to_string(), "bar".to_string()));
        assert_eq!(pairs[1], ("BAZ".to_string(), "qux".to_string()));
    }

    #[test]
    fn expand_braced() {
        env::set_var("HOME", "/home/neko");
        assert_eq!(expand_text("${HOME}/data"), "/home/neko/data");
        env::remove_var("HOME");
    }

    #[test]
    fn get_with_default() {
        let args = [
            Value::String("NENV_TEST_MISSING_XYZ".into()).ref_cell(),
            Value::String("fallback".into()).ref_cell(),
        ];
        match &*nenv_get(&args, span()).unwrap().borrow() {
            Value::String(s) => assert_eq!(s, "fallback"),
            other => panic!("expected string, got {other:?}"),
        }
    }

    #[test]
    fn parse_bool_values() {
        assert_eq!(parse_bool_str("true"), Some(true));
        assert_eq!(parse_bool_str("NO"), Some(false));
        assert_eq!(parse_bool_str("maybe"), None);
    }
}
