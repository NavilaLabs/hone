//! The command line `hone … --json` as a typed interface (concept Section 12, "Desktop GUI").
//! The GUI reaches hone only through this module. Its types mirror the JSON of the command
//! line, so a change to that JSON shows up here as a parse failure.

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::Stdio;

use serde::Deserialize;
use serde::de::DeserializeOwned;
use serde_json::Value;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader, Lines};
use tokio::process::{Child, ChildStderr, Command};

/// The command line, bound to one project root.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hone {
    binary: PathBuf,
    root: PathBuf,
    environment: Vec<(OsString, OsString)>,
    packages: Option<PathBuf>,
}

/// Why a call of the command line gave no result.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Failure {
    /// The `status` of the command line's error (`invalid`, `unavailable`, `io-error`, `error`,
    /// `refused`, ...), or `not-started` when `hone` could not be run, or `unreadable` when its
    /// output was not the expected JSON.
    pub status: String,
    /// What went wrong, for a person.
    pub message: String,
}

/// The result of `hone resolve --show`.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(tag = "status", rename_all = "kebab-case")]
pub enum ConfigView {
    /// The configuration resolved.
    Shown(ShownConfig),
    /// The resolution failed.
    Failed {
        /// The problems, each with `category`, `location`, and `message`.
        problems: Vec<Value>,
        /// The content of `.hone/lock.yaml`, absent when it is malformed.
        lock: Option<Value>,
    },
}

/// The resolved configuration with the layers of every definition and the lockfile.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct ShownConfig {
    /// The hash of the configuration.
    pub hash: String,
    /// `up-to-date` or `out-of-date`: whether `.hone/resolved.json` holds the configuration.
    pub file: String,
    /// The resolved configuration.
    pub config: Value,
    /// Per definition of `kinds`, `roles`, `steps`, `profiles`, and `plugins`, and per subtree
    /// `select`, `actors`, and `ticket`, the layers from bottom to top.
    pub origins: BTreeMap<String, Value>,
    /// The content of `.hone/lock.yaml`.
    pub lock: Value,
}

/// The author and role of the events the GUI writes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Writer {
    /// The handle of the author.
    pub author: String,
    /// The role of the author, which must exist in the ticket's configuration.
    pub role: String,
}

/// What a sync talks to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Peer {
    /// A shared folder.
    Folder(String),
    /// A replication server; the token comes from `HONE_TOKEN` in the GUI's environment.
    Server(String),
}

/// Where a new thread is attached.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Anchor {
    /// An artifact by ID, optionally with text to find the spot again.
    Artifact {
        artifact: String,
        quote: Option<String>,
    },
    /// Code, `path#symbol`.
    Code(String),
    /// An external change, for example a pull request and a line range.
    Change(String),
}

/// The state a step is set to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StepStatus {
    Open,
    Done,
    Skipped,
}

/// An event to write to the process store.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Write {
    /// `hone thread open`.
    OpenThread {
        ticket: String,
        kind: String,
        blocking: bool,
        anchor: Anchor,
        body: String,
    },
    /// `hone thread reply`.
    Reply { thread: String, body: String },
    /// `hone thread resolve`.
    Resolve { thread: String },
    /// `hone thread reopen`.
    Reopen { thread: String },
    /// `hone step set`.
    SetStep {
        ticket: String,
        step: String,
        status: StepStatus,
        reason: Option<String>,
    },
    /// `hone approve`, or with `changes_requested` `hone approve --changes-requested`.
    Approve {
        ticket: String,
        artifact: String,
        changes_requested: bool,
        proof: Option<String>,
    },
    /// `hone review confirm`.
    ConfirmReview { ticket: String, artifact: String },
}

/// An event of the process store. `raw` is the event as the command line printed it.
#[derive(Debug, Clone, PartialEq)]
pub struct Event {
    pub id: String,
    pub device: String,
    pub seq: u64,
    pub kind: String,
    pub ticket: Option<String>,
    pub author: Option<Author>,
    pub payload: Value,
    pub raw: Value,
}

/// The author of an event.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Author {
    pub name: String,
    pub role: String,
}

/// The result of `hone log`.
#[derive(Debug, Clone, PartialEq)]
pub struct StoreLog {
    /// Per device, the highest `seq` up to which its events are contiguous.
    pub cursor: BTreeMap<String, u64>,
    /// Every valid event, in the order of `read`.
    pub events: Vec<Event>,
    /// This device's ID in the store, `None` while it has written nothing.
    pub device: Option<String>,
}

/// The result of `hone events`.
#[derive(Debug, Clone, PartialEq)]
pub struct TicketEvents {
    pub events: Vec<Event>,
    pub threads: Vec<ThreadState>,
    /// The steps with at least one `step.changed`.
    pub steps: BTreeMap<String, StepState>,
    pub invalid: Vec<InvalidEvent>,
}

/// The computed state of a thread.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct ThreadState {
    pub thread: String,
    pub kind: String,
    pub blocking: bool,
    /// `open` or `resolved`.
    pub status: String,
    pub answered: bool,
    pub opened_by: Author,
}

/// The computed state of a step.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct StepState {
    /// `open`, `done`, or `skipped`.
    pub status: String,
    pub reason: Option<String>,
}

/// An event the store could not read.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct InvalidEvent {
    pub path: String,
    pub line: Option<usize>,
    pub ticket: Option<String>,
    pub message: String,
}

/// The result of `hone check`.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct CheckRun {
    /// `passed` or `failed`.
    pub status: String,
    pub gates: Vec<Gate>,
    /// The ID of the stored run, `None` when the project has no store yet.
    #[serde(default)]
    pub run: Option<String>,
}

