use dioxus::prelude::*;

use super::common::{Empty, FailureAlert, Field, Mark, PageHeader, SectionLabel, StatusMark};
use super::{LastCheck, Project, Session};
use crate::hone::{CheckRun as Run, Gate, GateScope};
use crate::ui::{
    Button, Choice, Combobox, Skeleton, Table, TableBody, TableCell, TableHead, TableHeader,
    TableRow, TableWrapper,
};

/// The gates of a run, failed first, each group in the order of the run.
fn ordered(run: &Run) -> Vec<Gate> {
    let mut gates = run.gates.clone();
    gates.sort_by_key(|gate| gate.ok);
    gates
}

/// The class of a gate: the standard gates of concept Section 7 are pure, `exec` runs a
/// command, every other gate is a plugin.
fn class(gate: &Gate) -> &'static str {
    const PURE: [&str; 10] = [
        "schema-valid",
        "ids-unique",
        "links-resolve",
        "artifact-exists",
        "status-is",
        "approvals-valid",
        "change-needs-binding",
        "no-open-blocking-threads",
        "threads-answered",
        "steps-complete",
    ];
    if gate.gate == "exec" {
        "exec"
    } else if PURE.contains(&gate.gate.as_str()) {
        "pure"
    } else {
        "plugin · wasm"
    }
}

/// `hone check`, run on demand: the exit code, the counts, and the gates.
#[component]
pub(crate) fn CheckRun() -> Element {
    let session = use_context::<Session>();
    let project = use_context::<Project>();
    let mut ticket = use_signal(|| (session.ticket)().unwrap_or_default());
    let mut last = session.last_check;
    let mut duration = use_signal(|| None::<std::time::Duration>);
    let mut running = use_signal(|| false);
    let mut run = move || {
        running.set(true);
        spawn(async move {
            let hone = (session.hone)();
            let ticket = Some(ticket().trim().to_owned()).filter(|ticket| !ticket.is_empty());
            let started = std::time::Instant::now();
            let outcome = hone.check(ticket.as_deref(), GateScope::All).await;
            duration.set(Some(started.elapsed()));
            last.set(Some(LastCheck { ticket, outcome }));
            running.set(false);
        });
    };
    let command = match ticket().trim() {
        "" => "hone check --json".to_owned(),
        ticket => format!("hone check --ticket {ticket} --json"),
    };
    rsx! {
        PageHeader {
            context: "check run",
            title: "hone check",
            actions: rsx! {
                Button { disabled: running(), onclick: move |_| run(), "Run hone check" }
            },
        }
        div { class: "flex flex-wrap items-end gap-4",
            Field { label: "Ticket (without one, only the directory gates run)",
                div { class: "w-96",
                    Combobox {
                        value: ticket(),
                        choices: std::iter::once(Choice::plain("").label("no ticket: directory gates only")).chain(project.ticket_choices()).collect::<Vec<_>>(),
                        placeholder: "no ticket: directory gates only",
                        label: "Ticket",
                        onselect: move |value: String| ticket.set(value),
                    }
                }
            }
        }
        pre { class: "m-0 bg-surface border border-line p-4 font-mono text-[13px] overflow-x-auto",
            span { class: "text-signal", "$ " }
            "{command}"
        }
        if running() {
            Skeleton { class: "h-24 w-full" }
        } else {
            match last() {
                None => rsx! { Empty { text: "No run yet. Run hone check to see every gate, exec gates included." } },
                Some(LastCheck { outcome: Err(failure), .. }) => rsx! { FailureAlert { failure } },
                Some(LastCheck { ticket: ran_for, outcome: Ok(run) }) => rsx! {
                    if ran_for.as_deref().unwrap_or_default() != ticket().trim() {
                        p { class: "m-0 text-[13px] text-ink-muted",
                            {format!("Showing the last run, for {}. Run again for the chosen ticket.", ran_for.as_deref().unwrap_or("no ticket"))}
                        }
                    }
                    RunResult { run, duration: duration() }
                },
            }
        }
    }
}

