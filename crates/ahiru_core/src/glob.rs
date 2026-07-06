/// Simple glob matcher: `*` matches any suffix segment.
pub fn glob_match(pattern: &str, path: &str) -> bool {
    if pattern == "*" {
        return true;
    }
    if !pattern.contains('*') {
        return pattern == path;
    }
    if let Some(prefix) = pattern.strip_suffix('*') {
        return path.starts_with(prefix);
    }
    if let Some(suffix) = pattern.strip_prefix('*') {
        return path.ends_with(suffix);
    }
    let parts: Vec<&str> = pattern.split('*').collect();
    let mut pos = 0;
    for (i, part) in parts.iter().enumerate() {
        if part.is_empty() {
            continue;
        }
        if let Some(idx) = path[pos..].find(part) {
            if i == 0 && idx != 0 {
                return false;
            }
            pos += idx + part.len();
        } else {
            return false;
        }
    }
    true
}

pub fn path_matches_scopes(path: &str, only: &[String], except: &[String]) -> bool {
    if !except.is_empty() && except.iter().any(|p| glob_match(p, path)) {
        return false;
    }
    if only.is_empty() {
        return true;
    }
    only.iter().any(|p| glob_match(p, path))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn glob_prefix() {
        assert!(glob_match("/admin/*", "/admin/users"));
        assert!(!glob_match("/admin/*", "/api/users"));
    }
}