/// A `hone check` of a ticket with the arguments of a stored run.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CheckCall {
    pub ticket: String,
    pub step: Option<String>,
    /// `None` passes no changed paths; an empty list passes an explicitly empty change.
    pub changed: Option<Vec<String>>,
    /// `pure` or `exec`; `None` runs every gate.
    pub class: Option<String>,
}

/// A check run stored in the process store, from `hone runs`.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct Run {
    /// The run ID, whose UUIDv7 carries the time.
    pub id: String,
    /// Who ran it, for example `gui`, `cli`, or `pre-push`.
    pub source: String,
    pub device: String,
    /// `step`, `changed`, `class`, `base`, and `lock` as the run was called.
    pub arguments: Value,
    /// `passed` or `failed`.
    pub status: String,
    pub duration_ms: u64,
    pub gates: Vec<Gate>,
}

/// The result of `hone ticket list`.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct TicketList {
    /// The tickets, the one with the newest event first.
    pub tickets: Vec<TicketSummary>,
}

/// A ticket with its progress, from `hone ticket list`.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct TicketSummary {
    pub ticket: String,
    /// Whether this checkout holds the ticket's record.
    pub record: bool,
    pub title: Option<String>,
    pub external: Vec<ExternalTicket>,
    pub profile: Option<String>,
    pub started: Option<Started>,
    /// Every name that wrote an event of the ticket.
    pub authors: Vec<String>,
    pub steps: Vec<TicketStep>,
    /// The first open step.
    pub current: Option<String>,
    /// Whether every required step is done or skipped; `None` without a readable record.
    pub complete: Option<bool>,
    pub threads: ThreadCounts,
    /// The ID of the newest event.
    pub last_event: Option<String>,
    pub last_run: Option<LastRun>,
    /// Why the ticket waits, most pressing first.
    pub waiting: Vec<Waiting>,
    /// Why the record or its snapshot cannot be read.
    pub problem: Option<String>,
}

/// Who started a ticket.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Started {
    pub by: Author,
    /// The ID of `ticket.started`.
    pub event: String,
}

/// A step of a [`TicketSummary`].
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct TicketStep {
    pub step: String,
    /// `required` or `optional`; `None` without a record.
    pub requirement: Option<String>,
    /// `done`, `skipped`, `waiting`, or `open`.
    pub status: String,
    pub reason: Option<String>,
}

/// The open threads of a ticket.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
pub struct ThreadCounts {
    pub open: usize,
    pub blocking: usize,
}

/// The newest stored check run of a ticket.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct LastRun {
    pub id: String,
    /// `passed` or `failed`.
    pub status: String,
    /// The names of the failed gates.
    pub failed: Vec<String>,
}

/// A reason a ticket waits.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum Waiting {
    /// Gates of the last run failed.
    Gates { gates: Vec<String> },
    /// Blocking threads are open.
    Threads { threads: Vec<String> },
    /// An approval of `role` is missing.
    Approval { role: Option<String>, gate: String },
    /// The current step waits for its work, decided by `role`.
    Step { step: String, role: Option<String> },
}

/// The result of one gate.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct Gate {
    /// The name in the configuration, `<step>/<key>` for a step gate.
    pub name: String,
    /// The gate type, for example `artifact-exists`.
    pub gate: String,
    pub ok: bool,
    pub findings: Vec<Finding>,
    /// How long the gate ran.
    #[serde(default)]
    pub duration_ms: u64,
}

/// A finding of a gate.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct Finding {
    pub code: String,
    /// `error` or `warning`.
    pub level: String,
    pub message: String,
    #[serde(default)]
    pub location: Option<Value>,
    /// The file of the finding, relative to the project root.
    #[serde(default)]
    pub path: Option<String>,
    /// A JSON Pointer into the file's data model.
    #[serde(default)]
    pub pointer: Option<String>,
}

/// The result of `hone sync`.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct SyncReport {
    pub received: BTreeMap<String, u64>,
    pub sent: BTreeMap<String, u64>,
    pub problems: Vec<SyncProblem>,
}

/// A problem a sync reported.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct SyncProblem {
    pub code: String,
    pub path: String,
    pub line: Option<usize>,
    pub message: String,
}

/// The result of a write: the event, and the thread ID for a new thread.
#[derive(Debug, Clone, PartialEq)]
pub struct Appended {
    pub event: Event,
    pub thread: Option<String>,
}

/// The result of `hone artifacts`: the artifacts of the checked directory.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Catalog {
    /// The project ID.
    pub project: String,
    /// Every readable artifact, ticket records included, sorted by path.
    pub artifacts: Vec<CatalogEntry>,
    /// Files that match an artifact pattern but cannot be read.
    pub unreadable: Vec<UnreadableFile>,
}

impl Catalog {
    /// The ticket records.
    pub fn tickets(&self) -> impl Iterator<Item = &CatalogEntry> {
        self.artifacts.iter().filter(|entry| entry.kind == "ticket")
    }
}

/// An artifact of a [`Catalog`].
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct CatalogEntry {
    pub id: Option<String>,
    /// The kind, or `ticket` for a ticket record.
    #[serde(rename = "type")]
    pub kind: String,
    pub path: String,
    pub ticket: Option<String>,
    pub title: Option<String>,
    /// The declared status.
    pub status: Option<String>,
    /// The effective status: the declared one, or `superseded`.
    pub effective: Option<String>,
    /// A ticket record's profile.
    pub profile: Option<String>,
    /// A ticket record's external tickets.
    #[serde(default)]
    pub external: Vec<ExternalTicket>,
    /// The ID links by type, for example `supersedes`.
    #[serde(default)]
    pub links: BTreeMap<String, Vec<String>>,
}

