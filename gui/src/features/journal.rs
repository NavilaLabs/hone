//! A ticket's events and check runs told in sentences, newest first.

use dioxus::prelude::*;

use super::Project;
use super::common::{Empty, FailureAlert, Loading, PageHeader, event_time};
use crate::hone::{Event, Run};

/// What an entry is about, shown as a mark beside it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Kind {
    Run { passed: bool },
    Approval,
    Thread,
    Step,
    Ticket,
}

/// One line of the Journal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Entry {
    /// The event or run ID; both carry the time.
    pub(crate) id: String,
    /// Who, if a person or agent wrote it.
    pub(crate) who: Option<String>,
    pub(crate) text: String,
    pub(crate) kind: Kind,
}

/// The entries of `events` and `runs`, newest first.
pub(crate) fn entries(events: &[Event], runs: &[Run]) -> Vec<Entry> {
    let mut entries: Vec<Entry> = events.iter().map(event_entry).collect();
    entries.extend(runs.iter().map(run_entry));
    // UUIDv7 IDs sort by time after their prefix.
    entries.sort_by(|a, b| time_key(&b.id).cmp(time_key(&a.id)));
    entries
}

fn time_key(id: &str) -> &str {
    id.split_once('-').map_or(id, |(_, rest)| rest)
}

fn event_entry(event: &Event) -> Entry {
    let text = |key: &str| event.payload[key].as_str().unwrap_or_default().to_owned();
    let (sentence, kind) = match event.kind.as_str() {
        "ticket.started" => (
            format!("started the ticket with profile {}", text("profile")),
            Kind::Ticket,
        ),
        "ticket.attributes_changed" => ("changed the attributes".to_owned(), Kind::Ticket),
        "ticket.config_changed" => ("switched to the current rules".to_owned(), Kind::Ticket),
        "step.changed" => {
            let reason = event.payload["reason"]
                .as_str()
                .map_or(String::new(), |reason| format!(": {reason}"));
            (
                format!("set step {} to {}{reason}", text("step"), text("status")),
                Kind::Step,
            )
        }
        "thread.opened" => {
            let blocking = if event.payload["blocking"] == true {
                "blocking "
            } else {
                ""
            };
            (
                format!("opened a {blocking}{} thread", text("kind")),
                Kind::Thread,
            )
        }
        "thread.message" => ("replied in a thread".to_owned(), Kind::Thread),
        "thread.resolved" => ("resolved a thread".to_owned(), Kind::Thread),
        "thread.reopened" => ("reopened a thread".to_owned(), Kind::Thread),
        "approval.given" => {
            let verb = if text("decision") == "changes-requested" {
                "requested changes on"
            } else {
                "approved"
            };
            (format!("{verb} {}", text("artifact")), Kind::Approval)
        }
        "review.confirmed" => (
            format!("confirmed the review of {}", text("artifact")),
            Kind::Approval,
        ),
        other => (format!("wrote {other}"), Kind::Ticket),
    };
    Entry {
        id: event.id.clone(),
        who: event.author.as_ref().map(|author| author.name.clone()),
        text: sentence,
        kind,
    }
}

fn run_entry(run: &Run) -> Entry {
    let failed = run.gates.iter().filter(|gate| !gate.ok).count();
    let scope = run.arguments["step"]
        .as_str()
        .map_or(String::new(), |step| format!(" on {step}"));
    let outcome = match failed {
        0 => "passed".to_owned(),
        1 => "failed: 1 gate".to_owned(),
        count => format!("failed: {count} gates"),
    };
    Entry {
        id: run.id.clone(),
        who: None,
        text: format!("Check run{scope} ({}) {outcome}", run.source),
        kind: Kind::Run {
            passed: failed == 0,
        },
    }
}

/// The time of an entry, `HH:MM` UTC.
pub(crate) fn clock(id: &str) -> String {
    event_time(id)
        .and_then(|time| time.split_once(' ').map(|(_, clock)| clock.to_owned()))
        .unwrap_or_default()
}

