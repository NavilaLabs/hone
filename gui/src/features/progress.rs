//! The progress of a ticket through its profile's steps: state, order, and gates per step.

use dioxus::prelude::*;

use super::Session;
use super::common::{FailureAlert, Field, SectionLabel};
use crate::hone::{CheckRun, Failure, Gate, StepStatus, TicketEvents, TicketOverview, Write};
use crate::ui::{Button, ButtonVariant, Input};

/// The state of a step as the track shows it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Stage {
    Done,
    Skipped,
    /// Open, and every step before it is done or skipped.
    Ready,
    /// Open, with a step before it still open.
    Blocked,
}

/// The gates of a step in the last check.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Gates {
    pub(crate) passed: usize,
    pub(crate) failed: Vec<Gate>,
    /// Gates that failed only because the GUI knows no changed paths, which a hook or CI
    /// supplies (`changed-paths-missing`).
    pub(crate) unchecked: Vec<Gate>,
}

/// Whether every error of a failed gate is `changed-paths-missing`.
fn needs_changed_paths(gate: &Gate) -> bool {
    let mut errors = gate
        .findings
        .iter()
        .filter(|finding| finding.level == "error")
        .peekable();
    errors.peek().is_some() && errors.all(|finding| finding.code == "changed-paths-missing")
}

/// A step of the ticket's profile with its progress.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct StepProgress {
    pub(crate) step: String,
    pub(crate) title: Option<String>,
    pub(crate) required: bool,
    pub(crate) stage: Stage,
    pub(crate) reason: Option<String>,
    /// The steps before it that are still open.
    pub(crate) waiting_for: Vec<String>,
    /// The first ready step; the one to work on.
    pub(crate) current: bool,
    /// `None` when the check did not run the step's gates.
    pub(crate) gates: Option<Gates>,
}

/// The steps of `overview` in its order, with their state from `state` and their gates from
/// `run`. A step without a `step.changed` is open (concept Section 5).
pub(crate) fn progress(
    overview: &TicketOverview,
    state: Option<&TicketEvents>,
    run: Option<&CheckRun>,
) -> Vec<StepProgress> {
    let status = |step: &str| {
        state
            .and_then(|state| state.steps.get(step))
            .map_or("open", |state| state.status.as_str())
    };
    let finished = |step: &str| matches!(status(step), "done" | "skipped");
    let mut current_taken = false;
    overview
        .steps
        .iter()
        .map(|step| {
            let waiting_for: Vec<String> = step
                .after
                .iter()
                .filter(|earlier| !finished(earlier))
                .cloned()
                .collect();
            let stage = match status(&step.step) {
                "done" => Stage::Done,
                "skipped" => Stage::Skipped,
                _ if waiting_for.is_empty() => Stage::Ready,
                _ => Stage::Blocked,
            };
            let current = stage == Stage::Ready && !current_taken;
            current_taken |= current;
            let prefix = format!("{}/", step.step);
            let gates = run.and_then(|run| {
                let own: Vec<&Gate> = run
                    .gates
                    .iter()
                    .filter(|gate| gate.name.starts_with(&prefix))
                    .collect();
                (!own.is_empty()).then(|| Gates {
                    passed: own.iter().filter(|gate| gate.ok).count(),
                    failed: own
                        .iter()
                        .filter(|gate| !gate.ok && !needs_changed_paths(gate))
                        .map(|gate| (*gate).clone())
                        .collect(),
                    unchecked: own
                        .iter()
                        .filter(|gate| !gate.ok && needs_changed_paths(gate))
                        .map(|gate| (*gate).clone())
                        .collect(),
                })
            });
            StepProgress {
                step: step.step.clone(),
                title: step.title.clone(),
                required: step.requirement == "required",
                stage,
                reason: state
                    .and_then(|state| state.steps.get(&step.step))
                    .and_then(|state| state.reason.clone()),
                waiting_for,
                current,
                gates,
            }
        })
        .collect()
}