/// The result of `hone review show`: the review of an artifact in a ticket.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct ReviewState {
    pub artifact: String,
    pub path: String,
    /// The artifact's current content hash.
    pub hash: String,
    /// Every person's and role's last decision.
    pub decisions: Vec<ReviewDecision>,
    pub confirmations: Vec<ReviewConfirmation>,
    /// The `review` block of the file.
    pub review: Option<Value>,
}

/// A person's last decision on an artifact.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct ReviewDecision {
    pub name: String,
    pub role: String,
    /// `approved` or `changes-requested`.
    pub decision: String,
    pub hash: String,
    /// Whether the decision is for the current content.
    pub current: bool,
    pub date: String,
    pub proof: Option<String>,
}

/// A confirmation of an artifact's condensed `review` block.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct ReviewConfirmation {
    pub name: String,
    pub role: String,
    pub hash: String,
    pub current: bool,
    pub date: String,
}

/// A file that matches an artifact pattern but cannot be read.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct UnreadableFile {
    pub path: String,
    pub message: String,
}

/// An entry of a ticket record's `external`.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct ExternalTicket {
    /// The tracker, for example `jira` or `github`.
    pub system: String,
    pub key: String,
    pub url: Option<String>,
}

/// The result of `hone ticket show`: a ticket with the steps of its profile.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct TicketOverview {
    pub ticket: String,
    pub path: String,
    pub profile: String,
    /// The hash of the ticket's snapshot.
    pub config: String,
    pub attributes: serde_json::Map<String, Value>,
    pub external: Vec<ExternalTicket>,
    /// The profile `select` of the snapshot gives for the attributes.
    pub selected: Option<String>,
    /// The roles of the ticket's snapshot.
    pub roles: Vec<String>,
    /// The profile's steps, each after the steps in its `after`.
    pub steps: Vec<ProfileStep>,
}

/// A step of a ticket's profile.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct ProfileStep {
    pub step: String,
    pub title: Option<String>,
    /// `required` or `optional`.
    pub requirement: String,
    /// The steps of the profile that come before it.
    pub after: Vec<String>,
}

/// The author a context writes as, from `hone context show`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContextAuthor {
    /// The context's name.
    pub context: String,
    /// The author, if the context has one.
    pub author: Option<Author>,
}

/// Which gates a check runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GateScope {
    /// Every gate.
    All,
    /// Only pure gates: no `exec` gate runs a command.
    Pure,
}

/// The value of an attribute, in one of the three types the protocol allows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AttributeValue {
    Text(String),
    Integer(i64),
    Boolean(bool),
}

impl AttributeValue {
    /// The value as `--attribute` reads it: a text always double-quoted, so that `3` or `true`
    /// typed as text stays text.
    #[must_use]
    pub fn argument(&self) -> String {
        match self {
            Self::Text(text) => serde_json::to_string(text).expect("a string serializes"),
            Self::Integer(number) => number.to_string(),
            Self::Boolean(flag) => flag.to_string(),
        }
    }

    /// The value as the JSON of the command line holds it.
    #[must_use]
    pub fn to_json(&self) -> Value {
        match self {
            Self::Text(text) => Value::String(text.clone()),
            Self::Integer(number) => Value::from(*number),
            Self::Boolean(flag) => Value::Bool(*flag),
        }
    }

    /// The value of a JSON attribute, or `None` for a type the protocol does not allow.
    #[must_use]
    pub fn from_json(value: &Value) -> Option<Self> {
        match value {
            Value::String(text) => Some(Self::Text(text.clone())),
            Value::Bool(flag) => Some(Self::Boolean(*flag)),
            Value::Number(number) => number.as_i64().map(Self::Integer),
            _ => None,
        }
    }
}

/// What a new ticket starts with.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TicketStart {
    pub attributes: Vec<(String, AttributeValue)>,
    /// External tickets as `<system>:<key>[=<url>]`.
    pub external: Vec<String>,
    /// A profile instead of the one `select` gives.
    pub profile: Option<String>,
}

/// The result of `hone ticket start`, `set-attributes`, and `switch-config`.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct TicketWritten {
    pub ticket: String,
    pub path: String,
    pub profile: String,
    /// The profile `select` gives for the attributes under the ticket's configuration.
    pub selected: Option<String>,
    pub config: String,
    pub attributes: serde_json::Map<String, Value>,
}

/// The result of `hone ticket select`.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct ProfileSelection {
    pub selected: Option<String>,
    /// The position of the matching rule, `None` for the default.
    pub rule: Option<usize>,
    /// The rules, `{ when, profile }`.
    pub rules: Vec<Value>,
    pub default: Option<String>,
    pub profiles: Vec<String>,
}

/// A kind of the configuration, from `hone artifact kinds`.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct KindInfo {
    #[serde(rename = "type")]
    pub kind: String,
    /// `decided` or `living`.
    pub lifecycle: String,
    pub id_prefix: String,
    pub path: String,
    pub first_status: Option<String>,
    /// The fields the kind's schema requires.
    pub required: Vec<String>,
}

/// A status of a kind.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct KindStatus {
    pub name: String,
    pub binding: bool,
}

