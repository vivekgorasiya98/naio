//! Native regular-expression standard library — fast matching via the `regex` crate.
//!
//! Registered as prefixed builtins (`re_match`, `re_search`, `re_compile`, ...).
//! Import with `import "re"` (or `import "std/re"`).
//!
//! Stateless functions compile the pattern on each call. For hot loops, use
//! `re_compile` / `re_*_h` handle APIs to reuse a compiled regex.

use crate::{NativeFn, NekoResult, RuntimeError, Value, ValueRef};
use neko_ast::Span;
use neko_errors::codes;
use regex::{Captures, Regex};
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;

fn type_err(span: Span, msg: impl Into<String>) -> RuntimeError {
    RuntimeError::TypeError {
        message: msg.into(),
        line: span.line,
        col: span.col,
    }
}

fn pattern_err(span: Span, msg: impl Into<String>) -> RuntimeError {
    RuntimeError::at(span, codes::E1301_RE_PATTERN, msg)
}

fn arity(args: &[ValueRef], n: usize, name: &str, span: Span) -> NekoResult<()> {
    if args.len() != n {
        return Err(RuntimeError::at(
            span,
            codes::E1300_RE_ARITY,
            format!("{name}() expects {n} argument(s), got {}", args.len()),
        ));
    }
    Ok(())
}

