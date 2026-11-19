use std::collections::BTreeMap;
use std::fmt;
use std::fs::File;
use std::future::Future;
use std::path::{Path, PathBuf};

use serde::Serialize;
use serde_json::{Map, Value, json};

use crate::canonical;
use crate::check::load_ticket;
use crate::client::{self, Server, Session};
use crate::condense;
use crate::directory::Directory;
use crate::event::{
    self, APPROVAL_GIVEN, REVIEW_CONFIRMED, STEP_CHANGED, THREAD_MESSAGE, THREAD_OPENED,
    THREAD_REOPENED, THREAD_RESOLVED,
};
use crate::git;
use crate::log::{self, Cursor, InvalidEvent, Log};
use crate::manifest;
use crate::replication::{self, SyncReport};
use crate::{CheckError, Resolved};

/// The local process store of one project (concept Section 4, "Process store"): one log per
/// device, read in a deterministic order. hone keeps it in the data directory of the platform
/// (concept Section 12, "Local process store").
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcessStore {
    project_root: PathBuf,
    workspace: String,
    project: String,
    directory: PathBuf,
}

/// Who writes an event. Without a server, authorship is trust-based.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Author {
    /// The handle, for example `alice`.
    pub name: String,
    /// The role key, for example `critic`. It must exist in the ticket's snapshot.
    pub role: String,
}

/// What a thread is attached to (concept Section 10, "Anchors").
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ThreadAnchor {
    /// An artifact by ID. Its content hash is taken from the artifact's current file.
    Artifact {
        /// The artifact ID, for example `dec-7k3m2q8p`.
        artifact: String,
        /// Text to find the spot again.
        quote: Option<String>,
    },
    /// Code as `path#symbol`.
    Code {
        /// The reference, for example `src/ledger.rs#Ledger`.
        reference: String,
    },
    /// An external change, for example a pull request and a line range.
    Change {
        /// A free description.
        reference: String,
    },
    /// A position in an agent transcript.
    Transcript {
        /// The URL of the transcript.
        url: String,
        /// A position in it.
        position: Option<String>,
    },
}

/// The state of a step (concept Section 5, "Ticket record").
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum StepStatus {
    /// Neither done nor skipped.
    Open,
    /// Done.
    Done,
    /// Skipped explicitly.
    Skipped,
}

impl StepStatus {
    fn name(self) -> &'static str {
        match self {
            Self::Open => "open",
            Self::Done => "done",
            Self::Skipped => "skipped",
        }
    }
}

/// A change to the process state of a ticket, written as one event.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Change {
    /// `thread.opened`.
    OpenThread {
        /// The ticket ID.
        ticket: String,
        /// The kind of the thread, a free key such as `concern`.
        kind: String,
        /// Whether the thread blocks `no-open-blocking-threads` while it is open.
        blocking: bool,
        /// What the thread is attached to.
        anchor: ThreadAnchor,
        /// The text.
        body: String,
    },
    /// `thread.message`.
    Reply {
        /// The thread ID.
        thread: String,
        /// The text.
        body: String,
    },
    /// `thread.resolved`.
    Resolve {
        /// The thread ID.
        thread: String,
    },
    /// `thread.reopened`.
    Reopen {
        /// The thread ID.
        thread: String,
    },
    /// `step.changed`.
    SetStep {
        /// The ticket ID.
        ticket: String,
        /// A step of the ticket's profile.
        step: String,
        /// The new state.
        status: StepStatus,
        /// Why, for example why the step was skipped.
        reason: Option<String>,
    },
    /// `approval.given` for the artifact's current content hash.
    Approve {
        /// The ticket ID.
        ticket: String,
        /// The artifact ID.
        artifact: String,
        /// `true` for `changes-requested`, `false` for `approved`.
        changes_requested: bool,
        /// A free proof that the person approved, stored as `proof`.
        proof: Option<String>,
    },
    /// `review.confirmed` for the artifact's current content hash.
    ConfirmReview {
        /// The ticket ID.
        ticket: String,
        /// The artifact ID.
        artifact: String,
    },
}

/// The result of [`ProcessStore::condense`].
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Condensed {
    /// The artifact's path, relative to the project root.
    pub path: String,
    /// The `review` block written into it.
    pub review: Value,
}

/// The result of [`ProcessStore::append`].
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Appended {
    /// The event as it was written.
    pub event: Value,
    /// The thread the event belongs to, for a thread event.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thread: Option<String>,
}

/// The result of [`ProcessStore::export`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Exported {
    /// The project's directory in the target, `<target>/<workspace>/<project>/`.
    pub directory: PathBuf,
    /// The number of events written, per device that had any.
    pub events: BTreeMap<String, u64>,
}

