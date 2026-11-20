use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use regex::Regex;
use serde::Serialize;
use serde_json::Value;

use crate::Resolved;
use crate::artifact;
use crate::directory::Directory;
use crate::finding::{Code, Finding, Level};
use crate::gates::{DIRECTORY_GATES, PROCESS_GATES, Scope, TicketCall};
use crate::host::Host;
use crate::manifest::{Lock, read_lock_file};
use crate::process::{ProcessError, ProcessStore};
use crate::resolved_file::read_resolved;
use crate::schema_valid::ContentRules;
use crate::validate::{LANGUAGE_GATES, STANDARD_GATES};

static TICKET_ID: LazyLock<Regex> =
    LazyLock::new(|| Regex::new("^tk-[0-9a-hjkmnp-tv-z]{8}$").expect("valid regex"));

/// The class of a gate (concept Section 9): `exec` gates run commands, all others are `pure`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum GateClass {
    /// Standard gates and plugin gates.
    Pure,
    /// Gates that run a command of the host.
    Exec,
}

/// What a call of [`check`] checks.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CheckRequest {
    /// The ticket whose gates run. Without one, only the directory gates run.
    pub ticket: Option<String>,
    /// The step whose gates run. Without one, the gates of every `required` step of the
    /// ticket's profile run. Requires a ticket.
    pub step: Option<String>,
    /// The paths the change touches, relative to the project root, or `None` if the caller does
    /// not know them.
    pub changed_paths: Option<Vec<String>>,
    /// Only gates of this class run. `None` runs both classes.
    pub class: Option<GateClass>,
    /// The base directory of the process store (see [`ProcessStore::locate`]), or `None` for
    /// `HONE_DATA_DIR` or the data directory of the platform.
    pub data_directory: Option<PathBuf>,
    /// The base state, a directory that plugins read under `/base`, or `None` without one.
    pub base: Option<PathBuf>,
    /// The lockfile that approves plugins and `exec` commands, or `None` for
    /// `.hone/lock.yaml`. CI passes the lockfile of the protected base state.
    pub lock: Option<PathBuf>,
}

/// The result of one gate.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct GateReport {
    /// The gate name of a directory gate, or `<step>/<label>` of a ticket gate.
    pub name: String,
    /// The gate that ran, for example `approvals-valid`.
    pub gate: String,
    /// Whether the gate passed: it has no finding of level `error`.
    pub ok: bool,
    /// The findings, sorted.
    pub findings: Vec<Finding>,
}

/// The result of a call of [`check`].
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct CheckReport {
    /// One report per gate that ran, directory gates first.
    pub gates: Vec<GateReport>,
}

impl CheckReport {
    /// Whether every gate passed.
    #[must_use]
    pub fn passed(&self) -> bool {
        self.gates.iter().all(|gate| gate.ok)
    }
}

/// `check` could not run, for example because `.hone/resolved.json` or the ticket's snapshot is
/// missing or invalid. This is not a finding.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CheckError {
    /// What is missing or wrong.
    pub message: String,
}

impl fmt::Display for CheckError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for CheckError {}

/// Runs the gates of the checked directory `project_root` (the directory that contains
/// `.hone/`), following concept Section 7, "What `check` runs".
///
/// # Errors
///
/// Returns a [`CheckError`] if `.hone/resolved.json`, the ticket record, or its snapshot is
/// missing or invalid, if the request names an unknown step, a base that is not a directory, or
/// a lockfile that does not exist, or if a plugin or `exec` gate runs and the lockfile is
/// malformed.
pub fn check(project_root: &Path, request: &CheckRequest) -> Result<CheckReport, CheckError> {
    if request.ticket.is_none() && request.step.is_some() {
        return Err(error("`step` requires a ticket"));
    }
    if let Some(base) = &request.base
        && !base.is_dir()
    {
        return Err(error(format!(
            "{}: the base is not a directory",
            base.display()
        )));
    }
    if let Some(lock) = &request.lock
        && !lock.is_file()
    {
        return Err(error(format!(
            "{}: the lockfile does not exist",
            lock.display()
        )));
    }
    let current = Resolved::load(project_root)?;
    let current = current.document();
    let mut report = CheckReport::default();
    if request.class != Some(GateClass::Exec) {
        let directory = Directory::scan(project_root, current);
        let scope = Scope::new(current, &directory);
        for gate in DIRECTORY_GATES {
            if contains_gate(current, gate) {
                report
                    .gates
                    .push(gate_report(gate.into(), gate, scope.directory_gate(gate)));
            }
        }
    }
    if let Some(ticket) = &request.ticket {
        let run = TicketRun {
            project_root,
            current,
            ticket,
            step: request.step.as_deref(),
            changed_paths: request.changed_paths.as_deref(),
            data_directory: request.data_directory.as_deref(),
            base: request.base.as_deref(),
            lock: request.lock.as_deref(),
        };
        run.gates(&mut report, |gate| {
            let class = if gate == "exec" {
                GateClass::Exec
            } else {
                GateClass::Pure
            };
            !DIRECTORY_GATES.contains(&gate) && request.class.is_none_or(|wanted| wanted == class)
        })?;
    }
    Ok(report)
}

