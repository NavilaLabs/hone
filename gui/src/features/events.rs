use dioxus::prelude::*;

use super::Session;
use super::common::{Empty, FailureAlert, Field, Loading, PageHeader, SectionLabel};
use crate::hone::{Event, InvalidEvent};
use crate::ui::{
    Button, ButtonSize, ButtonVariant, Card, CardContent, CardHeader, CardTitle, Input, Table,
    TableBody, TableCell, TableHead, TableHeader, TableRow, TableWrapper,
};

/// The events whose type and ticket contain the filters, ignoring case.
fn filtered<'a>(events: &'a [Event], kind: &str, ticket: &str) -> Vec<&'a Event> {
    let kind = kind.trim().to_lowercase();
    let ticket = ticket.trim().to_lowercase();
    events
        .iter()
        .filter(|event| event.kind.to_lowercase().contains(&kind))
        .filter(|event| {
            ticket.is_empty()
                || event
                    .ticket
                    .as_deref()
                    .is_some_and(|value| value.to_lowercase().contains(&ticket))
        })
        .collect()
}

/// A one-line summary of a payload: its body, its step and status, or its thread.
fn summary(event: &Event) -> String {
    let payload = &event.payload;
    if let Some(body) = payload["body"].as_str() {
        return body.lines().next().unwrap_or_default().to_owned();
    }
    if let (Some(step), Some(status)) = (payload["step"].as_str(), payload["status"].as_str()) {
        return format!("{step} → {status}");
    }
    payload["thread"].as_str().unwrap_or_default().to_owned()
}

/// Every event of the store, filterable, with the full event beside the selected row.
#[component]
pub(crate) fn EventLog() -> Element {
    let session = use_context::<Session>();
    let log = use_resource(move || async move {
        let _ = (session.revision)();
        let hone = (session.hone)();
        hone.log().await
    });
    let mut kind = use_signal(String::new);
    let mut ticket = use_signal(String::new);
    let mut selected = use_signal(|| None::<String>);
    rsx! {
        PageHeader { context: "process store", title: "Event log" }
        div { class: "flex flex-wrap gap-4",
            Field { label: "Type",
                Input { class: "font-mono w-56", value: kind(), placeholder: "thread.", oninput: move |value| kind.set(value) }
            }
            Field { label: "Ticket",
                Input { class: "font-mono w-56", value: ticket(), placeholder: "tk-", oninput: move |value| ticket.set(value) }
            }
        }
        match &*log.read() {
            None => rsx! { Loading {} },
            Some(Err(failure)) if failure.status == "unavailable" => rsx! {
                Empty { text: "No process store yet. Events appear here once one is written or synced." }
            },
            Some(Err(failure)) => rsx! { FailureAlert { failure: failure.clone() } },
            Some(Ok(log)) => {
                let shown: Vec<Event> = filtered(&log.events, &kind(), &ticket()).into_iter().cloned().collect();
                let detail = selected().and_then(|id| log.events.iter().find(|event| event.id == id).cloned());
                rsx! {
                    div { class: "grid grid-cols-[minmax(0,1fr)_minmax(280px,420px)] gap-6 items-start",
                        TableWrapper {
                            Table { class: "font-mono text-[12.5px]",
                                TableHeader { class: "bg-surface font-sans",
                                    TableRow {
                                        TableHead { "Device · seq" }
                                        TableHead { "Type" }
                                        TableHead { "Author" }
                                        TableHead { "Payload" }
                                    }
                                }
                                TableBody {
                                    for event in shown {
                                        TableRow {
                                            class: if selected().as_deref() == Some(event.id.as_str()) { "bg-ink text-bg hover:bg-ink cursor-pointer" } else { "cursor-pointer" },
                                            onclick: {
                                                let id = event.id.clone();
                                                move |_| selected.set(Some(id.clone()))
                                            },
                                            TableCell { class: "py-3 whitespace-nowrap", "{event.device} · {event.seq}" }
                                            TableCell { class: "py-3", "{event.kind}" }
                                            TableCell { class: "py-3",
                                                {event.author.as_ref().map(|author| author.name.clone()).unwrap_or_default()}
                                            }
                                            TableCell { class: "py-3 max-w-[320px] truncate", {summary(&event)} }
                                        }
                                    }
                                }
                            }
                        }
                        match detail {
                            Some(event) => rsx! { EventDetail { event } },
                            None => rsx! { Empty { text: "Select an event to see it in full." } },
                        }
                    }
                }
            }
        }
    }
}

