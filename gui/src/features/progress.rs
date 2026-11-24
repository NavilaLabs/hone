//! Setting a step of the open ticket, with the gates of its last check.

use dioxus::prelude::*;

use super::Session;
use super::common::{FailureAlert, Field};
use crate::hone::{Failure, Gate, StepStatus, TicketStep, Write};
use crate::ui::{Button, ButtonVariant, Input};

/// Whether every error of a failed gate is `changed-paths-missing`: the GUI knows no change,
/// so such a gate is unchecked, not failed.
pub(crate) fn needs_changed_paths(gate: &Gate) -> bool {
    let mut errors = gate
        .findings
        .iter()
        .filter(|finding| finding.level == "error")
        .peekable();
    errors.peek().is_some() && errors.all(|finding| finding.code == "changed-paths-missing")
}

/// The gates of `step` in `gates`, split into failed and unchecked.
pub(crate) fn step_gates(step: &str, gates: &[Gate]) -> (Vec<Gate>, Vec<Gate>) {
    let prefix = format!("{step}/");
    let own = gates
        .iter()
        .filter(|gate| gate.name.starts_with(&prefix) && !gate.ok);
    let (unchecked, failed): (Vec<Gate>, Vec<Gate>) = own.cloned().partition(needs_changed_paths);
    (failed, unchecked)
}

/// A step with its failed gates and the buttons that write `step.changed`.
#[component]
pub(crate) fn StepPanel(ticket: String, step: TicketStep, gates: Vec<Gate>) -> Element {
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
    let finished = matches!(step.status.as_str(), "done" | "skipped");
    let (failed, unchecked) = step_gates(&step.step, &gates);
    rsx! {
        div { class: "border border-line p-5 flex flex-col gap-4",
            div { class: "flex flex-wrap items-baseline gap-x-3 gap-y-1",
                h2 { class: "m-0 text-[18px] leading-[26px] font-bold", "{step.step}" }
                span { class: "font-mono text-[13px] text-ink-muted",
                    "{step.status} · "
                    {step.requirement.clone().unwrap_or_default()}
                }
            }
            if step.status == "waiting" {
                p { class: "m-0 text-[13px] text-ink-muted",
                    "A step before it is still open. hone does not stop you from finishing it first."
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
    use super::*;
    use crate::hone::Finding;

    fn gate(name: &str, codes: &[&str]) -> Gate {
        Gate {
            name: name.into(),
            gate: "change-needs-binding".into(),
            ok: codes.is_empty(),
            findings: codes
                .iter()
                .map(|code| Finding {
                    code: (*code).into(),
                    level: "error".into(),
                    message: String::new(),
                    location: None,
                    path: None,
                    pointer: None,
                })
                .collect(),
            duration_ms: 0,
        }
    }

    #[test]
    fn a_gate_missing_only_changed_paths_is_unchecked_not_failed() {
        let gates = [
            gate("implementation/binding", &["changed-paths-missing"]),
            gate("implementation/exists", &["artifact-missing"]),
            gate("implementation/passed", &[]),
            gate("decision/approved", &["approval-missing"]),
        ];
        let (failed, unchecked) = step_gates("implementation", &gates);
        let names = |gates: &[Gate]| {
            gates
                .iter()
                .map(|gate| gate.name.clone())
                .collect::<Vec<_>>()
        };
        assert_eq!(names(&failed), ["implementation/exists"]);
        assert_eq!(names(&unchecked), ["implementation/binding"]);
    }
}