/// A one-line summary: steps finished of all, skipped, and required steps still open.
pub(crate) fn summary(steps: &[StepProgress]) -> String {
    let done = steps
        .iter()
        .filter(|step| step.stage == Stage::Done)
        .count();
    let skipped = steps
        .iter()
        .filter(|step| step.stage == Stage::Skipped)
        .count();
    let open_required = steps
        .iter()
        .filter(|step| step.required && matches!(step.stage, Stage::Ready | Stage::Blocked))
        .count();
    let mut text = format!("{} of {} finished", done + skipped, steps.len());
    if skipped > 0 {
        text.push_str(&format!(" · {skipped} skipped"));
    }
    if open_required > 0 {
        text.push_str(&format!(" · {open_required} required open"));
    } else {
        text.push_str(" · every required step finished");
    }
    text
}

/// The steps of a ticket side by side, with a panel to change the selected one.
#[component]
pub(crate) fn StepTrack(
    ticket: String,
    overview: TicketOverview,
    state: Option<TicketEvents>,
    run: Option<Result<CheckRun, Failure>>,
    checking: bool,
) -> Element {
    let ran = run.as_ref().and_then(|run| run.as_ref().ok());
    let steps = progress(&overview, state.as_ref(), ran);
    let mut selected = use_signal(|| None::<String>);
    let chosen = selected()
        .filter(|chosen| steps.iter().any(|step| &step.step == chosen))
        .or_else(|| {
            steps
                .iter()
                .find(|step| step.current)
                .map(|step| step.step.clone())
        })
        .or_else(|| steps.first().map(|step| step.step.clone()));
    let detail = steps
        .iter()
        .find(|step| Some(&step.step) == chosen.as_ref())
        .cloned();
    let gate_note = if checking {
        "checking gates…".to_owned()
    } else {
        match &run {
            Some(Ok(run)) => {
                let failed = run.gates.iter().filter(|gate| !gate.ok).count();
                format!(
                    "pure gates: {} of {} pass",
                    run.gates.len() - failed,
                    run.gates.len()
                )
            }
            Some(Err(_)) => "gates could not run".to_owned(),
            None => String::new(),
        }
    };
    let columns = steps.len().clamp(1, 7);
    rsx! {
        section { class: "flex flex-col gap-3",
            SectionLabel { label: "Steps", aside: format!("{} · {gate_note}", summary(&steps)) }
            div {
                class: "grid border border-line",
                style: "grid-template-columns: repeat(auto-fit, minmax(max(150px, calc(100% / {columns})), 1fr));",
                role: "list",
                for (index , step) in steps.iter().cloned().enumerate() {
                    StepCell {
                        number: index + 1,
                        selected: Some(&step.step) == chosen.as_ref(),
                        step,
                        onselect: move |name: String| selected.set(Some(name)),
                    }
                }
            }
            if let Some(Err(failure)) = run {
                FailureAlert { failure }
            }
            if let Some(step) = detail {
                StepPanel { key: "{step.step}", ticket, step }
            }
        }
    }
}

