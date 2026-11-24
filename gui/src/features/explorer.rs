//! The explorer of the open ticket: its steps, artifacts, the code they reach, its review, and
//! its runs.

use std::collections::BTreeSet;

use dioxus::prelude::*;

use super::journal::clock;
use super::progress::step_gates;
use super::{Project, Session, Tab};
use crate::hone::{CatalogEntry, Run, TicketSummary};
use crate::ui::{Button, ButtonVariant};

/// The modules that the ticket's artifacts name under `affects`, in the order of their IDs.
pub(crate) fn reached_modules<'a>(
    artifacts: &[&'a CatalogEntry],
    catalog: &'a [CatalogEntry],
) -> Vec<&'a CatalogEntry> {
    let affected: BTreeSet<&str> = artifacts
        .iter()
        .flat_map(|artifact| artifact.links.get("affects").into_iter().flatten())
        .map(String::as_str)
        .collect();
    affected
        .into_iter()
        .filter_map(|id| catalog.iter().find(|entry| entry.id.as_deref() == Some(id)))
        .collect()
}

/// The left column of a ticket's workspace, or its narrow rail when collapsed.
#[component]
pub(crate) fn Explorer() -> Element {
    let session = use_context::<Session>();
    let mut open = session.explorer_open;
    if !open() {
        return rsx! { Rail {} };
    }
    let project = use_context::<Project>();
    let ticket = (session.ticket)().unwrap_or_default();
    let summary = project.summary(&ticket);
    let catalog = match &*project.catalog.read() {
        Some(Ok(catalog)) => catalog.artifacts.clone(),
        _ => Vec::new(),
    };
    let artifacts: Vec<&CatalogEntry> = catalog
        .iter()
        .filter(|entry| entry.kind != "ticket" && entry.ticket.as_deref() == Some(ticket.as_str()))
        .collect();
    let modules: Vec<CatalogEntry> = reached_modules(&artifacts, &catalog)
        .into_iter()
        .cloned()
        .collect();
    let artifacts: Vec<CatalogEntry> = artifacts.into_iter().cloned().collect();
    let threads = match &*project.state.read() {
        Some(Some(Ok(state))) => state
            .threads
            .iter()
            .filter(|thread| thread.status == "open")
            .cloned()
            .collect(),
        _ => Vec::new(),
    };
    let runs: Vec<Run> = match &*project.runs.read() {
        Some(Some(Ok(runs))) => runs.clone(),
        _ => Vec::new(),
    };
    let events = match &*project.state.read() {
        Some(Some(Ok(state))) => Some(state.events.len()),
        _ => None,
    };
    let title = summary
        .as_ref()
        .and_then(|summary| summary.title.clone())
        .unwrap_or_else(|| ticket.clone());
    let blocking = summary
        .as_ref()
        .map_or(0, |summary| summary.threads.blocking);
    rsx! {
        nav { class: "w-[300px] shrink-0 border-r border-line flex flex-col min-h-0", "aria-label": "Ticket explorer",
            div { class: "px-5 py-4 border-b border-line flex items-start gap-3",
                div { class: "flex-1 min-w-0 flex flex-col gap-1",
                    span { class: "font-mono text-[11px] tracking-[0.08em] uppercase text-ink-muted", "Ticket · {ticket}" }
                    span { class: "font-semibold text-[17px] leading-6 break-words", "{title}" }
                    if let Some(profile) = summary.as_ref().and_then(|summary| summary.profile.clone()) {
                        span { class: "self-start h-6 px-2 inline-flex items-center bg-ink text-bg font-mono text-[12px]", "{profile}" }
                    }
                }
                Button { variant: ButtonVariant::Outline, class: "h-9 w-9 px-0",
                    title: "Collapse the explorer",
                    onclick: move |_| open.set(false),
                    "«"
                }
            }
            div { class: "flex-1 min-h-0 overflow-y-auto",
                Group { label: "Ticket",
                    Item { label: "Overview", tab: Tab::Overview }
                    Item { label: "Journal", tab: Tab::Journal }
                }
                if let Some(summary) = summary.clone() {
                    Steps { summary }
                }
                Group { label: "Artifacts", aside: artifacts.len().to_string(),
                    for artifact in artifacts {
                        Item {
                            key: "{artifact.path}",
                            label: artifact.id.clone().unwrap_or_else(|| artifact.path.clone()),
                            note: artifact.title.clone().unwrap_or_default(),
                            mark: artifact.effective.clone().unwrap_or_default(),
                            tab: Tab::Artifact(artifact.id.clone().unwrap_or_default()),
                        }
                    }
                }
                if !modules.is_empty() {
                    Group { label: "Code", aside: modules.len().to_string(),
                        for module in modules {
                            Item {
                                key: "{module.path}",
                                label: module.id.clone().unwrap_or_else(|| module.path.clone()),
                                note: module.title.clone().unwrap_or_default(),
                                tab: Tab::Artifact(module.id.clone().unwrap_or_default()),
                            }
                        }
                    }
                }
                Group { label: "Review", aside: if blocking > 0 { format!("{blocking} blocking") } else { String::new() },
                    for thread in threads {
                        ThreadItem { key: "{thread.thread}", ticket: ticket.clone(), thread: thread.thread.clone(), kind: thread.kind.clone(), blocking: thread.blocking }
                    }
                    Item { label: "All threads", tab: Tab::Threads }
                }
                Group { label: "Runs",
                    if let Some(run) = runs.first() {
                        Item {
                            label: "Check run",
                            note: clock(&run.id),
                            mark: if run.status == "passed" { "passed".to_owned() } else { "failed".to_owned() },
                            bad: run.status != "passed",
                            tab: Tab::Run(run.id.clone()),
                        }
                    } else {
                        Item { label: "Check run", tab: Tab::Check }
                    }
                    Item { label: "Link graph", tab: Tab::Graph }
                    Item { label: "Event log", note: events.map(|count| count.to_string()).unwrap_or_default(), tab: Tab::Events }
                }
            }
            div { class: "p-4 border-t border-line grid grid-cols-2 gap-2",
                Button { variant: ButtonVariant::Outline,
                    onclick: move |_| {
                        let mut dialog = session.new_artifact;
                        dialog.set(true);
                    },
                    "New artifact"
                }
                Button { variant: ButtonVariant::Outline,
                    onclick: move |_| session.open_tab(Tab::Threads),
                    "Open thread"
                }
            }
        }
    }
}

