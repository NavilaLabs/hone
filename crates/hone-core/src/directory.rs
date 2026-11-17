use std::collections::BTreeSet;
use std::path::Path;

use regex::Regex;
use serde_json::Value;

use crate::artifact::{self, Content};

const RANDOM_PART: &str = "[0-9a-hjkmnp-tv-z]{8}";
const SLUG: &str = "[a-z0-9]+(?:-[a-z0-9]+)*";

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Place {
    Ticket,
    Kind(String),
}

impl Place {
    pub(crate) fn type_name(&self) -> &str {
        match self {
            Place::Ticket => "ticket",
            Place::Kind(name) => name,
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct Artifact {
    pub(crate) path: String,
    pub(crate) place: Place,
    pub(crate) id_in_path: Option<String>,
    pub(crate) ticket_in_path: Option<String>,
    pub(crate) content: Content,
}

impl Artifact {
    pub(crate) fn field(&self, key: &str) -> Option<&str> {
        self.content.data.get(key).and_then(Value::as_str)
    }

    pub(crate) fn id(&self) -> Option<&str> {
        self.field("id")
    }

    pub(crate) fn type_name(&self) -> Option<&str> {
        self.field("type")
    }
}

#[derive(Debug, Clone)]
pub(crate) struct Unreadable {
    pub(crate) path: String,
    pub(crate) violations: Vec<String>,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct Directory {
    pub(crate) artifacts: Vec<Artifact>,
    pub(crate) unreadable: Vec<Unreadable>,
    pub(crate) unmatched: Vec<String>,
}

impl Directory {
    pub(crate) fn scan(root: &Path, config: &Value) -> Self {
        let patterns = Patterns::of(config);
        let mut directory = Directory::default();
        for path in patterns.candidates(root) {
            let Some((place, captures)) = patterns.place(&path) else {
                if patterns.looks_like_artifact(&path) {
                    directory.unmatched.push(path);
                }
                continue;
            };
            let bytes = match std::fs::read(root.join(&path)) {
                Ok(bytes) => bytes,
                Err(error) => {
                    directory.unreadable.push(Unreadable {
                        path,
                        violations: vec![error.to_string()],
                    });
                    continue;
                }
            };
            match artifact::parse(&path, &bytes) {
                Ok(content) => directory.artifacts.push(Artifact {
                    path,
                    place,
                    id_in_path: captures.id,
                    ticket_in_path: captures.ticket,
                    content,
                }),
                Err(violations) => directory.unreadable.push(Unreadable { path, violations }),
            }
        }
        directory
    }

    pub(crate) fn is_artifact_path(config: &Value, path: &str) -> bool {
        Patterns::of(config).place(path).is_some()
    }
}

struct Pattern {
    place: Place,
    regex: Regex,
    leading: String,
    depth: usize,
}

#[derive(Default)]
struct Captures {
    id: Option<String>,
    ticket: Option<String>,
}

struct Patterns {
    patterns: Vec<Pattern>,
    id_prefixes: Vec<String>,
}

impl Patterns {
    fn of(config: &Value) -> Self {
        let mut patterns = Vec::new();
        if let Some(path) = config["ticket"]["path"].as_str() {
            patterns.push(Pattern::new(Place::Ticket, path, None));
        }
        let mut id_prefixes = Vec::new();
        if let Some(kinds) = config["kinds"].as_object() {
            for (name, kind) in kinds {
                let (Some(path), Some(prefix)) =
                    (kind["path"].as_str(), kind["id_prefix"].as_str())
                else {
                    continue;
                };
                patterns.push(Pattern::new(Place::Kind(name.clone()), path, Some(prefix)));
                id_prefixes.push(format!("{prefix}-"));
            }
        }
        Self {
            patterns,
            id_prefixes,
        }
    }

    fn place(&self, path: &str) -> Option<(Place, Captures)> {
        self.patterns.iter().find_map(|pattern| {
            let found = pattern.regex.captures(path)?;
            let text = |name: &str| found.name(name).map(|m| m.as_str().to_owned());
            Some((
                pattern.place.clone(),
                Captures {
                    id: text("id"),
                    ticket: text("ticket"),
                },
            ))
        })
    }

    fn looks_like_artifact(&self, path: &str) -> bool {
        let name = path.rsplit('/').next().unwrap_or(path);
        self.id_prefixes
            .iter()
            .any(|prefix| name.starts_with(prefix))
    }

    fn candidates(&self, root: &Path) -> BTreeSet<String> {
        let mut files = BTreeSet::new();
        for pattern in &self.patterns {
            collect_files(
                &root.join(&pattern.leading),
                &pattern.leading,
                pattern.depth,
                &mut files,
            );
        }
        files
    }
}

impl Pattern {
    fn new(place: Place, pattern: &str, id_prefix: Option<&str>) -> Self {
        let mut regex = String::from("^");
        let mut rest = pattern;
        while let Some(start) = rest.find('{') {
            regex.push_str(&regex::escape(&rest[..start]));
            let end = rest[start..]
                .find('}')
                .map_or(rest.len(), |end| start + end + 1);
            match (&rest[start..end], id_prefix) {
                ("{id}", Some(prefix)) => {
                    regex.push_str(&format!("(?P<id>{}-{RANDOM_PART})", regex::escape(prefix)))
                }
                ("{ticket}", _) => regex.push_str(&format!("(?P<ticket>tk-{RANDOM_PART})")),
                ("{slug}", Some(_)) => regex.push_str(SLUG),
                (literal, _) => regex.push_str(&regex::escape(literal)),
            }
            rest = &rest[end..];
        }
        regex.push_str(&regex::escape(rest));
        regex.push('$');

        let before_placeholder = &pattern[..pattern.find('{').unwrap_or(pattern.len())];
        let leading = before_placeholder
            .rfind('/')
            .map_or("", |slash| &pattern[..=slash])
            .to_owned();
        let depth = pattern[leading.len()..].split('/').count();
        Self {
            place,
            regex: Regex::new(&regex).expect("an escaped pattern is a valid regex"),
            leading,
            depth,
        }
    }
}

// Collects the files up to `depth` levels below `directory`, as paths relative to the project
// root with `/` as separator. Symbolic links and names that are not UTF-8 are skipped.
fn collect_files(directory: &Path, relative: &str, depth: usize, files: &mut BTreeSet<String>) {
    if depth == 0 {
        return;
    }
    let Ok(entries) = std::fs::read_dir(directory) else {
        return;
    };
    for entry in entries.flatten() {
        let (Ok(kind), Some(name)) = (
            entry.file_type(),
            entry.file_name().to_str().map(str::to_owned),
        ) else {
            continue;
        };
        let path = format!("{relative}{name}");
        if kind.is_file() {
            files.insert(path);
        } else if kind.is_dir() {
            collect_files(&entry.path(), &format!("{path}/"), depth - 1, files);
        }
    }
}
