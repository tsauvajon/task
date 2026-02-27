use std::path::Path;

pub fn parse_cmdline_bytes(bytes: &[u8]) -> Vec<String> {
    bytes
        .split(|&b| b == b'\0')
        .filter(|part| !part.is_empty())
        .map(|part| String::from_utf8_lossy(part).into_owned())
        .collect()
}

pub fn cmdline_matches_user_data_dir(args: &[String], user_data_dir: &Path) -> bool {
    let Some(arg0) = args.first() else {
        return false;
    };
    if !is_codium_binary(arg0) {
        return false;
    }

    let target = user_data_dir.to_string_lossy();
    // Check both `--user-data-dir <value>` (split form) and `--user-data-dir=<value>` (joined form).
    args.windows(2)
        .any(|pair| pair[0] == "--user-data-dir" && pair[1] == target.as_ref())
        || args.iter().any(|arg| {
            arg.strip_prefix("--user-data-dir=")
                .is_some_and(|v| v == target.as_ref())
        })
}

fn is_codium_binary(arg0: &str) -> bool {
    Path::new(arg0)
        .file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name.contains("codium"))
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::{cmdline_matches_user_data_dir, parse_cmdline_bytes};

    mod parse_cmdline_bytes {
        use super::*;

        #[test]
        fn splits_nul_separated_args() {
            let parsed = parse_cmdline_bytes(b"codium\0--new-window\0/tmp/wt/repo\0");
            assert_eq!(parsed, vec!["codium", "--new-window", "/tmp/wt/repo"]);
        }

        #[test]
        fn returns_empty_for_empty_input() {
            let parsed = parse_cmdline_bytes(b"");
            assert!(parsed.is_empty());
        }

        #[test]
        fn skips_empty_segments() {
            // Consecutive nul bytes produce empty segments that are filtered.
            let parsed = parse_cmdline_bytes(b"codium\0\0--flag\0");
            assert_eq!(parsed, vec!["codium", "--flag"]);
        }

        #[test]
        fn single_arg_no_trailing_nul() {
            let parsed = parse_cmdline_bytes(b"codium");
            assert_eq!(parsed, vec!["codium"]);
        }
    }

    mod cmdline_matches_user_data_dir {
        use super::*;

        #[test]
        fn matches_split_flag_form() {
            let args = vec![
                "codium".to_string(),
                "--new-window".to_string(),
                "--user-data-dir".to_string(),
                "/tmp/task/codium/a".to_string(),
                "/tmp/wt/repo".to_string(),
            ];
            assert!(cmdline_matches_user_data_dir(
                &args,
                Path::new("/tmp/task/codium/a")
            ));
        }

        #[test]
        fn matches_equals_flag_form() {
            let args = vec![
                "/usr/bin/codium".to_string(),
                "--user-data-dir=/tmp/task/codium/a".to_string(),
            ];
            assert!(cmdline_matches_user_data_dir(
                &args,
                Path::new("/tmp/task/codium/a")
            ));
        }

        #[test]
        fn rejects_non_matching_directory() {
            let args = vec![
                "codium".to_string(),
                "--user-data-dir".to_string(),
                "/tmp/task/codium/a".to_string(),
            ];
            assert!(!cmdline_matches_user_data_dir(
                &args,
                Path::new("/tmp/task/codium/b")
            ));
        }

        #[test]
        fn rejects_non_codium_binary() {
            let args = vec![
                "bash".to_string(),
                "--user-data-dir".to_string(),
                "/tmp/task/codium/a".to_string(),
            ];
            assert!(!cmdline_matches_user_data_dir(
                &args,
                Path::new("/tmp/task/codium/a")
            ));
        }

        #[test]
        fn rejects_empty_args() {
            assert!(!cmdline_matches_user_data_dir(
                &[],
                Path::new("/tmp/task/codium/a")
            ));
        }

        #[test]
        fn matches_path_binary_prefix() {
            // Binary with an absolute path containing "codium"
            let args = vec![
                "/usr/lib/vscodium-bin/codium".to_string(),
                "--user-data-dir".to_string(),
                "/tmp/task/codium/a".to_string(),
            ];
            assert!(cmdline_matches_user_data_dir(
                &args,
                Path::new("/tmp/task/codium/a")
            ));
        }

        #[test]
        fn rejects_equals_form_with_wrong_value() {
            let args = vec![
                "codium".to_string(),
                "--user-data-dir=/tmp/task/codium/wrong".to_string(),
            ];
            assert!(!cmdline_matches_user_data_dir(
                &args,
                Path::new("/tmp/task/codium/a")
            ));
        }
    }
}
