use std::collections::BTreeSet;

use dioxus::prelude::*;
use serde_json::Value;

use super::Project;
use super::common::{
    Empty, FailureAlert, Field, Loading, PageHeader, SectionLabel, event_time, now_time, short_hash,
};
use crate::hone::{Event, InvalidEvent};
use crate::ui::{
    Button, ButtonSize, ButtonVariant, Card, CardContent, CardHeader, CardTitle, Choice, Combobox,
    Table, TableBody, TableCell, TableHead, TableHeader, TableRow, TableWrapper,
};

/// Whether `kind` passes the type filter: every type for an empty filter, the types of a family
/// for `family.*`, otherwise exactly that type.
fn type_matches(kind: &str, filter: &str) -> bool {
    match filter.strip_suffix(".*") {
        _ if filter.is_empty() => true,
        Some(family) => kind
            .split_once('.')
            .is_some_and(|(prefix, _)| prefix == family),
        None => kind == filter,
    }
}

/// The events of `ticket` (every ticket when empty) that pass the type filter, newest first.
fn filtered<'a>(events: &'a [Event], kind: &str, ticket: &str) -> Vec<&'a Event> {
    events
        .iter()
        .rev()
        .filter(|event| type_matches(&event.kind, kind))
        .filter(|event| ticket.is_empty() || event.ticket.as_deref() == Some(ticket))
        .collect()
}

/// The choices of the type filter: all types, then every type of `events` in order, each family
/// with two types or more (`thread.*`) before its first type.
fn type_choices(events: &[Event]) -> Vec<Choice> {
    let types: BTreeSet<&str> = events.iter().map(|event| event.kind.as_str()).collect();
    let family = |kind: &str| kind.split_once('.').map(|(prefix, _)| prefix.to_owned());
    let mut choices = vec![Choice::plain("").label("All types")];
    let mut previous = None;
    for kind in &types {
        let current = family(kind);
        if current.is_some() && current != previous {
            let members = types
                .iter()
                .filter(|other| family(other) == current)
                .count();
            if members > 1 {
                choices.push(Choice::plain(format!(
                    "{}.*",
                    current.clone().unwrap_or_default()
                )));
            }
        }
        choices.push(Choice::plain(*kind));
        previous = current;
    }
    choices
}

/// The choices of the ticket filter: every ticket, then each ticket of `events` once.
fn ticket_choices(events: &[Event]) -> Vec<Choice> {
    let tickets: BTreeSet<&str> = events
        .iter()
        .filter_map(|event| event.ticket.as_deref())
        .collect();
    std::iter::once(Choice::plain("").label("All tickets"))
        .chain(tickets.into_iter().map(Choice::plain))
        .collect()
}

/// The time of a row: the UTC clock for an event of `today` (`YYYY-MM-DD`), otherwise its date.
fn row_time(id: &str, today: &str) -> String {
    match event_time(id)
        .as_deref()
        .and_then(|time| time.split_once(' '))
    {
        Some((date, clock)) if date == today => clock.to_owned(),
        Some((date, _)) => date.to_owned(),
        None => "—".to_owned(),
    }
}

/// `count` and `noun`, plural unless `count` is 1.
fn counted(count: usize, noun: &str) -> String {
    if count == 1 {
        format!("1 {noun}")
    } else {
        format!("{count} {noun}s")
    }
}

/// A device ID without its `dev-` prefix.
fn device_name(device: &str) -> &str {
    device.strip_prefix("dev-").unwrap_or(device)
}

/// The first line of a text field of `payload`.
fn first_line(payload: &Value, field: &str) -> String {
    payload[field]
        .as_str()
        .and_then(|text| text.lines().next())
        .unwrap_or_default()
        .to_owned()
}

/// What a thread's anchor points at: an artifact with its quote, a code or change reference, or a
/// transcript.
fn anchor_target(anchor: &Value) -> String {
    match anchor["kind"].as_str() {
        Some("artifact") => {
            let artifact = anchor["artifact"].as_str().unwrap_or_default();
            match anchor["quote"].as_str() {
                Some(quote) => format!("{artifact} “{}”", quote.lines().next().unwrap_or_default()),
                None => artifact.to_owned(),
            }
        }
        Some("code" | "change") => anchor["ref"].as_str().unwrap_or_default().to_owned(),
        Some("transcript") => anchor["url"].as_str().unwrap_or_default().to_owned(),
        _ => String::new(),
    }
}

