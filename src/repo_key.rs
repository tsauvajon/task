#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResolveResult {
    Resolved(String),
    Ambiguous(Vec<String>),
}

pub fn normalize_repo_key(input: &str) -> String {
    let mut key = input.trim().to_string();

    if key.starts_with("ssh://") {
        key = key.trim_start_matches("ssh://").to_string();
    }
    if key.starts_with("https://") {
        key = key.trim_start_matches("https://").to_string();
    }
    if key.starts_with("http://") {
        key = key.trim_start_matches("http://").to_string();
    }

    if key.starts_with("git@") {
        key = key.trim_start_matches("git@").to_string();
        if let Some((left, right)) = key.split_once(':') {
            key = format!("{left}/{right}");
        }
    }

    while key.starts_with('/') {
        key.remove(0);
    }

    if let Some(stripped) = key.strip_suffix(".git") {
        return stripped.to_string();
    }

    key
}

pub fn resolve_repo_key(query: &str, available_keys: &[String]) -> ResolveResult {
    let normalized = normalize_repo_key(query);

    if available_keys.iter().any(|key| key == &normalized) {
        return ResolveResult::Resolved(normalized);
    }

    let mut matches = Vec::new();
    for key in available_keys {
        let base = key.rsplit('/').next().unwrap_or_default();
        if key == &normalized || base == normalized || key.ends_with(&format!("/{normalized}")) {
            matches.push(key.clone());
        }
    }
    matches.sort();
    matches.dedup();

    if matches.is_empty() {
        return ResolveResult::Resolved(normalized);
    }

    if matches.len() == 1 {
        return ResolveResult::Resolved(matches[0].clone());
    }

    ResolveResult::Ambiguous(matches)
}