fn arity_range(args: &[ValueRef], min: usize, max: usize, name: &str, span: Span) -> NekoResult<()> {
    if args.len() < min || args.len() > max {
        return Err(RuntimeError::at(
            span,
            codes::E1300_RE_ARITY,
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

fn handle_arg(args: &[ValueRef], idx: usize, name: &str, span: Span) -> NekoResult<u64> {
    let id = int_arg(args, idx, name, span)?;
    if id <= 0 {
        return Err(type_err(
            span,
            format!("{name}() expects a positive regex handle as argument {}", idx + 1),
        ));
    }
    Ok(id as u64)
}

fn optional_flags(args: &[ValueRef], idx: usize) -> Option<String> {
    if args.len() <= idx {
        return None;
    }
    match &*args[idx].borrow() {
        Value::String(s) => Some(s.clone()),
        _ => None,
    }
}

fn apply_flags(pattern: &str, flags: Option<&str>) -> String {
    let Some(flags) = flags else {
        return pattern.to_string();
    };
    let mut prefix = String::new();
    if flags.contains('i') {
        prefix.push_str("(?i)");
    }
    if flags.contains('m') {
        prefix.push_str("(?m)");
    }
    if flags.contains('s') {
        prefix.push_str("(?s)");
    }
    if flags.contains('u') || flags.contains('U') {
        prefix.push_str("(?u)");
    }
    if prefix.is_empty() {
        pattern.to_string()
    } else {
        format!("{prefix}{pattern}")
    }
}

fn compile_pattern(pattern: &str, flags: Option<&str>, span: Span) -> NekoResult<Regex> {
    let full = apply_flags(pattern, flags);
    Regex::new(&full).map_err(|e| pattern_err(span, format!("invalid regex pattern: {e}")))
}

fn match_object(caps: &Captures<'_>) -> Value {
    let full = caps
        .get(0)
        .map(|m| m.as_str().to_string())
        .unwrap_or_default();
    let start = caps.get(0).map(|m| m.start() as i64).unwrap_or(-1);
    let end = caps.get(0).map(|m| m.end() as i64).unwrap_or(-1);
    let groups: Vec<ValueRef> = caps
        .iter()
        .map(|m| {
            Value::String(m.map(|x| x.as_str().to_string()).unwrap_or_default()).ref_cell()
        })
        .collect();

    let mut map = HashMap::new();
    map.insert("full".into(), Value::String(full).ref_cell());
    map.insert("start".into(), Value::Int(start).ref_cell());
    map.insert("end".into(), Value::Int(end).ref_cell());
    map.insert("groups".into(), Value::Array(groups).ref_cell());
    Value::Object(map)
}

fn simple_match_object(text: &str, start: usize, end: usize) -> Value {
    let full = text[start..end].to_string();
    let mut map = HashMap::new();
    map.insert("full".into(), Value::String(full.clone()).ref_cell());
    map.insert("start".into(), Value::Int(start as i64).ref_cell());
    map.insert("end".into(), Value::Int(end as i64).ref_cell());
    map.insert(
        "groups".into(),
        Value::Array(vec![Value::String(full).ref_cell()]).ref_cell(),
    );
    Value::Object(map)
}

fn is_full_match(re: &Regex, text: &str) -> bool {
    re.find(text)
        .map(|m| m.start() == 0 && m.end() == text.len())
        .unwrap_or(false)
}

thread_local! {
    static RE_HANDLES: RefCell<HashMap<u64, Regex>> = RefCell::new(HashMap::new());
    static NEXT_RE_HANDLE: Cell<u64> = const { Cell::new(1) };
}

fn alloc_regex(re: Regex) -> u64 {
    let id = NEXT_RE_HANDLE.with(|n| {
        let id = n.get();
        n.set(id.saturating_add(1));
        id
    });
    RE_HANDLES.with(|map| map.borrow_mut().insert(id, re));
    id
}

fn with_regex(
    id: u64,
    name: &str,
    span: Span,
    f: impl FnOnce(&Regex) -> NekoResult<ValueRef>,
) -> NekoResult<ValueRef> {
    RE_HANDLES.with(|map| {
        let guard = map.borrow();
        let re = guard.get(&id).ok_or_else(|| {
            RuntimeError::at(
                span,
                codes::E1302_RE_INVALID_HANDLE,
                format!("{name}(): invalid or closed regex handle {id}"),
            )
        })?;
        f(re)
    })
}

fn remove_regex(id: u64) -> bool {
    RE_HANDLES.with(|map| map.borrow_mut().remove(&id).is_some())
}

// ---------------------------------------------------------------------------
// Stateless builtins
// ---------------------------------------------------------------------------

fn re_valid(args: &[ValueRef], span: Span) -> NekoResult<ValueRef> {
    arity_range(args, 1, 2, "re_valid", span)?;
    let pattern = string_arg(args, 0, "re_valid", span)?;
    let flags = optional_flags(args, 1);
    let full = apply_flags(&pattern, flags.as_deref());
    Ok(Value::Bool(Regex::new(&full).is_ok()).ref_cell())
}

fn re_escape(args: &[ValueRef], span: Span) -> NekoResult<ValueRef> {
    arity(args, 1, "re_escape", span)?;
    let text = string_arg(args, 0, "re_escape", span)?;
    Ok(Value::String(regex::escape(&text)).ref_cell())
}

fn re_compile(args: &[ValueRef], span: Span) -> NekoResult<ValueRef> {
    arity_range(args, 1, 2, "re_compile", span)?;
    let pattern = string_arg(args, 0, "re_compile", span)?;
    let flags = optional_flags(args, 1);
    let re = compile_pattern(&pattern, flags.as_deref(), span)?;
    Ok(Value::Int(alloc_regex(re) as i64).ref_cell())
}

fn re_close(args: &[ValueRef], span: Span) -> NekoResult<ValueRef> {
    arity(args, 1, "re_close", span)?;
    let id = handle_arg(args, 0, "re_close", span)?;
    Ok(Value::Bool(remove_regex(id)).ref_cell())
}

fn re_test(args: &[ValueRef], span: Span) -> NekoResult<ValueRef> {
    arity_range(args, 2, 3, "re_test", span)?;
    let pattern = string_arg(args, 0, "re_test", span)?;
    let text = string_arg(args, 1, "re_test", span)?;
    let flags = optional_flags(args, 2);
    let re = compile_pattern(&pattern, flags.as_deref(), span)?;
    Ok(Value::Bool(re.is_match(&text)).ref_cell())
}

fn re_match(args: &[ValueRef], span: Span) -> NekoResult<ValueRef> {
    arity_range(args, 2, 3, "re_match", span)?;
    let pattern = string_arg(args, 0, "re_match", span)?;
    let text = string_arg(args, 1, "re_match", span)?;
    let flags = optional_flags(args, 2);
    let re = compile_pattern(&pattern, flags.as_deref(), span)?;
    Ok(Value::Bool(is_full_match(&re, &text)).ref_cell())
}

fn re_search(args: &[ValueRef], span: Span) -> NekoResult<ValueRef> {
    arity_range(args, 2, 3, "re_search", span)?;
    let pattern = string_arg(args, 0, "re_search", span)?;
    let text = string_arg(args, 1, "re_search", span)?;
    let flags = optional_flags(args, 2);
    let re = compile_pattern(&pattern, flags.as_deref(), span)?;
    match re.captures(&text) {
        Some(caps) => Ok(match_object(&caps).ref_cell()),
        None => Ok(Value::Nil.ref_cell()),
    }
}

fn re_find_all(args: &[ValueRef], span: Span) -> NekoResult<ValueRef> {
    arity_range(args, 2, 3, "re_find_all", span)?;
    let pattern = string_arg(args, 0, "re_find_all", span)?;
    let text = string_arg(args, 1, "re_find_all", span)?;
    let flags = optional_flags(args, 2);
    let re = compile_pattern(&pattern, flags.as_deref(), span)?;
    let out: Vec<ValueRef> = if re.capture_names().count() > 1 {
        re.captures_iter(&text)
            .map(|caps| match_object(&caps).ref_cell())
            .collect()
    } else {
        re.find_iter(&text)
            .map(|m| simple_match_object(&text, m.start(), m.end()).ref_cell())
            .collect()
    };
    Ok(Value::Array(out).ref_cell())
}

fn re_find_all_strings(args: &[ValueRef], span: Span) -> NekoResult<ValueRef> {
    arity_range(args, 2, 3, "re_find_all_strings", span)?;
    let pattern = string_arg(args, 0, "re_find_all_strings", span)?;
    let text = string_arg(args, 1, "re_find_all_strings", span)?;
    let flags = optional_flags(args, 2);
    let re = compile_pattern(&pattern, flags.as_deref(), span)?;
    let out: Vec<ValueRef> = re
        .find_iter(&text)
        .map(|m| Value::String(m.as_str().to_string()).ref_cell())
        .collect();
    Ok(Value::Array(out).ref_cell())
}

fn re_replace(args: &[ValueRef], span: Span) -> NekoResult<ValueRef> {
    arity_range(args, 3, 4, "re_replace", span)?;
    let pattern = string_arg(args, 0, "re_replace", span)?;
    let text = string_arg(args, 1, "re_replace", span)?;
    let replacement = string_arg(args, 2, "re_replace", span)?;
    let flags = optional_flags(args, 3);
    let re = compile_pattern(&pattern, flags.as_deref(), span)?;
    Ok(Value::String(re.replace_all(&text, replacement.as_str()).into_owned()).ref_cell())
}

fn re_replace_n(args: &[ValueRef], span: Span) -> NekoResult<ValueRef> {
    arity_range(args, 4, 5, "re_replace_n", span)?;
    let pattern = string_arg(args, 0, "re_replace_n", span)?;
    let text = string_arg(args, 1, "re_replace_n", span)?;
    let replacement = string_arg(args, 2, "re_replace_n", span)?;
    let count = int_arg(args, 3, "re_replace_n", span)?;
    if count < 0 {
        return Err(type_err(
            span,
            "re_replace_n() expects a non-negative int as argument 4",
        ));
    }
    let flags = optional_flags(args, 4);
    let re = compile_pattern(&pattern, flags.as_deref(), span)?;
    Ok(
        Value::String(
            re.replacen(&text, count as usize, replacement.as_str())
                .into_owned(),
        )
        .ref_cell(),
    )
}

fn re_split(args: &[ValueRef], span: Span) -> NekoResult<ValueRef> {
    arity_range(args, 2, 3, "re_split", span)?;
    let pattern = string_arg(args, 0, "re_split", span)?;
    let text = string_arg(args, 1, "re_split", span)?;
    let flags = optional_flags(args, 2);
    let re = compile_pattern(&pattern, flags.as_deref(), span)?;
    let out: Vec<ValueRef> = re
        .split(&text)
        .map(|s| Value::String(s.to_string()).ref_cell())
        .collect();
    Ok(Value::Array(out).ref_cell())
}

fn re_count(args: &[ValueRef], span: Span) -> NekoResult<ValueRef> {
    arity_range(args, 2, 3, "re_count", span)?;
    let pattern = string_arg(args, 0, "re_count", span)?;
    let text = string_arg(args, 1, "re_count", span)?;
    let flags = optional_flags(args, 2);
    let re = compile_pattern(&pattern, flags.as_deref(), span)?;
    Ok(Value::Int(re.find_iter(&text).count() as i64).ref_cell())
}

// ---------------------------------------------------------------------------
// Handle-based builtins (reuse compiled regex)
// ---------------------------------------------------------------------------

fn re_test_h(args: &[ValueRef], span: Span) -> NekoResult<ValueRef> {
    arity(args, 2, "re_test_h", span)?;
    let id = handle_arg(args, 0, "re_test_h", span)?;
    let text = string_arg(args, 1, "re_test_h", span)?;
    with_regex(id, "re_test_h", span, |re| Ok(Value::Bool(re.is_match(&text)).ref_cell()))
}

fn re_match_h(args: &[ValueRef], span: Span) -> NekoResult<ValueRef> {
    arity(args, 2, "re_match_h", span)?;
    let id = handle_arg(args, 0, "re_match_h", span)?;
    let text = string_arg(args, 1, "re_match_h", span)?;
    with_regex(id, "re_match_h", span, |re| {
        Ok(Value::Bool(is_full_match(re, &text)).ref_cell())
    })
}

fn re_search_h(args: &[ValueRef], span: Span) -> NekoResult<ValueRef> {
    arity(args, 2, "re_search_h", span)?;
    let id = handle_arg(args, 0, "re_search_h", span)?;
    let text = string_arg(args, 1, "re_search_h", span)?;
    with_regex(id, "re_search_h", span, |re| {
        Ok(match re.captures(&text) {
            Some(caps) => match_object(&caps).ref_cell(),
            None => Value::Nil.ref_cell(),
        })
    })
}

fn re_find_all_h(args: &[ValueRef], span: Span) -> NekoResult<ValueRef> {
    arity(args, 2, "re_find_all_h", span)?;
    let id = handle_arg(args, 0, "re_find_all_h", span)?;
    let text = string_arg(args, 1, "re_find_all_h", span)?;
    with_regex(id, "re_find_all_h", span, |re| {
        let out: Vec<ValueRef> = if re.capture_names().count() > 1 {
            re.captures_iter(&text)
                .map(|caps| match_object(&caps).ref_cell())
                .collect()
        } else {
            re.find_iter(&text)
                .map(|m| simple_match_object(&text, m.start(), m.end()).ref_cell())
                .collect()
        };
        Ok(Value::Array(out).ref_cell())
    })
}

fn re_find_all_strings_h(args: &[ValueRef], span: Span) -> NekoResult<ValueRef> {
    arity(args, 2, "re_find_all_strings_h", span)?;
    let id = handle_arg(args, 0, "re_find_all_strings_h", span)?;
    let text = string_arg(args, 1, "re_find_all_strings_h", span)?;
    with_regex(id, "re_find_all_strings_h", span, |re| {
        let out: Vec<ValueRef> = re
            .find_iter(&text)
            .map(|m| Value::String(m.as_str().to_string()).ref_cell())
            .collect();
        Ok(Value::Array(out).ref_cell())
    })
}

fn re_replace_h(args: &[ValueRef], span: Span) -> NekoResult<ValueRef> {
    arity(args, 3, "re_replace_h", span)?;
    let id = handle_arg(args, 0, "re_replace_h", span)?;
    let text = string_arg(args, 1, "re_replace_h", span)?;
    let replacement = string_arg(args, 2, "re_replace_h", span)?;
    with_regex(id, "re_replace_h", span, |re| {
        Ok(
            Value::String(re.replace_all(&text, replacement.as_str()).into_owned())
                .ref_cell(),
        )
    })
}

fn re_replace_n_h(args: &[ValueRef], span: Span) -> NekoResult<ValueRef> {
    arity(args, 4, "re_replace_n_h", span)?;
    let id = handle_arg(args, 0, "re_replace_n_h", span)?;
    let text = string_arg(args, 1, "re_replace_n_h", span)?;
    let replacement = string_arg(args, 2, "re_replace_n_h", span)?;
    let count = int_arg(args, 3, "re_replace_n_h", span)?;
    if count < 0 {
        return Err(type_err(
            span,
            "re_replace_n_h() expects a non-negative int as argument 4",
        ));
    }
    with_regex(id, "re_replace_n_h", span, |re| {
        Ok(
            Value::String(
                re.replacen(&text, count as usize, replacement.as_str())
                    .into_owned(),
            )
            .ref_cell(),
        )
    })
}

fn re_split_h(args: &[ValueRef], span: Span) -> NekoResult<ValueRef> {
    arity(args, 2, "re_split_h", span)?;
    let id = handle_arg(args, 0, "re_split_h", span)?;
    let text = string_arg(args, 1, "re_split_h", span)?;
    with_regex(id, "re_split_h", span, |re| {
        let out: Vec<ValueRef> = re
            .split(&text)
            .map(|s| Value::String(s.to_string()).ref_cell())
            .collect();
        Ok(Value::Array(out).ref_cell())
    })
}

fn re_count_h(args: &[ValueRef], span: Span) -> NekoResult<ValueRef> {
    arity(args, 2, "re_count_h", span)?;
    let id = handle_arg(args, 0, "re_count_h", span)?;
    let text = string_arg(args, 1, "re_count_h", span)?;
    with_regex(id, "re_count_h", span, |re| {
        Ok(Value::Int(re.find_iter(&text).count() as i64).ref_cell())
    })
}

/// All regex builtins in registration order.
pub fn builtins() -> Vec<(&'static str, NativeFn)> {
    vec![
        ("re_valid", Rc::new(re_valid)),
        ("re_escape", Rc::new(re_escape)),
        ("re_compile", Rc::new(re_compile)),
        ("re_close", Rc::new(re_close)),
        ("re_test", Rc::new(re_test)),
        ("re_match", Rc::new(re_match)),
        ("re_search", Rc::new(re_search)),
        ("re_find_all", Rc::new(re_find_all)),
        ("re_find_all_strings", Rc::new(re_find_all_strings)),
        ("re_replace", Rc::new(re_replace)),
        ("re_replace_n", Rc::new(re_replace_n)),
        ("re_split", Rc::new(re_split)),
        ("re_count", Rc::new(re_count)),
        ("re_test_h", Rc::new(re_test_h)),
        ("re_match_h", Rc::new(re_match_h)),
        ("re_search_h", Rc::new(re_search_h)),
        ("re_find_all_h", Rc::new(re_find_all_h)),
        ("re_find_all_strings_h", Rc::new(re_find_all_strings_h)),
        ("re_replace_h", Rc::new(re_replace_h)),
        ("re_replace_n_h", Rc::new(re_replace_n_h)),
        ("re_split_h", Rc::new(re_split_h)),
        ("re_count_h", Rc::new(re_count_h)),
    ]
}

/// Short-name regex module object for `re.match`, `re.search`, etc.
pub fn namespace() -> Value {
    let mut map = HashMap::new();
    let bind = |map: &mut HashMap<String, ValueRef>, name: &str, f: NativeFn| {
        map.insert(name.to_string(), Value::NativeFunction(f).ref_cell());
    };
    bind(&mut map, "valid", Rc::new(re_valid));
    bind(&mut map, "escape", Rc::new(re_escape));
    bind(&mut map, "compile", Rc::new(re_compile));
    bind(&mut map, "close", Rc::new(re_close));
    bind(&mut map, "test", Rc::new(re_test));
    bind(&mut map, "match", Rc::new(re_match));
    bind(&mut map, "search", Rc::new(re_search));
    bind(&mut map, "find_all", Rc::new(re_find_all));
    bind(&mut map, "find_all_strings", Rc::new(re_find_all_strings));
    bind(&mut map, "replace", Rc::new(re_replace));
    bind(&mut map, "replace_n", Rc::new(re_replace_n));
    bind(&mut map, "split", Rc::new(re_split));
    bind(&mut map, "count", Rc::new(re_count));
    bind(&mut map, "test_h", Rc::new(re_test_h));
    bind(&mut map, "match_h", Rc::new(re_match_h));
    bind(&mut map, "search_h", Rc::new(re_search_h));
    bind(&mut map, "find_all_h", Rc::new(re_find_all_h));
    bind(&mut map, "find_all_strings_h", Rc::new(re_find_all_strings_h));
    bind(&mut map, "replace_h", Rc::new(re_replace_h));
    bind(&mut map, "replace_n_h", Rc::new(re_replace_n_h));
    bind(&mut map, "split_h", Rc::new(re_split_h));
    bind(&mut map, "count_h", Rc::new(re_count_h));
    Value::Object(map)
}

/// Export name used when `import "re"` (or `import "std/re"`) is loaded.
pub const MODULE_NAME: &str = "re";

/// Paths that resolve to this native module.
pub const MODULE_PATHS: &[&str] = &["re", "std/re"];

#[cfg(test)]
mod tests {
    use super::*;
    use neko_ast::Span;

    fn span() -> Span {
        Span::dummy()
    }

    fn compile(pattern: &str) -> Regex {
        Regex::new(pattern).unwrap()
    }

    #[test]
    fn full_match_entire_string() {
        let re = compile(r"\d+");
        assert!(is_full_match(&re, "42"));
        assert!(!is_full_match(&re, "x42"));
    }

    #[test]
    fn flags_case_insensitive() {
        let full = apply_flags("hello", Some("i"));
        let re = Regex::new(&full).unwrap();
        assert!(re.is_match("HELLO"));
    }

    #[test]
    fn match_object_groups() {
        let re = Regex::new(r"(\w+)@(\w+)").unwrap();
        let caps = re.captures("alice@example").unwrap();
        let obj = match_object(&caps);
        match &obj {
            Value::Object(map) => {
                assert_eq!(map["full"].borrow().to_string(), "alice@example");
                let groups_val = map["groups"].borrow().clone();
                let groups = match &groups_val {
                    Value::Array(g) => g,
                    _ => panic!("expected groups array"),
                };
                assert_eq!(groups.len(), 3);
            }
            _ => panic!("expected object"),
        }
    }
}
