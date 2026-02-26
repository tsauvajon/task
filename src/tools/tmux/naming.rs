pub fn session_name(repo_key: &str, branch: &str) -> String {
    let raw = format!("{repo_key}-{branch}");
    let mut output = String::with_capacity(raw.len());

    for ch in raw.chars() {
        let mapped = match ch {
            '/' | ':' | '.' => '_',
            _ => ch,
        };

        if mapped.is_ascii_alphanumeric() || mapped == '_' || mapped == '-' {
            output.push(mapped);
        }
    }

    if output.len() > 80 {
        output.truncate(80);
    }

    if output.is_empty() {
        return "devtask".to_string();
    }

    output
}

#[cfg(test)]
mod tests {
    use super::session_name;

    #[test]
    fn session_name_is_sanitized() {
        assert_eq!(
            session_name("github.com/tsauvajon/goto", "feat/test.1"),
            "github_com_tsauvajon_goto-feat_test_1"
        );
    }

    #[test]
    fn session_name_truncates_at_80_chars() {
        // Create a very long branch name so the combined output exceeds 80 chars.
        let long_branch = "a".repeat(100);
        let name = session_name("r", &long_branch);
        assert_eq!(name.len(), 80);
    }

    #[test]
    fn session_name_strips_non_alphanumeric_except_hyphen_and_underscore() {
        // "@" and "$" are not alphanumeric and are not mapped by the match arm,
        // so they are dropped. Only the literal '-' separator survives.
        let name = session_name("@@@", "$$$");
        assert_eq!(name, "-");
    }

    #[test]
    fn session_name_preserves_hyphens_and_underscores() {
        let name = session_name("host_com", "feat-branch");
        assert_eq!(name, "host_com-feat-branch");
    }

    #[test]
    fn session_name_strips_colons() {
        // Colons are mapped to underscores, then kept.
        let name = session_name("host:8080", "branch");
        assert_eq!(name, "host_8080-branch");
    }
}