#[component]
fn StepCell(
    number: usize,
    step: StepProgress,
    selected: bool,
    onselect: EventHandler<String>,
) -> Element {
    let failing = step.gates.as_ref().map_or(0, |gates| gates.failed.len());
    let (edge, fill) = match (step.stage, step.current) {
        (Stage::Done, _) => ("border-t-ink", ""),
        (Stage::Skipped, _) => ("border-t-ink-muted border-dashed", ""),
        (_, true) => ("border-t-signal", "bg-surface"),
        (Stage::Blocked, _) => ("border-t-line", ""),
        (Stage::Ready, false) => ("border-t-line", ""),
    };
    let ring = if selected {
        "outline-2 outline-signal -outline-offset-2"
    } else {
        ""
    };
    let name_class = match step.stage {
        Stage::Skipped => "font-semibold break-words line-through text-ink-muted",
        Stage::Blocked => "font-semibold break-words text-ink-muted",
        _ => "font-semibold break-words",
    };
    let (glyph, word, mark_class) = match step.stage {
        Stage::Done => ("■", "done".to_owned(), "text-ok"),
        Stage::Skipped => (
            "⁄",
            step.reason.as_ref().map_or_else(
                || "skipped".to_owned(),
                |reason| format!("skipped · {reason}"),
            ),
            "text-ink-muted",
        ),
        Stage::Ready => ("□", "open".to_owned(), "text-ink"),
        Stage::Blocked => (
            "◌",
            format!("waits for {}", step.waiting_for.join(", ")),
            "text-ink-muted",
        ),
    };
    let label = step.title.clone().unwrap_or_else(|| step.step.clone());
    let name = step.step.clone();
    rsx! {
        button {
            role: "listitem",
            class: "border-t-[3px] {edge} {fill} {ring} p-4 flex flex-col gap-2 border-r border-b border-line min-w-0 text-left hover:bg-surface-2 cursor-pointer",
            "aria-pressed": if selected { "true" } else { "false" },
            title: "Select {name} to change its state",
            onclick: move |_| onselect.call(name.clone()),
            span { class: if step.current { "font-mono text-[13px] text-signal" } else { "font-mono text-[13px] text-ink-muted" },
                "{number:02}"
                if step.current {
                    " · current"
                }
                if !step.required {
                    span { class: "text-ink-muted", " · optional" }
                }
            }
            span { class: name_class, "{label}" }
            if step.title.is_some() {
                span { class: "font-mono text-[12px] text-ink-muted -mt-1", "{step.step}" }
            }
            span { class: "font-mono text-[12.5px] leading-[18px] {mark_class}",
                span { "aria-hidden": "true", "{glyph} " }
                "{word}"
            }
            if let Some(gates) = step.gates.as_ref().filter(|gates| !gates.unchecked.is_empty()) {
                span { class: "font-mono text-[12.5px] text-ink-muted",
                    span { "aria-hidden": "true", "◌ " }
                    if gates.unchecked.len() == 1 { "1 gate needs changed paths" } else { "{gates.unchecked.len()} gates need changed paths" }
                }
            }
            match &step.gates {
                Some(gates) if failing > 0 => rsx! {
                    span { class: "font-mono text-[12.5px] text-bad",
                        span { "aria-hidden": "true", "✕ " }
                        if failing == 1 { "1 gate failing" } else { "{failing} gates failing" }
                    }
                },
                Some(gates) if gates.passed > 0 => rsx! {
                    span { class: "font-mono text-[12.5px] text-ok",
                        span { "aria-hidden": "true", "■ " }
                        if gates.passed == 1 { "1 gate passes" } else { "{gates.passed} gates pass" }
                    }
                },
                _ => rsx! {},
            }
        }
    }
}

