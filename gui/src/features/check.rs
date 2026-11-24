//! A stored check run of the open ticket (`hone runs`): who ran it with which arguments, the
//! counts, and every gate with its findings and time; and Run again.

use dioxus::prelude::*;
use serde_json::Value;

use super::code::CodeTarget;
use super::common::{Empty, FailureAlert, Loading, Mark, ProblemAlert, StatusMark, WarningNote};
use super::journal::clock;
use super::{Project, Session, Tab};
use crate::hone::{CheckCall, CheckRun, Failure, Finding, Gate, Run};
use crate::ui::{
    Button, ButtonVariant, Table, TableBody, TableCell, TableHead, TableHeader, TableRow,
    TableWrapper,
};

/// The gates of a run, failed first, each group in the order of the run.
fn ordered(gates: &[Gate]) -> Vec<Gate> {
    let mut gates = gates.to_vec();
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

/// A time in milliseconds as the run table shows it: `412ms`, or `1.9s` from one second.
fn duration(milliseconds: u64) -> String {
    if milliseconds < 1000 {
        format!("{milliseconds}ms")
    } else {
        // Precision is irrelevant for a display of tenths of a second.
        #[allow(clippy::cast_precision_loss)]
        let seconds = milliseconds as f64 / 1000.0;
        format!("{seconds:.1}s")
    }
}

/// The call that repeats `run` of `ticket`: its step, changed paths, and class. The base state
/// and the lockfile are left out, because they name paths of the caller that may be gone.
fn call_of(ticket: &str, arguments: &Value) -> CheckCall {
    let text = |key: &str| arguments[key].as_str().map(str::to_owned);
    CheckCall {
        ticket: ticket.to_owned(),
        step: text("step"),
        changed: arguments["changed"].as_array().map(|paths| {
            paths
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_owned)
                .collect()
        }),
        class: text("class"),
    }
}

/// The command line of a stored run, as its arguments name it.
fn command_line(ticket: &str, arguments: &Value) -> String {
    let call = call_of(ticket, arguments);
    let mut line = format!("hone check --ticket {ticket}");
    if let Some(step) = &call.step {
        line.push_str(&format!(" --step {step}"));
    }
    match &call.changed {
        Some(changed) if changed.is_empty() => line.push_str(" --changed-from <empty>"),
        Some(changed) => {
            for path in changed {
                line.push_str(&format!(" --changed {path}"));
            }
        }
        None => {}
    }
    if let Some(class) = &call.class {
        line.push_str(&format!(" --class {class}"));
    }
    for key in ["base", "lock"] {
        if let Some(path) = arguments[key].as_str() {
            line.push_str(&format!(" --{key} {path}"));
        }
    }
    line
}

/// The ticket's check run while the store holds none: a button that runs every gate.
#[component]
pub(crate) fn NoRun(ticket: String) -> Element {
    let session = use_context::<Session>();
    let mut failure = use_signal(|| None::<Failure>);
    let transient = use_signal(|| None::<CheckRun>);
    let mut running = use_signal(|| false);
    let call = CheckCall {
        ticket: ticket.clone(),
        ..CheckCall::default()
    };
    let run = move |_| {
        let call = call.clone();
        running.set(true);
        spawn(async move {
            match (session.hone)().check_call(&call).await {
                Ok(result) => shown(session, &Tab::Check, result, transient),
                Err(error) => failure.set(Some(error)),
            }
            running.set(false);
        });
    };
    rsx! {
        Head { context: String::new(), running: running(), onrun: run }
        if let Some(failure) = failure() {
            FailureAlert { failure }
        }
        match transient() {
            Some(result) => rsx! {
                WarningNote { text: "The run was not stored, because the project has no process store yet." }
                RunBody { status: result.status, gates: result.gates, duration_ms: None }
            },
            None => rsx! { Empty { text: "No stored run of this ticket yet. Run hone check to run every gate, exec gates included." } },
        }
    }
}

/// Opens the stored run of `result` in the place of `tab`, or keeps an unstored result in
/// `transient`.
fn shown(
    mut session: Session,
    tab: &Tab,
    result: CheckRun,
    mut transient: Signal<Option<CheckRun>>,
) {
    match result.run.clone() {
        Some(id) => {
            if let Some(ticket) = (session.ticket)()
                && let Some(tabs) = session.tabs.write().get_mut(&ticket)
            {
                tabs.replace(tab, Tab::Run(id));
            }
            session.changed();
        }
        None => transient.set(Some(result)),
    }
}

