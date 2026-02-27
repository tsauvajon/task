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

    mod session_name {
        use super::*;

        #[test]
        fn is_sanitized() {
            assert_eq!(
                session_name("github.com/tsauvajon/goto", "feat/test.1"),
                "github_com_tsauvajon_goto-feat_test_1"
            );
        }

        #[test]
        fn truncates_at_80_chars() {
            let long_branch = "a".repeat(100);
            let name = session_name("r", &long_branch);
            assert_eq!(name.len(), 80);
        }

        #[test]
        fn exactly_80_chars_is_not_truncated() {
            // 39 + 1 separator + 40 = 80 chars total — must NOT be truncated.
            let key = "a".repeat(39);
            let branch = "b".repeat(40);
            let name = session_name(&key, &branch);
            assert_eq!(name.len(), 80);
        }

        #[test]
        fn strips_non_alphanumeric_except_hyphen_and_underscore() {
            // "@" and "$" are dropped; only the '-' separator survives.
            let name = session_name("@@@", "$$$");
            assert_eq!(name, "-");
        }

        #[test]
        fn preserves_hyphens_and_underscores() {
            let name = session_name("host_com", "feat-branch");
            assert_eq!(name, "host_com-feat-branch");
        }

        #[test]
        fn colons_become_underscores() {
            let name = session_name("host:8080", "branch");
            assert_eq!(name, "host_8080-branch");
        }

        #[test]
        fn dots_become_underscores() {
            let name = session_name("github.com", "v1.2");
            assert_eq!(name, "github_com-v1_2");
        }

        #[test]
        fn empty_inputs_produce_just_separator() {
            let name = session_name("", "");
            // The raw string is just "-" (the separator).
            assert_eq!(name, "-");
        }

        #[test]
        fn non_ascii_chars_are_dropped() {
            let name = session_name("§§§", "¶¶¶");
            // All non-ASCII chars are dropped; only the '-' separator remains.
            assert_eq!(name, "-");
        }
    }
}