#[component]
fn Rail() -> Element {
    let session = use_context::<Session>();
    let project = use_context::<Project>();
    let mut open = session.explorer_open;
    let entries = [
        ("OV", "Overview", Tab::Overview),
        ("JR", "Journal", Tab::Journal),
        ("RV", "Threads", Tab::Threads),
        ("CK", "Check run", project.newest_run()),
        ("GR", "Link graph", Tab::Graph),
        ("EV", "Event log", Tab::Events),
    ];
    let active = session.ticket_tabs().active;
    rsx! {
        nav { class: "w-16 shrink-0 border-r border-line flex flex-col items-stretch", "aria-label": "Ticket explorer",
            button { class: "h-14 font-mono text-[18px] hover:bg-surface-2", title: "Open the explorer", onclick: move |_| open.set(true), "»" }
            for (short , label , tab) in entries {
                {
                    let class = if active == tab { "h-14 font-mono text-[13px] font-semibold bg-ink text-bg" } else { "h-14 font-mono text-[13px] font-semibold hover:bg-surface-2" };
                    rsx! {
                        button { class, title: label, onclick: move |_| session.open_tab(tab.clone()), "{short}" }
                    }
                }
            }
        }
    }
}

#[component]
fn Group(
    #[props(into)] label: String,
    #[props(into, optional)] aside: Option<String>,
    children: Element,
) -> Element {
    let mut expanded = use_signal(|| true);
    rsx! {
        section { class: "border-b border-line py-2",
            button {
                class: "w-full h-9 px-5 flex items-center gap-2 font-mono text-[11px] tracking-[0.08em] uppercase text-ink-muted hover:text-ink",
                "aria-expanded": "{expanded}",
                onclick: move |_| expanded.toggle(),
                span { "aria-hidden": "true", if expanded() { "▾" } else { "▸" } }
                span { class: "flex-1 text-left", "{label}" }
                if let Some(aside) = aside {
                    span { class: "text-signal normal-case tracking-normal", "{aside}" }
                }
            }
            if expanded() {
                ul { class: "m-0 p-0 list-none", {children} }
            }
        }
    }
}

