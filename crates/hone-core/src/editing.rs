//! Reading, saving, and creating artifact files, and reading a source file with the line of a
//! symbol, for a user interface (concept Section 12, "Desktop GUI"). Every path comes from an
//! untrusted state: it is relative, below the root, and a regular file; writes go through
//! `below`, so no symbolic link below the root is followed.

use std::path::{Component, Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::Serialize;
use serde_json::{Map, Value, json};

use crate::Resolved;
use crate::below;
use crate::check::CheckError;
use crate::directory::{Directory, Place};
use crate::finding::{Finding, Level};
use crate::gates::{ID_LINKS, Scope, language_of, languages};
use crate::host::{Host, LanguageCall};
use crate::manifest::{Lock, read_lock_file};
use crate::schema_valid::check_file;

const CROCKFORD: &[u8; 32] = b"0123456789abcdefghjkmnpqrstvwxyz";
const LARGEST_FILE: u64 = 4 * 1024 * 1024;

/// An artifact file with what a form needs to edit it.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ArtifactSource {
    /// The artifact's ID.
    pub id: String,
    /// The kind.
    #[serde(rename = "type")]
    pub kind: String,
    /// The path relative to the project root.
    pub path: String,
    /// The file as it is.
    pub text: String,
    /// The data model: the front matter of a Markdown file, the whole of a YAML file.
    pub data: Map<String, Value>,
    /// The Markdown body, `None` for a YAML file.
    pub body: Option<String>,
    /// The kind's lifecycle, `decided` or `living`.
    pub lifecycle: String,
    /// The kind's statuses with whether they bind, sorted by name.
    pub statuses: Vec<KindStatus>,
    /// The kind's additional schema, if it has one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub schema: Option<Value>,
    /// The ID link types an artifact may store under `links`.
    pub link_types: Vec<String>,
}

/// A kind of the configuration, as a choice for a new artifact.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct KindInfo {
    /// The kind's name.
    #[serde(rename = "type")]
    pub kind: String,
    /// `decided` or `living`.
    pub lifecycle: String,
    /// The prefix of its IDs.
    pub id_prefix: String,
    /// The path pattern of its files.
    pub path: String,
    /// The status a new artifact of the kind starts with, for lifecycle `decided`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub first_status: Option<String>,
    /// The fields its additional schema requires.
    pub required: Vec<String>,
}

/// A status of a kind.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct KindStatus {
    /// The status's name.
    pub name: String,
    /// Whether the status binds.
    pub binding: bool,
}

/// The result of saving or creating an artifact file.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ArtifactWritten {
    /// The artifact's ID, if the text names one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// The path relative to the project root.
    pub path: String,
    /// Whether the file was written: only when no finding is an error.
    pub saved: bool,
    /// The findings of the check, sorted; warnings do not stop the write.
    pub findings: Vec<Finding>,
}

/// The result of checking an artifact file's text without writing it.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ArtifactChecked {
    /// The artifact's ID, if the text names one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// The path relative to the project root.
    pub path: String,
    /// The findings of the check, sorted.
    pub findings: Vec<Finding>,
}

/// What a new artifact starts with.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ArtifactNew {
    /// The kind.
    pub kind: String,
    /// The ticket it belongs to; a kind with lifecycle `decided` needs one.
    pub ticket: Option<String>,
    /// The title: the slug of the path, the first heading of a Markdown body, and the field
    /// `name` when the kind's schema requires one.
    pub title: String,
    /// The author's handle, the first of `authors` for lifecycle `decided`.
    pub author: Option<String>,
    /// Anchors as `(kind, ref)`, for example `("path", "src/payments")`.
    pub anchors: Vec<(String, String)>,
    /// The IDs it supersedes, written as `links.supersedes` in this order.
    pub supersedes: Vec<String>,
}

/// A source file, and the line of a symbol in it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SourceFile {
    /// The path relative to the project root.
    pub path: String,
    /// The file's text.
    pub text: String,
    /// The symbol asked for.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub symbol: Option<String>,
    /// The language plugin that handles the file.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub language: Option<String>,
    /// With a symbol: `found`, `missing`, `unparsable`, `unchecked` (no language plugin), or
    /// `failed` (the plugin could not run).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resolved: Option<String>,
    /// The line of the symbol, from 1, when the language plugin found it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub line: Option<u64>,
    /// Why the symbol has no line, when it has none.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

