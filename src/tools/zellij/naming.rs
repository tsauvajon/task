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
const HASH_SUFFIX_LEN: usize = 16;
const HASHED_STEM_LEN: usize = MAX_SESSION_NAME_LEN - HASH_SUFFIX_LEN - 1;
const FNV_1A_OFFSET_BASIS: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_1A_PRIME: u64 = 0x0000_0100_0000_01b3;

/// Build a sanitized Zellij session name from a repo key and a stable
/// worktree identity (the directory name under `wt/<repo>/`).
///
/// Use the worktree name — not the current Git branch — so that branch
/// renames don't break session lookup during park/finish.
///
/// Zellij itself accepts a wider set of characters than this output, but
/// we keep the sanitization conservative (ASCII alphanumeric, `_`, `-`)
/// so the same key is also safe to embed in filesystem paths (e.g.
/// `<tmp>/task-zellij-layouts/<session>.kdl`) and in `VSCodium` profile
/// directories.
///
/// Short sanitized names at or under the cap are preserved byte-for-byte
/// to avoid re-keying existing short normalization cases; this fix targets
/// truncation collisions only.
///
/// Names over the cap become `<readable-stem>-<64-bit-fnv1a-hex>`. This
/// may orphan old Zellij sessions, `VSCodium` profile directories, and temp
/// layout files created by the previous ambiguous truncation scheme;
/// intentionally do not fall back to those colliding names.
#[must_use]
pub fn session_name(repo_key: &str, worktree_name: &str) -> String {
    let raw = format!("{repo_key}-{worktree_name}");
    let output = sanitize_session_name(&raw);

    if output.len() <= MAX_SESSION_NAME_LEN {
        return output;
    }

    let hash = identity_hash(repo_key, worktree_name);
    let stem = hashed_stem(output);
    format!("{stem}-{hash:0HASH_SUFFIX_LEN$x}")
}

fn sanitize_session_name(raw: &str) -> String {
    let mut output = String::with_capacity(raw.len());

    for ch in raw.chars() {
        let mapped = match ch {
            '/' | ':' | '.' => '_',
            _ => ch,
        };

        if is_session_name_char(mapped) {
            output.push(mapped);
        }
    }

    output
}

const fn is_session_name_char(ch: char) -> bool {
    if ch.is_ascii_alphanumeric() {
        return true;
    }
    matches!(ch, '_' | '-')
}

fn hashed_stem(mut sanitized: String) -> String {
    debug_assert!(
        sanitized.is_ascii(),
        "session names are sanitized to ASCII before byte truncation"
    );
    // `String::truncate` takes a byte index; sanitization keeps every
    // surviving character ASCII so `HASHED_STEM_LEN` is a char boundary.
    sanitized.truncate(HASHED_STEM_LEN);
    sanitized
}

fn identity_hash(repo_key: &str, worktree_name: &str) -> u64 {
    fnv_1a(
        repo_key
            .bytes()
            .chain(std::iter::once(0))
            .chain(worktree_name.bytes()),
    )
}

fn fnv_1a(bytes: impl IntoIterator<Item = u8>) -> u64 {
    let mut hash = FNV_1A_OFFSET_BASIS;
    for byte in bytes {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(FNV_1A_PRIME);
    }
    hash
}

