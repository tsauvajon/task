use std::path::Path;

pub fn parse_cmdline_bytes(bytes: &[u8]) -> Vec<String> {
    bytes
        .split(|value| *value == b'\0')
        .filter(|part| !part.is_empty())
        .map(|part| String::from_utf8_lossy(part).to_string())
        .collect()
}

pub fn cmdline_matches_user_data_dir(args: &[String], user_data_dir: &Path) -> bool {
    if args.is_empty() || !is_codium_binary(&args[0]) {
        return false;
    }

    let target = user_data_dir.to_string_lossy();
    let mut index = 0;
    while index < args.len() {
        if args[index] == "--user-data-dir" {
            if let Some(value) = args.get(index + 1) {
                return value == target.as_ref();
            }
            return false;
        }

        if let Some(value) = args[index].strip_prefix("--user-data-dir=") {
            return value == target.as_ref();
        }

        index += 1;
    }

    false
}

fn is_codium_binary(arg0: &str) -> bool {
    Path::new(arg0)
        .file_name()
        .and_then(|name| name.to_str())
        .map(|name| name.contains("codium"))
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::{cmdline_matches_user_data_dir, parse_cmdline_bytes};

    #[test]
    fn parse_cmdline_bytes_splits_nul_separated_args() {
        let parsed = parse_cmdline_bytes(b"codium\0--new-window\0/tmp/wt/repo\0");
        assert_eq!(parsed, vec!["codium", "--new-window", "/tmp/wt/repo"]);
    }

    #[test]
    fn cmdline_matches_split_flag_form() {
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
    fn cmdline_matches_equals_flag_form() {
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
    fn cmdline_rejects_non_matching_directory() {
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
    fn cmdline_rejects_non_codium_binary() {
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
}
