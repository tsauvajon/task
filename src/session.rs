pub fn session_name_for(repo_key: &str, branch: &str) -> String {
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
