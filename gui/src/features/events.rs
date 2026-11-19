use dioxus::prelude::*;

use super::Session;
use super::common::{Empty, FailureAlert, Field, Loading, PageHeader, SectionLabel, event_time};
use crate::hone::{Event, InvalidEvent};
use crate::ui::{
    Button, ButtonSize, ButtonVariant, Card, CardContent, CardHeader, CardTitle, Choice, Combobox,
    Table, TableBody, TableCell, TableHead, TableHeader, TableRow, TableWrapper,
};

/// The events whose type and ticket equal the filters; an empty filter lets every event through.
fn filtered<'a>(events: &'a [Event], kind: &str, ticket: &str) -> Vec<&'a Event> {
    events
        .iter()
        .filter(|event| kind.is_empty() || event.kind == kind)
        .filter(|event| ticket.is_empty() || event.ticket.as_deref() == Some(ticket))
        .collect()
}

/// The choices of the two filters: every type and every ticket of `events`, each after an empty
/// choice that clears the filter.
fn filter_choices(events: &[Event]) -> (Vec<Choice>, Vec<Choice>) {
    let mut types: Vec<String> = events.iter().map(|event| event.kind.clone()).collect();
    let mut tickets: Vec<String> = events
        .iter()
        .filter_map(|event| event.ticket.clone())
        .collect();
    for values in [&mut types, &mut tickets] {
        values.sort();
        values.dedup();
    }
    let with_all = |values: Vec<String>, all: &str| {
        std::iter::once(Choice::plain("").label(all))
            .chain(values.into_iter().map(Choice::plain))
            .collect()
    };
    (
        with_all(types, "every type"),
        with_all(tickets, "every ticket"),
    )
}

/// The lines of pretty JSON, each split after its key (`"name":`), so that keys can be muted.
/// A line without a key has an empty first part.
fn json_lines(json: &str) -> Vec<(String, String)> {
    json.lines()
        .map(|line| {
            let trimmed = line.trim_start();
            if trimmed.starts_with('"')
                && let Some(end) = trimmed.find("\": ")
            {
                let split = line.len() - trimmed.len() + end + 2;
                (line[..split].to_owned(), line[split..].to_owned())
            } else {
                (String::new(), line.to_owned())
            }
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
    let (type_choices, ticket_choices) = match &*log.read() {
        Some(Ok(log)) => filter_choices(&log.events),
        _ => (Vec::new(), Vec::new()),
    };
    let mut kind = use_signal(String::new);
    let mut ticket = use_signal(String::new);
    let mut selected = use_signal(|| None::<String>);
    rsx! {
        PageHeader { context: "process store", title: "Event log" }
        div { class: "flex flex-wrap gap-4",
            Field { label: "Type",
                div { class: "w-56",
                    Combobox {
                        value: kind(),
                        choices: type_choices,
                        placeholder: "every type",
                        label: "Type",
                        onselect: move |value: String| kind.set(value),
                    }
                }
            }
            Field { label: "Ticket",
                div { class: "w-56",
                    Combobox {
                        value: ticket(),
                        choices: ticket_choices,
                        placeholder: "every ticket",
                        label: "Ticket",
                        onselect: move |value: String| ticket.set(value),
                    }
                }
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
                    div { class: "grid grid-cols-[minmax(0,1fr)_minmax(280px,360px)] gap-6 items-start",
                        TableWrapper {
                            Table { class: "font-mono text-[12.5px]",
                                TableHeader { class: "bg-surface font-sans",
                                    TableRow {
                                        TableHead { "Time (UTC)" }
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
                                            TableCell { class: "py-3 whitespace-nowrap",
                                                span { class: "block", {event_time(&event.id).unwrap_or_else(|| "—".to_owned())} }
                                                span { class: if selected().as_deref() == Some(event.id.as_str()) { "block" } else { "block text-ink-muted" }, "{event.device} · {event.seq}" }
                                            }
                                            TableCell { class: "py-3", "{event.kind}" }
                                            TableCell { class: "py-3",
                                                {event.author.as_ref().map(|author| author.name.clone()).unwrap_or_default()}
                                            }
                                            TableCell { class: "py-3 max-w-[220px] truncate", {summary(&event)} }
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
                pre { class: "m-0 font-mono text-[12.5px] leading-[1.7] whitespace-pre-wrap break-all",
                    for (key , rest) in json_lines(&json) {
                        span { class: "text-ink-muted", "{key}" }
                        span { class: if key.trim() == "\"type\":" { "text-signal" } else { "" }, "{rest}" }
                        "\n"
                    }
                }
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
    fn filters_match_type_and_ticket_exactly() {
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
        assert_eq!(kinds("thread.opened", ""), ["thread.opened"]);
        assert_eq!(kinds("", "tk-9f2x4a8m"), ["thread.opened", "step.changed"]);
        assert_eq!(kinds("step.changed", "tk-2b7n5k1q"), Vec::<String>::new());
        assert_eq!(
            kinds("", ""),
            ["thread.opened", "step.changed", "thread.message", "x-note"]
        );
    }

    #[test]
    fn json_lines_split_after_the_key() {
        assert_eq!(
            json_lines("{\n  \"type\": \"step.changed\",\n  \"after\": [\n    \"ev-1\"\n  ]\n}"),
            [
                (String::new(), "{".to_owned()),
                ("  \"type\":".to_owned(), " \"step.changed\",".to_owned()),
                ("  \"after\":".to_owned(), " [".to_owned()),
                (String::new(), "    \"ev-1\"".to_owned()),
                (String::new(), "  ]".to_owned()),
                (String::new(), "}".to_owned()),
            ]
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
