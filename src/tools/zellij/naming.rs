/// Maximum length (in bytes) of a generated session name.
///
/// macOS limits Unix-domain socket paths (`sun_path`) to 104 bytes,
/// shared between the socket directory and the session name. With
/// `ZELLIJ_SOCKET_DIR=/tmp` (which `src/tools/zellij/run.rs` forces),
/// Zellij prepends `/tmp/zellij-<user>/contract_version_1/` (~45–55
/// cells depending on the username), leaving roughly 50 cells for the
/// session name. 50 keeps a safety margin even for long usernames
/// while remaining long enough for readable `repo-worktree` names.
const MAX_SESSION_NAME_LEN: usize = 50;

/// Build a sanitized Zellij session name from a repo key and a stable
/// worktree identity (the directory name under `wt/<repo>/`).
///
/// Use the worktree name — not the current Git branch — so that branch
/// renames don't break session lookup during park/finish.
///
/// Zellij itself accepts a wider set of characters than this output, but
/// we keep the sanitization conservative (ASCII alphanumeric, `_`, `-`)
/// so the same key is also safe to embed in filesystem paths (e.g.
/// `<tmp>/task-zellij-layouts/<session>.kdl`) and in VSCodium profile
/// directories.
#[must_use]
pub fn session_name(repo_key: &str, worktree_name: &str) -> String {
    let raw = format!("{repo_key}-{worktree_name}");
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

    if output.len() > MAX_SESSION_NAME_LEN {
        output.truncate(MAX_SESSION_NAME_LEN);
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
        fn truncates_at_max_session_name_length() {
            // The hard cap is dictated by macOS's 104-byte `sun_path`
            // limit. Pin the truncation length so a future relaxation
            // doesn't silently re-expose long names that Zellij will
            // reject with `session name must be less than 0 characters`.
            let long_branch = "a".repeat(100);
            let name = session_name("r", &long_branch);
            assert_eq!(name.len(), 50);
        }

        #[test]
        fn exactly_max_length_is_not_truncated() {
            // 24 + 1 separator + 25 = 50 chars total — at the cap,
            // must NOT be truncated.
            let key = "a".repeat(24);
            let branch = "b".repeat(25);
            let name = session_name(&key, &branch);
            assert_eq!(name.len(), 50);
        }

        #[test]
        fn one_over_max_length_is_truncated() {
            // 25 + 1 + 25 = 51 chars — exactly one over the cap, must
            // be truncated. Guards the boundary in both directions.
            let key = "a".repeat(25);
            let branch = "b".repeat(25);
            let name = session_name(&key, &branch);
            assert_eq!(name.len(), 50);
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
