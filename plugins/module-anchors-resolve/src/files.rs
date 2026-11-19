// Paths of the checked state, mounted at `/project`.

pub(crate) fn project(relative: &str) -> String {
    format!("/project/{}", relative.trim_start_matches('/'))
}

pub(crate) fn exists(relative: &str) -> bool {
    std::fs::metadata(project(relative.trim_end_matches('/'))).is_ok()
}

pub(crate) fn is_file(relative: &str) -> bool {
    std::fs::metadata(project(relative)).is_ok_and(|metadata| metadata.is_file())
}

// `*` and `?` stay within a segment, `**` spans segments, as in `change-needs-binding`.
pub(crate) fn glob_matches(pattern: &str) -> bool {
    let pattern: Vec<&str> = pattern.split('/').collect();
    let mut files = Vec::new();
    walk("", &mut files);
    files.iter().any(|file| {
        let segments: Vec<&str> = file.split('/').collect();
        glob_segments(&pattern, &segments)
    })
}

fn walk(relative: &str, files: &mut Vec<String>) {
    let Ok(entries) = std::fs::read_dir(project(relative)) else {
        return;
    };
    for entry in entries.flatten() {
        let Ok(name) = entry.file_name().into_string() else {
            continue;
        };
        let path = if relative.is_empty() {
            name
        } else {
            format!("{relative}/{name}")
        };
        match entry.file_type() {
            Ok(kind) if kind.is_dir() => walk(&path, files),
            Ok(kind) if kind.is_file() => files.push(path),
            _ => {}
        }
    }
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
