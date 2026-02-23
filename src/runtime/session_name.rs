pub fn task_session_name(repo_key: &str, branch: &str) -> String {
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
    use super::task_session_name;

    #[test]
    fn task_session_name_is_sanitized() {
        assert_eq!(
            task_session_name("github.com/tsauvajon/goto", "feat/test.1"),
            "github_com_tsauvajon_goto-feat_test_1"
        );
    }
}