/// The stored run `id` of `ticket`.
#[component]
pub(crate) fn RunView(ticket: String, id: String) -> Element {
    let session = use_context::<Session>();
    let project = use_context::<Project>();
    let mut failure = use_signal(|| None::<Failure>);
    let transient = use_signal(|| None::<CheckRun>);
    let mut running = use_signal(|| false);
    let runs = match &*project.runs.read() {
        Some(Some(Ok(runs))) => runs.clone(),
        Some(Some(Err(failure))) => return rsx! { FailureAlert { failure: failure.clone() } },
        _ => return rsx! { Loading {} },
    };
    let Some(run) = runs.into_iter().find(|run| run.id == id) else {
        return rsx! {
            ProblemAlert { title: "No such run", message: format!("The store keeps the newest 100 runs of a ticket; `{id}` is not among them.") }
        };
    };
    let call = call_of(&ticket, &run.arguments);
    let tab = Tab::Run(id.clone());
    let again = move |_| {
        let call = call.clone();
        let tab = tab.clone();
        running.set(true);
        spawn(async move {
            match (session.hone)().check_call(&call).await {
                Ok(result) => shown(session, &tab, result, transient),
                Err(error) => failure.set(Some(error)),
            }
            running.set(false);
        });
    };
    let Run {
        source,
        device,
        arguments,
        status,
        duration_ms,
        gates,
        ..
    } = run;
    rsx! {
        Head { context: format!("{} · {source} · {device}", clock(&id)), running: running(), onrun: again }
        pre { class: "m-0 bg-surface border border-line p-4 font-mono text-[13px] overflow-x-auto",
            span { class: "text-signal", "$ " }
            {command_line(&ticket, &arguments)}
        }
        if arguments["base"].is_string() || arguments["lock"].is_string() {
            p { class: "m-0 text-[13px] text-ink-muted", "Run again leaves out --base and --lock: they name paths of the caller that may be gone." }
        }
        if let Some(failure) = failure() {
            FailureAlert { failure }
        }
        if let Some(result) = transient() {
            WarningNote { text: "The new run was not stored. It shows below in place of the stored one." }
            RunBody { status: result.status, gates: result.gates, duration_ms: None }
        } else {
            RunBody { status, gates, duration_ms: Some(duration_ms) }
        }
    }
}

#[component]
fn Head(context: String, running: bool, onrun: EventHandler<MouseEvent>) -> Element {
    rsx! {
        div { class: "flex flex-wrap items-end gap-4",
            div { class: "flex-1 min-w-0 flex flex-col gap-1",
                if !context.is_empty() {
                    span { class: "font-mono text-[13px] text-ink-muted", "{context}" }
                }
                h1 { class: "m-0 text-[32px] leading-10 font-bold tracking-[-0.01em]", "Check run" }
            }
            Button {
                class: "h-12 px-6",
                disabled: running,
                title: "Runs every gate the run ran, exec gates included",
                onclick: move |event| onrun.call(event),
                if running { "Running…" } else { "Run again" }
            }
        }
    }
}

