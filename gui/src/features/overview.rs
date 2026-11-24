//! The Overview of a ticket: its head, what to do next, its steps, and its newest entries.

use dioxus::prelude::*;

use super::common::{FailureAlert, Loading, Tag, event_time};
use super::journal::JournalTable;
use super::progress::{StepPanel, needs_changed_paths, step_gates};
use super::ticket_settings::{ExternalLink, TicketSettings};
use super::{Project, Session, Tab};
use crate::hone::{CheckRun, Finding, TicketSummary, Waiting};
use crate::ui::{Button, ButtonVariant};

/// A command the Next card offers for a finding.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Action {
    /// A new decision, for a public API that changed without one.
    DraftDecision,
}

/// What the Next card shows.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Next {
    /// The current step and its position from 1.
    pub(crate) step: Option<(usize, String)>,
    pub(crate) title: String,
    pub(crate) text: String,
    /// The finding that blocks the current step, if one does.
    pub(crate) finding: Option<Finding>,
    pub(crate) action: Option<Action>,
}

/// The Next card of `summary`: the first error finding of the current step in `run`, otherwise
/// the first reason the ticket waits.
pub(crate) fn next(summary: &TicketSummary, run: Option<&CheckRun>) -> Next {
    let step = summary.current.as_ref().map(|current| {
        let position = summary
            .steps
            .iter()
            .position(|step| &step.step == current)
            .map_or(0, |index| index + 1);
        (position, current.clone())
    });
    if let (Some((_, current)), Some(run)) = (&step, run) {
        let prefix = format!("{current}/");
        let blocking = run
            .gates
            .iter()
            .filter(|gate| gate.name.starts_with(&prefix) && !gate.ok && !needs_changed_paths(gate))
            .find_map(|gate| {
                let finding = gate
                    .findings
                    .iter()
                    .find(|finding| finding.level == "error")?;
                Some((gate, finding))
            });
        if let Some((gate, finding)) = blocking {
            return Next {
                step,
                title: format!("{} fails", gate.name),
                text: finding.message.clone(),
                finding: Some(finding.clone()),
                action: (finding.code == "public-api/changed-without-decision")
                    .then_some(Action::DraftDecision),
            };
        }
    }
    let (title, text) = if summary.complete == Some(true) {
        (
            "Every required step is done".to_owned(),
            "steps-complete passes for this ticket.".to_owned(),
        )
    } else {
        match summary.waiting.first() {
            Some(Waiting::Gates { gates }) => {
                ("Gates of other steps fail".to_owned(), gates.join(", "))
            }
            Some(Waiting::Threads { threads }) => (
                "A blocking thread is open".to_owned(),
                format!(
                    "Resolve {} before the ticket can finish.",
                    threads.join(", ")
                ),
            ),
            Some(Waiting::Approval { role, gate }) => (
                format!(
                    "Waiting for an approval of {}",
                    role.as_deref().unwrap_or("a role")
                ),
                format!("{gate} fails until it is given for the current content."),
            ),
            Some(Waiting::Step { step, role }) => (
                format!("Work on {step}"),
                role.as_ref().map_or_else(
                    || "Finish its outputs, then set the step to done.".to_owned(),
                    |role| format!("Finish its outputs; {role} decides."),
                ),
            ),
            None => ("Nothing waits".to_owned(), String::new()),
        }
    };
    Next {
        step,
        title,
        text,
        finding: None,
        action: None,
    }
}

