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
use tokio::io::{AsyncBufReadExt, BufReader, Lines};
use tokio::process::{Child, ChildStderr, Command};

/// The command line, bound to one project root.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hone {
    binary: PathBuf,
    root: PathBuf,
    environment: Vec<(OsString, OsString)>,
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
        }
    }

    /// The same command line with the environment variable `name` set to `value` for every
    /// call, for example `HONE_DATA_DIR`.
    #[must_use]
    pub fn with_variable(mut self, name: impl Into<OsString>, value: impl Into<OsString>) -> Self {
        self.environment.push((name.into(), value.into()));
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
        }
        let output: Output = self.run(&["log"], &[0]).await?;
        Ok(StoreLog {
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
        let output = Command::new(&self.binary)
            .envs(self.environment.iter().map(|(name, value)| (name, value)))
            .args(arguments)
            .arg("--root")
            .arg(&self.root)
            .arg("--json")
            .stdin(Stdio::null())
            .kill_on_drop(true)
            .output()
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

fn check_arguments(ticket: Option<&str>, scope: GateScope) -> Vec<&str> {
    let mut arguments = vec!["check"];
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
    fn a_pure_check_asks_for_the_pure_class_only() {
        assert_eq!(
            check_arguments(Some("tk-9f2x4a8m"), GateScope::Pure),
            ["check", "--ticket", "tk-9f2x4a8m", "--class", "pure"]
        );
        assert_eq!(check_arguments(None, GateScope::All), ["check"]);
    }
}
