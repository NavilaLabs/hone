use dioxus::prelude::*;

use super::common::{
    Empty, FailureAlert, Field, Loading, Mark, PageHeader, SectionLabel, StatusMark,
};
use super::{Screen, Session};
use crate::hone::{StepStatus, StoreLog, TicketEvents, Write};
use crate::ui::{
    Badge, Button, ButtonVariant, Input, Table, TableBody, TableCell, TableHead, TableHeader,
    TableRow, TableWrapper,
};

/// A ticket as the store knows it.
#[derive(Debug, Clone, PartialEq, Eq)]
struct TicketSummary {
    ticket: String,
    events: usize,
    last: String,
}

/// The tickets of the store, in the order their first event appears in the log, with the type
/// of their last event in that order.
fn summaries(log: &StoreLog) -> Vec<TicketSummary> {
    let mut tickets: Vec<TicketSummary> = Vec::new();
    for event in &log.events {
        let Some(ticket) = &event.ticket else {
            continue;
        };
        match tickets.iter_mut().find(|summary| &summary.ticket == ticket) {
            Some(summary) => {
                summary.events += 1;
                summary.last.clone_from(&event.kind);
            }
            None => tickets.push(TicketSummary {
                ticket: ticket.clone(),
                events: 1,
                last: event.kind.clone(),
            }),
        }
    }
    tickets
}

/// The tickets list, or the open ticket.
#[component]
pub(crate) fn Tickets() -> Element {
    let session = use_context::<Session>();
    match (session.ticket)() {
        Some(ticket) => rsx! { TicketDetail { ticket } },
        None => rsx! { TicketList {} },
    }
}