#[component]
fn RunResult(run: Run, duration: Option<std::time::Duration>) -> Element {
    let failed = run.gates.iter().filter(|gate| !gate.ok).count();
    let passed = run.gates.len() - failed;
    let exit = i32::from(failed > 0);
    rsx! {
        div { class: "grid grid-cols-[repeat(auto-fit,minmax(180px,1fr))] border border-line",
            Stat { label: "Exit", value: exit.to_string(), bad: exit == 1, note: if exit == 1 { "a gate failed" } else { "every gate passed" } }
            Stat { label: "Passed", value: passed.to_string(), bad: false }
            Stat { label: "Failed", value: failed.to_string(), bad: false, alarm: failed > 0 }
            if let Some(duration) = duration {
                Stat { label: "Duration", value: format!("{:.1}", duration.as_secs_f64()), unit: "s", bad: false }
            }
        }
        section {
            SectionLabel { label: "Gates", aside: format!("{} gates", run.gates.len()) }
            TableWrapper {
                Table {
                    TableHeader { class: "bg-surface",
                        TableRow {
                            TableHead { "Gate" }
                            TableHead { "Type" }
                            TableHead { "Class" }
                            TableHead { "Result" }
                        }
                    }
                    TableBody {
                        for gate in ordered(&run) {
                            TableRow {
                                TableCell { class: "font-mono", "{gate.name}" }
                                TableCell { class: "font-mono text-ink-muted", "{gate.gate}" }
                                TableCell { class: "font-mono text-ink-muted", {class(&gate)} }
                                TableCell {
                                    if gate.ok {
                                        StatusMark { mark: Mark::Done, word: "pass" }
                                    } else {
                                        StatusMark { mark: Mark::Fail, word: "fail" }
                                    }
                                }
                            }
                            for finding in gate.findings.clone() {
                                TableRow { class: "hover:bg-transparent",
                                    TableCell { colspan: 4, class: "pt-0",
                                        div { class: if finding.level == "error" { "border border-bad p-4 flex flex-col gap-1" } else { "border border-warn p-4 flex flex-col gap-1" },
                                            span { class: "font-mono text-[13px]", "{finding.level} {finding.code}" }
                                            span { "{finding.message}" }
                                            if let Some(location) = finding.location.clone() {
                                                span { class: "font-mono text-[13px] text-ink-muted break-all", {location.to_string()} }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

#[component]
fn Stat(
    label: String,
    value: String,
    bad: bool,
    #[props(optional)] alarm: bool,
    #[props(into, optional)] unit: Option<String>,
    #[props(into, optional)] note: Option<String>,
) -> Element {
    rsx! {
        div { class: if bad { "p-5 flex flex-col gap-2 bg-bad text-bg border-r border-line" } else { "p-5 flex flex-col gap-2 border-r border-line" },
            span { class: "font-mono text-[11px] tracking-[0.08em] uppercase", "{label}" }
            span { class: if alarm { "font-mono text-[40px] leading-10 font-bold text-bad" } else { "font-mono text-[40px] leading-10 font-bold" },
                "{value}"
                if let Some(unit) = unit {
                    span { class: "text-[16px] font-medium opacity-70", "{unit}" }
                }
            }
            if let Some(note) = note {
                span { class: "text-[13px] font-medium", "{note}" }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn gate(name: &str, kind: &str, ok: bool) -> Gate {
        Gate {
            name: name.into(),
            gate: kind.into(),
            ok,
            findings: Vec::new(),
            duration_ms: 0,
        }
    }

    #[test]
    fn failed_gates_come_first_and_keep_their_order() {
        let run = Run {
            status: "failed".into(),
            gates: vec![
                gate("a", "schema-valid", true),
                gate("b", "exec", false),
                gate("c", "ids-unique", true),
                gate("d", "public-api-has-decision", false),
            ],
            run: None,
        };
        let names: Vec<String> = ordered(&run).into_iter().map(|gate| gate.name).collect();
        assert_eq!(names, ["b", "d", "a", "c"]);
    }

    #[test]
    fn classes_follow_the_gate_type() {
        assert_eq!(class(&gate("x", "exec", true)), "exec");
        assert_eq!(class(&gate("x", "links-resolve", true)), "pure");
        assert_eq!(
            class(&gate("x", "public-api-has-decision", true)),
            "plugin · wasm"
        );
    }
}