/// The kinds of the current configuration of `project_root`, sorted by name.
///
/// # Errors
///
/// Returns a [`CheckError`] if the configuration cannot be read.
pub fn kinds(project_root: &Path) -> Result<Vec<KindInfo>, CheckError> {
    let current = Resolved::load(project_root)?;
    let text = |value: &Value| value.as_str().unwrap_or_default().to_owned();
    Ok(current.document()["kinds"]
        .as_object()
        .into_iter()
        .flatten()
        .map(|(name, definition)| KindInfo {
            kind: name.clone(),
            lifecycle: text(&definition["lifecycle"]),
            id_prefix: text(&definition["id_prefix"]),
            path: text(&definition["path"]),
            first_status: (definition["lifecycle"] == "decided")
                .then(|| first_status(definition))
                .flatten(),
            required: definition["schema"]["required"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(Value::as_str)
                .map(str::to_owned)
                .collect(),
        })
        .collect())
}

/// The artifact `id` of the checked directory `project_root`.
///
/// # Errors
///
/// Returns a [`CheckError`] if the configuration cannot be read or no readable artifact has the
/// ID.
pub fn show_artifact(project_root: &Path, id: &str) -> Result<ArtifactSource, CheckError> {
    let current = Resolved::load(project_root)?;
    let config = current.document();
    let directory = Directory::scan(project_root, config);
    let found = directory
        .artifacts
        .iter()
        .find(|artifact| artifact.id() == Some(id) && artifact.place != Place::Ticket)
        .ok_or_else(|| error(format!("no artifact `{id}` in the project")))?;
    let kind = found.place.type_name().to_owned();
    let definition = &config["kinds"][&kind];
    let text = read_below(project_root, &found.path)?;
    Ok(ArtifactSource {
        id: id.to_owned(),
        path: found.path.clone(),
        data: found.content.data.clone(),
        body: found
            .path
            .ends_with(".md")
            .then(|| found.content.body.clone()),
        text,
        lifecycle: definition["lifecycle"]
            .as_str()
            .unwrap_or_default()
            .to_owned(),
        statuses: statuses(definition),
        schema: definition.get("schema").cloned(),
        link_types: ID_LINKS.iter().map(|link| (*link).to_owned()).collect(),
        kind,
    })
}

/// Checks `bytes` as the artifact file `path` (relative to `project_root`) like `schema-valid`
/// does for one file, and writes it only if no finding is an error.
///
/// # Errors
///
/// Returns a [`CheckError`] if the configuration cannot be read, the path is not relative, leaves
/// the root, names a ticket record, or matches no artifact pattern, or the file cannot be
/// written.
pub fn save_artifact(
    project_root: &Path,
    path: &str,
    bytes: &[u8],
) -> Result<ArtifactWritten, CheckError> {
    let current = Resolved::load(project_root)?;
    check_relative(path)?;
    let config = current.document();
    let findings = check_text(config, path, bytes)?;
    let written = written(path, bytes, findings);
    if written.saved {
        let file = project_root.join(path);
        let result = if file.symlink_metadata().is_ok() {
            below::replace(&file, project_root, bytes)
        } else {
            below::create_new(&file, project_root, bytes)
        };
        result.map_err(|io| error(format!("{path}: {io}")))?;
    }
    Ok(written)
}

/// Checks `bytes` as the artifact file `path` (relative to `project_root`) without writing it:
/// the findings [`save_artifact`] gives, and the findings of `links-resolve` for this file
/// against the checked directory with this text in place of the file.
///
/// # Errors
///
/// Returns a [`CheckError`] if the configuration cannot be read, or the path is not relative,
/// leaves the root, names a ticket record, or matches no artifact pattern.
pub fn check_unsaved_artifact(
    project_root: &Path,
    path: &str,
    bytes: &[u8],
) -> Result<ArtifactChecked, CheckError> {
    let current = Resolved::load(project_root)?;
    check_relative(path)?;
    let config = current.document();
    let mut findings = check_text(config, path, bytes)?;
    if let Some(Ok(artifact)) = Directory::single(config, path, bytes) {
        let mut directory = Directory::scan(project_root, config);
        directory.artifacts.retain(|other| other.path != path);
        directory.unreadable.retain(|other| other.path != path);
        directory.artifacts.push(artifact);
        findings.extend(
            Scope::new(config, &directory)
                .directory_gate("links-resolve")
                .into_iter()
                .filter(|finding| finding.path.as_deref() == Some(path)),
        );
        findings.sort();
    }
    let written = written(path, bytes, findings);
    Ok(ArtifactChecked {
        id: written.id,
        path: written.path,
        findings: written.findings,
    })
}

/// Creates an artifact of `new.kind` at the kind's path pattern with an unused ID and the
/// required fields of its lifecycle, checked like [`save_artifact`].
///
/// # Errors
///
/// Returns a [`CheckError`] if the configuration cannot be read, the kind is unknown, a kind with
/// lifecycle `decided` has no ticket or a `living` one has one, the ticket has no record, the
/// title gives no slug, or the file cannot be written.
pub fn new_artifact(project_root: &Path, new: &ArtifactNew) -> Result<ArtifactWritten, CheckError> {
    let current = Resolved::load(project_root)?;
    let config = current.document();
    let definition = config["kinds"]
        .get(&new.kind)
        .ok_or_else(|| error(format!("the configuration has no kind `{}`", new.kind)))?;
    let lifecycle = definition["lifecycle"].as_str().unwrap_or_default();
    let pattern = definition["path"].as_str().unwrap_or_default();
    let prefix = definition["id_prefix"].as_str().unwrap_or_default();
    let title = new.title.trim();
    if title.is_empty() {
        return Err(error("a new artifact needs a title"));
    }
    let slug = slug(title);
    if pattern.contains("{slug}") && slug.is_empty() {
        return Err(error(format!(
            "the title `{title}` gives no slug: use letters or digits"
        )));
    }
    let directory = Directory::scan(project_root, config);
    let ticket =
        match (lifecycle, new.ticket.as_deref()) {
            ("decided", None) => {
                return Err(error(format!(
                    "the kind `{}` belongs to a ticket: pass --ticket",
                    new.kind
                )));
            }
            ("decided", Some(ticket)) => {
                if !directory.artifacts.iter().any(|artifact| {
                    artifact.place == Place::Ticket && artifact.id() == Some(ticket)
                }) {
                    return Err(error(format!("no ticket record `{ticket}` in the project")));
                }
                Some(ticket)
            }
            (_, Some(_)) => {
                return Err(error(format!(
                    "the kind `{}` is living and belongs to no ticket",
                    new.kind
                )));
            }
            (_, None) => None,
        };
    if let Some(missing) = new.supersedes.iter().find(|superseded| {
        !directory.artifacts.iter().any(|artifact| {
            artifact.place != Place::Ticket && artifact.id() == Some(superseded.as_str())
        })
    }) {
        return Err(error(format!(
            "`{missing}` is no artifact of the project to supersede"
        )));
    }
    let author = match (lifecycle, new.author.as_deref()) {
        ("decided", None | Some("")) => {
            return Err(error(
                "no author: pass --author, or set an author in the project's context",
            ));
        }
        (_, author) => author,
    };
    let (id, path) = loop {
        let id = format!("{prefix}-{}", random_part());
        let path = pattern
            .replace("{id}", &id)
            .replace("{slug}", &slug)
            .replace("{ticket}", ticket.unwrap_or_default());
        if project_root.join(&path).symlink_metadata().is_err() {
            break (id, path);
        }
    };
    check_relative(&path)?;
    let mut fields: Vec<(&str, String)> = vec![("id", quoted(&id)), ("type", quoted(&new.kind))];
    if lifecycle == "decided" {
        fields.push(("ticket", quoted(ticket.unwrap_or_default())));
        fields.push((
            "status",
            quoted(&first_status(definition).unwrap_or_default()),
        ));
        fields.push(("created", quoted(&today())));
        fields.push((
            "authors",
            format!("[{}]", quoted(author.unwrap_or_default())),
        ));
    }
    let requires = |field: &str| {
        definition["schema"]["required"]
            .as_array()
            .is_some_and(|required| required.iter().any(|name| name == field))
    };
    if requires("name") {
        fields.push(("name", quoted(title)));
    }
    if !new.supersedes.is_empty() {
        let ids: Vec<String> = new.supersedes.iter().map(|id| quoted(id)).collect();
        fields.push(("links", format!("{{ supersedes: [{}] }}", ids.join(", "))));
    }
    let markdown = path.ends_with(".md");
    let mut front = String::new();
    for (key, value) in &fields {
        front.push_str(&format!("{key}: {value}\n"));
    }
    if !new.anchors.is_empty() {
        front.push_str("anchors:\n");
        for (kind, reference) in &new.anchors {
            front.push_str(&format!(
                "  - {{ kind: {}, ref: {} }}\n",
                quoted(kind),
                quoted(reference)
            ));
        }
    }
    let text = if markdown {
        format!("---\n{front}---\n# {title}\n")
    } else {
        front
    };
    let findings = check_text(config, &path, text.as_bytes())?;
    let written = written(&path, text.as_bytes(), findings);
    if written.saved {
        below::create_new(&project_root.join(&path), project_root, text.as_bytes())
            .map_err(|io| error(format!("{path}: {io}")))?;
    }
    Ok(written)
}

/// The source file `path` (relative to `project_root`), and with `symbol` its line from the
/// language plugin that handles the file, as `module-anchors-resolve` resolves it.
///
/// # Errors
///
/// Returns a [`CheckError`] if the path is not relative, leaves the root, is no regular file,
/// is larger than 4 MiB, or is not UTF-8.
pub fn show_file(
    project_root: &Path,
    path: &str,
    symbol: Option<&str>,
) -> Result<SourceFile, CheckError> {
    check_relative(path)?;
    let text = read_below(project_root, path)?;
    let mut file = SourceFile {
        path: path.to_owned(),
        text,
        symbol: symbol.map(str::to_owned),
        language: None,
        resolved: None,
        line: None,
        message: None,
    };
    let Some(symbol) = symbol.filter(|symbol| !symbol.is_empty()) else {
        return Ok(file);
    };
    let current = Resolved::load(project_root)?;
    let config = current.document();
    let languages = languages(config);
    let Some(plugin) = language_of(&languages, path) else {
        file.resolved = Some("unchecked".into());
        file.message = Some(format!("no language plugin handles `{path}`"));
        return Ok(file);
    };
    file.language = Some(plugin.to_owned());
    let lock = read_lock(project_root);
    let host = Host::new(project_root, None, lock);
    let input = json!({
        "protocol": 1,
        "state": "project",
        "symbols": [{ "path": path, "symbol": symbol }],
    });
    match host.language(config, plugin, LanguageCall::Resolve, &input, 1) {
        Ok(results) => {
            let result = &results[0];
            file.resolved = result["status"].as_str().map(str::to_owned);
            file.line = result["line"].as_u64();
            file.message = match result["status"].as_str() {
                Some("missing") => Some(format!("no symbol `{symbol}` in `{path}`")),
                Some("unparsable") => result["message"].as_str().map(str::to_owned),
                _ => None,
            };
        }
        Err(finding) => {
            file.resolved = Some("failed".into());
            file.message = Some(finding.message);
        }
    }
    Ok(file)
}

fn error(message: impl Into<String>) -> CheckError {
    CheckError {
        message: message.into(),
    }
}

fn statuses(definition: &Value) -> Vec<KindStatus> {
    definition["statuses"]
        .as_object()
        .into_iter()
        .flatten()
        .map(|(name, status)| KindStatus {
            name: name.clone(),
            binding: status["binding"] == Value::Bool(true),
        })
        .collect()
}

// The resolved configuration keeps statuses as a map, so the order of the source is gone:
// `proposed` when the kind has it, otherwise the first status by name that does not bind, so a
// new artifact never starts binding, otherwise the first status by name.
fn first_status(definition: &Value) -> Option<String> {
    let statuses = statuses(definition);
    statuses
        .iter()
        .find(|status| status.name == "proposed")
        .or_else(|| statuses.iter().find(|status| !status.binding))
        .or_else(|| statuses.first())
        .map(|status| status.name.clone())
}

fn check_relative(path: &str) -> Result<(), CheckError> {
    let relative = Path::new(path);
    if path.is_empty()
        || relative
            .components()
            .any(|component| !matches!(component, Component::Normal(_)))
    {
        return Err(error(format!(
            "`{path}` is not a path relative to the project root"
        )));
    }
    Ok(())
}

fn check_text(config: &Value, path: &str, bytes: &[u8]) -> Result<Vec<Finding>, CheckError> {
    if Directory::place_of(config, path) == Some(Place::Ticket) {
        return Err(error(format!(
            "`{path}` is a ticket record: change it with hone ticket set-attributes"
        )));
    }
    match Directory::single(config, path, bytes) {
        None => Err(error(format!("`{path}` matches no artifact pattern"))),
        Some(Ok(artifact)) => Ok(check_file(config, &artifact)),
        Some(Err(violations)) => {
            let mut findings: Vec<Finding> = violations
                .into_iter()
                .map(|violation| {
                    Finding::error(crate::finding::Code::YamlProfile, violation).at_path(path)
                })
                .collect();
            findings.sort();
            Ok(findings)
        }
    }
}

fn written(path: &str, bytes: &[u8], findings: Vec<Finding>) -> ArtifactWritten {
    let id = crate::artifact::parse(path, bytes)
        .ok()
        .and_then(|content| {
            content
                .data
                .get("id")
                .and_then(Value::as_str)
                .map(str::to_owned)
        });
    ArtifactWritten {
        id,
        path: path.to_owned(),
        saved: !findings.iter().any(|finding| finding.level == Level::Error),
        findings,
    }
}

fn read_below(project_root: &Path, path: &str) -> Result<String, CheckError> {
    let root = project_root
        .canonicalize()
        .map_err(|io| error(format!("{}: {io}", project_root.display())))?;
    let file = crate::log::open_below(&root.join(path), &root, false)
        .map_err(|io| error(format!("{path}: {io}")))?
        .ok_or_else(|| {
            error(format!(
                "`{path}` is not a regular file below the project root"
            ))
        })?;
    let length = file
        .metadata()
        .map_err(|io| error(format!("{path}: {io}")))?
        .len();
    if length > LARGEST_FILE {
        return Err(error(format!("`{path}` is larger than 4 MiB")));
    }
    let mut bytes = Vec::new();
    std::io::Read::read_to_end(&mut &file, &mut bytes)
        .map_err(|io| error(format!("{path}: {io}")))?;
    String::from_utf8(bytes).map_err(|_| error(format!("`{path}` is not UTF-8")))
}

fn read_lock(project_root: &Path) -> Lock {
    let directory: PathBuf = project_root.join(".hone");
    read_lock_file(&directory, "lock.yaml").unwrap_or_default()
}

fn quoted(text: &str) -> String {
    serde_json::to_string(text).expect("a string serializes")
}

/// Lower-case letters and digits of the title, other characters as single dashes, at most 48
/// characters, without dashes at either end.
fn slug(title: &str) -> String {
    let mut slug = String::new();
    for character in title.to_lowercase().chars() {
        if character.is_ascii_alphanumeric() {
            slug.push(character);
        } else if !slug.ends_with('-') && !slug.is_empty() {
            slug.push('-');
        }
    }
    let mut slug: String = slug.chars().take(48).collect();
    while slug.ends_with('-') {
        slug.pop();
    }
    slug
}

fn random_part() -> String {
    let mut bytes = [0u8; 8];
    getrandom::fill(&mut bytes).expect("the operating system provides random bytes");
    bytes
        .iter()
        .map(|byte| char::from(CROCKFORD[usize::from(byte & 0x1f)]))
        .collect()
}

fn today() -> String {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since| since.as_millis());
    crate::condense::date_of_millis(u64::try_from(millis).unwrap_or(0))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_slug_keeps_letters_and_digits() {
        assert_eq!(
            slug("Payments run asynchronously!"),
            "payments-run-asynchronously"
        );
        assert_eq!(slug("  Über 3 Wege  "), "ber-3-wege");
        assert_eq!(slug("***"), "");
    }

    #[test]
    fn the_first_status_does_not_bind() {
        let kind = json!({ "statuses": { "accepted": { "binding": true }, "draft": { "binding": false }, "proposed": { "binding": false } } });
        assert_eq!(first_status(&kind).as_deref(), Some("proposed"));
        let kind = json!({ "statuses": { "accepted": { "binding": true }, "draft": { "binding": false } } });
        assert_eq!(first_status(&kind).as_deref(), Some("draft"));
        let kind = json!({ "statuses": { "accepted": { "binding": true } } });
        assert_eq!(first_status(&kind).as_deref(), Some("accepted"));
    }
}
