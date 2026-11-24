use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use serde::Serialize;
use serde_json::{Map, Value};

use crate::Resolved;
use crate::check::{CheckError, load_ticket};
use crate::directory::{Artifact, Directory, Place};
use crate::gates::{ID_LINKS, Scope};
use crate::manifest;

/// The artifacts of a project's checked directory.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Catalog {
    /// The project ID from `.hone/config.yaml`.
    pub project: String,
    /// Every artifact that could be read, ticket records included, in the order of their paths.
    pub artifacts: Vec<CatalogEntry>,
    /// The files that match an artifact pattern but cannot be read.
    pub unreadable: Vec<UnreadableFile>,
}

/// One artifact of a [`Catalog`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CatalogEntry {
    /// The artifact's ID, if its data has one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// The kind, or `ticket` for a ticket record, taken from the path.
    #[serde(rename = "type")]
    pub kind: String,
    /// The path relative to the project root.
    pub path: String,
    /// The ticket the artifact belongs to, if it names one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ticket: Option<String>,
    /// The field `title`, otherwise the first `# ` heading of a Markdown body.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// The declared status.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
    /// The effective status: the declared one, or `superseded`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub effective: Option<String>,
    /// A ticket record's profile.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub profile: Option<String>,
    /// A ticket record's external tickets.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub external: Vec<ExternalTicket>,
    /// The ID links under `links`, by link type (`supersedes`, `affects`, `verifies`,
    /// `depends-on`, `relates`), each with the IDs it names.
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub links: BTreeMap<String, Vec<String>>,
}

/// A file that matches an artifact pattern but cannot be read.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct UnreadableFile {
    /// The path relative to the project root.
    pub path: String,
    /// Why it cannot be read.
    pub message: String,
}

/// An entry of a ticket record's `external`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ExternalTicket {
    /// The tracker, for example `jira`.
    pub system: String,
    /// The key in the tracker, for example `PAY-123`.
    pub key: String,
    /// The address of the ticket in the tracker.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
}

/// A ticket with the steps of its profile, from the ticket record and its snapshot.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TicketOverview {
    /// The ticket ID.
    pub ticket: String,
    /// The path of the ticket record relative to the project root.
    pub path: String,
    /// The ticket record's profile.
    pub profile: String,
    /// The profile `select` of the snapshot gives for the record's attributes; it differs from
    /// `profile` when the profile was chosen by hand.
    pub selected: Option<String>,
    /// The hash of the snapshot, `sha256:<hex>`.
    pub config: String,
    /// The ticket record's attributes.
    pub attributes: Map<String, Value>,
    /// The ticket record's external tickets.
    pub external: Vec<ExternalTicket>,
    /// The roles of the snapshot, sorted.
    pub roles: Vec<String>,
    /// The steps of the profile, every step after the steps that come before it.
    pub steps: Vec<ProfileStep>,
}

/// A step of a ticket's profile.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ProfileStep {
    /// The step's name.
    pub step: String,
    /// The step's `title`, if it has one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// `required` or `optional`, as the profile lists it.
    pub requirement: String,
    /// The steps of the profile that come before it: those it `requires`, and those whose
    /// outputs are its inputs. Sorted.
    pub after: Vec<String>,
}

/// Lists the artifacts of the checked directory `project_root` under its current resolved
/// configuration.
///
/// # Errors
///
/// Returns a [`CheckError`] if `.hone/config.yaml` or `.hone/resolved.json` cannot be read.
pub fn catalog(project_root: &Path) -> Result<Catalog, CheckError> {
    let project = manifest::read_scope(&project_root.join(".hone"))
        .map_err(|message| CheckError { message })?
        .1;
    let current = Resolved::load(project_root)?;
    let config = current.document();
    let directory = Directory::scan(project_root, config);
    let scope = Scope::new(config, &directory);
    let mut artifacts: Vec<CatalogEntry> = directory
        .artifacts
        .iter()
        .enumerate()
        .map(|(index, artifact)| entry(artifact, scope.effective(index)))
        .collect();
    artifacts.sort_by(|a, b| a.path.cmp(&b.path));
    let mut unreadable: Vec<UnreadableFile> = directory
        .unreadable
        .into_iter()
        .map(|file| UnreadableFile {
            path: file.path,
            message: file.violations.join("; "),
        })
        .collect();
    unreadable.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(Catalog {
        project,
        artifacts,
        unreadable,
    })
}