#[component]
fn StepPanel(ticket: String, step: StepProgress) -> Element {
    let session = use_context::<Session>();
    let mut reason = use_signal(String::new);
    let mut failure = use_signal(|| None::<Failure>);
    let mut busy = use_signal(|| false);
    let name = step.step.clone();
    let set = move |status: StepStatus| {
        let ticket = ticket.clone();
        let step = name.clone();
        busy.set(true);
        spawn(async move {
            let write = Write::SetStep {
                ticket,
                step,
                status,
                reason: Some(reason().trim().to_owned()).filter(|reason| !reason.is_empty()),
            };
            let hone = (session.hone)();
            let writer = (session.writer)();
            match hone.write(&writer, &write).await {
                Ok(_) => {
                    failure.set(None);
                    reason.set(String::new());
                    session.changed();
                }
                Err(error) => failure.set(Some(error)),
            }
            busy.set(false);
        });
    };
    let mut done = set.clone();
    let mut skip = set.clone();
    let mut open = set;
    let finished = matches!(step.stage, Stage::Done | Stage::Skipped);
    let state_word = match step.stage {
        Stage::Done => "done",
        Stage::Skipped => "skipped",
        Stage::Ready => "open",
        Stage::Blocked => "open, waiting",
    };
    let failed = step
        .gates
        .as_ref()
        .map(|gates| gates.failed.clone())
        .unwrap_or_default();
    let unchecked = step
        .gates
        .as_ref()
        .map(|gates| gates.unchecked.clone())
        .unwrap_or_default();
    rsx! {
        div { class: "border border-line p-5 flex flex-col gap-4",
            div { class: "flex flex-wrap items-baseline gap-x-3 gap-y-1",
                h2 { class: "m-0 text-[18px] leading-[26px] font-bold",
                    {step.title.clone().unwrap_or_else(|| step.step.clone())}
                }
                span { class: "font-mono text-[13px] text-ink-muted",
                    "{step.step} · {state_word} · "
                    if step.required { "required" } else { "optional" }
                }
            }
            if step.stage == Stage::Blocked {
                p { class: "m-0 text-[13px] text-ink-muted",
                    "Its inputs come from {step.waiting_for.join(\", \")}, which "
                    if step.waiting_for.len() == 1 { "is" } else { "are" }
                    " still open. hone does not stop you from finishing it first."
                }
            }
            for gate in failed {
                div { class: "border border-bad p-3 flex flex-col gap-1",
                    span { class: "font-mono text-[13px] text-bad", "✕ {gate.name} · {gate.gate}" }
                    for finding in gate.findings.iter().filter(|finding| finding.level == "error") {
                        span { class: "text-[13px]", "{finding.code}: {finding.message}" }
                    }
                }
            }
            if !unchecked.is_empty() {
                p { class: "m-0 text-[13px] text-ink-muted",
                    "◌ "
                    span { class: "font-mono", {unchecked.iter().map(|gate| gate.name.clone()).collect::<Vec<_>>().join(", ")} }
                    " compares the changed paths with binding decisions. The GUI knows no change, so it runs in the hook or in CI: hone check --ticket … --changed <path>."
                }
            }
            div { class: "flex flex-wrap items-end gap-2",
                Field { label: "Reason (optional, stored with the event)",
                    Input { class: "w-80", value: reason(), placeholder: if finished { "why it is reopened" } else { "for example: no new terms" }, oninput: move |value| reason.set(value) }
                }
                if finished {
                    Button { disabled: busy(), onclick: move |_| open(StepStatus::Open), "Reopen step" }
                } else {
                    Button { variant: ButtonVariant::Outline, disabled: busy(), onclick: move |_| skip(StepStatus::Skipped), "Skip step" }
                    Button { disabled: busy(), onclick: move |_| done(StepStatus::Done), "Mark done" }
                }
                span { class: "font-mono text-[12.5px] text-ink-muted ml-auto self-center", "writes step.changed" }
            }
            if let Some(failure) = failure() {
                FailureAlert { failure }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;
    use crate::hone::{ProfileStep, StepState};

    fn step(name: &str, requirement: &str, after: &[&str]) -> ProfileStep {
        ProfileStep {
            step: name.into(),
            title: None,
            requirement: requirement.into(),
            after: after.iter().map(|step| (*step).to_owned()).collect(),
        }
    }

    fn overview() -> TicketOverview {
        TicketOverview {
            ticket: "tk-9f2x4a8m".into(),
            path: String::new(),
            profile: "standard".into(),
            selected: Some("standard".into()),
            config: String::new(),
            attributes: serde_json::Map::new(),
            external: Vec::new(),
            roles: Vec::new(),
            steps: vec![
                step("specify", "required", &[]),
                step("domain", "optional", &[]),
                step("decision", "required", &["specify"]),
                step("implementation", "required", &["decision"]),
            ],
        }
    }

    fn state(steps: &[(&str, &str)]) -> TicketEvents {
        TicketEvents {
            events: Vec::new(),
            threads: Vec::new(),
            steps: steps
                .iter()
                .map(|(step, status)| {
                    (
                        (*step).to_owned(),
                        StepState {
                            status: (*status).to_owned(),
                            reason: None,
                        },
                    )
                })
                .collect::<BTreeMap<_, _>>(),
            invalid: Vec::new(),
        }
    }

    fn gate(name: &str, ok: bool) -> Gate {
        Gate {
            name: name.into(),
            gate: "artifact-exists".into(),
            ok,
            findings: Vec::new(),
        }
    }

    #[test]
    fn steps_wait_for_open_earlier_steps_and_the_first_ready_one_is_current() {
        let steps = progress(&overview(), Some(&state(&[("domain", "skipped")])), None);
        let stages: Vec<(Stage, bool)> = steps
            .iter()
            .map(|step| (step.stage, step.current))
            .collect();
        assert_eq!(
            stages,
            [
                (Stage::Ready, true),
                (Stage::Skipped, false),
                (Stage::Blocked, false),
                (Stage::Blocked, false)
            ]
        );
        assert_eq!(steps[2].waiting_for, ["specify"]);

        let steps = progress(&overview(), Some(&state(&[("specify", "done")])), None);
        let current: Vec<&str> = steps
            .iter()
            .filter(|step| step.current)
            .map(|step| step.step.as_str())
            .collect();
        assert_eq!(current, ["domain"]);
        assert_eq!(steps[2].stage, Stage::Ready);
    }

    #[test]
    fn gates_belong_to_the_step_of_their_name() {
        let run = CheckRun {
            status: "failed".into(),
            gates: vec![
                gate("schema-valid", true),
                gate("decision/approved", false),
                gate("decision/decision-exists", true),
                gate("specify/spec-exists", true),
            ],
        };
        let steps = progress(&overview(), None, Some(&run));
        let decision = steps[2].gates.as_ref().expect("ran");
        assert_eq!(decision.passed, 1);
        assert_eq!(decision.failed[0].name, "decision/approved");
        assert!(decision.unchecked.is_empty());
        assert_eq!(steps[0].gates.as_ref().map(|gates| gates.passed), Some(1));
        assert_eq!(steps[1].gates, None);
    }

    #[test]
    fn a_gate_that_only_misses_changed_paths_is_unchecked_not_failed() {
        let finding = |code: &str| crate::hone::Finding {
            code: code.into(),
            level: "error".into(),
            message: String::new(),
            location: None,
            path: None,
            pointer: None,
        };
        let mut binding = gate("implementation/binding", false);
        binding.findings = vec![finding("changed-paths-missing")];
        let mut both = gate("implementation/other", false);
        both.findings = vec![finding("changed-paths-missing"), finding("binding-missing")];
        let run = CheckRun {
            status: "failed".into(),
            gates: vec![binding, both],
        };
        let steps = progress(&overview(), None, Some(&run));
        let gates = steps[3].gates.as_ref().expect("ran");
        assert_eq!(gates.unchecked[0].name, "implementation/binding");
        assert_eq!(gates.failed[0].name, "implementation/other");
        assert_eq!((gates.unchecked.len(), gates.failed.len()), (1, 1));
    }

    #[test]
    fn the_summary_counts_finished_skipped_and_open_required_steps() {
        let steps = progress(
            &overview(),
            Some(&state(&[("specify", "done"), ("domain", "skipped")])),
            None,
        );
        assert_eq!(
            summary(&steps),
            "2 of 4 finished · 1 skipped · 2 required open"
        );
        let all = progress(
            &overview(),
            Some(&state(&[
                ("specify", "done"),
                ("decision", "done"),
                ("implementation", "done"),
            ])),
            None,
        );
        assert_eq!(
            summary(&all),
            "3 of 4 finished · every required step finished"
        );
    }
}