/// The Overview tab.
#[component]
pub(crate) fn Overview(ticket: String) -> Element {
    let session = use_context::<Session>();
    let project = use_context::<Project>();
    let mut chosen = use_signal(|| None::<String>);
    let summary = project.summary(&ticket);
    let tracker = project.tracker();
    // The title lives in the tracker when the record has none.
    let linked = use_resource(move || async move {
        let tracker = project.tracker()?;
        let key = match &*project.overview.read() {
            Some(Some(Ok(overview))) => overview
                .external
                .iter()
                .find(|external| external.system.eq_ignore_ascii_case(tracker.system()))?
                .key
                .clone(),
            _ => return None,
        };
        tracker.issue(&key).await.ok().flatten()
    });
    let linked_title = linked.read().clone().flatten().map(|issue| issue.title);
    let overview = match &*project.overview.read() {
        Some(Some(Ok(overview))) => overview.clone(),
        Some(Some(Err(failure))) => return rsx! { FailureAlert { failure: failure.clone() } },
        _ => return rsx! { Loading {} },
    };
    let Some(summary) = summary else {
        return rsx! { Loading {} };
    };
    let run = match &*project.check.read() {
        Some(Some(Ok(run))) => Some(run.clone()),
        _ => None,
    };
    let checking = project.check.read().is_none();
    let next = next(&summary, run.as_ref());
    let mut context = vec![ticket.clone()];
    context.extend(
        overview
            .external
            .iter()
            .map(|external| format!("{} {}", external.system, external.key)),
    );
    if let Some(started) = &summary.started {
        let date = event_time(&started.event)
            .map(|time| {
                time.split_once(' ')
                    .map_or(time.clone(), |(date, _)| date.to_owned())
            })
            .unwrap_or_default();
        context.push(format!("started {date} by {}", started.by.name));
    }
    let title = linked_title
        .or_else(|| summary.title.clone())
        .unwrap_or_else(|| ticket.clone());
    rsx! {
        div { class: "flex flex-col gap-3",
            span { class: "font-mono text-[13px] text-ink-muted", {context.join(" · ")} }
            h1 { class: "m-0 text-[36px] leading-[42px] font-bold tracking-[-0.02em] break-words", "{title}" }
            div { class: "flex flex-wrap gap-2",
                for (name , value) in overview.attributes.iter() {
                    Tag {
                        bad: name == "risk" && value.as_str() == Some("high"),
                        "{name}: {value.as_str().map(str::to_owned).unwrap_or_else(|| value.to_string())}"
                    }
                }
                Tag { inverted: true, "profile: {overview.profile}" }
                for external in overview.external.iter() {
                    if let Some(url) = external.url.clone().or_else(|| tracker.as_ref().filter(|tracker| tracker.system() == external.system).map(|tracker| tracker.issue_url(&external.key))) {
                        ExternalLink { label: format!("{} {}", external.system, external.key), url }
                    }
                }
            }
        }
        if overview.selected.as_ref() != Some(&overview.profile) {
            div { class: "border-2 border-warn px-5 py-4", role: "status",
                span { class: "text-warn", "aria-hidden": "true", "⚠ " }
                "profile-not-selected: this ticket uses {overview.profile}, but select gives "
                span { class: "font-mono", {overview.selected.clone().unwrap_or_else(|| "no profile".to_owned())} }
                " for its attributes."
            }
        }
        NextCard { next, checking }
        section {
            h2 { class: "m-0 pb-3 font-mono text-[12px] font-semibold tracking-[0.08em] uppercase", "Steps" }
            div { class: "grid border border-line", style: "grid-template-columns: repeat({summary.steps.len().max(1)}, minmax(0, 1fr))",
                for step in summary.steps.iter() {
                    {
                        let current = summary.current.as_deref() == Some(step.step.as_str());
                        let failing = run.as_ref().is_some_and(|run| !step_gates(&step.step, &run.gates).0.is_empty());
                        let (top, word, word_class) = match step.status.as_str() {
                            "done" => ("border-t-ink", "■ done", ""),
                            "skipped" => ("border-t-ink-muted border-dashed", "/ skipped", "text-ink-muted"),
                            _ if current && failing => ("border-t-signal", "× blocked", "text-bad"),
                            "waiting" => ("border-t-line", "□ waiting", "text-ink-muted"),
                            _ => ("border-t-line", "□ open", "text-ink-muted"),
                        };
                        let cell = if chosen().as_deref() == Some(step.step.as_str()) { "bg-surface-2 outline outline-2 outline-signal" } else if current { "bg-surface-2" } else { "hover:bg-surface" };
                        let name = step.step.clone();
                        rsx! {
                            button { key: "{step.step}", class: "text-left px-4 py-4 border-t-4 border-r border-r-line last:border-r-0 flex flex-col gap-2 min-w-0 {top} {cell}",
                                "aria-pressed": "{chosen().as_deref() == Some(name.as_str())}",
                                onclick: move |_| chosen.set(if chosen().as_deref() == Some(name.as_str()) { None } else { Some(name.clone()) }),
                                span { class: if step.status == "skipped" { "font-semibold line-through text-ink-muted truncate" } else { "font-semibold truncate" }, "{step.step}" }
                                span { class: "font-mono text-[13px] {word_class}", "{word}" }
                            }
                        }
                    }
                }
            }
        }
        if let Some(step) = chosen().and_then(|chosen| summary.steps.iter().find(|step| step.step == chosen).cloned()) {
            StepPanel { key: "{step.step}", ticket: ticket.clone(), step, gates: run.as_ref().map(|run| run.gates.clone()).unwrap_or_default() }
        }
        section {
            div { class: "flex items-baseline justify-between pb-3",
                h2 { class: "m-0 font-mono text-[12px] font-semibold tracking-[0.08em] uppercase", "Recent" }
                button { class: "underline text-ink-muted hover:text-ink", onclick: move |_| session.open_tab(Tab::Journal), "Journal" }
            }
            JournalTable { ticket: ticket.clone(), limit: 4 }
        }
        TicketSettings { ticket: ticket.clone(), overview }
    }
}