/// The result of [`ProcessStore::forget`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Forgotten {
    /// The store directory that was removed.
    pub directory: PathBuf,
    /// The number of valid events removed, per device that had any.
    pub events: BTreeMap<String, u64>,
}

/// Whether a thread is open.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum ThreadStatus {
    /// Open, after `thread.opened` or `thread.reopened`.
    Open,
    /// Resolved, after `thread.resolved`.
    Resolved,
}

/// The computed state of a thread (concept Section 10, "Rules").
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ThreadState {
    /// The thread ID.
    pub thread: String,
    /// The kind of the thread.
    pub kind: String,
    /// Whether the thread blocks while it is open.
    pub blocking: bool,
    /// Open or resolved: the last resolution or reopening decides.
    pub status: ThreadStatus,
    /// Whether someone other than the opener wrote a message.
    pub answered: bool,
    /// Who opened the thread.
    pub opened_by: Author,
}

/// The computed state of a step.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct StepState {
    /// The status of the step's last `step.changed`.
    pub status: StepStatus,
    /// Its reason, if any.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

/// The process state of one ticket.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct TicketEvents {
    /// The valid events of the ticket in the order of concept Section 4.
    pub events: Vec<Value>,
    /// The threads of the ticket, in the order they were opened.
    pub threads: Vec<ThreadState>,
    /// The steps with at least one `step.changed`. A step without one is open.
    pub steps: BTreeMap<String, StepState>,
    /// The invalid events of the ticket and those whose ticket cannot be read.
    pub invalid: Vec<InvalidEvent>,
}

/// A peer to sync with (concept Section 4, "Replication").
#[derive(Debug, Clone, Copy)]
pub enum Peer<'a> {
    /// A shared folder: its root, which holds `<workspace>/<project>/log/…`.
    Folder(&'a Path),
    /// A replication server.
    Server(&'a Server),
    /// A branch of the project's Git repository, moved through `remote` (concept Section 12,
    /// "Process store in Git").
    Git {
        /// The remote, for example `origin`.
        remote: &'a str,
        /// The branch, for example `hone/process`.
        branch: &'a str,
    },
}

/// A call of [`ProcessStore`] could not finish.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "status", rename_all = "kebab-case")]
pub enum ProcessError {
    /// The store directory or the shared folder does not exist, for example in CI, or the
    /// server cannot be reached.
    Unavailable {
        /// What was looked for, and the cause.
        message: String,
    },
    /// The request does not fit the project, for example an unknown step, thread, or role.
    Invalid {
        /// What is wrong.
        message: String,
    },
    /// The server refused the call as a whole, for example an invalid token (concept Section 4,
    /// "Error codes").
    Refused {
        /// The server's error code, or `http-<status>` for a status without one.
        code: String,
        /// The server's message.
        message: String,
    },
    /// The project configuration, the ticket, or the store could not be read or written.
    IoError {
        /// The path and the cause.
        message: String,
    },
}

impl fmt::Display for ProcessError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unavailable { message }
            | Self::Invalid { message }
            | Self::IoError { message } => formatter.write_str(message),
            Self::Refused { code, message } => write!(formatter, "{code}: {message}"),
        }
    }
}

impl std::error::Error for ProcessError {}

impl From<CheckError> for ProcessError {
    fn from(error: CheckError) -> Self {
        Self::IoError {
            message: error.message,
        }
    }
}

impl ProcessStore {
    /// Locates the store of the project in `project_root` (the directory that contains
    /// `.hone/`) from `workspace` and `project` in `.hone/config.yaml`. The base directory is
    /// `data_directory`, otherwise the environment variable `HONE_DATA_DIR`, otherwise the data
    /// directory of the platform followed by `hone`. The store does not have to exist.
    ///
    /// # Errors
    ///
    /// Returns [`ProcessError::IoError`] if `.hone/config.yaml` cannot be read or lacks a valid
    /// `workspace` or `project`, or if no base directory can be determined.
    pub fn locate(
        project_root: &Path,
        data_directory: Option<&Path>,
    ) -> Result<Self, ProcessError> {
        let (workspace, project) = manifest::read_scope(&project_root.join(".hone"))
            .map_err(|message| ProcessError::IoError { message })?;
        let base = match data_directory {
            Some(directory) => directory.to_path_buf(),
            None => default_data_directory().ok_or_else(|| ProcessError::IoError {
                message: "no data directory: set HONE_DATA_DIR".into(),
            })?,
        };
        Ok(Self {
            project_root: project_root.to_path_buf(),
            directory: base.join(&workspace).join(&project),
            workspace,
            project,
        })
    }

    /// The project ID from `.hone/config.yaml`.
    #[must_use]
    pub fn project(&self) -> &str {
        &self.project
    }

    /// The project root, the directory that contains `.hone/`.
    #[must_use]
    pub fn root(&self) -> &Path {
        &self.project_root
    }

