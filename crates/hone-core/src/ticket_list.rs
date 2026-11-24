use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use serde::Serialize;
use serde_json::Value;

use crate::catalog::{ExternalTicket, catalog, profile_steps};
use crate::check::{CheckError, is_ticket_id, load_ticket};
use crate::event::TICKET_STARTED;
use crate::gates::incomplete_steps;
use crate::process::{Author, ProcessStore, StepStatus, ThreadStatus, TicketEvents};

/// Every ticket of a project with its progress (concept Section 12, "Tickets and check runs for
/// a user interface").
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TicketList {
    /// The tickets, the one with the newest event first, then by ticket ID.
    pub tickets: Vec<TicketSummary>,
}

/// One ticket of a [`TicketList`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TicketSummary {
    /// The ticket ID.
    pub ticket: String,
    /// Whether the checked directory holds the ticket's record; without one, only the process
    /// store knows the ticket.
    pub record: bool,
    /// The record's title, as `hone artifacts` takes it.
    pub title: Option<String>,
    /// The record's external tickets.
    pub external: Vec<ExternalTicket>,
    /// The record's profile, or without a record the profile of `ticket.started`.
    pub profile: Option<String>,
    /// Who started the ticket, from `ticket.started`.
    pub started: Option<Started>,
    /// Every name that wrote an event of the ticket, sorted.
    pub authors: Vec<String>,
    /// The steps, in the order of `hone ticket show`.
    pub steps: Vec<TicketStep>,
    /// The first open step.
    pub current: Option<String>,
    /// What `steps-complete` decides for the ticket; `null` without a readable record.
    pub complete: Option<bool>,
    /// The ticket's threads.
    pub threads: ThreadCounts,
    /// The ID of the ticket's newest event.
    pub last_event: Option<String>,
    /// The newest stored check run.
    pub last_run: Option<LastRun>,
    /// Why the ticket waits, in a fixed order; empty for a complete ticket.
    pub waiting: Vec<Waiting>,
    /// Why the record or its snapshot cannot be read, if so.
    pub problem: Option<String>,
}

/// Who started a ticket.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Started {
    /// The author of `ticket.started`.
    pub by: Author,
    /// The ID of `ticket.started`.
    pub event: String,
}

/// A step of a [`TicketSummary`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TicketStep {
    /// The step's name.
    pub step: String,
    /// `required` or `optional`; `null` without a record.
    pub requirement: Option<String>,
    /// `done`, `skipped`, `waiting` (open while a step in its `after` is neither done nor
    /// skipped), or `open`.
    pub status: String,
    /// The reason of its last `step.changed`.
    pub reason: Option<String>,
}

/// The open threads of a ticket.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct ThreadCounts {
    /// Open threads.
    pub open: usize,
    /// Open blocking threads.
    pub blocking: usize,
}

/// The newest stored check run of a ticket.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct LastRun {
    /// The run ID.
    pub id: String,
    /// `passed` or `failed`.
    pub status: String,
    /// The names of the gates that failed.
    pub failed: Vec<String>,
    #[serde(skip)]
    changed_paths_missing: BTreeSet<String>,
}

/// A reason a ticket waits.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum Waiting {
    /// Gates of the last run failed, other than `approvals-valid`.
    Gates {
        /// Their names.
        gates: Vec<String>,
    },
    /// Blocking threads are open.
    Threads {
        /// Their IDs.
        threads: Vec<String>,
    },
    /// An `approvals-valid` gate of the last run failed.
    Approval {
        /// The role its parameters name.
        role: Option<String>,
        /// The gate's name.
        gate: String,
    },
    /// The current step waits for its work.
    Step {
        /// The current step.
        step: String,
        /// The role of its `decides`.
        role: Option<String>,
    },
}

/// Lists every ticket of the project in `project_root`: the ticket records of the checked
/// directory and the tickets of the process store without a record. `data_directory` is the
/// base directory of the process store, as for [`ProcessStore::locate`].
///
/// # Errors
///
/// Returns a [`CheckError`] if `.hone/config.yaml` or `.hone/resolved.json` cannot be read, or
/// if the process store or a stored run cannot be read.
pub fn ticket_list(
    project_root: &Path,
    data_directory: Option<&Path>,
) -> Result<TicketList, CheckError> {
    let records = catalog(project_root)?;
    let store = ProcessStore::locate(project_root, data_directory).map_err(store_error)?;
    let mut events = store.every_ticket().map_err(store_error)?;
    let mut tickets = Vec::new();
    for entry in records
        .artifacts
        .iter()
        .filter(|entry| entry.kind == "ticket")
    {
        let Some(ticket) = entry.id.clone() else {
            continue;
        };
        let process = events.remove(&ticket).unwrap_or_else(no_events);
        let mut summary = summary(&store, &ticket, &process)?;
        summary.record = true;
        summary.title.clone_from(&entry.title);
        summary.external.clone_from(&entry.external);
        summary.profile.clone_from(&entry.profile);
        match load_ticket(project_root, &ticket) {
            Ok((record, snapshot)) => {
                progress(&mut summary, &record, snapshot.document(), &process);
            }
            Err(CheckError { message }) => summary.problem = Some(message),
        }
        tickets.push(summary);
    }
    for (ticket, process) in events {
        let mut summary = summary(&store, &ticket, &process)?;
        summary.profile = process
            .events
            .iter()
            .find(|event| event["type"] == TICKET_STARTED)
            .and_then(|event| event["payload"]["profile"].as_str())
            .map(str::to_owned);
        summary.steps = process
            .steps
            .iter()
            .map(|(step, state)| TicketStep {
                step: step.clone(),
                requirement: None,
                status: status_name(state.status).to_owned(),
                reason: state.reason.clone(),
            })
            .collect();
        tickets.push(summary);
    }
    tickets.sort_by(|a, b| {
        b.last_event
            .cmp(&a.last_event)
            .then_with(|| a.ticket.cmp(&b.ticket))
    });
    Ok(TicketList { tickets })
}

