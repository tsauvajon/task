use crate::git::parsing::normalize_repo_key;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResolveResult {
    Resolved(String),
    Ambiguous(Vec<String>),
}

pub fn resolve_repo_query(query: &str, available_keys: &[String]) -> ResolveResult {
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

#[cfg(test)]
mod tests {
    use super::{ResolveResult, resolve_repo_query};

    #[test]
    fn resolve_repo_query_by_short_name_when_unique() {
        let keys = vec![
            "github.com/tsauvajon/goto".to_string(),
            "github.com/tsauvajon/task".to_string(),
        ];

        let resolved = resolve_repo_query("goto", &keys);
        assert_eq!(
            resolved,
            ResolveResult::Resolved("github.com/tsauvajon/goto".to_string())
        );
    }

    #[test]
    fn resolve_repo_query_reports_ambiguity() {
        let keys = vec![
            "github.com/tsauvajon/goto".to_string(),
            "github.com/example/goto".to_string(),
        ];

        let resolved = resolve_repo_query("goto", &keys);
        assert_eq!(
            resolved,
            ResolveResult::Ambiguous(vec![
                "github.com/example/goto".to_string(),
                "github.com/tsauvajon/goto".to_string(),
            ])
        );
    }
}