    /// The store directory, which holds `log/`.
    #[must_use]
    pub fn directory(&self) -> &Path {
        &self.directory
    }

    /// The process state of `ticket`.
    ///
    /// # Errors
    ///
    /// Returns [`ProcessError::Unavailable`] if the store directory does not exist and
    /// [`ProcessError::IoError`] if a segment cannot be read.
    pub fn events(&self, ticket: &str) -> Result<TicketEvents, ProcessError> {
        Ok(ticket_events(&self.load_existing()?, ticket))
    }

    /// The cursor of the store (concept Section 4, "Replication"): for every device, the
    /// highest `seq` up to which its valid events are contiguous.
    ///
    /// # Errors
    ///
    /// Returns [`ProcessError::Unavailable`] if the store directory does not exist and
    /// [`ProcessError::IoError`] if a segment cannot be read.
    pub fn cursor(&self) -> Result<Cursor, ProcessError> {
        Ok(self.load_existing()?.cursor())
    }

    /// `read(cursor)` of the replication contract: every valid event with a `seq` above the
    /// cursor's value for its device, up to the store's own cursor. The events of one device
    /// come in ascending `seq`, the devices in the order of their IDs.
    ///
    /// # Errors
    ///
    /// Returns [`ProcessError::Unavailable`] if the store directory does not exist and
    /// [`ProcessError::IoError`] if a segment cannot be read.
    pub fn read(&self, cursor: &Cursor) -> Result<Vec<Value>, ProcessError> {
        Ok(self.load_existing()?.after(cursor))
    }

    /// Syncs the store with `peer` (concept Section 4, "Replication"). The local store is
    /// created if it does not exist. Forks, conflicts, invalid lines, unsafe paths, and events
    /// the server rejects are reported, not raised.
    ///
    /// With a shared folder, it imports the missing events of every device from
    /// `<folder>/<workspace>/<project>/`, then writes the missing part of this device's own log
    /// there. With a server, it appends this device's events the server lacks, then imports
    /// the events the local store lacks.
    ///
    /// # Errors
    ///
    /// Returns [`ProcessError::Unavailable`] if the folder is not a directory, without creating
    /// anything, or if the server cannot be reached, [`ProcessError::Refused`] if the server
    /// refuses the call, and [`ProcessError::IoError`] if a store cannot be read or written.
    pub fn sync(&self, peer: Peer<'_>) -> Result<SyncReport, ProcessError> {
        match peer {
            Peer::Git { remote, branch } => self.sync_git(remote, branch),
            Peer::Folder(folder) => self.sync_folder(folder),
            Peer::Server(server) => {
                let own = self.existing_device()?;
                client::run(|| {
                    let session =
                        Session::new(server, &self.directory, &self.project, own.as_deref())?;
                    client::block_on(session.sync())
                })
            }
        }
    }

    /// Syncs the store with `server` once, then stays subscribed (concept Section 12, "Sync
    /// with a server"): imports every event as it arrives, appends new own events every 2
    /// seconds, and subscribes again after a dropped connection. `on_report` receives the
    /// result of the first sync and of every later batch received or sent. Returns when
    /// `until` completes.
    ///
    /// # Errors
    ///
    /// Returns [`ProcessError::Unavailable`] if the server cannot be reached for the first
    /// sync, [`ProcessError::Refused`] if the server refuses a call, and
    /// [`ProcessError::IoError`] if the store cannot be read or written.
    pub fn watch(
        &self,
        server: &Server,
        until: impl Future<Output = ()> + Send,
        on_report: impl FnMut(&SyncReport) + Send,
    ) -> Result<(), ProcessError> {
        let own = self.existing_device()?;
        client::run(|| {
            let session = Session::new(server, &self.directory, &self.project, own.as_deref())?;
            client::block_on(session.watch(until, on_report))
        })
    }

    fn sync_git(&self, remote: &str, branch: &str) -> Result<SyncReport, ProcessError> {
        // Both reach the Git command line as arguments; a leading `-` would be read as an option.
        let valid = |name: &str| {
            !name.is_empty()
                && !name.starts_with('-')
                && !name.contains("..")
                && name.chars().all(|character| {
                    character.is_ascii_alphanumeric() || "/._-".contains(character)
                })
        };
        if !valid(remote) || !valid(branch) {
            return Err(invalid(format!(
                "`{remote}` / `{branch}` is not a remote and branch name hone accepts: ASCII letters, digits, `/`, `.`, `_`, `-`, not starting with `-`"
            )));
        }
        let _lock = self.lock()?;
        let unavailable = |message: String| ProcessError::Unavailable { message };
        let checkout = git::Branch {
            repository: &self.project_root,
            remote,
            branch,
            worktree: git::worktree_of(&self.directory, branch),
        };
        let folder = checkout.checkout().map_err(unavailable)?.to_path_buf();
        let own = self.existing_device()?;
        let report = replication::sync_folder(
            &self.directory,
            &self.workspace,
            &self.project,
            own.as_deref(),
            &folder,
        )
        .map_err(|error| ProcessError::IoError {
            message: format!("{}: {error}", folder.display()),
        })?;
        if let Some(own) = own {
            let log = format!("{}/{}/log/{own}", self.workspace, self.project);
            if folder.join(&log).is_dir() {
                checkout
                    .publish(&log, &format!("hone: log of {own}"))
                    .map_err(unavailable)?;
            }
        }
        Ok(report)
    }

