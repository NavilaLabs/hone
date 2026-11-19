use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use serde::Serialize;
use serde_json::Value;

use crate::canonical;
use crate::log::{self, Cursor, Log};
use crate::manifest;
use crate::process::ProcessError;
use crate::transport::ServerCode;

const APPROVAL_GIVEN: &str = "approval.given";

/// The store of one project on a server (concept Section 4, "Role of the server"): the same
/// layout and reading rules as a local store, with the checks a server applies to `append`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostedStore {
    directory: PathBuf,
    project: String,
}

/// Who appends, as the server knows them: the authenticated identity and the project's actors,
/// a map from handle to roles with the fallback `"*"`.
#[derive(Debug, Clone, Copy)]
pub struct Appender<'a> {
    /// The authenticated identity, compared with `author.name`.
    pub identity: &'a str,
    /// Who has which roles, compared with `author.role` of an `approval.given`.
    pub actors: &'a BTreeMap<String, BTreeSet<String>>,
}

/// An `append` that stopped at an event (concept Section 4, "Error codes"). Every event before
/// it is stored. It serializes to the error body of the transport.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Rejection {
    /// Why the event was not stored.
    pub code: ServerCode,
    /// What is wrong, for a person.
    pub message: String,
    /// The `seq` of the rejected event, if it can be read.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub seq: Option<u64>,
    /// The `id` of the rejected event, if it can be read.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub event: Option<String>,
    /// The server's cursor after the events stored before the rejected one.
    pub cursor: Cursor,
}

/// An `append` that did not finish.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AppendError {
    /// An event was rejected.
    Rejected(Rejection),
    /// The store could not be read or written.
    Store(ProcessError),
}

impl HostedStore {
    /// The store of `project` of `workspace` below the data directory `data`, in the layout of a
    /// shared folder: `<data>/<workspace>/<project>/`. The store does not have to exist.
    ///
    /// # Errors
    ///
    /// Returns [`ProcessError::Invalid`] if `workspace` or `project` is not a scope ID.
    pub fn new(data: &Path, workspace: &str, project: &str) -> Result<Self, ProcessError> {
        if !manifest::is_workspace_id(workspace) || !manifest::is_project_id(project) {
            return Err(ProcessError::Invalid {
                message: format!("`{workspace}/{project}` is not a workspace and a project ID"),
            });
        }
        Ok(Self {
            directory: data.join(workspace).join(project),
            project: project.to_owned(),
        })
    }

    /// The cursor of the store. A store that does not exist has the empty cursor.
    ///
    /// # Errors
    ///
    /// Returns [`ProcessError::IoError`] if a segment cannot be read.
    pub fn cursor(&self) -> Result<Cursor, ProcessError> {
        Ok(self.load()?.cursor())
    }

    /// `read(cursor)`: every valid event after `cursor`, one canonical line each, per device in
    /// ascending `seq` and the devices in the order of their IDs.
    ///
    /// # Errors
    ///
    /// Returns [`ProcessError::IoError`] if a segment cannot be read.
    pub fn read(&self, cursor: &Cursor) -> Result<Vec<String>, ProcessError> {
        Ok(self
            .load()?
            .after(cursor)
            .iter()
            .map(canonical::to_canonical)
            .collect())
    }

    /// `append(events)`: stores the events of `body`, one per line, in order, and returns the
    /// cursor afterwards. The last line may lack its newline. An event already held with the
    /// same bytes is accepted and changes nothing. A call that stores nothing creates nothing.
    ///
    /// # Errors
    ///
    /// Returns [`AppendError::Rejected`] for the first event that cannot be stored, after the
    /// events before it were stored, and [`AppendError::Store`] if the store cannot be read or
    /// written.
    pub fn append(&self, appender: Appender<'_>, body: &str) -> Result<Cursor, AppendError> {
        let lines = split_body(body);
        // Planned once without the lock, so that a call that stores nothing creates no store,
        // and again under the lock, where it counts.
        let draft = self.plan(&self.load().map_err(AppendError::Store)?, appender, &lines);
        if draft.accepted.is_empty() {
            return draft.finish();
        }
        let _lock =
            log::lock(&self.directory).map_err(|error| AppendError::Store(self.io(&error)))?;
        let log = self.load().map_err(AppendError::Store)?;
        let plan = self.plan(&log, appender, &lines);
        for (device, lines) in &plan.accepted {
            let mut open = log.open_segments.get(device).cloned().unwrap_or_default();
            log::append_lines(&self.directory.join("log"), device, &mut open, lines)
                .map_err(|error| AppendError::Store(self.io(&error)))?;
        }
        plan.finish()
    }

