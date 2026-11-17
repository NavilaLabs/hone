use std::collections::BTreeMap;
use std::path::Path;
use std::sync::LazyLock;

use regex::Regex;
use serde_json::{Map, Value};

use crate::error::{Category, Origin, Problem, file_problem};
use crate::yaml;

static PACKAGE_NAME: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^[a-z][a-z0-9]*(-[a-z0-9]+)*(/[a-z][a-z0-9]*(-[a-z0-9]+)*)*$")
        .expect("valid regex")
});
static MAJOR: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^(0|[1-9][0-9]*)$").expect("valid regex"));
static SEMVER: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"^(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)(?:-(?:0|[1-9]\d*|\d*[a-zA-Z-][0-9a-zA-Z-]*)(?:\.(?:0|[1-9]\d*|\d*[a-zA-Z-][0-9a-zA-Z-]*))*)?(?:\+[0-9a-zA-Z-]+(?:\.[0-9a-zA-Z-]+)*)?$",
    )
    .expect("valid regex")
});
static HASH: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^sha256:[0-9a-f]{64}$").expect("valid regex"));

const PROJECT_CONFIG_KEYS: [&str; 4] = ["workspace", "project", "extends", "generate"];

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Reference {
    pub(crate) name: String,
    pub(crate) major: String,
}

impl std::fmt::Display for Reference {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{}@{}", self.name, self.major)
    }
}

#[derive(Debug, Clone)]
pub(crate) struct PinnedPackage {
    pub(crate) version: String,
    pub(crate) hash: String,
}

impl PinnedPackage {
    pub(crate) fn major(&self) -> &str {
        self.version.split('.').next().unwrap_or_default()
    }
}