#[component]
fn NextCard(next: Next, checking: bool) -> Element {
    let session = use_context::<Session>();
    let heading = match &next.step {
        Some((position, step)) => format!("Next · step {position:02} {step}"),
        None => "Next".to_owned(),
    };
    rsx! {
        section { class: "border-2 border-signal px-8 py-6 flex flex-col gap-3", "aria-live": "polite",
            span { class: "font-mono text-[12px] tracking-[0.08em] uppercase text-signal", "{heading}" }
            h2 { class: "m-0 text-[22px] leading-7 font-bold", "{next.title}" }
            if checking {
                p { class: "m-0 text-ink-muted", "Running the pure gates…" }
            } else if !next.text.is_empty() {
                p { class: "m-0 text-ink-muted max-w-[70ch]", "{next.text}" }
            }
            if next.finding.is_some() || next.action.is_some() {
                div { class: "flex flex-wrap gap-3 pt-2",
                    if next.finding.is_some() {
                        Button { variant: ButtonVariant::Outline, class: "h-12 px-6",
                            onclick: move |_| session.open_tab(Tab::Check),
                            "Open finding"
                        }
                    }
                    if next.action == Some(Action::DraftDecision) {
                        Button { class: "h-12 px-6",
                            onclick: move |_| {
                                let mut dialog = session.new_artifact;
                                dialog.set(true);
                            },
                            "Draft decision"
                        }
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hone::{Gate, ThreadCounts, TicketStep};

    fn summary() -> TicketSummary {
        let step = |name: &str, status: &str| TicketStep {
            step: name.into(),
            requirement: Some("required".into()),
            status: status.into(),
            reason: None,
        };
        TicketSummary {
            ticket: "tk-9f2x4a8m".into(),
            record: true,
            title: None,
            external: Vec::new(),
            profile: Some("full".into()),
            started: None,
            authors: Vec::new(),
            steps: vec![step("decision", "done"), step("interfaces", "open")],
            current: Some("interfaces".into()),
            complete: Some(false),
            threads: ThreadCounts {
                open: 0,
                blocking: 0,
            },
            last_event: None,
            last_run: None,
            waiting: vec![Waiting::Step {
                step: "interfaces".into(),
                role: Some("architect".into()),
            }],
            problem: None,
        }
    }

    fn finding(code: &str, level: &str, message: &str) -> Finding {
        Finding {
            code: code.into(),
            level: level.into(),
            message: message.into(),
            location: None,
            path: None,
            pointer: None,
        }
    }

    fn gate(name: &str, findings: Vec<Finding>) -> Gate {
        Gate {
            name: name.into(),
            gate: "public-api-has-decision".into(),
            ok: findings.iter().all(|finding| finding.level != "error"),
            findings,
            duration_ms: 0,
        }
    }

    #[test]
    fn a_failing_gate_of_the_current_step_comes_first_with_its_action() {
        let run = CheckRun {
            status: "failed".into(),
            gates: vec![
                gate(
                    "decision/other",
                    vec![finding("x", "error", "not this step")],
                ),
                gate(
                    "interfaces/public-api",
                    vec![
                        finding("api-unchecked", "warning", "a warning"),
                        finding(
                            "public-api/changed-without-decision",
                            "error",
                            "The public API changed.",
                        ),
                    ],
                ),
            ],
            run: None,
        };
        let shown = next(&summary(), Some(&run));
        assert_eq!(shown.step, Some((2, "interfaces".into())));
        assert_eq!(shown.title, "interfaces/public-api fails");
        assert_eq!(shown.text, "The public API changed.");
        assert_eq!(shown.action, Some(Action::DraftDecision));
    }

    #[test]
    fn without_a_finding_the_first_reason_to_wait_shows() {
        let run = CheckRun {
            status: "failed".into(),
            gates: vec![gate(
                "interfaces/binding",
                vec![finding(
                    "changed-paths-missing",
                    "error",
                    "no changed paths",
                )],
            )],
            run: None,
        };
        let shown = next(&summary(), Some(&run));
        assert_eq!(shown.title, "Work on interfaces");
        assert_eq!(shown.text, "Finish its outputs; architect decides.");
        assert_eq!(shown.finding, None);
    }
}