#[component]
fn EventDetail(event: Event) -> Element {
    let json = serde_json::to_string_pretty(&event.raw).unwrap_or_default();
    let copied = serde_json::to_string(&event.raw).unwrap_or_default();
    rsx! {
        Card { class: "py-0 gap-0",
            CardHeader { class: "bg-surface border-b border-line py-2 flex-row items-center justify-between",
                CardTitle { class: "font-mono text-[11px] tracking-[0.08em] uppercase font-medium", "Event" }
                Button {
                    variant: ButtonVariant::Ghost,
                    size: ButtonSize::Sm,
                    onclick: move |_| {
                        let script = format!(
                            "navigator.clipboard.writeText({})",
                            serde_json::to_string(&copied).unwrap_or_default()
                        );
                        document::eval(&script);
                    },
                    "Copy JSON"
                }
            }
            CardContent { class: "p-4",
                pre { class: "m-0 font-mono text-[12.5px] leading-[1.7] whitespace-pre-wrap break-all", "{json}" }
            }
        }
    }
}

/// The invalid events of a result: file, line, and reason. They are not read, but reported.
#[component]
pub(crate) fn InvalidPanel(invalid: Vec<InvalidEvent>) -> Element {
    rsx! {
        section {
            SectionLabel { label: "Invalid events", aside: format!("{} not read", invalid.len()) }
            Card { class: "py-0 gap-0 border-bad",
                CardHeader { class: "bg-bad text-bg py-2",
                    CardTitle { class: "font-mono text-[11px] tracking-[0.08em] uppercase font-medium", "✕ invalid" }
                }
                CardContent { class: "p-0",
                    for entry in invalid {
                        div { class: "px-5 py-3 border-b border-line flex flex-col gap-1",
                            span { class: "font-mono text-[13px] break-all",
                                "{entry.path}"
                                if let Some(line) = entry.line {
                                    ":{line}"
                                }
                            }
                            span { class: "text-[13px]", "{entry.message}" }
                        }
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn event(kind: &str, ticket: Option<&str>, payload: serde_json::Value) -> Event {
        Event {
            id: format!("ev-{kind}"),
            device: "dev-3k9a5h2t".into(),
            seq: 1,
            kind: kind.into(),
            ticket: ticket.map(str::to_owned),
            author: None,
            payload,
            raw: json!({}),
        }
    }

    #[test]
    fn filters_match_parts_of_type_and_ticket_ignoring_case() {
        let events = [
            event("thread.opened", Some("tk-9f2x4a8m"), json!({})),
            event("step.changed", Some("tk-9f2x4a8m"), json!({})),
            event("thread.message", Some("tk-2b7n5k1q"), json!({})),
            event("x-note", None, json!({})),
        ];
        let kinds = |kind: &str, ticket: &str| -> Vec<String> {
            filtered(&events, kind, ticket)
                .into_iter()
                .map(|event| event.kind.clone())
                .collect()
        };
        assert_eq!(kinds("THREAD", ""), ["thread.opened", "thread.message"]);
        assert_eq!(kinds("", "9F2X"), ["thread.opened", "step.changed"]);
        assert_eq!(
            kinds("", ""),
            ["thread.opened", "step.changed", "thread.message", "x-note"]
        );
    }

    #[test]
    fn a_summary_prefers_the_body_then_the_step() {
        assert_eq!(
            summary(&event(
                "thread.message",
                None,
                json!({ "body": "first\nsecond", "thread": "th-1" })
            )),
            "first"
        );
        assert_eq!(
            summary(&event(
                "step.changed",
                None,
                json!({ "step": "decision", "status": "done" })
            )),
            "decision → done"
        );
        assert_eq!(
            summary(&event("thread.resolved", None, json!({ "thread": "th-1" }))),
            "th-1"
        );
    }
}
