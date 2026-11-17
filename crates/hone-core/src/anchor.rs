use serde_json::Value;

pub(crate) fn hits(anchor: &Value, changed: &str) -> bool {
    let (Some(kind), Some(reference)) = (anchor["kind"].as_str(), anchor["ref"].as_str()) else {
        return false;
    };
    let (anchor_repository, anchor_path) = split_repository(reference);
    let (changed_repository, changed_path) = split_repository(changed);
    if anchor_repository != changed_repository {
        return false;
    }
    let changed_segments: Vec<&str> = changed_path.split('/').collect();
    match kind {
        "path" => starts_with_segments(&changed_segments, anchor_path),
        "symbol" => {
            let file = anchor_path.split('#').next().unwrap_or(anchor_path);
            starts_with_segments(&changed_segments, file)
                && file.split('/').count() == changed_segments.len()
        }
        "glob" => {
            let pattern: Vec<&str> = anchor_path.split('/').collect();
            glob_segments(&pattern, &changed_segments)
        }
        _ => false,
    }
}

fn split_repository(reference: &str) -> (Option<&str>, &str) {
    match reference.split_once(':') {
        Some((repository, path)) => (Some(repository), path),
        None => (None, reference),
    }
}

fn starts_with_segments(changed: &[&str], prefix: &str) -> bool {
    let prefix: Vec<&str> = prefix.trim_end_matches('/').split('/').collect();
    changed.len() >= prefix.len() && changed[..prefix.len()] == prefix[..]
}

fn glob_segments(pattern: &[&str], path: &[&str]) -> bool {
    match pattern.split_first() {
        None => path.is_empty(),
        Some((&"**", rest)) => (0..=path.len()).any(|skip| glob_segments(rest, &path[skip..])),
        Some((segment, rest)) => match path.split_first() {
            Some((name, path_rest)) => {
                glob_segment(segment, name) && glob_segments(rest, path_rest)
            }
            None => false,
        },
    }
}

fn glob_segment(pattern: &str, name: &str) -> bool {
    let pattern: Vec<char> = pattern.chars().collect();
    let name: Vec<char> = name.chars().collect();
    glob_characters(&pattern, &name)
}

fn glob_characters(pattern: &[char], name: &[char]) -> bool {
    match pattern.split_first() {
        None => name.is_empty(),
        Some(('*', rest)) => (0..=name.len()).any(|skip| glob_characters(rest, &name[skip..])),
        Some(('?', rest)) => !name.is_empty() && glob_characters(rest, &name[1..]),
        Some((character, rest)) => {
            name.first() == Some(character) && glob_characters(rest, &name[1..])
        }
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::hits;

    fn anchor(kind: &str, reference: &str) -> serde_json::Value {
        json!({ "kind": kind, "ref": reference })
    }

    #[test]
    fn path_hits_the_file_and_everything_below_by_whole_segments() {
        assert!(hits(&anchor("path", "src/payments"), "src/payments/fee.rs"));
        assert!(hits(
            &anchor("path", "src/payments/fee.rs"),
            "src/payments/fee.rs"
        ));
        assert!(!hits(&anchor("path", "src/pay"), "src/payments/fee.rs"));
        assert!(!hits(
            &anchor("path", "src/payments/fee.rs"),
            "src/payments"
        ));
    }

    #[test]
    fn symbol_hits_its_file_only() {
        let symbol = anchor("symbol", "src/payments/fee.rs#FeeCalculator");
        assert!(hits(&symbol, "src/payments/fee.rs"));
        assert!(!hits(&symbol, "src/payments/fee.rs/x"));
        assert!(!hits(&symbol, "src/payments/other.rs"));
    }

    #[test]
    fn glob_knows_star_double_star_and_question_mark() {
        assert!(hits(&anchor("glob", "src/**/*.rs"), "src/fee.rs"));
        assert!(hits(&anchor("glob", "src/**/*.rs"), "src/a/b/fee.rs"));
        assert!(!hits(&anchor("glob", "src/*.rs"), "src/a/fee.rs"));
        assert!(hits(&anchor("glob", "src/fe?.rs"), "src/fee.rs"));
        assert!(!hits(&anchor("glob", "src/fe?.rs"), "src/fe.rs"));
        assert!(!hits(&anchor("glob", "src/[f]ee.rs"), "src/fee.rs"));
    }

    #[test]
    fn repository_prefixes_must_agree() {
        assert!(hits(
            &anchor("path", "billing:src/x.rs"),
            "billing:src/x.rs"
        ));
        assert!(!hits(&anchor("path", "billing:src/x.rs"), "src/x.rs"));
        assert!(!hits(&anchor("path", "src/x.rs"), "billing:src/x.rs"));
    }
}