#[component]
fn Item(
    #[props(into)] label: String,
    #[props(into, optional)] note: Option<String>,
    #[props(into, optional)] mark: Option<String>,
    #[props(optional)] bad: bool,
    tab: Tab,
) -> Element {
    let session = use_context::<Session>();
    let active = session.ticket_tabs().active == tab;
    let class = if active {
        "w-full min-h-10 px-5 flex items-baseline gap-2 text-left bg-ink text-bg"
    } else {
        "w-full min-h-10 px-5 flex items-baseline gap-2 text-left hover:bg-surface-2"
    };
    let mark_class = if bad {
        "font-mono text-[12px] text-bad"
    } else {
        "font-mono text-[12px] text-ink-muted"
    };
    rsx! {
        li {
            button { class, onclick: move |_| session.open_tab(tab.clone()),
                span { class: "shrink-0", "{label}" }
                if let Some(note) = note.filter(|note| !note.is_empty()) {
                    span { class: "font-mono text-[12px] opacity-70 truncate", "{note}" }
                }
                if let Some(mark) = mark.filter(|mark| !mark.is_empty()) {
                    span { class: "ml-auto {mark_class}", "{mark}" }
                }
            }
        }
    }
}

#[component]
fn ThreadItem(ticket: String, thread: String, kind: String, blocking: bool) -> Element {
    let session = use_context::<Session>();
    let shown = thread.clone();
    rsx! {
        li {
            button {
                class: "w-full min-h-10 px-5 flex items-baseline gap-2 text-left hover:bg-surface-2",
                onclick: move |_| session.open_thread(ticket.clone(), thread.clone()),
                span { class: "shrink-0", "{kind}" }
                span { class: "font-mono text-[12px] text-ink-muted truncate", title: "{shown}", "{shown}" }
                if blocking {
                    span { class: "ml-auto shrink-0 font-mono text-[12px] text-signal", title: "blocking", "▲" }
                }
            }
        }
    }
}

#[component]
fn Steps(summary: TicketSummary) -> Element {
    let project = use_context::<Project>();
    let gates = match &*project.check.read() {
        Some(Some(Ok(run))) => run.gates.clone(),
        _ => Vec::new(),
    };
    let finished = summary
        .steps
        .iter()
        .filter(|step| step.status == "done" || step.status == "skipped")
        .count();
    rsx! {
        Group { label: "Steps", aside: format!("{finished}/{}", summary.steps.len()),
            for step in summary.steps.clone() {
                {
                    let current = summary.current.as_deref() == Some(step.step.as_str());
                    // A gate that misses only changed paths is unchecked, not failed.
                    let failing = !step_gates(&step.step, &gates).0.is_empty();
                    let (glyph, class) = match step.status.as_str() {
                        "done" => ("■", "text-ink"),
                        "skipped" => ("/", "text-ink-muted"),
                        _ if failing => ("×", "text-bad"),
                        _ => ("□", "text-ink-muted"),
                    };
                    let label_class = match step.status.as_str() {
                        "skipped" => "line-through text-ink-muted",
                        "waiting" => "text-ink-muted",
                        _ => "",
                    };
                    rsx! {
                        li { key: "{step.step}",
                            button {
                                class: "w-full min-h-10 px-5 flex items-baseline gap-2 text-left hover:bg-surface-2",
                                onclick: move |_| {
                                    let session = use_context::<Session>();
                                    session.open_tab(Tab::Overview);
                                },
                                span { class: label_class, "{step.step}" }
                                if current {
                                    span { class: "font-mono text-[12px] text-ink-muted", "current" }
                                } else if let Some(reason) = step.reason.clone() {
                                    span { class: "font-mono text-[12px] text-ink-muted truncate", "{reason}" }
                                }
                                span { class: "ml-auto font-mono {class}", "aria-label": "{step.status}", "{glyph}" }
                            }
                        }
                    }
                }
            }
        }
    }
}