/// The result of `hone artifact show`.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct ArtifactSource {
    pub id: String,
    #[serde(rename = "type")]
    pub kind: String,
    pub path: String,
    /// The file as it is.
    pub text: String,
    pub data: serde_json::Map<String, Value>,
    /// The Markdown body, `None` for a YAML file.
    pub body: Option<String>,
    pub lifecycle: String,
    pub statuses: Vec<KindStatus>,
    /// The kind's additional schema.
    pub schema: Option<Value>,
    pub link_types: Vec<String>,
}

/// The result of `hone artifact save` and `hone artifact new`.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct ArtifactWritten {
    pub id: Option<String>,
    pub path: String,
    /// Whether the file was written: only without an error finding.
    pub saved: bool,
    pub findings: Vec<Finding>,
}

/// The result of `hone artifact check`.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct ArtifactChecked {
    pub path: String,
    pub findings: Vec<Finding>,
}

/// What `hone artifact new` creates.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ArtifactNew {
    pub kind: String,
    pub title: String,
    pub ticket: Option<String>,
    /// The handle for `authors`; empty for the context's author.
    pub author: Option<String>,
    /// Anchors as `<kind>:<ref>`.
    pub anchors: Vec<String>,
}

/// The result of `hone file show`.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct SourceFile {
    pub path: String,
    pub text: String,
    pub symbol: Option<String>,
    pub language: Option<String>,
    /// `found`, `missing`, `unparsable`, `unchecked`, or `failed`.
    pub resolved: Option<String>,
    pub line: Option<usize>,
    pub message: Option<String>,
}

/// A running `hone serve`. Dropping it stops the server.
#[derive(Debug)]
pub struct ServerProcess {
    child: Child,
    output: Lines<BufReader<ChildStderr>>,
}