/// The newest `limit` entries as a table, every entry without a limit.
#[component]
pub(crate) fn JournalTable(ticket: String, #[props(optional)] limit: Option<usize>) -> Element {
    let project = use_context::<Project>();
    let events = match &*project.state.read() {
        Some(Some(Ok(state))) => Some(state.events.clone()),
        Some(Some(Err(failure))) if failure.status == "unavailable" => Some(Vec::new()),
        Some(Some(Err(failure))) => return rsx! { FailureAlert { failure: failure.clone() } },
        _ => None,
    };
    let runs = match &*project.runs.read() {
        Some(Some(Ok(runs))) => runs.clone(),
        _ => Vec::new(),
    };
    let Some(events) = events else {
        return rsx! { Loading {} };
    };
    let mut all = entries(&events, &runs);
    if let Some(limit) = limit {
        all.truncate(limit);
    }
    if all.is_empty() {
        return rsx! { Empty { text: "Nothing happened in this ticket yet." } };
    }
    rsx! {
        ol { class: "m-0 p-0 list-none border border-line",
            for entry in all {
                li { key: "{entry.id}", class: "flex items-baseline gap-6 px-5 py-3 border-b border-line last:border-b-0",
                    span { class: "font-mono text-[13px] text-ink-muted w-12 shrink-0", {clock(&entry.id)} }
                    span { class: "flex-1 min-w-0",
                        if let Some(who) = &entry.who {
                            strong { "{who} " }
                        }
                        "{entry.text}"
                    }
                    KindMark { kind: entry.kind }
                }
            }
        }
    }
}

#[component]
fn KindMark(kind: Kind) -> Element {
    let (glyph, word, class) = match kind {
        Kind::Run { passed: true } => ("■", "check", "text-ok"),
        Kind::Run { passed: false } => ("×", "check", "text-bad"),
        Kind::Approval => ("■", "approval", "text-ok"),
        Kind::Thread => ("▲", "thread", "text-signal"),
        Kind::Step => ("/", "step", "text-ink-muted"),
        Kind::Ticket => ("·", "ticket", "text-ink-muted"),
    };
    rsx! {
        span { class: "font-mono text-[13px] shrink-0 {class}",
            span { "aria-hidden": "true", "{glyph} " }
            "{word}"
        }
    }
}

/// The Journal tab.
#[component]
pub(crate) fn Journal(ticket: String) -> Element {
    rsx! {
        PageHeader { context: format!("{ticket} · journal"), title: "Journal" }
        JournalTable { ticket }
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::hone::{Author, Gate};

    fn event(id: &str, kind: &str, payload: serde_json::Value) -> Event {
        Event {
            id: id.into(),
            device: "dev-3k9a5h2t".into(),
            seq: 1,
            kind: kind.into(),
            ticket: Some("tk-9f2x4a8m".into()),
            author: Some(Author {
                name: "alice".into(),
                role: "architect".into(),
            }),
            payload,
            raw: json!({}),
        }
    }

    #[test]
    fn events_and_runs_are_told_newest_first() {
        let events = [
            event(
                "ev-019a9da5-3000-7000-8000-000000000000",
                "step.changed",
                json!({ "step": "domain", "status": "skipped", "reason": "no new terms" }),
            ),
            event(
                "ev-019a9da5-5000-7000-8000-000000000000",
                "approval.given",
                json!({ "artifact": "dec-7k3m2q8p", "decision": "approved" }),
            ),
        ];
        let run = Run {
            id: "run-019a9da5-4000-7000-8000-000000000000".into(),
            source: "pre-push".into(),
            device: "dev-3k9a5h2t".into(),
            arguments: json!({ "step": "interfaces" }),
            status: "failed".into(),
            duration_ms: 12,
            gates: vec![Gate {
                name: "interfaces/public-api".into(),
                gate: "public-api-has-decision".into(),
                ok: false,
                findings: Vec::new(),
                duration_ms: 12,
            }],
        };
        let told: Vec<(Option<String>, String, Kind)> = entries(&events, &[run])
            .into_iter()
            .map(|entry| (entry.who, entry.text, entry.kind))
            .collect();
        assert_eq!(
            told,
            [
                (
                    Some("alice".into()),
                    "approved dec-7k3m2q8p".into(),
                    Kind::Approval
                ),
                (
                    None,
                    "Check run on interfaces (pre-push) failed: 1 gate".into(),
                    Kind::Run { passed: false }
                ),
                (
                    Some("alice".into()),
                    "set step domain to skipped: no new terms".into(),
                    Kind::Step
                ),
            ]
        );
    }

    #[test]
    fn a_run_id_has_a_time_like_an_event_id() {
        // 0x019a9da53000 ms = 2025-11-19 19:44 UTC, as in the test of `event_time`.
        assert_eq!(clock("run-019a9da5-3000-7000-8000-000000000000"), "19:44");
    }
}
