//! The commands of a ticket (concept Section 12, "Process commands"): start it, change its
//! attributes, switch it to the current rules. Each writes the ticket record, a snapshot where
//! needed, and one event.

use std::path::{Path, PathBuf};

use serde::Serialize;
use serde_json::{Map, Value, json};

use crate::below;
use crate::check::{check_artifact, load_ticket};
use crate::event::{self, TICKET_ATTRIBUTES_CHANGED, TICKET_CONFIG_CHANGED, TICKET_STARTED};
use crate::finding::Level;
use crate::process::Prepared;
use crate::resolved_file::{ResolvedFile, compare_resolved};
use crate::{Author, CheckError, PackageSource, ProcessError, ProcessStore, Resolved};

/// What a new ticket starts with.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TicketStart {
    /// The attributes for profile selection: strings, integers, or booleans.
    pub attributes: Map<String, Value>,
    /// The tickets of external trackers it belongs to.
    pub external: Vec<External>,
    /// A profile chosen instead of the result of `select`.
    pub profile: Option<String>,
}

/// A ticket of an external tracker, an entry of the record's `external`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct External {
    /// The tracker, for example `github` or `jira`.
    pub system: String,
    /// The ticket's key in the tracker, for example `8798` or `PAY-12`.
    pub key: String,
    /// The ticket's address.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
}

/// The result of a command of a ticket: the record as written and the event.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct TicketWritten {
    /// The ticket ID.
    pub ticket: String,
    /// The record's path, relative to the project root.
    pub path: String,
    /// The ticket's profile.
    pub profile: String,
    /// The profile `select` gives for the attributes under the ticket's configuration.
    pub selected: Option<String>,
    /// The hash of the ticket's snapshot.
    pub config: String,
    /// The ticket's attributes.
    pub attributes: Map<String, Value>,
    /// The event as it was written.
    pub event: Value,
}

impl ProcessStore {
    /// Starts a ticket: picks an unused ticket ID, writes the ticket record and the snapshot of
    /// the current resolved configuration, and the event `ticket.started` by `author`. The
    /// profile is the result of `select` unless `start.profile` names another one.
    ///
    /// # Errors
    ///
    /// Returns [`ProcessError::Invalid`] if an attribute, an external ticket, or the profile is
    /// invalid, the author's role does not exist, or `.hone/resolved.json` does not match its
    /// sources, all before anything is written, and [`ProcessError::IoError`] if a file
    /// cannot be read or written.
    pub fn start_ticket(
        &self,
        author: &Author,
        packages: &dyn PackageSource,
        start: TicketStart,
    ) -> Result<TicketWritten, ProcessError> {
        check_attributes(&start.attributes)?;
        for external in &start.external {
            if external.system.is_empty() || external.key.is_empty() {
                return Err(invalid("an external ticket needs a system and a key"));
            }
        }
        let root = self.root();
        match compare_resolved(root, packages) {
            Ok(ResolvedFile::UpToDate { .. }) => {}
            Ok(_) => {
                return Err(invalid(
                    ".hone/resolved.json does not match its sources: run hone resolve",
                ));
            }
            Err(error) => return Err(invalid(error.to_string())),
        }
        let current = Resolved::load(root)?;
        let document = current.document();
        let selected = selected_profile(document, &start.attributes);
        let profile = match start.profile.or_else(|| selected.clone()) {
            Some(profile) => profile,
            None => return Err(invalid("`select` gives no profile for these attributes")),
        };
        if document["profiles"].get(&profile).is_none() {
            return Err(invalid(format!(
                "the configuration has no profile `{profile}`"
            )));
        }
        check_role(document, author)?;

        let config = current.hash();
        let (ticket, path) = loop {
            let ticket = event::new_ticket_id();
            let path = record_path(document, &ticket);
            if !root.join(&path).exists() {
                break (ticket, path);
            }
        };
        let record = [
            ("id", ticket.as_str()),
            ("type", "ticket"),
            ("project", self.project()),
            ("profile", profile.as_str()),
            ("config", config.as_str()),
        ];
        let bytes = render_record(&record, &start.attributes, &start.external);
        check_record(&current, &path, &bytes)?;

        let snapshot = snapshot_path(root, &config);
        let new_snapshot = !snapshot.exists();
        if new_snapshot {
            below::create_new(&snapshot, root, &current.file_contents()).map_err(io(&snapshot))?;
        }
        let file = root.join(&path);
        below::create_new(&file, root, &bytes).map_err(io(&file))?;
        let payload = object(json!({ "profile": profile, "config": config }));
        let appended = self.write_event(author, prepared(&ticket, TICKET_STARTED, payload));
        let event = match appended {
            Ok(appended) => appended.event,
            Err(error) => {
                let _ = below::remove(&file, root);
                if new_snapshot {
                    let _ = below::remove(&snapshot, root);
                }
                return Err(error);
            }
        };
        Ok(TicketWritten {
            ticket,
            path,
            profile,
            selected,
            config,
            attributes: start.attributes,
            event,
        })
    }