impl Hone {
    /// The command line for the project in `root`. `hone` is `HONE_BIN` if set, otherwise
    /// `hone` next to this executable if it exists there, otherwise `hone` on the `PATH`.
    #[must_use]
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self::with_binary(binary(), root)
    }

    /// The command line `binary` for the project in `root`.
    #[must_use]
    pub fn with_binary(binary: impl Into<PathBuf>, root: impl Into<PathBuf>) -> Self {
        Self {
            binary: binary.into(),
            root: root.into(),
            environment: Vec::new(),
            packages: None,
        }
    }

    /// The same command line with the environment variable `name` set to `value` for every
    /// call, for example `HONE_DATA_DIR`.
    #[must_use]
    pub fn with_variable(mut self, name: impl Into<OsString>, value: impl Into<OsString>) -> Self {
        self.environment.push((name.into(), value.into()));
        self
    }

    /// The same command line with `dir` passed as `--packages` to the commands that resolve
    /// the configuration (`resolve`, `ticket start`, `ticket switch-config`).
    #[must_use]
    pub fn with_packages(mut self, dir: Option<PathBuf>) -> Self {
        self.packages = dir;
        self
    }

    /// The project root.
    #[must_use]
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// `hone log`: every valid event of the store and its cursor.
    ///
    /// # Errors
    ///
    /// Returns a [`Failure`], `unavailable` if the store does not exist yet.
    pub async fn log(&self) -> Result<StoreLog, Failure> {
        #[derive(Deserialize)]
        struct Output {
            cursor: BTreeMap<String, u64>,
            events: Vec<Value>,
            device: Option<String>,
        }
        let output: Output = self.run(&["log"], &[0]).await?;
        Ok(StoreLog {
            device: output.device,
            cursor: output.cursor,
            events: events(output.events)?,
        })
    }

    /// `hone events --ticket <ticket>`: the process state of a ticket.
    ///
    /// # Errors
    ///
    /// Returns a [`Failure`], `unavailable` if the store does not exist yet.
    pub async fn events(&self, ticket: &str) -> Result<TicketEvents, Failure> {
        #[derive(Deserialize)]
        struct Output {
            events: Vec<Value>,
            threads: Vec<ThreadState>,
            steps: BTreeMap<String, StepState>,
            invalid: Vec<InvalidEvent>,
        }
        let output: Output = self.run(&["events", "--ticket", ticket], &[0]).await?;
        Ok(TicketEvents {
            events: events(output.events)?,
            threads: output.threads,
            steps: output.steps,
            invalid: output.invalid,
        })
    }

    /// `hone check [--ticket <ticket>] [--class pure]`. A failed gate is a result, not a failure.
    ///
    /// # Errors
    ///
    /// Returns a [`Failure`] when the check could not run (exit code 2).
    pub async fn check(&self, ticket: Option<&str>, scope: GateScope) -> Result<CheckRun, Failure> {
        self.run(&check_arguments(ticket, scope), &[0, 1]).await
    }

    /// `hone check` as `call` says, stored with the source `gui`.
    ///
    /// # Errors
    ///
    /// Returns a [`Failure`] when the check could not run.
    pub async fn check_call(&self, call: &CheckCall) -> Result<CheckRun, Failure> {
        let mut arguments = vec!["check", "--source", "gui", "--ticket", call.ticket.as_str()];
        if let Some(step) = &call.step {
            arguments.extend(["--step", step]);
        }
        if let Some(class) = &call.class {
            arguments.extend(["--class", class]);
        }
        match &call.changed {
            Some(changed) if changed.is_empty() => arguments.extend(["--changed-from", "-"]),
            Some(changed) => {
                for path in changed {
                    arguments.extend(["--changed", path]);
                }
            }
            None => {}
        }
        self.run(&arguments, &[0, 1]).await
    }

    /// `hone ticket list`: every ticket with its progress.
    ///
    /// # Errors
    ///
    /// Returns a [`Failure`] `error` when the configuration or the store cannot be read.
    pub async fn ticket_list(&self) -> Result<TicketList, Failure> {
        self.run(&["ticket", "list"], &[0]).await
    }

    /// `hone runs --ticket <ticket>`: the stored check runs of a ticket, newest first.
    ///
    /// # Errors
    ///
    /// Returns a [`Failure`], `invalid` for a malformed ticket ID.
    pub async fn runs(&self, ticket: &str) -> Result<Vec<Run>, Failure> {
        #[derive(Deserialize)]
        struct Output {
            runs: Vec<Run>,
        }
        let output: Output = self.run(&["runs", "--ticket", ticket], &[0]).await?;
        Ok(output.runs)
    }

    /// `hone artifacts`: the artifacts of the checked directory.
    ///
    /// # Errors
    ///
    /// Returns a [`Failure`] `error` when the configuration cannot be read.
    pub async fn artifacts(&self) -> Result<Catalog, Failure> {
        self.run(&["artifacts"], &[0]).await
    }

    /// `hone ticket show --ticket <ticket>`: the ticket with the steps of its profile.
    ///
    /// # Errors
    ///
    /// Returns a [`Failure`] `error` when the ticket record or its snapshot cannot be read.
    pub async fn ticket(&self, ticket: &str) -> Result<TicketOverview, Failure> {
        self.run(&["ticket", "show", "--ticket", ticket], &[0])
            .await
    }

    /// `hone review show --ticket <ticket> <artifact>`: the review of an artifact.
    ///
    /// # Errors
    ///
    /// Returns a [`Failure`], `invalid` for an unknown ticket or artifact.
    pub async fn review(&self, ticket: &str, artifact: &str) -> Result<ReviewState, Failure> {
        self.run(&["review", "show", "--ticket", ticket, artifact], &[0])
            .await
    }

    /// `hone review condense --ticket <ticket> <artifact>`: writes the approvals of the current
    /// content into the artifact's `review` block and returns the artifact's path. Writes no
    /// event.
    ///
    /// # Errors
    ///
    /// Returns a [`Failure`], `invalid` for an unknown ticket or artifact.
    pub async fn condense(&self, ticket: &str, artifact: &str) -> Result<String, Failure> {
        #[derive(Deserialize)]
        struct Output {
            path: String,
        }
        let output: Output = self
            .run(&["review", "condense", "--ticket", ticket, artifact], &[0])
            .await?;
        Ok(output.path)
    }

    /// `hone context show`: the context the project is bound to and its author.
    ///
    /// # Errors
    ///
    /// Returns a [`Failure`], `invalid` when the project is bound to no context.
    pub async fn context(&self) -> Result<ContextAuthor, Failure> {
        #[derive(Deserialize)]
        struct Bound {
            name: String,
            context: Fields,
        }
        #[derive(Deserialize)]
        struct Fields {
            author: Option<Author>,
        }
        let bound: Bound = self.run(&["context", "show"], &[0]).await?;
        Ok(ContextAuthor {
            context: bound.name,
            author: bound.context.author,
        })
    }

    /// `hone sync`. Problems are a result, not a failure.
    ///
    /// # Errors
    ///
    /// Returns a [`Failure`] when the sync could not run, for example `unavailable` for a
    /// missing folder or an unreachable server.
    pub async fn sync(&self, peer: &Peer) -> Result<SyncReport, Failure> {
        let arguments = match peer {
            Peer::Folder(folder) => ["sync", "--folder", folder.as_str()],
            Peer::Server(url) => ["sync", "--server", url.as_str()],
        };
        self.run(&arguments, &[0, 1]).await
    }

    /// Writes an event as `writer`, or, if its author or role is empty, as the author of the
    /// project's context.
    ///
    /// # Errors
    ///
    /// Returns a [`Failure`], `invalid` for example for a step outside the ticket's profile.
    pub async fn write(&self, writer: &Writer, write: &Write) -> Result<Appended, Failure> {
        #[derive(Deserialize)]
        struct Output {
            event: Value,
            thread: Option<String>,
        }
        let mut arguments = write_arguments(write);
        // Without both, the command line takes the author of the project's context.
        if !writer.author.is_empty() && !writer.role.is_empty() {
            arguments.extend([
                "--author".into(),
                writer.author.clone(),
                "--role".into(),
                writer.role.clone(),
            ]);
        }
        let arguments: Vec<&str> = arguments.iter().map(String::as_str).collect();
        let output: Output = self.run(&arguments, &[0]).await?;
        Ok(Appended {
            event: event(output.event)?,
            thread: output.thread,
        })
    }

    /// `hone ticket start` as `writer`.
    ///
    /// # Errors
    ///
    /// Returns a [`Failure`], `invalid` for example for a stale `.hone/resolved.json`.
    pub async fn start_ticket(
        &self,
        writer: &Writer,
        start: &TicketStart,
    ) -> Result<TicketWritten, Failure> {
        let mut arguments = vec!["ticket".to_owned(), "start".to_owned()];
        arguments.extend(attribute_arguments(&start.attributes));
        for external in &start.external {
            arguments.extend(["--external".to_owned(), external.clone()]);
        }
        if let Some(profile) = &start.profile {
            arguments.extend(["--profile".to_owned(), profile.clone()]);
        }
        arguments.extend(self.packages_arguments());
        self.run_as(writer, arguments).await
    }

    /// `hone ticket set-attributes` as `writer`: sets `set` and removes `unset`.
    ///
    /// # Errors
    ///
    /// Returns a [`Failure`], `invalid` for example for an unknown ticket.
    pub async fn set_attributes(
        &self,
        writer: &Writer,
        ticket: &str,
        set: &[(String, AttributeValue)],
        unset: &[String],
    ) -> Result<TicketWritten, Failure> {
        let mut arguments = vec![
            "ticket".to_owned(),
            "set-attributes".to_owned(),
            "--ticket".to_owned(),
            ticket.to_owned(),
        ];
        arguments.extend(attribute_arguments(set));
        for key in unset {
            arguments.extend(["--unset".to_owned(), key.clone()]);
        }
        self.run_as(writer, arguments).await
    }

    /// `hone ticket switch-config` as `writer`.
    ///
    /// # Errors
    ///
    /// Returns a [`Failure`], `invalid` when the ticket already uses the current configuration.
    pub async fn switch_config(
        &self,
        writer: &Writer,
        ticket: &str,
    ) -> Result<TicketWritten, Failure> {
        let mut arguments = ["ticket", "switch-config", "--ticket", ticket]
            .map(str::to_owned)
            .to_vec();
        arguments.extend(self.packages_arguments());
        self.run_as(writer, arguments).await
    }

    /// `hone ticket select`: the profile `select` gives for `attributes`, and its rules.
    ///
    /// # Errors
    ///
    /// Returns a [`Failure`] `error` when the configuration cannot be read.
    pub async fn select(
        &self,
        attributes: &[(String, AttributeValue)],
    ) -> Result<ProfileSelection, Failure> {
        let mut arguments = vec!["ticket".to_owned(), "select".to_owned()];
        arguments.extend(attribute_arguments(attributes));
        let arguments: Vec<&str> = arguments.iter().map(String::as_str).collect();
        self.run(&arguments, &[0]).await
    }

    /// `hone resolve`: writes `.hone/resolved.json` from its sources and returns its hash.
    ///
    /// # Errors
    ///
    /// Returns a [`Failure`] with the problems of the resolution.
    pub async fn resolve(&self) -> Result<String, Failure> {
        #[derive(Deserialize)]
        struct Output {
            hash: Option<String>,
            problems: Option<Vec<Value>>,
            status: Option<String>,
        }
        let mut arguments = vec!["resolve".to_owned()];
        arguments.extend(self.packages_arguments());
        let arguments: Vec<&str> = arguments.iter().map(String::as_str).collect();
        let output: Output = self.run(&arguments, &[0, 1]).await?;
        match (output.hash, output.problems) {
            (Some(hash), _) => Ok(hash),
            (None, problems) => Err(Failure {
                // The category of the first problem, for example `package-missing`.
                status: problems
                    .as_ref()
                    .and_then(|problems| problems.first())
                    .and_then(|problem| problem["category"].as_str())
                    .map(str::to_owned)
                    .or(output.status)
                    .unwrap_or_else(|| "failed".into()),
                message: problems
                    .unwrap_or_default()
                    .iter()
                    .map(|problem| {
                        problem["message"]
                            .as_str()
                            .map_or_else(|| problem.to_string(), str::to_owned)
                    })
                    .collect::<Vec<_>>()
                    .join("; "),
            }),
        }
    }

    /// `hone resolve --show`: the configuration from its sources with the layers of every
    /// definition and the lockfile, or the problems of a failed resolution. Writes nothing.
    ///
    /// # Errors
    ///
    /// Returns a [`Failure`] when `hone` cannot run or its output is not understood.
    pub async fn show_config(&self) -> Result<ConfigView, Failure> {
        let mut arguments = vec!["resolve".to_owned(), "--show".to_owned()];
        arguments.extend(self.packages_arguments());
        let arguments: Vec<&str> = arguments.iter().map(String::as_str).collect();
        self.run(&arguments, &[0, 1]).await
    }

    /// `hone artifact kinds`: the kinds of the configuration.
    ///
    /// # Errors
    ///
    /// Returns a [`Failure`] `error` when the configuration cannot be read.
    pub async fn kinds(&self) -> Result<Vec<KindInfo>, Failure> {
        self.run(&["artifact", "kinds"], &[0]).await
    }

    /// `hone artifact show --id <id>`: an artifact file and what a form needs.
    ///
    /// # Errors
    ///
    /// Returns a [`Failure`] `error` for an unknown ID.
    pub async fn artifact(&self, id: &str) -> Result<ArtifactSource, Failure> {
        self.run(&["artifact", "show", "--id", id], &[0]).await
    }

    /// `hone artifact save --path <path>` with `text` on standard input. A refusal for an error
    /// finding is a result with `saved: false`.
    ///
    /// # Errors
    ///
    /// Returns a [`Failure`] `error` when the save could not run, for example for a path outside
    /// the artifact patterns.
    pub async fn save_artifact(&self, path: &str, text: &str) -> Result<ArtifactWritten, Failure> {
        self.run_with_input(
            &["artifact", "save", "--path", path],
            &[0, 1],
            text.as_bytes(),
        )
        .await
    }

    /// `hone artifact check --path <path>` with `text` on standard input: the findings of a save
    /// and of `links-resolve`, without writing.
    ///
    /// # Errors
    ///
    /// Returns a [`Failure`] `error` when the check could not run, for example for a path
    /// outside the artifact patterns.
    pub async fn check_artifact(&self, path: &str, text: &str) -> Result<ArtifactChecked, Failure> {
        self.run_with_input(
            &["artifact", "check", "--path", path],
            &[0, 1],
            text.as_bytes(),
        )
        .await
    }

    /// `hone artifact new` as `author`.
    ///
    /// # Errors
    ///
    /// Returns a [`Failure`] `error` for example for a decided kind without a ticket.
    pub async fn new_artifact(&self, new: &ArtifactNew) -> Result<ArtifactWritten, Failure> {
        let mut arguments = vec![
            "artifact".to_owned(),
            "new".to_owned(),
            "--kind".to_owned(),
            new.kind.clone(),
            "--title".to_owned(),
            new.title.clone(),
        ];
        if let Some(ticket) = &new.ticket {
            arguments.extend(["--ticket".to_owned(), ticket.clone()]);
        }
        if let Some(author) = new.author.as_ref().filter(|author| !author.is_empty()) {
            arguments.extend(["--author".to_owned(), author.clone()]);
        }
        for anchor in &new.anchors {
            arguments.extend(["--anchor".to_owned(), anchor.clone()]);
        }
        let arguments: Vec<&str> = arguments.iter().map(String::as_str).collect();
        self.run(&arguments, &[0, 1]).await
    }

    /// `hone file show --path <path> [--symbol <symbol>]`: a source file and the line of a
    /// symbol.
    ///
    /// # Errors
    ///
    /// Returns a [`Failure`] `error` for a path that is no regular file below the root.
    pub async fn file(&self, path: &str, symbol: Option<&str>) -> Result<SourceFile, Failure> {
        let mut arguments = vec!["file", "show", "--path", path];
        if let Some(symbol) = symbol.filter(|symbol| !symbol.is_empty()) {
            arguments.extend(["--symbol", symbol]);
        }
        self.run(&arguments, &[0]).await
    }

    fn packages_arguments(&self) -> Vec<String> {
        self.packages
            .iter()
            .flat_map(|dir| ["--packages".to_owned(), dir.display().to_string()])
            .collect()
    }

    async fn run_as<T: DeserializeOwned>(
        &self,
        writer: &Writer,
        mut arguments: Vec<String>,
    ) -> Result<T, Failure> {
        // Without both, the command line takes the author of the project's context.
        if !writer.author.is_empty() && !writer.role.is_empty() {
            arguments.extend([
                "--author".into(),
                writer.author.clone(),
                "--role".into(),
                writer.role.clone(),
            ]);
        }
        let arguments: Vec<&str> = arguments.iter().map(String::as_str).collect();
        self.run(&arguments, &[0]).await
    }

    /// Starts `hone serve --config <config>`.
    ///
    /// # Errors
    ///
    /// Returns a [`Failure`] `not-started` if `hone` cannot be run.
    pub fn serve(&self, config: &Path) -> Result<ServerProcess, Failure> {
        let mut child = Command::new(&self.binary)
            .envs(self.environment.iter().map(|(name, value)| (name, value)))
            .arg("serve")
            .arg("--config")
            .arg(config)
            .current_dir(&self.root)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .map_err(|error| self.not_started(&error))?;
        let stderr = child.stderr.take().ok_or_else(|| Failure {
            status: "not-started".into(),
            message: "the server's output cannot be read".into(),
        })?;
        Ok(ServerProcess {
            child,
            output: BufReader::new(stderr).lines(),
        })
    }

    async fn run<T: DeserializeOwned>(
        &self,
        arguments: &[&str],
        accepted: &[i32],
    ) -> Result<T, Failure> {
        self.run_with_input(arguments, accepted, &[]).await
    }

    async fn run_with_input<T: DeserializeOwned>(
        &self,
        arguments: &[&str],
        accepted: &[i32],
        input: &[u8],
    ) -> Result<T, Failure> {
        let mut child = Command::new(&self.binary)
            .envs(self.environment.iter().map(|(name, value)| (name, value)))
            .args(arguments)
            .arg("--root")
            .arg(&self.root)
            .arg("--json")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .map_err(|error| self.not_started(&error))?;
        if let Some(mut stdin) = child.stdin.take() {
            // A command that reads no input closes the pipe early; that is not a failure.
            let _ = stdin.write_all(input).await;
        }
        let output = child
            .wait_with_output()
            .await
            .map_err(|error| self.not_started(&error))?;
        let code = output.status.code();
        if code.is_some_and(|code| accepted.contains(&code)) {
            return serde_json::from_slice(&output.stdout).map_err(|error| Failure {
                status: "unreadable".into(),
                message: format!(
                    "the output of `hone {}` is not understood: {error}",
                    arguments[0]
                ),
            });
        }
        #[derive(Deserialize)]
        struct Error {
            status: String,
            message: Option<String>,
        }
        match serde_json::from_slice::<Error>(&output.stdout) {
            Ok(error) => Err(Failure {
                message: error.message.unwrap_or_else(|| error.status.clone()),
                status: error.status,
            }),
            Err(_) => Err(Failure {
                status: "unreadable".into(),
                message: format!(
                    "`hone {}` ended with {}: {}",
                    arguments[0],
                    code.map_or_else(|| "a signal".to_owned(), |code| format!("exit code {code}")),
                    String::from_utf8_lossy(&output.stderr).trim()
                ),
            }),
        }
    }

    fn not_started(&self, error: &std::io::Error) -> Failure {
        Failure {
            status: "not-started".into(),
            message: format!(
                "`{}` cannot be run: {error}. Set HONE_BIN to the hone executable.",
                self.binary.display()
            ),
        }
    }
}