/// A one-line summary of an event's payload, per type of version 1; the compact payload for any
/// other type.
fn summary(event: &Event) -> String {
    let payload = &event.payload;
    let text = |field: &str| payload[field].as_str().unwrap_or_default();
    match event.kind.as_str() {
        "thread.opened" => {
            let blocking = if payload["blocking"].as_bool() == Some(true) {
                "blocking · "
            } else {
                ""
            };
            format!(
                "{blocking}{} · {}",
                text("kind"),
                anchor_target(&payload["anchor"])
            )
        }
        "thread.message" => format!("{} “{}”", text("thread"), first_line(payload, "body")),
        "thread.resolved" | "thread.reopened" => text("thread").to_owned(),
        "approval.given" => format!("{} {}", text("artifact"), text("decision")),
        "review.confirmed" => format!("{} · {}", text("artifact"), short_hash(text("hash"))),
        "step.changed" => format!("{} → {}", text("step"), text("status")),
        "ticket.started" => format!(
            "profile {} · {}",
            text("profile"),
            short_hash(text("config"))
        ),
        "ticket.config_changed" => short_hash(text("config")),
        "ticket.attributes_changed" => payload["attributes"]
            .as_object()
            .map(|attributes| {
                attributes
                    .iter()
                    .map(|(name, value)| format!("{name}: {value}"))
                    .collect::<Vec<_>>()
                    .join(", ")
            })
            .unwrap_or_default(),
        _ => payload.to_string(),
    }
}

/// The fields of an event, one per line as key and compact JSON: the envelope in a fixed order,
/// then any further field (`x-…`) by name.
fn detail_lines(raw: &Value) -> Vec<(String, String)> {
    const ORDER: [&str; 9] = [
        "id", "type", "ticket", "device", "seq", "author", "project", "after", "payload",
    ];
    let Some(fields) = raw.as_object() else {
        return Vec::new();
    };
    let known = ORDER
        .iter()
        .filter_map(|key| fields.get(*key).map(|value| (*key, value)));
    let rest = fields
        .iter()
        .filter(|(key, _)| !ORDER.contains(&key.as_str()))
        .map(|(key, value)| (key.as_str(), value));
    known
        .chain(rest)
        .map(|(key, value)| {
            let shown = value
                .as_str()
                .map_or_else(|| value.to_string(), str::to_owned);
            (key.to_owned(), shown)
        })
        .collect()
}