#[cfg(test)]
mod tests {
    use super::{
        HASH_SUFFIX_LEN, HASHED_STEM_LEN, MAX_SESSION_NAME_LEN, fnv_1a, identity_hash,
        sanitize_session_name, session_name,
    };

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
        fn hashes_long_names_at_max_session_name_length() {
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
        fn one_over_max_length_is_hashed() {
            let key = "a".repeat(25);
            let branch = "b".repeat(25);
            let name = session_name(&key, &branch);
            assert_eq!(name.len(), 50);
            assert_eq!(hash_suffix(&name).map(str::len), Some(HASH_SUFFIX_LEN));
        }

        #[test]
        fn preserves_short_name_compatibility() {
            let name = session_name("github.com/acme/tool", "feat/test.1");
            assert_eq!(name, "github_com_acme_tool-feat_test_1");
        }

        #[test]
        fn long_colliding_prefixes_produce_distinct_names() {
            let common_prefix = "same-readable-prefix-with-identical-start";
            let first = session_name(
                "github.com/acme/tool",
                &format!("{common_prefix}-first-{}", "a".repeat(40)),
            );
            let second = session_name(
                "github.com/acme/tool",
                &format!("{common_prefix}-second-{}", "b".repeat(40)),
            );

            assert_ne!(first, second);
            assert_eq!(first.len(), 50);
            assert_eq!(second.len(), 50);
        }

        #[test]
        fn long_names_stay_within_max_session_name_length() {
            let name = session_name(
                "github.com/acme/very-long-repository-name",
                &"very-long-worktree-name".repeat(8),
            );

            assert!(name.len() <= 50);
        }

        #[test]
        fn long_names_include_hash_suffix() {
            let name = session_name(
                "github.com/acme/tool",
                "same-readable-prefix-with-identical-start-first-aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            );
            let suffix = hash_suffix(&name);

            assert_eq!(suffix.map(str::len), Some(HASH_SUFFIX_LEN));
            assert!(suffix.is_some_and(is_lowercase_hex));
        }

        #[test]
        fn long_name_differs_from_old_truncation() {
            let repo_key = "github.com/acme/tool";
            let worktree_name = "same-readable-prefix-with-identical-start-first-aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
            let mut old_name = sanitize_session_name(&format!("{repo_key}-{worktree_name}"));
            old_name.truncate(MAX_SESSION_NAME_LEN);

            assert_ne!(session_name(repo_key, worktree_name), old_name);
        }

        #[test]
        fn long_name_stem_uses_reserved_stem_length() {
            let name = session_name(
                "github.com/acme/tool",
                "same-readable-prefix-with-identical-start-first-aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            );

            assert_eq!(
                name.rsplit_once('-').map(|(stem, _)| stem.len()),
                Some(HASHED_STEM_LEN)
            );
        }

        #[test]
        fn long_name_output_is_stable() {
            let name = session_name(
                "github.com/acme/tool",
                "same-readable-prefix-with-identical-start-first-aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            );

            assert_eq!(name, "github_com_acme_tool-same-readabl-d5fe6112b3285c7b");
        }

        #[test]
        fn nul_hash_separator_disambiguates_repo_and_worktree_boundary() {
            let left_repo = "ab";
            let left_worktree = "c";
            let right_repo = "a";
            let right_worktree = "bc";

            assert_eq!(
                format!("{left_repo}{left_worktree}"),
                format!("{right_repo}{right_worktree}")
            );
            assert_eq!(
                fnv_1a(identity_bytes_without_separator(left_repo, left_worktree)),
                fnv_1a(identity_bytes_without_separator(right_repo, right_worktree))
            );
            assert_ne!(
                identity_hash(left_repo, left_worktree),
                identity_hash(right_repo, right_worktree)
            );
        }

        #[test]
        fn punctuation_normalization_collisions_produce_distinct_names() {
            let first = session_name(
                "github.com/acme/tool",
                &format!("feature/colliding-prefix-{}", "a".repeat(40)),
            );
            let second = session_name(
                "github_com/acme/tool",
                &format!("feature_colliding-prefix-{}", "a".repeat(40)),
            );

            assert_ne!(first, second);
            assert_eq!(first.len(), 50);
            assert_eq!(second.len(), 50);
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

        fn hash_suffix(name: &str) -> Option<&str> {
            name.rsplit_once('-').map(|(_, suffix)| suffix)
        }

        fn is_lowercase_hex(suffix: &str) -> bool {
            suffix
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        }

        fn identity_bytes_without_separator<'a>(
            repo_key: &'a str,
            worktree_name: &'a str,
        ) -> impl Iterator<Item = u8> + 'a {
            repo_key.bytes().chain(worktree_name.bytes())
        }
    }
}