/// The check before an agent writes the file `path` (relative to `project_root`) for `ticket`:
/// the `change-needs-binding` gates of every `required` step of the ticket's profile, with
/// `path` as the only changed path. Edits to artifact files are never checked, so the report is
/// then empty.
///
/// # Errors
///
/// Returns a [`CheckError`] under the same conditions as [`check`].
pub fn check_edit(
    project_root: &Path,
    ticket: &str,
    path: &str,
) -> Result<CheckReport, CheckError> {
    let current = Resolved::load(project_root)?;
    let current = current.document();
    let mut report = CheckReport::default();
    if Directory::is_artifact_path(current, path) {
        return Ok(report);
    }
    let changed = [path.to_owned()];
    let run = TicketRun {
        project_root,
        current,
        ticket,
        step: None,
        changed_paths: Some(&changed),
        data_directory: None,
        base: None,
        lock: None,
    };
    run.gates(&mut report, |gate| gate == "change-needs-binding")?;
    Ok(report)
}

/// The findings of `schema-valid` for one artifact file by its content alone, without its place
/// in a directory: the YAML profile, the kind, the prefix, the status, the schema, and unknown
/// fields. The extension of `path` selects the file shape, and every finding carries `path`.
#[must_use]
pub fn check_artifact(config: &Resolved, path: &str, bytes: &[u8]) -> Vec<Finding> {
    let findings = match artifact::parse(path, bytes) {
        Ok(content) => ContentRules::new(config.document()).check(&content.data),
        Err(violations) => violations
            .into_iter()
            .map(|violation| Finding::error(Code::YamlProfile, violation))
            .collect(),
    };
    let mut findings: Vec<Finding> = findings.into_iter().map(|f| f.at_path(path)).collect();
    findings.sort();
    findings
}

/// The content hash of an artifact file (concept Section 5, "Content hash"). The extension of
/// `path` selects the file shape.
///
/// # Errors
///
/// Returns the violations of the YAML profile or of the file shape.
pub fn content_hash(path: &str, bytes: &[u8]) -> Result<String, crate::YamlProfileError> {
    artifact::parse(path, bytes)
        .map(|content| content.hash())
        .map_err(|violations| crate::YamlProfileError { violations })
}

struct TicketRun<'a> {
    project_root: &'a Path,
    current: &'a Value,
    ticket: &'a str,
    step: Option<&'a str>,
    changed_paths: Option<&'a [String]>,
    data_directory: Option<&'a Path>,
    base: Option<&'a Path>,
    lock: Option<&'a Path>,
}