impl ServerProcess {
    /// The next line the server wrote to its error output, or `None` once it has ended.
    pub async fn next_line(&mut self) -> Option<String> {
        self.output.next_line().await.ok().flatten()
    }

    /// Stops the server.
    pub async fn stop(&mut self) {
        let _ = self.child.kill().await;
    }
}

fn binary() -> PathBuf {
    if let Some(binary) = std::env::var_os("HONE_BIN").filter(|value| !value.is_empty()) {
        return binary.into();
    }
    let name = OsString::from(format!("hone{}", std::env::consts::EXE_SUFFIX));
    std::env::current_exe()
        .ok()
        .and_then(|executable| Some(executable.parent()?.join(&name)))
        .filter(|sibling| sibling.is_file())
        .unwrap_or_else(|| name.into())
}

fn attribute_arguments(attributes: &[(String, AttributeValue)]) -> Vec<String> {
    attributes
        .iter()
        .flat_map(|(key, value)| {
            [
                "--attribute".to_owned(),
                format!("{key}={}", value.argument()),
            ]
        })
        .collect()
}

fn check_arguments(ticket: Option<&str>, scope: GateScope) -> Vec<&str> {
    let mut arguments = vec!["check", "--source", "gui"];
    if let Some(ticket) = ticket {
        arguments.extend(["--ticket", ticket]);
    }
    if scope == GateScope::Pure {
        arguments.extend(["--class", "pure"]);
    }
    arguments
}

