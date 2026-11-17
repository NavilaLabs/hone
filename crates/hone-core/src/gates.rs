use std::collections::{BTreeMap, BTreeSet};

use serde_json::{Map, Value};

use crate::anchor;
use crate::directory::{Artifact, Directory, Place};
use crate::finding::{Code, Finding};
use crate::pointer::Pointer;
use crate::process::{StepStatus, ThreadStatus, TicketEvents};
use crate::schema_valid;

pub(crate) const DIRECTORY_GATES: [&str; 3] = ["schema-valid", "ids-unique", "links-resolve"];
pub(crate) const PROCESS_GATES: [&str; 3] = [
    "no-open-blocking-threads",
    "threads-answered",
    "steps-complete",
];
const ID_LINKS: [&str; 5] = ["supersedes", "affects", "verifies", "depends-on", "relates"];

pub(crate) struct Scope<'a> {
    config: &'a Value,
    directory: &'a Directory,
    effective: Vec<Option<String>>,
}

pub(crate) struct TicketCall<'a> {
    pub(crate) ticket: &'a str,
    pub(crate) profile: &'a Map<String, Value>,
    pub(crate) step: &'a Value,
    pub(crate) changed_paths: Option<&'a [String]>,
    // `None` when the process store is unavailable.
    pub(crate) process: Option<&'a TicketEvents>,
}

impl<'a> Scope<'a> {
    pub(crate) fn new(config: &'a Value, directory: &'a Directory) -> Self {
        let mut scope = Self {
            config,
            directory,
            effective: Vec::new(),
        };
        scope.effective = scope.effective_statuses();
        scope
    }

    pub(crate) fn directory_gate(&self, gate: &str) -> Vec<Finding> {
        match gate {
            "schema-valid" => schema_valid::check_directory(self.config, self.directory),
            "ids-unique" => self.ids_unique(),
            "links-resolve" => self.links_resolve(),
            _ => unreachable!("not a directory gate: {gate}"),
        }
    }

    pub(crate) fn ticket_gate(&self, entry: &Value, call: &TicketCall<'_>) -> Vec<Finding> {
        let params = &entry["params"];
        match entry["gate"].as_str().unwrap_or_default() {
            "artifact-exists" => self.artifact_exists(params, call.ticket),
            "status-is" => self.status_is(params, call.ticket),
            "approvals-valid" => self.approvals_valid(params, call),
            "change-needs-binding" => self.change_needs_binding(params, call.changed_paths),
            gate @ ("no-open-blocking-threads" | "threads-answered" | "steps-complete") => {
                let Some(process) = call.process else {
                    return vec![Finding::error(
                        Code::ProcessStoreUnavailable,
                        "the gate needs threads or step state from the process store, which is not available",
                    )];
                };
                let mut findings = match gate {
                    "no-open-blocking-threads" => open_blocking_threads(process),
                    "threads-answered" => unanswered_threads(process, params),
                    _ => self.incomplete_steps(process, call.profile),
                };
                findings.extend(process.invalid.iter().map(|invalid| {
                    let line = invalid
                        .line
                        .map_or(String::new(), |line| format!(" line {line}"));
                    Finding::warning(
                        Code::EventInvalid,
                        format!("{}{line}: {}", invalid.path, invalid.message),
                    )
                }));
                findings
            }
            "exec" => vec![Finding::error(
                Code::Unsupported,
                "`exec` gates are not supported yet",
            )],
            plugin => vec![Finding::error(
                Code::Unsupported,
                format!("plugin gates are not supported yet (`{plugin}`)"),
            )],
        }
    }

    fn artifacts(&self) -> impl Iterator<Item = (usize, &Artifact)> {
        self.directory.artifacts.iter().enumerate()
    }

