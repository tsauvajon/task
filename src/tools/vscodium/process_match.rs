use std::path::Path;

#[must_use]
pub(super) fn cmdline_matches_user_data_dir(args: &[String], user_data_dir: &Path) -> bool {
    let Some(program_arg) = args.first() else {
        return false;
    };
    if !is_codium_binary(program_arg) {
        return false;
    }

    let target = user_data_dir.to_string_lossy();
    // Check both `--user-data-dir <value>` (split form) and `--user-data-dir=<value>` (joined form).
    args.windows(2).any(
        |pair| matches!(pair, [flag, value] if flag == "--user-data-dir" && value == target.as_ref()),
    )
        || args.iter().any(|arg| {
            arg.strip_prefix("--user-data-dir=")
                .is_some_and(|v| v == target.as_ref())
        })
}

fn is_codium_binary(program_arg: &str) -> bool {
    // On Linux, argv[0] is typically the `codium` wrapper script or binary.
    // On macOS (nix), VSCodium launches as `VSCodium` (note the capital C)
    // inside the .app bundle, so we use case-insensitive matching.
    // It can also appear as the `Electron` binary under a vscodium nix store
    // path.
    let path = Path::new(program_arg);
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("");

    if file_name.to_ascii_lowercase().contains("codium") {
        return true;
    }

    if file_name == "Electron" {
        let lower = path.to_string_lossy().to_ascii_lowercase();
        return lower.contains("vscodium");
    }

    false
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::cmdline_matches_user_data_dir;

    fn parse_cmdline_bytes(bytes: &[u8]) -> Vec<String> {
        bytes
            .split(|value| *value == b'\0')
            .filter(|part| !part.is_empty())
            .map(|part| String::from_utf8_lossy(part).to_string())
            .collect()
    }

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
                "codium".to_owned(),
                "--new-window".to_owned(),
                "--user-data-dir".to_owned(),
                "/tmp/task/codium/a".to_owned(),
                "/tmp/wt/repo".to_owned(),
            ];
            assert!(cmdline_matches_user_data_dir(
                &args,
                Path::new("/tmp/task/codium/a")
            ));
        }

        #[test]
        fn matches_equals_flag_form() {
            let args = vec![
                "/usr/bin/codium".to_owned(),
                "--user-data-dir=/tmp/task/codium/a".to_owned(),
            ];
            assert!(cmdline_matches_user_data_dir(
                &args,
                Path::new("/tmp/task/codium/a")
            ));
        }

        #[test]
        fn rejects_non_matching_directory() {
            let args = vec![
                "codium".to_owned(),
                "--user-data-dir".to_owned(),
                "/tmp/task/codium/a".to_owned(),
            ];
            assert!(!cmdline_matches_user_data_dir(
                &args,
                Path::new("/tmp/task/codium/b")
            ));
        }

        #[test]
        fn rejects_non_codium_binary() {
            let args = vec![
                "bash".to_owned(),
                "--user-data-dir".to_owned(),
                "/tmp/task/codium/a".to_owned(),
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
                "/usr/lib/vscodium-bin/codium".to_owned(),
                "--user-data-dir".to_owned(),
                "/tmp/task/codium/a".to_owned(),
            ];
            assert!(cmdline_matches_user_data_dir(
                &args,
                Path::new("/tmp/task/codium/a")
            ));
        }

        #[test]
        fn rejects_equals_form_with_wrong_value() {
            let args = vec![
                "codium".to_owned(),
                "--user-data-dir=/tmp/task/codium/wrong".to_owned(),
            ];
            assert!(!cmdline_matches_user_data_dir(
                &args,
                Path::new("/tmp/task/codium/a")
            ));
        }
    }

    #[test]
    fn cmdline_matches_macos_electron_in_vscodium_bundle() {
        let args = vec![
            "/nix/store/abc123-vscodium-1.2.3/Applications/VSCodium.app/Contents/MacOS/Electron"
                .to_owned(),
            "--new-window".to_owned(),
            "--user-data-dir".to_owned(),
            "/tmp/task/codium/a".to_owned(),
            "/tmp/wt/repo".to_owned(),
        ];
        assert!(cmdline_matches_user_data_dir(
            &args,
            Path::new("/tmp/task/codium/a")
        ));
    }

    #[test]
    fn cmdline_rejects_non_vscodium_electron_binary() {
        // Electron in a non-vscodium app should not match
        let args = vec![
            "/Applications/SomeOtherApp.app/Contents/MacOS/Electron".to_owned(),
            "--user-data-dir".to_owned(),
            "/tmp/task/codium/a".to_owned(),
        ];
        assert!(!cmdline_matches_user_data_dir(
            &args,
            Path::new("/tmp/task/codium/a")
        ));
    }

    #[test]
    fn cmdline_matches_macos_vscodium_binary() {
        // On macOS the main process binary is named "VSCodium" (capital C),
        // not "codium" as on Linux.
        let args = vec![
            "/nix/store/abc-vscodium-1.2.3/Applications/VSCodium.app/Contents/MacOS/VSCodium"
                .to_owned(),
            "--user-data-dir".to_owned(),
            "/tmp/task/codium/a".to_owned(),
        ];
        assert!(cmdline_matches_user_data_dir(
            &args,
            Path::new("/tmp/task/codium/a")
        ));
    }
}