    /// Changes the attributes of `ticket`: sets `set`, removes `unset`, writes the record and
    /// the event `ticket.attributes_changed` with every attribute after the change. The
    /// profile stays.
    ///
    /// # Errors
    ///
    /// Returns [`ProcessError::Invalid`] if the ticket does not exist, an attribute is
    /// invalid, or the author's role does not exist in the ticket's snapshot, before anything
    /// is written, and [`ProcessError::IoError`] if a file cannot be read or written.
    pub fn set_attributes(
        &self,
        author: &Author,
        ticket: &str,
        set: Map<String, Value>,
        unset: &[String],
    ) -> Result<TicketWritten, ProcessError> {
        check_attributes(&set)?;
        let (record, snapshot) = load_ticket(self.root(), ticket).map_err(not_found)?;
        check_role(snapshot.document(), author)?;
        let mut attributes = record
            .get("attributes")
            .unwrap_or(&Value::Null)
            .as_object()
            .cloned()
            .unwrap_or_default();
        for key in unset {
            attributes.remove(key);
        }
        attributes.extend(set);
        let payload = object(json!({ "attributes": attributes }));
        let value = (!attributes.is_empty()).then(|| render_attributes(&attributes));
        self.rewrite(
            author,
            ticket,
            "attributes",
            value,
            &snapshot,
            payload,
            TICKET_ATTRIBUTES_CHANGED,
        )
    }

    /// Switches `ticket` to the current rules: resolves the configuration with the ticket's
    /// `overrides`, writes it as a snapshot, its hash into the record's `config`, and the event
    /// `ticket.config_changed`. The profile stays and must exist in the new configuration.
    ///
    /// # Errors
    ///
    /// Returns [`ProcessError::Invalid`] if the ticket does not exist, the configuration does
    /// not resolve or lacks the ticket's profile, or the author's role does not exist in it,
    /// before anything is written, and [`ProcessError::IoError`] if a file cannot be read or
    /// written.
    pub fn switch_config(
        &self,
        author: &Author,
        packages: &dyn PackageSource,
        ticket: &str,
    ) -> Result<TicketWritten, ProcessError> {
        let root = self.root();
        let (record, _) = load_ticket(root, ticket).map_err(not_found)?;
        let resolved = crate::resolve(root, packages, record.get("overrides"))
            .map_err(|error| invalid(error.to_string()))?;
        let profile = record
            .get("profile")
            .unwrap_or(&Value::Null)
            .as_str()
            .unwrap_or_default();
        if resolved.document()["profiles"].get(profile).is_none() {
            return Err(invalid(format!(
                "the current configuration has no profile `{profile}`, the ticket's profile"
            )));
        }
        check_role(resolved.document(), author)?;
        let config = resolved.hash();
        if record.get("config").and_then(Value::as_str) == Some(config.as_str()) {
            return Err(invalid(format!(
                "the ticket `{ticket}` already uses the current configuration"
            )));
        }
        let snapshot = snapshot_path(root, &config);
        if !snapshot.exists() {
            below::create_new(&snapshot, root, &resolved.file_contents()).map_err(io(&snapshot))?;
        }
        let payload = object(json!({ "config": config }));
        let value = Some(serde_json::to_string(&config).expect("a string serializes"));
        self.rewrite(
            author,
            ticket,
            "config",
            value,
            &resolved,
            payload,
            TICKET_CONFIG_CHANGED,
        )
    }