fn write_arguments(write: &Write) -> Vec<String> {
    let owned = |parts: &[&str]| {
        parts
            .iter()
            .map(|part| (*part).to_owned())
            .collect::<Vec<_>>()
    };
    match write {
        Write::OpenThread {
            ticket,
            kind,
            blocking,
            anchor,
            body,
        } => {
            let mut arguments = owned(&["thread", "open", "--ticket", ticket, "--kind", kind]);
            if *blocking {
                arguments.push("--blocking".into());
            }
            match anchor {
                Anchor::Artifact { artifact, quote } => {
                    arguments.extend(owned(&["--anchor-artifact", artifact]));
                    if let Some(quote) = quote {
                        arguments.extend(owned(&["--quote", quote]));
                    }
                }
                Anchor::Code(reference) => arguments.extend(owned(&["--anchor-code", reference])),
                Anchor::Change(reference) => {
                    arguments.extend(owned(&["--anchor-change", reference]));
                }
            }
            arguments.extend(owned(&["--body", body]));
            arguments
        }
        Write::Reply { thread, body } => owned(&["thread", "reply", thread, "--body", body]),
        Write::Resolve { thread } => owned(&["thread", "resolve", thread]),
        Write::Reopen { thread } => owned(&["thread", "reopen", thread]),
        Write::SetStep {
            ticket,
            step,
            status,
            reason,
        } => {
            let status = match status {
                StepStatus::Open => "open",
                StepStatus::Done => "done",
                StepStatus::Skipped => "skipped",
            };
            let mut arguments = owned(&["step", "set", "--ticket", ticket, step, status]);
            if let Some(reason) = reason {
                arguments.extend(owned(&["--reason", reason]));
            }
            arguments
        }
        Write::Approve {
            ticket,
            artifact,
            changes_requested,
            proof,
        } => {
            let mut arguments = owned(&["approve", "--ticket", ticket, artifact]);
            if *changes_requested {
                arguments.push("--changes-requested".into());
            }
            if let Some(proof) = proof {
                arguments.extend(owned(&["--proof", proof]));
            }
            arguments
        }
        Write::ConfirmReview { ticket, artifact } => {
            owned(&["review", "confirm", "--ticket", ticket, artifact])
        }
    }
}