    fn sync_folder(&self, folder: &Path) -> Result<SyncReport, ProcessError> {
        if !folder.is_dir() {
            return Err(ProcessError::Unavailable {
                message: format!("no shared folder at {}", folder.display()),
            });
        }
        let _lock = self.lock()?;
        let own = self.existing_device()?;
        replication::sync_folder(
            &self.directory,
            &self.workspace,
            &self.project,
            own.as_deref(),
            folder,
        )
        .map_err(|error| ProcessError::IoError {
            message: format!("{}: {error}", folder.display()),
        })
    }

    /// Writes every device's log into `target`, a new or empty directory, in the layout of a
    /// shared folder (concept Section 12, "Tenant separation"): the complete lines of each
    /// segment, byte for byte, below `<target>/<workspace>/<project>/log/`. A sync with
    /// `target` as the folder imports them again.
    ///
    /// # Errors
    ///
    /// Returns [`ProcessError::Invalid`] if `target` is not empty, and
    /// [`ProcessError::Unavailable`] if the project has no process store, both without
    /// creating anything, and [`ProcessError::IoError`] if the store cannot be read or
    /// `target` cannot be written.
    pub fn export(&self, target: &Path) -> Result<Exported, ProcessError> {
        let io = |error: std::io::Error| ProcessError::IoError {
            message: format!("{}: {error}", target.display()),
        };
        match std::fs::read_dir(target) {
            Ok(mut entries) => {
                if entries.next().is_some() {
                    return Err(ProcessError::Invalid {
                        message: format!(
                            "{} is not empty: export into a new or empty directory",
                            target.display()
                        ),
                    });
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(io(error)),
        }
        if !self.directory.join("log").is_dir() {
            return Err(ProcessError::Unavailable {
                message: format!("no process store at {}", self.directory.display()),
            });
        }
        let _lock = self.lock()?;
        std::fs::create_dir_all(target).map_err(io)?;
        let report = replication::export(&self.directory, &self.workspace, &self.project, target)
            .map_err(io)?;
        if let Some(problem) = report.problems.first() {
            return Err(ProcessError::IoError {
                message: format!("{}: {}", problem.path, problem.message),
            });
        }
        Ok(Exported {
            directory: target.join(&self.workspace).join(&self.project),
            events: report.sent,
        })
    }

    /// Removes the project's local process store (concept Section 12, "Tenant separation"):
    /// every device's log, the device ID, and the worktrees of `hone sync --git`. Copies in a
    /// shared folder or on a server stay. A project without a store has nothing to remove.
    ///
    /// # Errors
    ///
    /// Returns [`ProcessError::IoError`] if the store cannot be read or removed.
    pub fn forget(&self) -> Result<Forgotten, ProcessError> {
        let mut forgotten = Forgotten {
            directory: self.directory.clone(),
            events: BTreeMap::new(),
        };
        if !self.directory.is_dir() {
            return Ok(forgotten);
        }
        let lock = self.lock()?;
        for (device, events) in self.load()?.chains() {
            forgotten
                .events
                .insert(device.to_owned(), events.len() as u64);
        }
        let had_worktrees = self.directory.join("git").is_dir();
        let entries = std::fs::read_dir(&self.directory).map_err(|error| self.io(&error))?;
        for entry in entries {
            let entry = entry.map_err(|error| self.io(&error))?;
            if entry.file_name() == "lock" {
                continue;
            }
            // `remove_dir_all` does not follow symbolic links, so nothing outside the store goes.
            let removed = if entry.file_type().map_err(|error| self.io(&error))?.is_dir() {
                std::fs::remove_dir_all(entry.path())
            } else {
                std::fs::remove_file(entry.path())
            };
            removed.map_err(|error| self.io(&error))?;
        }
        // The lock file goes last, after the lock is released, because some platforms cannot
        // remove an open file.
        drop(lock);
        std::fs::remove_file(self.directory.join("lock")).map_err(|error| self.io(&error))?;
        std::fs::remove_dir(&self.directory).map_err(|error| self.io(&error))?;
        if let Some(workspace) = self.directory.parent() {
            // Only an empty workspace directory goes; other projects may live in it.
            let _ = std::fs::remove_dir(workspace);
        }
        if had_worktrees {
            // Git drops the registrations of removed worktrees itself after a while; pruning now
            // only cleans up earlier, so a failure changes nothing.
            git::prune_worktrees(&self.project_root);
        }
        Ok(forgotten)
    }

    /// Writes the `review` block into the artifact `artifact` of `ticket` (concept Section 12,
    /// "Approvals and condensation"): `approved_by` from the ticket's approvals of the current
    /// content hash, and `summary` from the YAML file `summary` if given. Every other byte of
    /// the file stays unchanged. Writes no event.
    ///
    /// # Errors
    ///
    /// Returns [`ProcessError::Invalid`] if the ticket or the artifact does not exist, the
    /// summary is malformed or refused by `review_summary: approvals-only`, or the file cannot
    /// be condensed, and [`ProcessError::IoError`] if a file cannot be read or written.
    pub fn condense(
        &self,
        ticket: &str,
        artifact: &str,
        summary: Option<&Path>,
    ) -> Result<Condensed, ProcessError> {
        let (record, snapshot) = load_ticket(&self.project_root, ticket)?;
        let profile = record["profile"].as_str().unwrap_or_default();
        let approvals_only =
            snapshot.document()["profiles"][profile]["review_summary"] == "approvals-only";
        let summary = match summary {
            None => None,
            Some(_) if approvals_only => {
                return Err(invalid(format!(
                    "the profile `{profile}` keeps the summary out of the file (`review_summary: approvals-only`)"
                )));
            }
            Some(path) => {
                let bytes = std::fs::read(path).map_err(|error| ProcessError::IoError {
                    message: format!("{}: {error}", path.display()),
                })?;
                let value = crate::yaml::parse(&bytes).map_err(|violations| {
                    invalid(format!(
                        "{}: {}",
                        path.display(),
                        violations
                            .iter()
                            .map(ToString::to_string)
                            .collect::<Vec<_>>()
                            .join("; ")
                    ))
                })?;
                Some(condense::summary(&value).map_err(invalid)?)
            }
        };
        let (path, _, hash) = self.artifact(artifact)?;
        let events: Vec<Value> = self
            .load()?
            .events
            .into_iter()
            .filter(|event| event["ticket"] == ticket)
            .collect();
        let mut review = json!({ "approved_by": condense::approved_by(&events, artifact, &hash) });
        if let Some(summary) = summary {
            review["summary"] = summary;
        }
        let file = self.project_root.join(&path);
        let io = |error: std::io::Error| ProcessError::IoError {
            message: format!("{path}: {error}"),
        };
        // The project directory is untrusted like every checked directory: read the opened file
        // only if it is the regular file below the root.
        let mut opened = log::open_below(&file, &self.project_root, false)
            .map_err(io)?
            .ok_or_else(|| invalid(format!("{path}: not a regular file below the project root")))?;
        let mut bytes = Vec::new();
        std::io::Read::read_to_end(&mut opened, &mut bytes).map_err(io)?;
        let condensed = condense::rewrite(&path, &bytes, &review)
            .map_err(|message| invalid(format!("{path}: {message}")))?;
        replace(&file, &self.project_root, &condensed).map_err(io)?;
        Ok(Condensed { path, review })
    }

    /// Writes `change` as one event of this device by `author`, after checking it against the
    /// project: the ticket and its snapshot exist, the role exists in the snapshot, a step
    /// belongs to the ticket's profile, a thread was opened, an anchored or approved artifact
    /// exists, an approving role may approve, a confirming role decides. The
    /// store is created on the first write. `after` is filled with the newest event of every
    /// other device.
    ///
    /// # Errors
    ///
    /// Returns [`ProcessError::Invalid`] if the change does not fit the project and
    /// [`ProcessError::IoError`] if the project or the store cannot be read or written.
    pub fn append(&self, author: &Author, change: Change) -> Result<Appended, ProcessError> {
        let Prepared {
            ticket,
            kind,
            payload,
            thread,
        } = self.prepare(&self.load()?, author, change)?;
        let snapshot = load_ticket(&self.project_root, &ticket)?.1;
        if snapshot.document()["roles"].get(&author.role).is_none() {
            return Err(invalid(format!(
                "the role `{}` does not exist in the ticket's configuration",
                author.role
            )));
        }
        let id = event::new_event_id();
        let build = |device: &str, seq: u64, heads: Vec<String>| {
            let mut event = Map::new();
            event.insert("id".into(), json!(id));
            event.insert("project".into(), json!(self.project));
            event.insert("ticket".into(), json!(ticket));
            event.insert("device".into(), json!(device));
            event.insert("seq".into(), json!(seq));
            event.insert("type".into(), json!(kind));
            event.insert(
                "author".into(),
                json!({ "name": author.name, "role": author.role }),
            );
            if !heads.is_empty() {
                event.insert("after".into(), json!(heads));
            }
            event.insert("payload".into(), Value::Object(payload.clone()));
            Value::Object(event)
        };
        // Checked before anything is created, so that a rejected change leaves no store behind.
        let draft = build("dev-00000000", 1, Vec::new());
        if let Some(violation) = event::schema_violations(&draft).into_iter().next() {
            return Err(invalid(violation));
        }

        let _lock = self.lock()?;
        let device = self.device()?;
        let log = self.load()?;
        let mut open = log.open_segments.get(&device).cloned().unwrap_or_default();
        let seq = match open.next_seq {
            Some(seq) => seq,
            None if open.lines == 0 => 1,
            None => {
                return Err(ProcessError::IoError {
                    message: format!(
                        "log/{device}/{}.ndjson: the sequence number cannot be continued",
                        open.number
                    ),
                });
            }
        };
        let event = build(&device, seq, log.heads(&device));
        log::append_lines(
            &self.directory.join("log"),
            &device,
            &mut open,
            &[canonical::to_canonical(&event)],
        )
        .map_err(|error| self.io(&error))?;
        let thread = thread.or_else(|| (kind == THREAD_OPENED).then(|| event::thread_of(&id)));
        Ok(Appended { event, thread })
    }

    fn load(&self) -> Result<Log, ProcessError> {
        log::read(&self.directory, &self.project, &self.directory).map_err(|error| self.io(&error))
    }

    fn load_existing(&self) -> Result<Log, ProcessError> {
        if !self.directory.is_dir() {
            return Err(ProcessError::Unavailable {
                message: format!("no process store at {}", self.directory.display()),
            });
        }
        self.load()
    }

    fn lock(&self) -> Result<File, ProcessError> {
        log::lock(&self.directory).map_err(|error| self.io(&error))
    }

    fn io(&self, error: &std::io::Error) -> ProcessError {
        ProcessError::IoError {
            message: format!("{}: {error}", self.directory.display()),
        }
    }

    fn device(&self) -> Result<String, ProcessError> {
        if let Some(device) = self.existing_device()? {
            return Ok(device);
        }
        let device = event::new_device_id();
        std::fs::write(self.directory.join("device"), format!("{device}\n"))
            .map_err(|error| self.io(&error))?;
        Ok(device)
    }

    fn existing_device(&self) -> Result<Option<String>, ProcessError> {
        let path = self.directory.join("device");
        let device = match std::fs::read_to_string(&path) {
            Ok(text) => text.trim().to_owned(),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(self.io(&error)),
        };
        if manifest::is_device_id(&device) {
            Ok(Some(device))
        } else {
            Err(ProcessError::IoError {
                message: format!("{}: `{device}` is not a device ID", path.display()),
            })
        }
    }

    fn prepare(
        &self,
        log: &Log,
        author: &Author,
        change: Change,
    ) -> Result<Prepared, ProcessError> {
        let thread_event = |thread: String, kind: &'static str, body: Option<String>| {
            let opening = format!("ev-{}", thread.strip_prefix("th-").unwrap_or_default());
            let ticket = log
                .events
                .iter()
                .find(|event| event["id"] == opening.as_str() && event["type"] == THREAD_OPENED)
                .and_then(|event| event["ticket"].as_str())
                .ok_or_else(|| invalid(format!("no thread `{thread}` in the process store")))?;
            let mut payload = Map::new();
            payload.insert("thread".into(), json!(thread));
            if let Some(body) = body {
                payload.insert("body".into(), json!(body));
            }
            Ok(Prepared {
                ticket: ticket.to_owned(),
                kind,
                payload,
                thread: Some(thread),
            })
        };
        match change {
            Change::OpenThread {
                ticket,
                kind,
                blocking,
                anchor,
                body,
            } => {
                let anchor = self.anchor(anchor)?;
                let payload =
                    json!({ "kind": kind, "blocking": blocking, "anchor": anchor, "body": body });
                Ok(Prepared {
                    ticket,
                    kind: THREAD_OPENED,
                    payload: object(payload),
                    thread: None,
                })
            }
            Change::Reply { thread, body } => thread_event(thread, THREAD_MESSAGE, Some(body)),
            Change::Resolve { thread } => thread_event(thread, THREAD_RESOLVED, None),
            Change::Reopen { thread } => thread_event(thread, THREAD_REOPENED, None),
            Change::SetStep {
                ticket,
                step,
                status,
                reason,
            } => {
                let (record, snapshot) = load_ticket(&self.project_root, &ticket)?;
                let profile = record["profile"].as_str().unwrap_or_default();
                if snapshot.document()["profiles"][profile]["steps"]
                    .get(&step)
                    .is_none()
                {
                    return Err(invalid(format!(
                        "the profile `{profile}` of the ticket does not list the step `{step}`"
                    )));
                }
                let mut payload = Map::new();
                payload.insert("step".into(), json!(step));
                payload.insert("status".into(), json!(status.name()));
                if let Some(reason) = reason {
                    payload.insert("reason".into(), json!(reason));
                }
                Ok(Prepared {
                    ticket,
                    kind: STEP_CHANGED,
                    payload,
                    thread: None,
                })
            }
            Change::Approve {
                ticket,
                artifact,
                changes_requested,
                proof,
            } => {
                let snapshot = load_ticket(&self.project_root, &ticket)?.1;
                let config = snapshot.document();
                if !may_approve(config, &author.role) {
                    return Err(invalid(format!(
                        "the role `{}` may not approve: its `can` lacks `approve`",
                        author.role
                    )));
                }
                if !actor_has_role(config, &author.name, &author.role) {
                    return Err(invalid(format!(
                        "`actors` does not give `{}` the role `{}`",
                        author.name, author.role
                    )));
                }
                let (_, _, hash) = self.artifact(&artifact)?;
                let decision = if changes_requested {
                    "changes-requested"
                } else {
                    "approved"
                };
                let mut payload =
                    object(json!({ "artifact": artifact, "hash": hash, "decision": decision }));
                if let Some(proof) = proof {
                    payload.insert("proof".into(), json!(proof));
                }
                Ok(Prepared {
                    ticket,
                    kind: APPROVAL_GIVEN,
                    payload,
                    thread: None,
                })
            }
            Change::ConfirmReview { ticket, artifact } => {
                let (record, snapshot) = load_ticket(&self.project_root, &ticket)?;
                let (_, kind, hash) = self.artifact(&artifact)?;
                let config = snapshot.document();
                if !actor_has_role(config, &author.name, &author.role) {
                    return Err(invalid(format!(
                        "`actors` does not give `{}` the role `{}`",
                        author.name, author.role
                    )));
                }
                let profile = record["profile"].as_str().unwrap_or_default();
                let deciders: Vec<&str> = config["profiles"][profile]["steps"]
                    .as_object()
                    .into_iter()
                    .flatten()
                    .map(|(step, _)| &config["steps"][step])
                    .filter(|step| {
                        step["outputs"]
                            .as_array()
                            .is_some_and(|outputs| outputs.iter().any(|output| output == &kind))
                    })
                    .filter_map(|step| step["decides"].as_str())
                    .collect();
                let allowed = if deciders.is_empty() {
                    may_approve(config, &author.role)
                } else {
                    deciders.contains(&author.role.as_str())
                };
                if !allowed {
                    return Err(invalid(if deciders.is_empty() {
                        format!(
                            "the role `{}` may not confirm: no step names `decides` for `{kind}`, and the role may not approve",
                            author.role
                        )
                    } else {
                        format!(
                            "the role `{}` may not confirm: the step decides with {}",
                            author.role,
                            deciders.join(", ")
                        )
                    }));
                }
                Ok(Prepared {
                    ticket,
                    kind: REVIEW_CONFIRMED,
                    payload: object(json!({ "artifact": artifact, "hash": hash })),
                    thread: None,
                })
            }
        }
    }

    // The artifact `id` in the project's current directory: its path, kind, and content hash.
    fn artifact(&self, id: &str) -> Result<(String, String, String), ProcessError> {
        let current = Resolved::load(&self.project_root)?;
        let directory = Directory::scan(&self.project_root, current.document());
        let found = directory
            .artifacts
            .iter()
            .find(|candidate| candidate.id() == Some(id))
            .ok_or_else(|| invalid(format!("no artifact `{id}` in the project")))?;
        Ok((
            found.path.clone(),
            found.type_name().unwrap_or_default().to_owned(),
            found.content.hash(),
        ))
    }

    fn anchor(&self, anchor: ThreadAnchor) -> Result<Value, ProcessError> {
        Ok(match anchor {
            ThreadAnchor::Artifact { artifact, quote } => {
                let (_, _, hash) = self.artifact(&artifact)?;
                let mut value = json!({ "kind": "artifact", "artifact": artifact, "hash": hash });
                if let Some(quote) = quote {
                    value["quote"] = json!(quote);
                }
                value
            }
            ThreadAnchor::Code { reference } => json!({ "kind": "code", "ref": reference }),
            ThreadAnchor::Change { reference } => json!({ "kind": "change", "ref": reference }),
            ThreadAnchor::Transcript { url, position } => {
                let mut value = json!({ "kind": "transcript", "url": url });
                if let Some(position) = position {
                    value["position"] = json!(position);
                }
                value
            }
        })
    }
}

struct Prepared {
    ticket: String,
    kind: &'static str,
    payload: Map<String, Value>,
    thread: Option<String>,
}

fn ticket_events(log: &Log, ticket: &str) -> TicketEvents {
    let events: Vec<Value> = log
        .events
        .iter()
        .filter(|event| event["ticket"] == ticket)
        .cloned()
        .collect();
    let mut threads: Vec<ThreadState> = Vec::new();
    let mut steps = BTreeMap::new();
    for event in &events {
        let payload = &event["payload"];
        let text = |key: &str| payload[key].as_str().unwrap_or_default().to_owned();
        let author = Author {
            name: event["author"]["name"]
                .as_str()
                .unwrap_or_default()
                .to_owned(),
            role: event["author"]["role"]
                .as_str()
                .unwrap_or_default()
                .to_owned(),
        };
        let kind = event["type"].as_str().unwrap_or_default();
        if kind == THREAD_OPENED {
            threads.push(ThreadState {
                thread: event::thread_of(event["id"].as_str().unwrap_or_default()),
                kind: text("kind"),
                blocking: payload["blocking"] == true,
                status: ThreadStatus::Open,
                answered: false,
                opened_by: author,
            });
        } else if kind == STEP_CHANGED {
            let status = match payload["status"].as_str() {
                Some("done") => StepStatus::Done,
                Some("skipped") => StepStatus::Skipped,
                _ => StepStatus::Open,
            };
            let reason = payload["reason"].as_str().map(str::to_owned);
            steps.insert(text("step"), StepState { status, reason });
        } else if let Some(state) = threads
            .iter_mut()
            .find(|state| payload["thread"] == state.thread.as_str())
        {
            match kind {
                THREAD_MESSAGE if author.name != state.opened_by.name => state.answered = true,
                THREAD_RESOLVED => state.status = ThreadStatus::Resolved,
                THREAD_REOPENED => state.status = ThreadStatus::Open,
                _ => {}
            }
        }
    }
    let invalid = log
        .invalid
        .iter()
        .filter(|invalid| invalid.ticket.as_deref().is_none_or(|of| of == ticket))
        .cloned()
        .collect();
    TicketEvents {
        events,
        threads,
        steps,
        invalid,
    }
}

fn default_data_directory() -> Option<PathBuf> {
    if let Some(directory) = std::env::var_os("HONE_DATA_DIR").filter(|value| !value.is_empty()) {
        return Some(PathBuf::from(directory));
    }
    platform_data_directory().map(|directory| directory.join("hone"))
}

#[cfg(target_os = "macos")]
fn platform_data_directory() -> Option<PathBuf> {
    std::env::home_dir().map(|home| home.join("Library").join("Application Support"))
}

#[cfg(windows)]
fn platform_data_directory() -> Option<PathBuf> {
    std::env::var_os("APPDATA").map(PathBuf::from)
}

#[cfg(not(any(target_os = "macos", windows)))]
fn platform_data_directory() -> Option<PathBuf> {
    std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .filter(|directory| directory.is_absolute())
        .or_else(|| std::env::home_dir().map(|home| home.join(".local").join("share")))
}

fn object(value: Value) -> Map<String, Value> {
    match value {
        Value::Object(map) => map,
        _ => Map::new(),
    }
}

// A new file next to `path`, renamed over it: a rename replaces a link planted at `path` instead
// of writing through it. The parent directory is checked below `root` before the file is created
// and again just before the rename, which narrows, but does not close, the window in which a
// writer of the directory can swap the parent for a link.
fn replace(path: &Path, root: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let name = path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    let parent = path.parent().unwrap_or(root);
    let outside = || {
        std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            "the directory leads outside the project root",
        )
    };
    let canonical = parent.canonicalize()?;
    if !log::below(&canonical, root) {
        return Err(outside());
    }
    let temporary = path.with_file_name(format!(".{name}.hone-condense"));
    let written = (|| {
        let mut file = File::options()
            .write(true)
            .create_new(true)
            .open(&temporary)?;
        std::io::Write::write_all(&mut file, bytes)?;
        file.sync_all()?;
        if parent.canonicalize()? != canonical {
            return Err(outside());
        }
        std::fs::rename(&temporary, path)
    })();
    if written.is_err() {
        let _ = std::fs::remove_file(&temporary);
    }
    written
}

fn may_approve(config: &Value, role: &str) -> bool {
    config["roles"][role]["can"]
        .as_array()
        .is_some_and(|can| can.iter().any(|capability| capability == "approve"))
}

fn actor_has_role(config: &Value, name: &str, role: &str) -> bool {
    let actors = &config["actors"];
    actors
        .get(name)
        .or_else(|| actors.get("*"))
        .and_then(Value::as_array)
        .is_some_and(|roles| roles.iter().any(|item| item == role))
}

fn invalid(message: impl Into<String>) -> ProcessError {
    ProcessError::Invalid {
        message: message.into(),
    }
}