fn summary(
    store: &ProcessStore,
    ticket: &str,
    process: &TicketEvents,
) -> Result<TicketSummary, CheckError> {
    let started = process
        .events
        .iter()
        .find(|event| event["type"] == TICKET_STARTED)
        .map(|event| Started {
            by: author(event),
            event: event["id"].as_str().unwrap_or_default().to_owned(),
        });
    let authors: BTreeSet<String> = process
        .events
        .iter()
        .filter_map(|event| event["author"]["name"].as_str())
        .map(str::to_owned)
        .collect();
    let open = |blocking: bool| {
        process
            .threads
            .iter()
            .filter(|thread| thread.status == ThreadStatus::Open && (thread.blocking || !blocking))
            .count()
    };
    let stored = if is_ticket_id(ticket) {
        store.last_run(ticket).map_err(store_error)?
    } else {
        None
    };
    let last_run = stored.map(|run| {
        let failed: Vec<&Value> = run["gates"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|gate| gate["ok"] == false)
            .collect();
        let names = |gates: &[&Value]| -> Vec<String> {
            gates
                .iter()
                .filter_map(|gate| gate["name"].as_str())
                .map(str::to_owned)
                .collect()
        };
        let unchecked: Vec<&Value> = failed
            .iter()
            .copied()
            .filter(|gate| needs_changed_paths(gate))
            .collect();
        LastRun {
            id: run["id"].as_str().unwrap_or_default().to_owned(),
            status: run["status"].as_str().unwrap_or_default().to_owned(),
            failed: names(&failed),
            changed_paths_missing: names(&unchecked).into_iter().collect(),
        }
    });
    Ok(TicketSummary {
        ticket: ticket.to_owned(),
        record: false,
        title: None,
        external: Vec::new(),
        profile: None,
        started,
        authors: authors.into_iter().collect(),
        steps: Vec::new(),
        current: None,
        complete: None,
        threads: ThreadCounts {
            open: open(false),
            blocking: open(true),
        },
        last_event: process
            .events
            .iter()
            .filter_map(|event| event["id"].as_str())
            .max()
            .map(str::to_owned),
        last_run,
        waiting: Vec::new(),
        problem: None,
    })
}

fn progress(
    summary: &mut TicketSummary,
    record: &serde_json::Map<String, Value>,
    config: &Value,
    process: &TicketEvents,
) {
    let profile_name = record
        .get("profile")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let finished = |step: &str| {
        process
            .steps
            .get(step)
            .is_some_and(|state| state.status != StepStatus::Open)
    };
    summary.steps = profile_steps(config, profile_name)
        .into_iter()
        .map(|step| {
            let state = process.steps.get(&step.step);
            let status = match state.map(|state| state.status) {
                Some(status @ (StepStatus::Done | StepStatus::Skipped)) => status_name(status),
                _ if step.after.iter().all(|earlier| finished(earlier)) => "open",
                _ => "waiting",
            };
            TicketStep {
                requirement: Some(step.requirement),
                status: status.to_owned(),
                reason: state.and_then(|state| state.reason.clone()),
                step: step.step,
            }
        })
        .collect();
    summary.current = summary
        .steps
        .iter()
        .find(|step| step.status == "open")
        .map(|step| step.step.clone());
    let profile = config["profiles"][profile_name]["steps"]
        .as_object()
        .cloned()
        .unwrap_or_default();
    let complete = incomplete_steps(config, process, &profile).is_empty();
    summary.complete = Some(complete);
    if !complete {
        summary.waiting = waiting(summary, config, process);
    }
}

fn needs_changed_paths(gate: &Value) -> bool {
    let mut errors = gate["findings"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|finding| finding["level"] == "error")
        .peekable();
    errors.peek().is_some() && errors.all(|finding| finding["code"] == "changed-paths-missing")
}