/// The events of the store, newest first, filterable by type, with the selected event in full
/// below. With `ticket`, only that ticket's events; without, a filter by ticket too.
#[component]
pub(crate) fn EventLog(#[props(optional)] ticket: Option<String>) -> Element {
    let project = use_context::<Project>();
    let mut kind = use_signal(String::new);
    let mut ticket_filter = use_signal(String::new);
    let mut selected = use_signal(|| None::<String>);
    let scope = ticket.clone().unwrap_or_else(&*ticket_filter);
    let log = project.log.read();
    let (context, types, tickets) = match &*log {
        Some(Ok(log)) => {
            let in_scope: Vec<Event> = filtered(&log.events, "", &scope)
                .into_iter()
                .cloned()
                .collect();
            let devices: BTreeSet<&str> =
                in_scope.iter().map(|event| event.device.as_str()).collect();
            let mut context = vec!["process store".to_owned()];
            context.extend(ticket.clone());
            context.push(format!(
                "{} · {}",
                counted(in_scope.len(), "event"),
                counted(devices.len(), "device")
            ));
            (
                context.join(" · "),
                type_choices(&in_scope),
                ticket_choices(&log.events),
            )
        }
        _ => ("process store".to_owned(), Vec::new(), Vec::new()),
    };
    let today = now_time()
        .split_once(' ')
        .map(|(date, _)| date.to_owned())
        .unwrap_or_default();
    rsx! {
        PageHeader {
            context,
            title: "Event log",
            actions: rsx! {
                if ticket.is_none() {
                    Field { label: "Ticket",
                        div { class: "w-56",
                            Combobox {
                                value: ticket_filter(),
                                choices: tickets,
                                placeholder: "All tickets",
                                label: "Ticket",
                                onselect: move |value: String| ticket_filter.set(value),
                            }
                        }
                    }
                }
                Field { label: "Type",
                    div { class: "w-56",
                        Combobox {
                            value: kind(),
                            choices: types,
                            placeholder: "All types",
                            label: "Type",
                            onselect: move |value: String| kind.set(value),
                        }
                    }
                }
            },
        }
        match &*log {
            None => rsx! { Loading {} },
            Some(Err(failure)) if failure.status == "unavailable" => rsx! {
                Empty { text: "No process store yet. Events appear here once one is written or synced." }
            },
            Some(Err(failure)) => rsx! { FailureAlert { failure: failure.clone() } },
            Some(Ok(log)) => {
                let shown: Vec<Event> = filtered(&log.events, &kind(), &scope).into_iter().cloned().collect();
                let detail = selected().and_then(|id| shown.iter().find(|event| event.id == id).cloned());
                rsx! {
                    if shown.is_empty() {
                        Empty { text: "No events match." }
                    } else {
                        TableWrapper { class: "shrink-0",
                            Table { class: "font-mono text-[12.5px]",
                                TableHeader { class: "bg-surface font-sans",
                                    TableRow {
                                        TableHead { "Time (UTC)" }
                                        TableHead { "Device · seq" }
                                        TableHead { "Type" }
                                        TableHead { "Payload" }
                                    }
                                }
                                TableBody {
                                    for event in shown {
                                        EventRow {
                                            key: "{event.id}",
                                            selected: selected().as_deref() == Some(event.id.as_str()),
                                            time: row_time(&event.id, &today),
                                            onselect: move |id: String| selected.set(Some(id)),
                                            event,
                                        }
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

#[component]
fn EventRow(event: Event, selected: bool, time: String, onselect: EventHandler<String>) -> Element {
    let id = event.id.clone();
    rsx! {
        TableRow {
            class: if selected { "bg-ink text-bg hover:bg-ink data-[state=selected]:bg-ink cursor-pointer" } else { "cursor-pointer" },
            selected,
            onclick: move |_| onselect.call(id.clone()),
            TableCell { class: if selected { "py-3 whitespace-nowrap" } else { "py-3 whitespace-nowrap text-ink-muted" }, "{time}" }
            TableCell { class: "py-3 whitespace-nowrap", "{device_name(&event.device)} · {event.seq}" }
            TableCell { class: if selected { "py-3 font-bold" } else { "py-3" }, "{event.kind}" }
            TableCell { class: "py-3 whitespace-normal break-words", {summary(&event)} }
        }
    }
}

#[component]
fn EventDetail(event: Event) -> Element {
    let copied = serde_json::to_string(&event.raw).unwrap_or_default();
    rsx! {
        Card { class: "py-0 gap-0 shrink-0",
            CardHeader { class: "bg-surface border-b border-line py-2 flex-row items-center justify-between",
                CardTitle { class: "font-mono text-[11px] tracking-[0.08em] uppercase font-medium",
                    "Event · seq {event.seq}"
                }
                Button {
                    variant: ButtonVariant::Outline,
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
                    if let Some(time) = event_time(&event.id) {
                        span { class: "text-ink-muted", "time: " }
                        "{time} UTC\n"
                    }
                    for (key , value) in detail_lines(&event.raw) {
                        span { class: "text-ink-muted", "{key}: " }
                        span { class: if key == "type" { "text-signal" } else { "" }, "{value}" }
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

    fn event(kind: &str, ticket: Option<&str>, payload: Value) -> Event {
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

    fn kinds(events: &[Event], kind: &str, ticket: &str) -> Vec<String> {
        filtered(events, kind, ticket)
            .into_iter()
            .map(|event| event.kind.clone())
            .collect()
    }

    #[test]
    fn filters_match_type_family_and_ticket_newest_first() {
        let events = [
            event("thread.opened", Some("tk-9f2x4a8m"), json!({})),
            event("step.changed", Some("tk-9f2x4a8m"), json!({})),
            event("thread.message", Some("tk-2b7n5k1q"), json!({})),
            event("x-note", Some("tk-9f2x4a8m"), json!({})),
        ];
        assert_eq!(kinds(&events, "thread.opened", ""), ["thread.opened"]);
        assert_eq!(
            kinds(&events, "thread.*", ""),
            ["thread.message", "thread.opened"]
        );
        assert_eq!(
            kinds(&events, "", "tk-9f2x4a8m"),
            ["x-note", "step.changed", "thread.opened"]
        );
        assert_eq!(
            kinds(&events, "thread.*", "tk-2b7n5k1q"),
            ["thread.message"]
        );
        assert_eq!(
            kinds(&events, "step.changed", "tk-2b7n5k1q"),
            Vec::<String>::new()
        );
        assert_eq!(kinds(&events, "x-note.*", ""), Vec::<String>::new());
    }

    #[test]
    fn a_family_is_offered_before_its_types_when_it_has_two() {
        let events = [
            event("thread.resolved", None, json!({})),
            event("step.changed", None, json!({})),
            event("thread.opened", None, json!({})),
            event("thread.opened", None, json!({})),
            event("x-note", None, json!({})),
        ];
        let values: Vec<String> = type_choices(&events)
            .into_iter()
            .map(|choice| choice.value)
            .collect();
        assert_eq!(
            values,
            [
                "",
                "step.changed",
                "thread.*",
                "thread.opened",
                "thread.resolved",
                "x-note"
            ]
        );
    }

    #[test]
    fn a_row_shows_the_clock_today_and_the_date_before() {
        // 0x019a9da53000 ms = 2025-11-19 19:44 UTC.
        let id = "ev-019a9da5-3000-7000-8000-000000000000";
        assert_eq!(row_time(id, "2025-11-19"), "19:44");
        assert_eq!(row_time(id, "2025-11-20"), "2025-11-19");
        assert_eq!(row_time("ev-not-a-uuid", "2025-11-19"), "—");
    }

    #[test]
    fn a_count_is_plural_unless_one() {
        assert_eq!(counted(0, "device"), "0 devices");
        assert_eq!(counted(1, "device"), "1 device");
        assert_eq!(counted(3, "event"), "3 events");
    }

    #[test]
    fn a_summary_names_the_payload_per_type() {
        let cases = [
            (
                event(
                    "thread.opened",
                    None,
                    json!({ "kind": "question", "blocking": true, "body": "Why?",
                        "anchor": { "kind": "artifact", "artifact": "dec-7k3m2q8p", "hash": "sha256:00", "quote": "Consequences\nmore" } }),
                ),
                "blocking · question · dec-7k3m2q8p “Consequences”",
            ),
            (
                event(
                    "thread.opened",
                    None,
                    json!({ "kind": "risk", "blocking": false, "body": "x",
                        "anchor": { "kind": "code", "ref": "src/fee.rs#quote" } }),
                ),
                "risk · src/fee.rs#quote",
            ),
            (
                event(
                    "thread.message",
                    None,
                    json!({ "thread": "th-1", "body": "Works for me.\nsecond" }),
                ),
                "th-1 “Works for me.”",
            ),
            (
                event("thread.resolved", None, json!({ "thread": "th-1" })),
                "th-1",
            ),
            (
                event(
                    "approval.given",
                    None,
                    json!({ "artifact": "dec-7k3m2q8p", "hash": "sha256:00", "decision": "approved" }),
                ),
                "dec-7k3m2q8p approved",
            ),
            (
                event(
                    "step.changed",
                    None,
                    json!({ "step": "domain", "status": "skipped" }),
                ),
                "domain → skipped",
            ),
            (
                event(
                    "ticket.started",
                    None,
                    json!({ "profile": "full", "config": "sha256:5be2a1c9d0" }),
                ),
                "profile full · sha256:5be2a1c9…",
            ),
            (
                event(
                    "ticket.attributes_changed",
                    None,
                    json!({ "attributes": { "risk": "high", "size": 3 } }),
                ),
                "risk: \"high\", size: 3",
            ),
            (event("x-note", None, json!({ "a": 1 })), "{\"a\":1}"),
        ];
        for (event, expected) in cases {
            assert_eq!(summary(&event), expected, "{}", event.kind);
        }
    }

    #[test]
    fn detail_lines_put_the_envelope_first_and_further_fields_after() {
        let raw = json!({
            "x-origin": "import",
            "payload": { "step": "domain" },
            "author": { "name": "alice", "role": "architect" },
            "type": "step.changed",
            "seq": 118,
            "id": "ev-1",
        });
        assert_eq!(
            detail_lines(&raw),
            [
                ("id".to_owned(), "ev-1".to_owned()),
                ("type".to_owned(), "step.changed".to_owned()),
                ("seq".to_owned(), "118".to_owned()),
                (
                    "author".to_owned(),
                    "{\"name\":\"alice\",\"role\":\"architect\"}".to_owned()
                ),
                ("payload".to_owned(), "{\"step\":\"domain\"}".to_owned()),
                ("x-origin".to_owned(), "import".to_owned()),
            ]
        );
    }
}