    fn plan(&self, log: &Log, appender: Appender<'_>, lines: &[&str]) -> Plan {
        let mut cursor = log.cursor();
        let holes: BTreeSet<&str> = log
            .open_segments
            .iter()
            .filter(|(device, open)| {
                let next = cursor.get(*device).copied().unwrap_or(0) + 1;
                open.number != 0 && open.next_seq != Some(next) && !(open.lines == 0 && next == 1)
            })
            .map(|(device, _)| device.as_str())
            .collect();
        let mut slots: BTreeMap<(String, u64), String> = BTreeMap::new();
        let mut ids: BTreeSet<String> = BTreeSet::new();
        // A device belongs to the author of its first event, so that nobody can append under a
        // foreign device ID and block it with a fork.
        let mut owners: BTreeMap<String, String> = log
            .chains()
            .into_iter()
            .filter_map(|(device, chain)| {
                let author = chain.first()?["author"]["name"].as_str()?;
                Some((device.to_owned(), author.to_owned()))
            })
            .collect();
        for event in &log.events {
            let line = canonical::to_canonical(event);
            slots.insert(
                (
                    event["device"].as_str().unwrap_or_default().to_owned(),
                    event["seq"].as_u64().unwrap_or_default(),
                ),
                line,
            );
            ids.insert(event["id"].as_str().unwrap_or_default().to_owned());
        }
        let mut plan = Plan::default();
        for (index, line) in lines.iter().enumerate() {
            let event = log::parse_line(line.as_bytes());
            let reject = |code: ServerCode, message: String| Rejection {
                code,
                message: format!("line {}: {message}", index + 1),
                seq: event.as_ref().and_then(|event| event["seq"].as_u64()),
                event: event
                    .as_ref()
                    .and_then(|event| event["id"].as_str())
                    .map(str::to_owned),
                cursor: Cursor::new(),
            };
            let Some(event) = event.as_ref() else {
                let message = if line.is_empty() {
                    "the line is empty"
                } else {
                    "the line is not JSON"
                };
                plan.rejection = Some(reject(ServerCode::InvalidEvent, message.into()));
                break;
            };
            let device = event["device"].as_str().unwrap_or_default().to_owned();
            let seq = event["seq"].as_u64().unwrap_or_default();
            let id = event["id"].as_str().unwrap_or_default().to_owned();
            let next = cursor.get(&device).copied().unwrap_or(0) + 1;
            let problem = if let Some(problem) = log::line_problem(event, line) {
                Some((ServerCode::InvalidEvent, problem))
            } else if event["project"] != self.project.as_str() {
                Some((
                    ServerCode::ScopeMismatch,
                    format!(
                        "the event belongs to `{}`, not `{}`",
                        event["project"].as_str().unwrap_or_default(),
                        self.project
                    ),
                ))
            } else if event["author"]["name"] != appender.identity {
                Some((
                    ServerCode::IdentityMismatch,
                    format!(
                        "the author is `{}`, the authenticated identity `{}`",
                        event["author"]["name"].as_str().unwrap_or_default(),
                        appender.identity
                    ),
                ))
            } else if let Some(owner) = owners
                .get(&device)
                .filter(|owner| owner.as_str() != appender.identity)
            {
                Some((
                    ServerCode::IdentityMismatch,
                    format!("the device `{device}` belongs to `{owner}` on the server"),
                ))
            } else if event["type"] == APPROVAL_GIVEN && !has_role(appender, event) {
                Some((
                    ServerCode::RoleMismatch,
                    format!(
                        "`actors` does not give `{}` the role `{}`",
                        appender.identity,
                        event["author"]["role"].as_str().unwrap_or_default()
                    ),
                ))
            } else if let Some(held) = slots.get(&(device.clone(), seq)) {
                if held == line {
                    continue;
                }
                let held_id = log::parse_line(held.as_bytes())
                    .and_then(|held| held["id"].as_str().map(str::to_owned))
                    .unwrap_or_default();
                Some(if held_id == id {
                    (
                        ServerCode::IdConflict,
                        format!("the server holds `{id}` with other content"),
                    )
                } else {
                    (
                        ServerCode::Fork,
                        format!("`seq` {seq} of `{device}` is `{held_id}` on the server"),
                    )
                })
            } else if ids.contains(&id) {
                Some((
                    ServerCode::IdConflict,
                    format!("the server holds `{id}` at another place"),
                ))
            } else if seq != next || holes.contains(device.as_str()) {
                Some((
                    ServerCode::SeqGap,
                    format!(
                        "`seq` is {seq}, the log of `{device}` on the server continues with {next}"
                    ),
                ))
            } else {
                None
            };
            if let Some((code, message)) = problem {
                plan.rejection = Some(reject(code, message));
                break;
            }
            slots.insert((device.clone(), seq), (*line).to_owned());
            ids.insert(id);
            owners
                .entry(device.clone())
                .or_insert_with(|| appender.identity.to_owned());
            cursor.insert(device.clone(), seq);
            plan.accepted
                .entry(device)
                .or_default()
                .push((*line).to_owned());
        }
        plan.cursor = cursor;
        plan
    }

    fn load(&self) -> Result<Log, ProcessError> {
        log::read(&self.directory, &self.project, &self.directory).map_err(|error| self.io(&error))
    }

    fn io(&self, error: &std::io::Error) -> ProcessError {
        ProcessError::IoError {
            message: format!("{}: {error}", self.directory.display()),
        }
    }
}

#[derive(Default)]
struct Plan {
    accepted: BTreeMap<String, Vec<String>>,
    rejection: Option<Rejection>,
    cursor: Cursor,
}

impl Plan {
    fn finish(self) -> Result<Cursor, AppendError> {
        match self.rejection {
            Some(rejection) => Err(AppendError::Rejected(Rejection {
                cursor: self.cursor,
                ..rejection
            })),
            None => Ok(self.cursor),
        }
    }
}

fn split_body(body: &str) -> Vec<&str> {
    let mut lines: Vec<&str> = body.split('\n').collect();
    if lines.last() == Some(&"") {
        lines.pop();
    }
    lines
}

fn has_role(appender: Appender<'_>, event: &Value) -> bool {
    let role = event["author"]["role"].as_str().unwrap_or_default();
    appender
        .actors
        .get(appender.identity)
        .or_else(|| appender.actors.get("*"))
        .is_some_and(|roles| roles.contains(role))
}
