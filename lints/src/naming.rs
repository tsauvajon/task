pub fn common_prefix<'a>(a: &'a str, b: &str) -> &'a str {
    let mut last_boundary = 0;
    for (i, (ca, cb)) in a.bytes().zip(b.bytes()).enumerate() {
        if ca != cb {
            break;
        }
        if ca == b'_' {
            last_boundary = i + 1;
        }
    }

    let min_len = a.len().min(b.len());
    if a.as_bytes()[..min_len] == b.as_bytes()[..min_len]
        && min_len > 0
        && a.as_bytes().get(min_len - 1) == Some(&b'_')
    {
        last_boundary = min_len;
    }
    &a[..last_boundary]
}

pub fn common_suffix<'a>(a: &'a str, b: &str) -> &'a str {
    let mut last_boundary = a.len();
    let a_bytes = a.as_bytes();
    let b_bytes = b.as_bytes();

    let mut ai = a.len();
    let mut bi = b.len();
    while ai > 0 && bi > 0 {
        ai -= 1;
        bi -= 1;
        if a_bytes[ai] != b_bytes[bi] {
            break;
        }
        if a_bytes[ai] == b'_' {
            last_boundary = ai;
        }
    }

    if ai == 0 && bi == 0 && a_bytes[0] == b_bytes[0] && a_bytes[0] == b'_' {
        last_boundary = 0;
    }
    &a[last_boundary..]
}

pub fn suggest_helper_name(names: &[&str]) -> String {
    if names.len() < 2 {
        return String::from("shared_helper");
    }

    let mut prefix = common_prefix(names[0], names[1]);
    for name in &names[2..] {
        prefix = common_prefix(prefix, name);
    }

    let mut suffix = common_suffix(names[0], names[1]);
    for name in &names[2..] {
        let next_suffix = common_suffix(suffix, name);
        suffix = &names[0][names[0].len() - next_suffix.len()..];
    }

    let suffix_trimmed = suffix.strip_prefix('_').unwrap_or(suffix);
    if !prefix.is_empty() && !suffix_trimmed.is_empty() {
        format!("{prefix}{suffix_trimmed}")
    } else if !prefix.is_empty() {
        let base = prefix.strip_suffix('_').unwrap_or(prefix);
        format!("{base}_op")
    } else if !suffix_trimmed.is_empty() {
        format!("shared_{suffix_trimmed}")
    } else {
        String::from("shared_helper")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prefixes_follow_underscore_boundaries() {
        assert_eq!(common_prefix("call_left", "call_right"), "call_");
        assert_eq!(
            common_prefix("eval_all_predicate", "eval_any_predicate"),
            "eval_"
        );
        assert_eq!(common_prefix("foo", "bar"), "");
    }

    #[test]
    fn suffixes_follow_underscore_boundaries() {
        assert_eq!(
            common_suffix("eval_all_predicate", "eval_any_predicate"),
            "_predicate"
        );
        assert_eq!(common_suffix("call_left", "call_right"), "");
    }

    #[test]
    fn helper_names_use_shared_name_parts() {
        assert_eq!(suggest_helper_name(&["call_left", "call_right"]), "call_op");
        assert_eq!(
            suggest_helper_name(&[
                "eval_all_predicate",
                "eval_any_predicate",
                "eval_none_predicate"
            ]),
            "eval_predicate"
        );
        assert_eq!(suggest_helper_name(&["foo", "bar"]), "shared_helper");
    }
}
