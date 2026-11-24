use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use regex::Regex;
use serde_json::{Value, json};

use crate::check::{CheckReport, CheckRequest, GateClass, is_ticket_id};
use crate::event;
use crate::log;
use crate::process::{ProcessError, ProcessStore};

/// How many runs of one ticket the store keeps.
const KEPT_RUNS: usize = 100;

static SOURCE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new("^[a-z0-9-]{1,32}$").expect("valid regex"));

/// Whether `source` may name who ran a check: lowercase letters, digits, and `-`, at most 32
/// characters.
pub(crate) fn is_source(source: &str) -> bool {
    SOURCE.is_match(source)
}

/// What became of a finished check run in the store.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum RunRecord {
    /// Nothing was to be stored: no ticket, no source, or no store.
    #[default]
    NotStored,
    /// Stored under this run ID.
    Stored(String),
    /// Storing failed with this message; the check's result stands.
    Failed(String),
}

impl ProcessStore {
    /// The stored check runs of `ticket`, newest first (concept Section 12, "Tickets and check
    /// runs for a user interface"). A store or ticket without runs has none.
    ///
    /// # Errors
    ///
    /// Returns [`ProcessError::Invalid`] if `ticket` is not a ticket ID, and
    /// [`ProcessError::IoError`] if a run file cannot be read or is not a run.
    pub fn runs(&self, ticket: &str) -> Result<Vec<Value>, ProcessError> {
        if !is_ticket_id(ticket) {
            return Err(ProcessError::Invalid {
                message: format!("`{ticket}` is not a ticket ID"),
            });
        }
        let directory = runs_directory(self.directory(), ticket);
        let mut runs = Vec::new();
        for path in run_files(&directory).map_err(|error| io(&directory, &error))? {
            let bytes = std::fs::read(&path).map_err(|error| io(&path, &error))?;
            let run: Value =
                serde_json::from_slice(&bytes).map_err(|error| ProcessError::IoError {
                    message: format!("{}: {error}", path.display()),
                })?;
            if run["ticket"] != ticket || !run["id"].is_string() {
                return Err(ProcessError::IoError {
                    message: format!("{}: not a run of {ticket}", path.display()),
                });
            }
            runs.push(run);
        }
        runs.sort_by(|a, b| b["id"].as_str().cmp(&a["id"].as_str()));
        Ok(runs)
    }

    /// The newest stored run of `ticket`, if any.
    pub(crate) fn last_run(&self, ticket: &str) -> Result<Option<Value>, ProcessError> {
        Ok(self.runs(ticket)?.into_iter().next())
    }

    pub(crate) fn store_run(
        &self,
        request: &CheckRequest,
        ticket: &str,
        source: &str,
        report: &CheckReport,
    ) -> Result<String, ProcessError> {
        let _lock = log::lock(self.directory()).map_err(|error| io(self.directory(), &error))?;
        let device = self.device()?;
        let id = format!("run-{}", event::new_event_id().trim_start_matches("ev-"));
        let class = request.class.map(|class| match class {
            GateClass::Pure => "pure",
            GateClass::Exec => "exec",
        });
        let path_text =
            |path: &Option<PathBuf>| path.as_ref().map(|path| path.display().to_string());
        let run = json!({
            "id": id,
            "ticket": ticket,
            "source": source,
            "device": device,
            "arguments": {
                "step": request.step,
                "changed": request.changed_paths,
                "class": class,
                "base": path_text(&request.base),
                "lock": path_text(&request.lock),
            },
            "status": if report.passed() { "passed" } else { "failed" },
            "duration_ms": report.duration_ms,
            "gates": report.gates,
        });
        let directory = runs_directory(self.directory(), ticket);
        std::fs::create_dir_all(&directory).map_err(|error| io(&directory, &error))?;
        let target = directory.join(format!("{id}.json"));
        let partial = directory.join(format!("{id}.partial"));
        // A reader never sees half a run: the file appears complete by its rename.
        std::fs::write(&partial, run.to_string()).map_err(|error| io(&partial, &error))?;
        std::fs::rename(&partial, &target).map_err(|error| io(&target, &error))?;
        let mut files = run_files(&directory).map_err(|error| io(&directory, &error))?;
        if files.len() > KEPT_RUNS {
            files.sort();
            for old in &files[..files.len() - KEPT_RUNS] {
                std::fs::remove_file(old).map_err(|error| io(old, &error))?;
            }
        }
        Ok(id)
    }
}

fn runs_directory(store: &Path, ticket: &str) -> PathBuf {
    store.join("runs").join(ticket)
}

fn run_files(directory: &Path) -> std::io::Result<Vec<PathBuf>> {
    let entries = match std::fs::read_dir(directory) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error),
    };
    let mut files = Vec::new();
    for entry in entries {
        let path = entry?.path();
        let is_run = path
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.starts_with("run-") && name.ends_with(".json"));
        if is_run {
            files.push(path);
        }
    }
    Ok(files)
}

fn io(path: &Path, error: &std::io::Error) -> ProcessError {
    ProcessError::IoError {
        message: format!("{}: {error}", path.display()),
    }
}