#[component]
fn RunBody(status: String, gates: Vec<Gate>, duration_ms: Option<u64>) -> Element {
    let failed = gates.iter().filter(|gate| !gate.ok).count();
    let passed = gates.len() - failed;
    let exit = i32::from(status != "passed");
    rsx! {
        div { class: if duration_ms.is_some() { "grid grid-cols-4 border border-line" } else { "grid grid-cols-3 border border-line" },
            Stat { label: "Exit", value: exit.to_string(), bad: exit == 1 }
            Stat { label: "Passed", value: passed.to_string(), bad: false }
            Stat { label: "Failed", value: failed.to_string(), bad: false, alarm: failed > 0 }
            if let Some(milliseconds) = duration_ms {
                Stat { label: "Time", value: duration(milliseconds), bad: false }
            }
        }
        TableWrapper {
            Table {
                TableHeader { class: "bg-surface",
                    TableRow {
                        TableHead { "Result" }
                        TableHead { "Gate" }
                        TableHead { "Class" }
                        TableHead { class: "text-right", "Time" }
                    }
                }
                TableBody {
                    for gate in ordered(&gates) {
                        TableRow {
                            TableCell {
                                if gate.ok {
                                    StatusMark { mark: Mark::Done, word: "pass" }
                                } else {
                                    StatusMark { mark: Mark::Fail, word: "FAIL" }
                                }
                            }
                            TableCell { class: if gate.ok { "font-mono" } else { "font-mono font-semibold" },
                                "{gate.name}"
                                if gate.name != gate.gate {
                                    span { class: "ml-2 text-ink-muted font-normal", "{gate.gate}" }
                                }
                            }
                            TableCell { class: "font-mono text-ink-muted", {class(&gate)} }
                            TableCell { class: "font-mono text-ink-muted text-right", {duration(gate.duration_ms)} }
                        }
                        for finding in gate.findings.clone() {
                            TableRow { class: "hover:bg-transparent",
                                TableCell { colspan: 4, class: "pt-0",
                                    FindingBox { finding }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

/// A finding with what it points at, which opens as a tab.
#[component]
fn FindingBox(finding: Finding) -> Element {
    let session = use_context::<Session>();
    let project = use_context::<Project>();
    let artifact = finding
        .path
        .as_ref()
        .and_then(|path| match &*project.catalog.read() {
            Some(Ok(catalog)) => catalog
                .artifacts
                .iter()
                .find(|entry| &entry.path == path)
                .and_then(|entry| entry.id.clone()),
            _ => None,
        });
    let target = finding
        .path
        .as_ref()
        .filter(|_| artifact.is_none())
        .and_then(|path| CodeTarget::of("path", path));
    let place = finding.path.as_ref().map(|path| match &finding.pointer {
        Some(pointer) => format!("{path} {pointer}"),
        None => path.clone(),
    });
    let error = finding.level == "error";
    rsx! {
        div { class: if error { "border border-bad p-4 flex items-start gap-4" } else { "border border-warn p-4 flex items-start gap-4" },
            div { class: "flex-1 min-w-0 flex flex-col gap-1",
                span { class: if error { "font-mono text-[13px] text-bad font-semibold" } else { "font-mono text-[13px] text-warn font-semibold" },
                    "{finding.level} {finding.code}"
                }
                span { "{finding.message}" }
                if let Some(place) = place {
                    if artifact.is_some() || target.is_some() {
                        button {
                            class: "self-start font-mono text-[13px] text-ink-muted underline break-all text-left",
                            onclick: move |_| {
                                if let Some(id) = artifact.clone() {
                                    session.open_artifact(id);
                                } else if let Some(target) = target.clone() {
                                    session.open_tab(Tab::Code(target));
                                }
                            },
                            "{place}"
                        }
                    } else {
                        span { class: "font-mono text-[13px] text-ink-muted break-all", "{place}" }
                    }
                }
                if let Some(location) = finding.location.clone() {
                    span { class: "font-mono text-[13px] text-ink-muted break-all", {location.to_string()} }
                }
            }
            Button {
                variant: ButtonVariant::Outline,
                disabled: true,
                title: "The assistant is not connected yet",
                "Ask assistant"
            }
        }
    }
}

#[component]
fn Stat(label: String, value: String, bad: bool, #[props(optional)] alarm: bool) -> Element {
    rsx! {
        div { class: if bad { "p-5 flex flex-col gap-2 bg-bad text-bg border-r border-line" } else { "p-5 flex flex-col gap-2 border-r border-line" },
            span { class: "font-mono text-[11px] tracking-[0.08em] uppercase", "{label}" }
            span { class: if alarm { "font-mono text-[40px] leading-10 font-bold text-bad" } else { "font-mono text-[40px] leading-10 font-bold" },
                "{value}"
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

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
        let gates = [
            gate("a", "schema-valid", true),
            gate("b", "exec", false),
            gate("c", "ids-unique", true),
            gate("d", "public-api-has-decision", false),
        ];
        let names: Vec<String> = ordered(&gates).into_iter().map(|gate| gate.name).collect();
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

    #[test]
    fn times_switch_to_seconds_at_one_second() {
        assert_eq!(duration(0), "0ms");
        assert_eq!(duration(999), "999ms");
        assert_eq!(duration(1000), "1.0s");
        assert_eq!(duration(1949), "1.9s");
    }

    #[test]
    fn run_again_repeats_step_changes_and_class_but_not_the_callers_paths() {
        let arguments = json!({
            "step": "interfaces",
            "changed": ["src/fee.rs", "src/pay.rs"],
            "class": null,
            "base": "/tmp/base",
            "lock": null,
        });
        assert_eq!(
            call_of("tk-1", &arguments),
            CheckCall {
                ticket: "tk-1".into(),
                step: Some("interfaces".into()),
                changed: Some(vec!["src/fee.rs".into(), "src/pay.rs".into()]),
                class: None,
            }
        );
        assert_eq!(
            command_line("tk-1", &arguments),
            "hone check --ticket tk-1 --step interfaces --changed src/fee.rs --changed src/pay.rs --base /tmp/base"
        );
        let empty =
            json!({ "step": null, "changed": [], "class": "pure", "base": null, "lock": null });
        assert_eq!(call_of("tk-1", &empty).changed, Some(Vec::new()));
        assert_eq!(
            command_line("tk-1", &empty),
            "hone check --ticket tk-1 --changed-from <empty> --class pure"
        );
        let none =
            json!({ "step": null, "changed": null, "class": null, "base": null, "lock": null });
        assert_eq!(call_of("tk-1", &none).changed, None);
        assert_eq!(command_line("tk-1", &none), "hone check --ticket tk-1");
    }
}