impl TicketRun<'_> {
    fn gates(
        &self,
        report: &mut CheckReport,
        runs: impl Fn(&str) -> bool,
    ) -> Result<(), CheckError> {
        let (record, snapshot) = read_ticket(self.project_root, self.current, self.ticket)?;
        let snapshot = snapshot.document();
        let profile_name = record["profile"].as_str().unwrap_or_default();
        let Some(profile) = snapshot["profiles"][profile_name]["steps"].as_object() else {
            return Err(error(format!(
                "the snapshot has no profile `{profile_name}`"
            )));
        };
        let steps: Vec<&str> = match self.step {
            Some(step) if profile.contains_key(step) => vec![step],
            Some(step) => {
                return Err(error(format!(
                    "the profile `{profile_name}` does not list the step `{step}`"
                )));
            }
            None => profile
                .iter()
                .filter(|(_, strictness)| *strictness == "required")
                .map(|(step, _)| step.as_str())
                .collect(),
        };
        let selected = |step: &str| {
            snapshot["steps"][step]["gates"]
                .as_object()
                .into_iter()
                .flatten()
                .map(|(label, entry)| (label, entry, entry["gate"].as_str().unwrap_or_default()))
                .filter(|(_, _, gate)| runs(gate))
                .collect::<Vec<_>>()
        };
        let needs_store = steps.iter().any(|step| {
            selected(step)
                .iter()
                .any(|(_, _, gate)| PROCESS_GATES.contains(gate))
        });
        let process = if needs_store { self.process()? } else { None };
        let needs_lock = steps.iter().any(|step| {
            selected(step)
                .iter()
                .any(|(_, _, gate)| !STANDARD_GATES.contains(gate) || LANGUAGE_GATES.contains(gate))
        });
        let lock = if needs_lock {
            self.read_lock()?
        } else {
            Lock::default()
        };
        let host = Host::new(self.project_root, self.base, lock);
        let directory = Directory::scan(self.project_root, snapshot);
        let scope = Scope::new(snapshot, &directory);
        for step in steps {
            let call = TicketCall {
                ticket: self.ticket,
                record: &record,
                profile,
                step_name: step,
                step: &snapshot["steps"][step],
                changed_paths: self.changed_paths,
                process: process.as_ref(),
                host: &host,
            };
            for (label, entry, gate) in selected(step) {
                let findings = scope.ticket_gate(entry, &call);
                report
                    .gates
                    .push(gate_report(format!("{step}/{label}"), gate, findings));
            }
        }
        Ok(())
    }

    fn read_lock(&self) -> Result<Lock, CheckError> {
        let default = self.project_root.join(".hone").join("lock.yaml");
        let path = self.lock.unwrap_or(&default);
        let (Some(directory), Some(file)) = (path.parent(), path.file_name()) else {
            return Err(error(format!("{}: not a lockfile", path.display())));
        };
        read_lock_file(directory, &file.to_string_lossy()).map_err(|problems| {
            let problems: Vec<String> = problems.iter().map(ToString::to_string).collect();
            error(format!("{}: {}", path.display(), problems.join("; ")))
        })
    }

    fn process(&self) -> Result<Option<crate::TicketEvents>, CheckError> {
        let store = ProcessStore::locate(self.project_root, self.data_directory)
            .map_err(|failure| error(failure.to_string()))?;
        match store.events(self.ticket) {
            Ok(events) => Ok(Some(events)),
            Err(ProcessError::Unavailable { .. }) => Ok(None),
            Err(failure) => Err(error(failure.to_string())),
        }
    }
}

pub(crate) fn load_ticket(
    project_root: &Path,
    ticket: &str,
) -> Result<(serde_json::Map<String, Value>, Resolved), CheckError> {
    let current = Resolved::load(project_root)?;
    read_ticket(project_root, current.document(), ticket)
}

fn read_ticket(
    project_root: &Path,
    current: &Value,
    ticket: &str,
) -> Result<(serde_json::Map<String, Value>, Resolved), CheckError> {
    if !TICKET_ID.is_match(ticket) {
        return Err(error(format!("`{ticket}` is not a ticket ID")));
    }
    let pattern = current["ticket"]["path"].as_str().unwrap_or_default();
    let path = pattern.replace("{ticket}", ticket);
    let bytes =
        std::fs::read(project_root.join(&path)).map_err(|io| error(format!("{path}: {io}")))?;
    let record = artifact::parse(&path, &bytes)
        .map(|content| content.data)
        .map_err(|violations| error(format!("{path}: {}", violations.join("; "))))?;
    let hash = record["config"].as_str().unwrap_or_default();
    let Some(hex) = hash.strip_prefix("sha256:") else {
        return Err(error(format!(
            "the ticket record has no configuration hash (`config: {hash}`)"
        )));
    };
    let path = project_root
        .join(".hone")
        .join("snapshots")
        .join(format!("{hex}.json"));
    let snapshot = read_resolved(&path)?;
    if snapshot.hash() != hash {
        return Err(error(format!(
            "{}: the content has the hash {}, the ticket record names {hash}",
            path.display(),
            snapshot.hash()
        )));
    }
    Ok((record, snapshot))
}

fn contains_gate(config: &Value, gate: &str) -> bool {
    config["steps"]
        .as_object()
        .into_iter()
        .flatten()
        .any(|(_, step)| {
            step["gates"]
                .as_object()
                .into_iter()
                .flatten()
                .any(|(_, entry)| entry["gate"] == gate)
        })
}

fn gate_report(name: String, gate: &str, mut findings: Vec<Finding>) -> GateReport {
    findings.sort();
    GateReport {
        name,
        gate: gate.to_owned(),
        ok: findings.iter().all(|finding| finding.level != Level::Error),
        findings,
    }
}

fn error(message: impl Into<String>) -> CheckError {
    CheckError {
        message: message.into(),
    }
}
