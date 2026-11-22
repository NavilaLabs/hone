//! Showing the resolved configuration with the layers it comes from (concept Section 12,
//! "Showing the resolved configuration"). The origins are tool data and never part of the
//! resolved configuration.

use std::collections::BTreeMap;
use std::path::Path;

use serde::Serialize;
use serde_json::{Map, Value};

use crate::check::read_ticket;
use crate::layer::{DEFINITION_DIRECTORIES, SUBTREE_FILES};
use crate::resolved_file::matches_file;
use crate::{Origin, PackageSource, Problem, manifest, resolve, resolve_with_origins};

/// The resolved configuration from its sources, with the layers of every definition and the
/// lockfile.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "status", rename = "shown")]
pub struct Shown {
    /// The hash of the resolved configuration.
    pub hash: String,
    /// Whether the configuration matches `.hone/resolved.json`, or the ticket's snapshot.
    #[serde(flatten)]
    pub comparison: Comparison,
    /// The resolved configuration.
    pub config: Value,
    /// Per definition of `kinds`, `roles`, `steps`, `profiles`, and `plugins`, and per subtree
    /// `select`, `actors`, and `ticket`, the layers that contributed to it, from bottom to top.
    pub origins: Value,
    /// The content of `.hone/lock.yaml`, `{}` without the file.
    pub lock: Value,
}

/// What the shown configuration is compared with.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Comparison {
    /// `.hone/resolved.json`, outside a ticket.
    File(FileState),
    /// The snapshot that the ticket record names in `config`.
    Snapshot(SnapshotState),
}

/// Whether `.hone/resolved.json` holds the shown configuration.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum FileState {
    /// The file holds exactly the configuration.
    UpToDate,
    /// The file is missing or differs.
    OutOfDate,
}

/// Whether the ticket's snapshot is the shown configuration.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum SnapshotState {
    /// The record's `config` is the hash of the configuration.
    Same,
    /// A switch of the configuration would change the snapshot.
    Differs,
}

/// [`show_resolved`] could not show a configuration.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "status", rename_all = "kebab-case")]
pub enum ShowError {
    /// The resolution failed.
    Failed {
        /// The problems of the first resolution step that found any.
        problems: Vec<Problem>,
        /// The content of `.hone/lock.yaml`, absent when the lockfile is malformed.
        #[serde(skip_serializing_if = "Option::is_none")]
        lock: Option<Value>,
    },
    /// The ticket does not exist or its record cannot be read.
    Invalid {
        /// The cause.
        message: String,
    },
}

impl std::fmt::Display for ShowError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Failed { problems, .. } => {
                let error = crate::ResolveError {
                    problems: problems.clone(),
                };
                write!(formatter, "{error}")
            }
            Self::Invalid { message } => formatter.write_str(message),
        }
    }
}

impl std::error::Error for ShowError {}

/// Resolves the configuration of the project in `project_root` from its sources, without
/// writing anything, and returns it with the layers of every definition and the lockfile.
/// With `ticket`, the ticket's `overrides` are applied as the top layer, and the result is
/// compared with the ticket's snapshot instead of `.hone/resolved.json`.
///
/// # Errors
///
/// Returns [`ShowError::Failed`] if the resolution fails and [`ShowError::Invalid`] if the
/// ticket does not exist or its record cannot be read.
pub fn show_resolved(
    project_root: &Path,
    packages: &dyn PackageSource,
    ticket: Option<&str>,
) -> Result<Shown, ShowError> {
    let lock = manifest::lock_content(&project_root.join(".hone"));
    let failed = |error: crate::ResolveError| ShowError::Failed {
        problems: error.problems,
        lock: lock.clone(),
    };
    let record = match ticket {
        None => None,
        Some(ticket) => {
            // The record's path comes from the current sources, not from a stale
            // `resolved.json`.
            let current = resolve(project_root, packages, None).map_err(failed)?;
            let (record, _) =
                read_ticket(project_root, current.document(), ticket).map_err(|error| {
                    ShowError::Invalid {
                        message: error.message,
                    }
                })?;
            Some(record)
        }
    };
    let overrides = record.as_ref().and_then(|record| record.get("overrides"));
    let (resolved, origins) =
        resolve_with_origins(project_root, packages, overrides).map_err(failed)?;
    let hash = resolved.hash();
    let comparison = match &record {
        None if matches_file(project_root, &resolved) => Comparison::File(FileState::UpToDate),
        None => Comparison::File(FileState::OutOfDate),
        Some(record) if record.get("config").and_then(Value::as_str) == Some(hash.as_str()) => {
            Comparison::Snapshot(SnapshotState::Same)
        }
        Some(_) => Comparison::Snapshot(SnapshotState::Differs),
    };
    let config = resolved.document().clone();
    Ok(Shown {
        hash,
        comparison,
        origins: origins.to_json(&config),
        config,
        lock: lock.unwrap_or_default(),
    })
}

/// A layer that contributes to the resolved configuration, in the form of [`Origin`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case", tag = "kind", content = "name")]
pub(crate) enum Contributor {
    Project,
    Package(String),
    Overrides,
}

impl From<&Origin> for Contributor {
    fn from(origin: &Origin) -> Self {
        match origin {
            Origin::Project => Self::Project,
            Origin::Package(name) => Self::Package(name.clone()),
        }
    }
}

#[derive(Default)]
pub(crate) struct Origins {
    definitions: BTreeMap<&'static str, BTreeMap<String, Vec<Contributor>>>,
    subtrees: BTreeMap<&'static str, Vec<Contributor>>,
}

impl Origins {
    // Records the definitions and subtrees that `tree`, the tree of `layer`, sets. A `null`
    // removes what the layers below contributed.
    pub(crate) fn record(&mut self, layer: &Contributor, tree: &Map<String, Value>) {
        for section in DEFINITION_DIRECTORIES {
            let origins = self.definitions.entry(section).or_default();
            match tree.get(section) {
                Some(Value::Null) => origins.clear(),
                Some(Value::Object(definitions)) => {
                    for (name, definition) in definitions {
                        if definition.is_null() {
                            origins.remove(name);
                        } else {
                            origins.entry(name.clone()).or_default().push(layer.clone());
                        }
                    }
                }
                _ => {}
            }
        }
        for (_, subtree) in SUBTREE_FILES {
            let origins = self.subtrees.entry(subtree).or_default();
            match tree.get(subtree) {
                Some(Value::Null) => origins.clear(),
                Some(_) => origins.push(layer.clone()),
                None => {}
            }
        }
    }

    // Every section and subtree, with only the definitions that `config` holds.
    fn to_json(&self, config: &Value) -> Value {
        let mut json = Map::new();
        for section in DEFINITION_DIRECTORIES {
            let definitions: Map<String, Value> = self
                .definitions
                .get(section)
                .into_iter()
                .flatten()
                .filter(|(name, _)| config[section].get(name.as_str()).is_some())
                .map(|(name, layers)| (name.clone(), to_value(layers)))
                .collect();
            json.insert(section.to_owned(), Value::Object(definitions));
        }
        for (_, subtree) in SUBTREE_FILES {
            let layers = self.subtrees.get(subtree).cloned().unwrap_or_default();
            json.insert(subtree.to_owned(), to_value(&layers));
        }
        Value::Object(json)
    }
}

fn to_value(layers: &[Contributor]) -> Value {
    serde_json::to_value(layers).expect("layers serialize")
}