    // Replaces one top-level key of the record, then writes the event; if the event cannot be
    // written, the record gets its old bytes back.
    #[allow(clippy::too_many_arguments)]
    fn rewrite(
        &self,
        author: &Author,
        ticket: &str,
        key: &str,
        value: Option<String>,
        config: &Resolved,
        payload: Map<String, Value>,
        kind: &'static str,
    ) -> Result<TicketWritten, ProcessError> {
        let root = self.root();
        let path = record_path(Resolved::load(root)?.document(), ticket);
        let file = root.join(&path);
        let before = std::fs::read(&file).map_err(io(&file))?;
        let after = replace_key(&before, key, value.as_deref())
            .ok_or_else(|| invalid(format!("{path}: has carriage returns or is not UTF-8")))?;
        let record = check_record(config, &path, &after)?;
        below::replace(&file, root, &after).map_err(io(&file))?;
        let event = match self.write_event(author, prepared(ticket, kind, payload)) {
            Ok(appended) => appended.event,
            Err(error) => {
                let _ = below::replace(&file, root, &before);
                return Err(error);
            }
        };
        let attributes = record["attributes"]
            .as_object()
            .cloned()
            .unwrap_or_default();
        Ok(TicketWritten {
            ticket: ticket.to_owned(),
            path,
            profile: record["profile"].as_str().unwrap_or_default().to_owned(),
            selected: selected_profile(config.document(), &attributes),
            config: record["config"].as_str().unwrap_or_default().to_owned(),
            attributes,
            event,
        })
    }
}

/// What `select` of the current configuration gives for some attributes, with the rules it
/// chose from.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ProfileSelection {
    /// The selected profile.
    pub selected: Option<String>,
    /// The position of the rule that matched, counted from 0, or `None` when `default` applies.
    pub rule: Option<usize>,
    /// The rules of `select`, `{ when, profile }`, in their order.
    pub rules: Vec<Value>,
    /// The profile of `select` when no rule matches.
    pub default: Option<String>,
    /// The profiles of the configuration, sorted.
    pub profiles: Vec<String>,
}

/// The profile `select` of the current resolved configuration of `project_root` gives for
/// `attributes`, without starting a ticket.
///
/// # Errors
///
/// Returns a [`CheckError`] if `.hone/resolved.json` cannot be read.
pub fn select_profile(
    project_root: &Path,
    attributes: &Map<String, Value>,
) -> Result<ProfileSelection, CheckError> {
    let current = Resolved::load(project_root)?;
    let config = current.document();
    Ok(ProfileSelection {
        selected: selected_profile(config, attributes),
        rule: matching_rule(config, attributes),
        rules: config["select"]["rules"]
            .as_array()
            .cloned()
            .unwrap_or_default(),
        default: config["select"]["default"].as_str().map(str::to_owned),
        profiles: config["profiles"]
            .as_object()
            .map(|profiles| profiles.keys().cloned().collect())
            .unwrap_or_default(),
    })
}

/// The position of the first rule of `select` whose `when` matches every attribute it names, a
/// scalar by equality and a list by set membership.
fn matching_rule(config: &Value, attributes: &Map<String, Value>) -> Option<usize> {
    let matches = |when: &Map<String, Value>| {
        when.iter().all(|(key, wanted)| {
            let Some(actual) = attributes.get(key) else {
                return false;
            };
            match wanted {
                Value::Array(set) => set.contains(actual),
                scalar => scalar == actual,
            }
        })
    };
    config["select"]["rules"]
        .as_array()?
        .iter()
        .position(|rule| rule["when"].as_object().is_some_and(matches))
}