fn waiting(summary: &TicketSummary, config: &Value, process: &TicketEvents) -> Vec<Waiting> {
    let failed: Vec<String> = summary
        .last_run
        .iter()
        .flat_map(|run| {
            run.failed
                .iter()
                .filter(|name| !run.changed_paths_missing.contains(*name))
        })
        .cloned()
        .collect();
    let entry = |name: &str| {
        let (step, label) = name.split_once('/')?;
        Some(&config["steps"][step]["gates"][label])
    };
    let is_approval =
        |name: &str| entry(name).is_some_and(|entry| entry["gate"] == "approvals-valid");
    let mut reasons = Vec::new();
    let gates: Vec<String> = failed
        .iter()
        .filter(|name| !is_approval(name))
        .cloned()
        .collect();
    if !gates.is_empty() {
        reasons.push(Waiting::Gates { gates });
    }
    let threads: Vec<String> = process
        .threads
        .iter()
        .filter(|thread| thread.blocking && thread.status == ThreadStatus::Open)
        .map(|thread| thread.thread.clone())
        .collect();
    if !threads.is_empty() {
        reasons.push(Waiting::Threads { threads });
    }
    for name in failed.iter().filter(|name| is_approval(name)) {
        reasons.push(Waiting::Approval {
            role: entry(name)
                .and_then(|entry| entry["params"]["role"].as_str())
                .map(str::to_owned),
            gate: name.clone(),
        });
    }
    if let Some(step) = &summary.current {
        reasons.push(Waiting::Step {
            step: step.clone(),
            role: config["steps"][step]["decides"].as_str().map(str::to_owned),
        });
    }
    reasons
}

fn author(event: &Value) -> Author {
    let text = |key: &str| event["author"][key].as_str().unwrap_or_default().to_owned();
    Author {
        name: text("name"),
        role: text("role"),
    }
}

fn status_name(status: StepStatus) -> &'static str {
    match status {
        StepStatus::Open => "open",
        StepStatus::Done => "done",
        StepStatus::Skipped => "skipped",
    }
}

fn no_events() -> TicketEvents {
    TicketEvents {
        events: Vec::new(),
        threads: Vec::new(),
        steps: BTreeMap::new(),
        invalid: Vec::new(),
    }
}

fn store_error(error: impl ToString) -> CheckError {
    CheckError {
        message: error.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::process::ThreadState;

    fn thread(id: &str, blocking: bool, status: ThreadStatus) -> ThreadState {
        ThreadState {
            thread: id.into(),
            kind: "concern".into(),
            blocking,
            status,
            answered: false,
            opened_by: Author {
                name: "bob".into(),
                role: "human".into(),
            },
        }
    }

    #[test]
    fn a_gate_is_unchecked_only_when_every_error_is_changed_paths_missing() {
        let finding = |level: &str, code: &str| json!({ "level": level, "code": code });
        let gate = |findings: Vec<Value>| json!({ "ok": false, "findings": findings });
        assert!(needs_changed_paths(&gate(vec![
            finding("error", "changed-paths-missing"),
            finding("warning", "path-mismatch"),
        ])));
        assert!(!needs_changed_paths(&gate(vec![
            finding("error", "changed-paths-missing"),
            finding("error", "path-mismatch"),
        ])));
        assert!(!needs_changed_paths(&gate(vec![finding(
            "warning",
            "changed-paths-missing"
        )])));
    }

    #[test]
    fn waiting_names_gates_threads_approvals_and_the_deciding_role_in_order() {
        let config = json!({ "steps": {
            "decision": {
                "decides": "architect",
                "gates": {
                    "approved": { "gate": "approvals-valid", "params": { "role": "architect" } },
                    "accepted": { "gate": "status-is", "params": {} },
                },
            },
        }});
        let mut process = no_events();
        process.threads = vec![
            thread("th-open-blocking", true, ThreadStatus::Open),
            thread("th-open-question", false, ThreadStatus::Open),
            thread("th-resolved", true, ThreadStatus::Resolved),
        ];
        let summary = TicketSummary {
            ticket: "tk-9f2x4a8m".into(),
            record: true,
            title: None,
            external: Vec::new(),
            profile: None,
            started: None,
            authors: Vec::new(),
            steps: Vec::new(),
            current: Some("decision".into()),
            complete: Some(false),
            threads: ThreadCounts {
                open: 2,
                blocking: 1,
            },
            last_event: None,
            last_run: Some(LastRun {
                id: "run-1".into(),
                status: "failed".into(),
                failed: vec![
                    "decision/approved".into(),
                    "decision/accepted".into(),
                    "decision/bound".into(),
                ],
                changed_paths_missing: BTreeSet::from(["decision/bound".into()]),
            }),
            waiting: Vec::new(),
            problem: None,
        };
        assert_eq!(
            serde_json::to_value(waiting(&summary, &config, &process)).expect("serializable"),
            json!([
                { "kind": "gates", "gates": ["decision/accepted"] },
                { "kind": "threads", "threads": ["th-open-blocking"] },
                { "kind": "approval", "role": "architect", "gate": "decision/approved" },
                { "kind": "step", "step": "decision", "role": "architect" },
            ])
        );
    }
}