    fn of_ticket<'s>(
        &'s self,
        ticket: &'s str,
        kind: &'s str,
    ) -> impl Iterator<Item = (usize, &'s Artifact)> + 's {
        self.artifacts().filter(move |(_, artifact)| {
            artifact.type_name() == Some(kind) && artifact.field("ticket") == Some(ticket)
        })
    }

    fn is_binding(&self, kind: Option<&str>, status: Option<&str>) -> bool {
        let (Some(kind), Some(status)) = (kind, status) else {
            return false;
        };
        self.config["kinds"][kind]["statuses"][status]["binding"] == Value::Bool(true)
    }

    fn effective_statuses(&self) -> Vec<Option<String>> {
        let superseded: BTreeSet<&str> = self
            .artifacts()
            .filter(|(_, artifact)| self.is_binding(artifact.type_name(), artifact.field("status")))
            .flat_map(|(_, artifact)| id_list(artifact, "supersedes"))
            .collect();
        self.artifacts()
            .map(|(_, artifact)| {
                let declared = artifact.field("status")?;
                let replaced = artifact.id().is_some_and(|id| superseded.contains(id));
                if replaced && self.is_binding(artifact.type_name(), Some(declared)) {
                    Some("superseded".to_owned())
                } else {
                    Some(declared.to_owned())
                }
            })
            .collect()
    }

    fn effectively_binding(&self, index: usize) -> bool {
        let status = self.effective[index].as_deref();
        status != Some("superseded")
            && self.is_binding(self.directory.artifacts[index].type_name(), status)
    }

    fn ids_unique(&self) -> Vec<Finding> {
        let mut by_id: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
        for (_, artifact) in self.artifacts() {
            if let Some(id) = artifact.id() {
                by_id.entry(id).or_default().push(&artifact.path);
            }
        }
        by_id
            .into_iter()
            .filter(|(_, paths)| paths.len() > 1)
            .flat_map(|(id, paths)| {
                let all = paths.join(", ");
                paths.into_iter().map(move |path| {
                    Finding::error(Code::IdDuplicate, format!("`{id}` is carried by {all}"))
                        .at_path(path)
                        .at_pointer("/id")
                })
            })
            .collect()
    }

    fn links_resolve(&self) -> Vec<Finding> {
        let ids: BTreeSet<&str> = self.artifacts().filter_map(|(_, a)| a.id()).collect();
        let tickets: BTreeSet<&str> = self
            .artifacts()
            .filter(|(_, artifact)| artifact.place == Place::Ticket)
            .filter_map(|(_, artifact)| artifact.id())
            .collect();
        let mut findings = Vec::new();
        for (_, artifact) in self.artifacts() {
            let mut unresolved = |pointer: Pointer, target: &str, what: &str| {
                findings.push(
                    Finding::error(Code::LinkUnresolved, format!("no {what} `{target}`"))
                        .at_path(&artifact.path)
                        .at_pointer(pointer),
                );
            };
            let data = &artifact.content.data;
            for link in ID_LINKS {
                if let Some(targets) = data.get("links").and_then(|links| links[link].as_array()) {
                    for (index, target) in targets.iter().enumerate() {
                        if let Some(target) = target.as_str()
                            && !ids.contains(target)
                        {
                            unresolved(
                                Pointer::from_segments(["links", link]).child(index),
                                target,
                                "artifact",
                            );
                        }
                    }
                }
            }
            if artifact.place != Place::Ticket
                && let Some(ticket) = artifact.field("ticket")
                && !tickets.contains(ticket)
            {
                unresolved(Pointer::root().child("ticket"), ticket, "ticket record");
            }
            if let Some(observed) = data
                .get("context")
                .and_then(|context| context["observed"].as_array())
            {
                for (index, observation) in observed.iter().enumerate() {
                    if let Some(module) = observation["module"].as_str()
                        && !ids.contains(module)
                    {
                        unresolved(
                            Pointer::from_segments(["context", "observed"])
                                .child(index)
                                .child("module"),
                            module,
                            "artifact",
                        );
                    }
                }
            }
        }
        findings
    }

    fn artifact_exists(&self, params: &Value, ticket: &str) -> Vec<Finding> {
        let kind = params["type"].as_str().unwrap_or_default();
        let found = self
            .of_ticket(ticket, kind)
            .any(|(index, _)| in_selection(&params["status"], self.effective[index].as_deref()));
        if found {
            return Vec::new();
        }
        let wanted = match &params["status"] {
            Value::Null => String::new(),
            selection => format!(" with an effective status in {selection}"),
        };
        vec![Finding::error(
            Code::ArtifactMissing,
            format!("the ticket has no artifact of kind `{kind}`{wanted}"),
        )]
    }

    fn status_is(&self, params: &Value, ticket: &str) -> Vec<Finding> {
        let kind = params["type"].as_str().unwrap_or_default();
        self.of_ticket(ticket, kind)
            .filter(|(index, _)| {
                !in_selection(&params["status"], self.effective[*index].as_deref())
            })
            .map(|(index, artifact)| {
                Finding::error(
                    Code::StatusMismatch,
                    format!(
                        "effective status `{}` is not in {}",
                        self.effective[index].as_deref().unwrap_or("none"),
                        params["status"]
                    ),
                )
                .at_path(&artifact.path)
                .at_pointer("/status")
            })
            .collect()
    }

    fn approvals_valid(&self, params: &Value, call: &TicketCall<'_>) -> Vec<Finding> {
        let role = params["role"].as_str().unwrap_or_default();
        let min = params["min"].as_u64().unwrap_or(1);
        let require_proof = params["require_proof"] == Value::Bool(true);
        let outputs: Vec<&str> = call.step["outputs"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .filter(|kind| self.config["kinds"][kind]["lifecycle"] == "decided")
            .filter(|kind| params["type"].as_str().is_none_or(|wanted| wanted == *kind))
            .collect();
        let mut findings = Vec::new();
        for kind in outputs {
            for (_, artifact) in self.of_ticket(call.ticket, kind) {
                let hash = artifact.content.hash();
                let approvers: BTreeSet<&str> = artifact
                    .content
                    .data
                    .get("review")
                    .and_then(|review| review["approved_by"].as_array())
                    .into_iter()
                    .flatten()
                    .filter(|entry| entry["role"] == role && entry["hash"] == hash.as_str())
                    .filter(|entry| !require_proof || entry.get("proof").is_some())
                    .filter_map(|entry| entry["name"].as_str())
                    .filter(|name| self.actor_has_role(name, role))
                    .collect();
                if (approvers.len() as u64) < min {
                    findings.push(
                        Finding::error(
                            Code::ApprovalsMissing,
                            format!(
                                "{} valid approval(s) of role `{role}`, {min} required (content hash {hash})",
                                approvers.len()
                            ),
                        )
                        .at_path(&artifact.path),
                    );
                }
            }
        }
        findings
    }

    fn incomplete_steps(
        &self,
        process: &TicketEvents,
        profile: &Map<String, Value>,
    ) -> Vec<Finding> {
        profile
            .iter()
            .filter(|(_, strictness)| *strictness == "required")
            .map(|(step, _)| step)
            .filter(|step| !self.contains_steps_complete(step))
            .filter(|step| {
                process
                    .steps
                    .get(step.as_str())
                    .is_none_or(|state| state.status == StepStatus::Open)
            })
            .map(|step| {
                Finding::error(
                    Code::StepIncomplete,
                    format!("the required step `{step}` is neither done nor skipped"),
                )
            })
            .collect()
    }

    fn contains_steps_complete(&self, step: &str) -> bool {
        self.config["steps"][step]["gates"]
            .as_object()
            .into_iter()
            .flatten()
            .any(|(_, entry)| entry["gate"] == "steps-complete")
    }

    fn actor_has_role(&self, name: &str, role: &str) -> bool {
        let actors = &self.config["actors"];
        let roles = actors.get(name).or_else(|| actors.get("*"));
        roles
            .and_then(Value::as_array)
            .is_some_and(|roles| roles.iter().any(|item| item == role))
    }

    fn change_needs_binding(&self, params: &Value, changed: Option<&[String]>) -> Vec<Finding> {
        let Some(changed) = changed else {
            return vec![Finding::error(
                Code::ChangedPathsMissing,
                "the caller supplied no changed paths",
            )];
        };
        let anchored = params["anchored"].as_str().unwrap_or_default();
        let binding = params["binding"].as_str().unwrap_or_default();
        let mut findings = Vec::new();
        for (_, artifact) in self.artifacts() {
            if artifact.type_name() != Some(anchored) {
                continue;
            }
            let Some(id) = artifact.id() else { continue };
            let anchors = artifact
                .content
                .data
                .get("anchors")
                .and_then(Value::as_array);
            let hit: Vec<&str> = changed
                .iter()
                .filter(|path| {
                    anchors
                        .into_iter()
                        .flatten()
                        .any(|anchor| anchor::hits(anchor, path))
                })
                .map(String::as_str)
                .collect();
            if hit.is_empty() {
                continue;
            }
            let bound = self.artifacts().any(|(index, candidate)| {
                candidate.type_name() == Some(binding)
                    && id_list(candidate, "affects").any(|target| target == id)
                    && self.effectively_binding(index)
            });
            if !bound {
                findings.push(
                    Finding::error(
                        Code::BindingMissing,
                        format!(
                            "{} changes `{id}`, but no binding artifact of kind `{binding}` affects it",
                            hit.join(", ")
                        ),
                    )
                    .at_path(&artifact.path),
                );
            }
        }
        findings
    }
}

fn id_list<'a>(artifact: &'a Artifact, link: &str) -> impl Iterator<Item = &'a str> {
    artifact
        .content
        .data
        .get("links")
        .and_then(|links| links.get(link))
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
}

fn in_selection(selection: &Value, status: Option<&str>) -> bool {
    match selection {
        Value::Null => true,
        Value::Array(items) => items.iter().any(|item| item.as_str() == status),
        single => single.as_str() == status,
    }
}

fn open_blocking_threads(process: &TicketEvents) -> Vec<Finding> {
    process
        .threads
        .iter()
        .filter(|thread| thread.blocking && thread.status == ThreadStatus::Open)
        .map(|thread| {
            Finding::error(
                Code::ThreadBlockingOpen,
                format!("the blocking thread `{}` is open", thread.thread),
            )
        })
        .collect()
}

fn unanswered_threads(process: &TicketEvents, params: &Value) -> Vec<Finding> {
    let role = params["role"].as_str().unwrap_or_default();
    process
        .threads
        .iter()
        .filter(|thread| thread.opened_by.role == role && !thread.answered)
        .map(|thread| {
            Finding::error(
                Code::ThreadUnanswered,
                format!(
                    "the thread `{}` of role `{role}` has no message from anyone but {}",
                    thread.thread, thread.opened_by.name
                ),
            )
        })
        .collect()
}