fn events(values: Vec<Value>) -> Result<Vec<Event>, Failure> {
    values.into_iter().map(event).collect()
}

fn event(raw: Value) -> Result<Event, Failure> {
    #[derive(Deserialize)]
    struct Fields {
        id: String,
        device: String,
        seq: u64,
        #[serde(rename = "type")]
        kind: String,
        ticket: Option<String>,
        author: Option<Author>,
        #[serde(default)]
        payload: Value,
    }
    let fields = Fields::deserialize(&raw).map_err(|error| Failure {
        status: "unreadable".into(),
        message: format!("an event is not understood: {error}"),
    })?;
    Ok(Event {
        id: fields.id,
        device: fields.device,
        seq: fields.seq,
        kind: fields.kind,
        ticket: fields.ticket,
        author: fields.author,
        payload: fields.payload,
        raw,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn attribute_values_keep_their_type_on_the_command_line() {
        let arguments = attribute_arguments(&[
            ("size".into(), AttributeValue::Text("3".into())),
            ("count".into(), AttributeValue::Integer(-2)),
            ("urgent".into(), AttributeValue::Boolean(true)),
            ("note".into(), AttributeValue::Text("say \"hi\"".into())),
        ]);
        assert_eq!(
            arguments,
            [
                "--attribute",
                "size=\"3\"",
                "--attribute",
                "count=-2",
                "--attribute",
                "urgent=true",
                "--attribute",
                "note=\"say \\\"hi\\\"\"",
            ]
        );
        assert_eq!(
            AttributeValue::from_json(&serde_json::json!(3)),
            Some(AttributeValue::Integer(3))
        );
        assert_eq!(AttributeValue::from_json(&serde_json::json!(1.5)), None);
    }

    #[test]
    fn a_pure_check_asks_for_the_pure_class_only() {
        assert_eq!(
            check_arguments(Some("tk-9f2x4a8m"), GateScope::Pure),
            [
                "check",
                "--source",
                "gui",
                "--ticket",
                "tk-9f2x4a8m",
                "--class",
                "pure"
            ]
        );
        assert_eq!(
            check_arguments(None, GateScope::All),
            ["check", "--source", "gui"]
        );
    }
}