/// The profile `select` of `config` gives for `attributes`: the profile of the first matching
/// rule, otherwise `default`.
pub(crate) fn selected_profile(config: &Value, attributes: &Map<String, Value>) -> Option<String> {
    let select = &config["select"];
    match matching_rule(config, attributes) {
        Some(index) => select["rules"][index]["profile"].as_str(),
        None => select["default"].as_str(),
    }
    .map(str::to_owned)
}

fn check_attributes(attributes: &Map<String, Value>) -> Result<(), ProcessError> {
    for (key, value) in attributes {
        if key.is_empty() {
            return Err(invalid("an attribute needs a name"));
        }
        if !(value.is_string() || value.is_i64() || value.is_u64() || value.is_boolean()) {
            return Err(invalid(format!(
                "the attribute `{key}` must be a string, an integer, or a boolean"
            )));
        }
    }
    Ok(())
}

fn check_role(config: &Value, author: &Author) -> Result<(), ProcessError> {
    if config["roles"].get(&author.role).is_none() {
        return Err(invalid(format!(
            "the role `{}` does not exist in the ticket's configuration",
            author.role
        )));
    }
    Ok(())
}

// The record is checked like any artifact before it is written, so a command never leaves a
// record behind that `schema-valid` rejects.
fn check_record(config: &Resolved, path: &str, bytes: &[u8]) -> Result<Value, ProcessError> {
    let findings = check_artifact(config, path, bytes);
    if let Some(error) = findings
        .iter()
        .find(|finding| finding.level == Level::Error)
    {
        return Err(invalid(format!("{path}: {}", error.message)));
    }
    crate::parse_yaml(bytes).map_err(|error| invalid(format!("{path}: {error}")))
}

fn record_path(config: &Value, ticket: &str) -> String {
    config["ticket"]["path"]
        .as_str()
        .unwrap_or_default()
        .replace("{ticket}", ticket)
}

fn snapshot_path(root: &Path, config: &str) -> PathBuf {
    let hex = config.strip_prefix("sha256:").unwrap_or(config);
    root.join(".hone")
        .join("snapshots")
        .join(format!("{hex}.json"))
}

// Double-quoted strings and flow maps keep the record inside the strict YAML profile whatever
// a value holds. JSON string escapes are valid YAML double-quoted escapes.
fn quote(text: &str) -> String {
    serde_json::to_string(text).expect("a string serializes")
}

fn scalar(value: &Value) -> String {
    match value {
        Value::String(text) => quote(text),
        other => other.to_string(),
    }
}

fn render_attributes(attributes: &Map<String, Value>) -> String {
    let fields: Vec<String> = attributes
        .iter()
        .map(|(key, value)| format!("{}: {}", quote(key), scalar(value)))
        .collect();
    format!("{{ {} }}", fields.join(", "))
}

fn render_record(
    record: &[(&str, &str)],
    attributes: &Map<String, Value>,
    external: &[External],
) -> Vec<u8> {
    let mut text = String::new();
    for (key, value) in record {
        text.push_str(&format!("{key}: {}\n", quote(value)));
    }
    if !attributes.is_empty() {
        text.push_str(&format!("attributes: {}\n", render_attributes(attributes)));
    }
    if !external.is_empty() {
        text.push_str("external:\n");
        for entry in external {
            let mut fields = vec![
                format!("system: {}", quote(&entry.system)),
                format!("key: {}", quote(&entry.key)),
            ];
            if let Some(url) = &entry.url {
                fields.push(format!("url: {}", quote(url)));
            }
            text.push_str(&format!("  - {{ {} }}\n", fields.join(", ")));
        }
    }
    text.into_bytes()
}