#[derive(Debug, Clone)]
pub(crate) struct ApprovedPlugin {
    pub(crate) hash: String,
    pub(crate) capabilities: Map<String, Value>,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct Lock {
    pub(crate) packages: BTreeMap<String, PinnedPackage>,
    pub(crate) plugins: BTreeMap<String, ApprovedPlugin>,
}

pub(crate) fn read_project_config(hone: &Path) -> Result<Vec<Reference>, Vec<Problem>> {
    let origin = Origin::Project;
    let Some(config) = read_map(hone, "config.yaml", &origin)? else {
        return Err(vec![manifest(&origin, "config.yaml", "missing")]);
    };
    let mut problems = Vec::new();
    for key in config.keys() {
        if !PROJECT_CONFIG_KEYS.contains(&key.as_str()) && !key.starts_with("x-") {
            problems.push(manifest(
                &origin,
                "config.yaml",
                format!("unknown key `{key}`"),
            ));
        }
    }
    match config.get("generate") {
        None => {}
        Some(Value::Array(generators)) if generators.is_empty() => {}
        Some(Value::Array(_)) => problems.push(manifest(
            &origin,
            "config.yaml",
            "generators are not supported by this implementation yet",
        )),
        Some(_) => problems.push(manifest(
            &origin,
            "config.yaml",
            "`generate` must be a list",
        )),
    }
    let extends = read_extends(&config, &origin, &mut problems);
    if problems.is_empty() {
        Ok(extends)
    } else {
        Err(problems)
    }
}

pub(crate) fn read_package_config(
    root: &Path,
    origin: &Origin,
) -> Result<Vec<Reference>, Vec<Problem>> {
    let Some(config) = read_map(root, "config.yaml", origin)? else {
        return Ok(Vec::new());
    };
    let mut problems = Vec::new();
    for key in config.keys().filter(|key| *key != "extends") {
        problems.push(manifest(
            origin,
            "config.yaml",
            format!("a package config.yaml contains only `extends`, found `{key}`"),
        ));
    }
    let extends = read_extends(&config, origin, &mut problems);
    if problems.is_empty() {
        Ok(extends)
    } else {
        Err(problems)
    }
}

pub(crate) fn read_lock(hone: &Path) -> Result<Lock, Vec<Problem>> {
    let origin = Origin::Project;
    let Some(lock) = read_map(hone, "lock.yaml", &origin)? else {
        return Ok(Lock::default());
    };
    let mut problems = Vec::new();
    let mut result = Lock::default();
    let mut complain = |message: String| problems.push(manifest(&origin, "lock.yaml", message));
    for (key, value) in &lock {
        match (key.as_str(), value) {
            ("packages", Value::Object(packages)) => {
                for (name, entry) in packages {
                    match read_pinned_package(entry) {
                        Ok(pinned) => {
                            result.packages.insert(name.clone(), pinned);
                        }
                        Err(message) => complain(format!("packages/{name}: {message}")),
                    }
                }
            }
            ("plugins", Value::Object(plugins)) => {
                for (name, entry) in plugins {
                    match read_approved_plugin(entry) {
                        Ok(approved) => {
                            result.plugins.insert(name.clone(), approved);
                        }
                        Err(message) => complain(format!("plugins/{name}: {message}")),
                    }
                }
            }
            ("packages" | "plugins", _) => complain(format!("`{key}` must be a map")),
            (other, _) if other.starts_with("x-") => {}
            (other, _) => complain(format!("unknown key `{other}`")),
        }
    }
    if problems.is_empty() {
        Ok(result)
    } else {
        Err(problems)
    }
}

fn read_pinned_package(entry: &Value) -> Result<PinnedPackage, String> {
    let entry = entry.as_object().ok_or("must be a map")?;
    if let Some(key) = entry
        .keys()
        .find(|key| !matches!(key.as_str(), "version" | "hash"))
    {
        return Err(format!("unknown key `{key}`"));
    }
    let version = match entry.get("version") {
        Some(Value::String(version)) if SEMVER.is_match(version) => version.clone(),
        Some(_) => return Err("`version` must be an exact semver version".into()),
        None => return Err("`version` is missing".into()),
    };
    Ok(PinnedPackage {
        version,
        hash: read_hash(entry)?,
    })
}

fn read_approved_plugin(entry: &Value) -> Result<ApprovedPlugin, String> {
    let entry = entry.as_object().ok_or("must be a map")?;
    if let Some(key) = entry
        .keys()
        .find(|key| !matches!(key.as_str(), "hash" | "capabilities"))
    {
        return Err(format!("unknown key `{key}`"));
    }
    let capabilities = match entry.get("capabilities") {
        Some(Value::Object(capabilities)) => capabilities.clone(),
        Some(_) => return Err("`capabilities` must be a map".into()),
        None => return Err("`capabilities` is missing".into()),
    };
    Ok(ApprovedPlugin {
        hash: read_hash(entry)?,
        capabilities,
    })
}

fn read_hash(entry: &Map<String, Value>) -> Result<String, String> {
    match entry.get("hash") {
        Some(Value::String(hash)) if HASH.is_match(hash) => Ok(hash.clone()),
        Some(_) => Err("`hash` must be `sha256:` and 64 lowercase hex digits".into()),
        None => Err("`hash` is missing".into()),
    }
}

fn read_extends(
    config: &Map<String, Value>,
    origin: &Origin,
    problems: &mut Vec<Problem>,
) -> Vec<Reference> {
    let items = match config.get("extends") {
        None => return Vec::new(),
        Some(Value::Array(items)) => items,
        Some(_) => {
            problems.push(manifest(origin, "config.yaml", "`extends` must be a list"));
            return Vec::new();
        }
    };
    let mut references = Vec::new();
    for item in items {
        match item.as_str().and_then(parse_reference) {
            Some(reference) => references.push(reference),
            None => problems.push(manifest(
                origin,
                "config.yaml",
                format!("`{item}` is not a reference of the form `<name>@<major>`"),
            )),
        }
    }
    references
}

fn parse_reference(text: &str) -> Option<Reference> {
    let (name, major) = text.rsplit_once('@')?;
    (PACKAGE_NAME.is_match(name) && MAJOR.is_match(major)).then(|| Reference {
        name: name.to_owned(),
        major: major.to_owned(),
    })
}

fn read_map(
    directory: &Path,
    file: &str,
    origin: &Origin,
) -> Result<Option<Map<String, Value>>, Vec<Problem>> {
    let path = directory.join(file);
    if !path.exists() {
        return Ok(None);
    }
    let bytes = std::fs::read(&path)
        .map_err(|error| vec![manifest(origin, file, format!("cannot be read: {error}"))])?;
    match yaml::parse(&bytes) {
        Ok(Value::Object(map)) => Ok(Some(map)),
        Ok(_) => Err(vec![manifest(origin, file, "must be a map")]),
        Err(violations) => Err(violations
            .into_iter()
            .map(|violation| {
                file_problem(Category::YamlProfile, origin, file, violation.to_string())
            })
            .collect()),
    }
}

fn manifest(origin: &Origin, file: &str, message: impl Into<String>) -> Problem {
    file_problem(Category::Manifest, origin, file, message)
}