#[component]
fn TicketList() -> Element {
    let session = use_context::<Session>();
    let log = use_resource(move || async move {
        let _ = (session.revision)();
        let hone = (session.hone)();
        hone.log().await
    });
    let mut typed = use_signal(String::new);
    rsx! {
        PageHeader {
            context: session.hone.read().root().display().to_string(),
            title: "Tickets",
        }
        form {
            class: "flex items-end gap-2 max-w-xl",
            onsubmit: move |event| {
                event.prevent_default();
                if !typed().trim().is_empty() {
                    session.open_ticket(typed().trim().to_owned());
                }
            },
            Field { label: "Open a ticket by ID",
                Input {
                    class: "font-mono",
                    value: typed(),
                    placeholder: "tk-9f2x4a8m",
                    oninput: move |value| typed.set(value),
                }
            }
            Button {
                variant: ButtonVariant::Outline,
                onclick: move |_| {
                    if !typed().trim().is_empty() {
                        session.open_ticket(typed().trim().to_owned());
                    }
                },
                "Open"
            }
        }
        match &*log.read() {
            None => rsx! { Loading {} },
            Some(Err(failure)) if failure.status == "unavailable" => rsx! {
                Empty { text: "No process store yet. Tickets appear here once an event is written or synced." }
            },
            Some(Err(failure)) => rsx! { FailureAlert { failure: failure.clone() } },
            Some(Ok(log)) => {
                let tickets = summaries(log);
                if tickets.is_empty() {
                    rsx! { Empty { text: "No ticket has events yet. Open a ticket by ID to write the first one." } }
                } else {
                    rsx! {
                        TableWrapper {
                            Table {
                                TableHeader { class: "bg-surface",
                                    TableRow {
                                        TableHead { "Ticket" }
                                        TableHead { class: "text-right", "Events" }
                                        TableHead { "Last event" }
                                    }
                                }
                                TableBody {
                                    for summary in tickets {
                                        TicketRow { summary }
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
fn TicketRow(summary: TicketSummary) -> Element {
    let session = use_context::<Session>();
    let ticket = summary.ticket.clone();
    rsx! {
        TableRow { class: "cursor-pointer", onclick: move |_| session.open_ticket(ticket.clone()),
            TableCell { class: "font-mono underline", "{summary.ticket}" }
            TableCell { class: "font-mono text-right", "{summary.events}" }
            TableCell { class: "font-mono text-ink-muted", "{summary.last}" }
        }
    }
}

/// The steps the store knows for a ticket, in the order they were first changed.
fn step_order(events: &TicketEvents) -> Vec<String> {
    let mut order: Vec<String> = Vec::new();
    for event in &events.events {
        if event.kind == "step.changed"
            && let Some(step) = event.payload["step"].as_str()
            && !order.iter().any(|known| known == step)
        {
            order.push(step.to_owned());
        }
    }
    order
}

#[component]
fn TicketDetail(ticket: String) -> Element {
    let mut session = use_context::<Session>();
    let shown = ticket.clone();
    let events = use_resource(move || {
        let ticket = shown.clone();
        async move {
            let _ = (session.revision)();
            let hone = (session.hone)();
            hone.events(&ticket).await
        }
    });
    let check_ticket = ticket.clone();
    rsx! {
        PageHeader {
            context: "ticket",
            title: ticket.clone(),
            actions: rsx! {
                Button { variant: ButtonVariant::Outline, onclick: move |_| session.ticket.set(None),
                    "All tickets"
                }
                Button {
                    onclick: move |_| {
                        session.ticket.set(Some(check_ticket.clone()));
                        session.screen.set(Screen::Check);
                    },
                    "Run hone check"
                }
            },
        }
        match &*events.read() {
            None => rsx! { Loading {} },
            Some(Err(failure)) if failure.status == "unavailable" => rsx! {
                Empty { text: "No process store yet. Set a step or open a thread to write the first event." }
                SetStep { ticket: ticket.clone() }
            },
            Some(Err(failure)) => rsx! { FailureAlert { failure: failure.clone() } },
            Some(Ok(state)) => rsx! {
                StepTrack { state: state.clone() }
                SetStep { ticket: ticket.clone() }
                ThreadTable { ticket: ticket.clone(), state: state.clone() }
                if !state.invalid.is_empty() {
                    super::events::InvalidPanel { invalid: state.invalid.clone() }
                }
            },
        }
    }
}

#[component]
fn StepTrack(state: TicketEvents) -> Element {
    let steps = step_order(&state);
    let done = state
        .steps
        .values()
        .filter(|step| step.status == "done")
        .count();
    let skipped = state
        .steps
        .values()
        .filter(|step| step.status == "skipped")
        .count();
    rsx! {
        section {
            SectionLabel { label: "Steps", aside: format!("{done} done · {skipped} skipped") }
            if steps.is_empty() {
                Empty { text: "No step has a state yet. A step without one is open." }
            } else {
                div { class: "grid grid-cols-[repeat(auto-fill,minmax(180px,1fr))] border border-line",
                    for (index , step) in steps.into_iter().enumerate() {
                        StepCell {
                            number: index + 1,
                            status: state.steps.get(&step).map(|state| state.status.clone()).unwrap_or_else(|| "open".into()),
                            reason: state.steps.get(&step).and_then(|state| state.reason.clone()),
                            step,
                        }
                    }
                }
            }
        }
    }
}

#[component]
fn StepCell(number: usize, step: String, status: String, reason: Option<String>) -> Element {
    let (edge, mark, struck) = match status.as_str() {
        "done" => ("border-t-ink", Mark::Done, ""),
        "skipped" => (
            "border-t-ink-muted border-dashed",
            Mark::Skip,
            "line-through text-ink-muted",
        ),
        _ => ("border-t-line", Mark::Open, ""),
    };
    let word = match &reason {
        Some(reason) => format!("{status} · {reason}"),
        None => status.clone(),
    };
    rsx! {
        div { class: "border-t-[3px] {edge} p-5 flex flex-col gap-3 border-r border-line min-w-0",
            span { class: "font-mono text-[13px] text-ink-muted", "{number:02}" }
            span { class: "font-semibold break-words {struck}", "{step}" }
            StatusMark { mark, word }
        }
    }
}

#[component]
fn SetStep(ticket: String) -> Element {
    let session = use_context::<Session>();
    let mut step = use_signal(String::new);
    let mut reason = use_signal(String::new);
    let mut failure = use_signal(|| None);
    let set = move |status: StepStatus| {
        let ticket = ticket.clone();
        spawn(async move {
            let write = Write::SetStep {
                ticket,
                step: step().trim().to_owned(),
                status,
                reason: Some(reason().trim().to_owned()).filter(|reason| !reason.is_empty()),
            };
            let hone = (session.hone)();
            let writer = (session.writer)();
            match hone.write(&writer, &write).await {
                Ok(_) => {
                    failure.set(None);
                    step.set(String::new());
                    reason.set(String::new());
                    session.changed();
                }
                Err(error) => failure.set(Some(error)),
            }
        });
    };
    let done = set.clone();
    let skip = set.clone();
    let open = set;
    rsx! {
        section { class: "flex flex-col gap-3",
            SectionLabel { label: "Set a step" }
            div { class: "flex flex-wrap items-end gap-2",
                Field { label: "Step of the ticket's profile",
                    Input { class: "font-mono w-56", value: step(), placeholder: "decision", oninput: move |value| step.set(value) }
                }
                Field { label: "Reason (optional)",
                    Input { class: "w-72", value: reason(), oninput: move |value| reason.set(value) }
                }
                Button { variant: ButtonVariant::Outline, onclick: move |_| open(StepStatus::Open), "Reopen" }
                Button { variant: ButtonVariant::Outline, onclick: move |_| skip(StepStatus::Skipped), "Skip step" }
                Button { variant: ButtonVariant::Outline, onclick: move |_| done(StepStatus::Done), "Mark done" }
            }
            if let Some(failure) = failure() {
                FailureAlert { failure }
            }
        }
    }
}

#[component]
fn ThreadTable(ticket: String, state: TicketEvents) -> Element {
    let session = use_context::<Session>();
    rsx! {
        section {
            SectionLabel { label: "Threads", aside: format!("{} total", state.threads.len()) }
            if state.threads.is_empty() {
                Empty { text: "No threads yet. Open one under Threads." }
            } else {
                TableWrapper {
                    Table {
                        TableHeader { class: "bg-surface",
                            TableRow {
                                TableHead { "Thread" }
                                TableHead { "Kind" }
                                TableHead { "Status" }
                                TableHead { "Opened by" }
                            }
                        }
                        TableBody {
                            for thread in state.threads.clone() {
                                TableRow {
                                    class: "cursor-pointer",
                                    onclick: {
                                        let ticket = ticket.clone();
                                        let id = thread.thread.clone();
                                        move |_| session.open_thread(ticket.clone(), id.clone())
                                    },
                                    TableCell { class: "font-mono underline break-all", "{thread.thread}" }
                                    TableCell {
                                        span { class: "font-mono", "{thread.kind}" }
                                        if thread.blocking {
                                            Badge { class: "ml-2 font-mono", "BLOCKING" }
                                        }
                                    }
                                    TableCell {
                                        StatusMark {
                                            mark: if thread.status == "open" { Mark::Open } else { Mark::Done },
                                            word: if thread.answered || thread.status != "open" { thread.status.clone() } else { format!("{} · unanswered", thread.status) },
                                        }
                                    }
                                    TableCell { class: "font-mono", "{thread.opened_by.name} · {thread.opened_by.role}" }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use serde_json::json;

    use super::*;
    use crate::hone::Event;

    fn event(id: &str, kind: &str, ticket: &str, payload: serde_json::Value) -> Event {
        Event {
            id: id.into(),
            device: "dev-3k9a5h2t".into(),
            seq: 1,
            kind: kind.into(),
            ticket: Some(ticket.into()),
            author: None,
            payload,
            raw: json!({}),
        }
    }

    #[test]
    fn tickets_appear_in_log_order_with_their_count_and_last_event() {
        let log = StoreLog {
            cursor: BTreeMap::new(),
            events: vec![
                event("ev-1", "thread.opened", "tk-b", json!({})),
                event(
                    "ev-2",
                    "step.changed",
                    "tk-a",
                    json!({ "step": "decision" }),
                ),
                event("ev-3", "thread.message", "tk-b", json!({})),
            ],
        };
        assert_eq!(
            summaries(&log),
            [
                TicketSummary {
                    ticket: "tk-b".into(),
                    events: 2,
                    last: "thread.message".into()
                },
                TicketSummary {
                    ticket: "tk-a".into(),
                    events: 1,
                    last: "step.changed".into()
                },
            ]
        );
    }

    #[test]
    fn steps_keep_the_order_of_their_first_change() {
        let events = TicketEvents {
            events: vec![
                event("ev-1", "step.changed", "tk-a", json!({ "step": "specify" })),
                event(
                    "ev-2",
                    "step.changed",
                    "tk-a",
                    json!({ "step": "decision" }),
                ),
                event("ev-3", "step.changed", "tk-a", json!({ "step": "specify" })),
            ],
            threads: Vec::new(),
            steps: BTreeMap::new(),
            invalid: Vec::new(),
        };
        assert_eq!(step_order(&events), ["specify", "decision"]);
    }
}