// Replaces the top-level `key:` and the lines that continue it with `key: value` at the same
// place, or appends it at the end when the key is new, so every other byte of the record stays.
// `None` only removes the key.
fn replace_key(bytes: &[u8], key: &str, value: Option<&str>) -> Option<Vec<u8>> {
    let text = std::str::from_utf8(bytes).ok()?;
    if text.contains('\r') {
        return None;
    }
    let replacement = value.map(|value| format!("{key}: {value}\n"));
    let mut kept = String::new();
    let mut inside = false;
    let mut replaced = false;
    for line in text.split_inclusive('\n') {
        let bare = line.trim_end_matches('\n');
        let starts = bare
            .strip_prefix(key)
            .is_some_and(|rest| rest.starts_with(':'));
        if starts {
            inside = true;
            if let Some(replacement) = replacement.as_deref().filter(|_| !replaced) {
                kept.push_str(replacement);
            }
            replaced = true;
            continue;
        }
        if inside && (bare.is_empty() || bare.starts_with([' ', '\t', '-'])) {
            continue;
        }
        inside = false;
        kept.push_str(line);
    }
    if let Some(replacement) = replacement.filter(|_| !replaced) {
        if !kept.is_empty() && !kept.ends_with('\n') {
            kept.push('\n');
        }
        kept.push_str(&replacement);
    }
    Some(kept.into_bytes())
}

fn prepared(ticket: &str, kind: &'static str, payload: Map<String, Value>) -> Prepared {
    Prepared {
        ticket: ticket.to_owned(),
        kind,
        payload,
        thread: None,
    }
}

fn object(value: Value) -> Map<String, Value> {
    match value {
        Value::Object(map) => map,
        _ => Map::new(),
    }
}

fn invalid(message: impl Into<String>) -> ProcessError {
    ProcessError::Invalid {
        message: message.into(),
    }
}

// A missing ticket is the caller's mistake, not a broken store.
fn not_found(error: crate::CheckError) -> ProcessError {
    invalid(error.message)
}

fn io(path: &Path) -> impl Fn(std::io::Error) -> ProcessError + '_ {
    move |error| ProcessError::IoError {
        message: format!("{}: {error}", path.display()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn select_takes_the_first_matching_rule_then_the_default() {
        let config = json!({ "select": {
            "rules": [
                { "when": { "risk": "high" }, "profile": "strict" },
                { "when": { "size": [3, 5], "team": "payments" }, "profile": "careful" },
            ],
            "default": "standard",
        }});
        let attributes = |value: Value| object(value);
        assert_eq!(
            selected_profile(&config, &attributes(json!({ "risk": "high", "size": 3 }))).as_deref(),
            Some("strict")
        );
        assert_eq!(
            selected_profile(
                &config,
                &attributes(json!({ "size": 5, "team": "payments" }))
            )
            .as_deref(),
            Some("careful")
        );
        assert_eq!(
            selected_profile(
                &config,
                &attributes(json!({ "size": "5", "team": "payments" }))
            )
            .as_deref(),
            Some("standard"),
            "`5` and `\"5\"` differ"
        );
        assert_eq!(
            selected_profile(&config, &attributes(json!({ "size": 3 }))).as_deref(),
            Some("standard"),
            "every attribute of `when` must match"
        );
    }

    #[test]
    fn replace_key_keeps_every_other_byte() {
        let record =
            b"id: \"tk-1\"\n# a comment\nattributes:\n  risk: \"low\"\n  size: 3\nprofile: \"x\"\n";
        let replaced =
            replace_key(record, "attributes", Some("{ \"risk\": \"high\" }")).expect("text");
        assert_eq!(
            String::from_utf8(replaced).expect("text"),
            "id: \"tk-1\"\n# a comment\nattributes: { \"risk\": \"high\" }\nprofile: \"x\"\n",
            "the value stays where the key was"
        );
        let removed = replace_key(record, "attributes", None).expect("text");
        assert_eq!(
            String::from_utf8(removed).expect("text"),
            "id: \"tk-1\"\n# a comment\nprofile: \"x\"\n"
        );
        let added = replace_key(b"id: \"tk-1\"", "attributes", Some("{ \"a\": 1 }")).expect("text");
        assert_eq!(
            String::from_utf8(added).expect("text"),
            "id: \"tk-1\"\nattributes: { \"a\": 1 }\n",
            "a new key goes to the end"
        );
        assert!(replace_key(b"a: 1\r\n", "a", None).is_none());
    }
}