/// The ticket `ticket` of the project in `project_root`, with the steps of its profile from the
/// ticket's snapshot.
///
/// # Errors
///
/// Returns a [`CheckError`] if `.hone/resolved.json`, the ticket record, or its snapshot is
/// missing or invalid.
pub fn ticket_overview(project_root: &Path, ticket: &str) -> Result<TicketOverview, CheckError> {
    let (record, snapshot) = load_ticket(project_root, ticket)?;
    let current = Resolved::load(project_root)?;
    let path = current.document()["ticket"]["path"]
        .as_str()
        .unwrap_or_default()
        .replace("{ticket}", ticket);
    let config = snapshot.document();
    let profile = record
        .get("profile")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned();
    let roles = config["roles"]
        .as_object()
        .map(|roles| roles.keys().cloned().collect())
        .unwrap_or_default();
    let record_attributes = record
        .get("attributes")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    Ok(TicketOverview {
        ticket: ticket.to_owned(),
        path,
        steps: profile_steps(config, &profile),
        config: record
            .get("config")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned(),
        attributes: record_attributes.clone(),
        external: external(&record),
        selected: crate::ticket::selected_profile(config, &record_attributes),
        profile,
        roles,
    })
}

fn entry(artifact: &Artifact, effective: Option<&str>) -> CatalogEntry {
    let text = |key: &str| artifact.field(key).map(str::to_owned);
    let is_ticket = artifact.place == Place::Ticket;
    CatalogEntry {
        id: text("id"),
        kind: artifact.place.type_name().to_owned(),
        path: artifact.path.clone(),
        ticket: if is_ticket {
            None
        } else {
            text("ticket").or_else(|| artifact.ticket_in_path.clone())
        },
        title: text("title").or_else(|| heading(&artifact.content.body)),
        status: text("status"),
        effective: effective.map(str::to_owned),
        profile: if is_ticket { text("profile") } else { None },
        external: if is_ticket {
            external(&artifact.content.data)
        } else {
            Vec::new()
        },
        links: id_links(&artifact.content.data),
    }
}

fn id_links(data: &Map<String, Value>) -> BTreeMap<String, Vec<String>> {
    ID_LINKS
        .iter()
        .filter_map(|&link| {
            let ids: Vec<String> = data
                .get("links")?
                .get(link)?
                .as_array()?
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_owned)
                .collect();
            (!ids.is_empty()).then(|| (link.to_owned(), ids))
        })
        .collect()
}

fn heading(body: &str) -> Option<String> {
    body.lines()
        .find_map(|line| line.strip_prefix("# "))
        .map(|title| title.trim().to_owned())
        .filter(|title| !title.is_empty())
}

fn external(record: &Map<String, Value>) -> Vec<ExternalTicket> {
    record
        .get("external")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|entry| {
            Some(ExternalTicket {
                system: entry["system"].as_str()?.to_owned(),
                key: entry["key"].as_str()?.to_owned(),
                url: entry["url"].as_str().map(str::to_owned),
            })
        })
        .collect()
}

// spec/gates.md item 4: the order follows from inputs and outputs, `requires` adds to it.
// Steps that no order separates keep the order of their names, and a cycle of inputs and
// outputs puts its steps at the end, in the order of their names.
pub(crate) fn profile_steps(config: &Value, profile: &str) -> Vec<ProfileStep> {
    let listed: BTreeMap<&str, &str> = config["profiles"][profile]["steps"]
        .as_object()
        .into_iter()
        .flatten()
        .filter_map(|(step, requirement)| Some((step.as_str(), requirement.as_str()?)))
        .collect();
    let strings = |value: &Value| -> BTreeSet<String> {
        value
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .map(str::to_owned)
            .collect()
    };
    let before: BTreeMap<&str, BTreeSet<String>> = listed
        .keys()
        .map(|&step| {
            let definition = &config["steps"][step];
            let inputs = strings(&definition["inputs"]);
            let mut earlier: BTreeSet<String> = strings(&definition["requires"])
                .into_iter()
                .filter(|other| listed.contains_key(other.as_str()))
                .collect();
            earlier.extend(
                listed
                    .keys()
                    .filter(|&&other| other != step)
                    .filter(|&&other| {
                        strings(&config["steps"][other]["outputs"])
                            .iter()
                            .any(|output| inputs.contains(output))
                    })
                    .map(|&other| other.to_owned()),
            );
            (step, earlier)
        })
        .collect();
    let mut placed: Vec<&str> = Vec::new();
    while placed.len() < listed.len() {
        let ready = listed.keys().copied().find(|step| {
            !placed.contains(step)
                && before[step]
                    .iter()
                    .all(|earlier| placed.contains(&earlier.as_str()))
        });
        match ready {
            Some(step) => placed.push(step),
            None => {
                let rest: Vec<&str> = listed
                    .keys()
                    .copied()
                    .filter(|step| !placed.contains(step))
                    .collect();
                placed.extend(rest);
            }
        }
    }
    placed
        .into_iter()
        .map(|step| ProfileStep {
            step: step.to_owned(),
            title: config["steps"][step]["title"].as_str().map(str::to_owned),
            requirement: listed[step].to_owned(),
            after: before[step].iter().cloned().collect(),
        })
        .collect()
}
