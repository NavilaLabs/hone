use dioxus::prelude::*;

use super::Session;
use super::common::{
    Empty, FailureAlert, Field, Loading, Mark, PageHeader, SectionLabel, StatusMark,
};
use crate::hone::{Anchor, Event, ThreadState, Write};
use crate::ui::{
    Badge, Button, ButtonVariant, Checkbox, Input, Separator, Tabs, TabsContent, TabsList,
    TabsTrigger, TabsVariant, Textarea,
};

/// One event of a thread, as the conversation shows it.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Entry {
    author: String,
    /// `opened`, `message`, `resolved`, or `reopened`.
    kind: String,
    body: Option<String>,
    anchor: Option<String>,
}

/// The thread ID of an opening event: `th-` plus the UUID of the event (concept Section 10).
fn thread_id(event: &Event) -> Option<String> {
    event
        .id
        .strip_prefix("ev-")
        .map(|uuid| format!("th-{uuid}"))
}

/// The events of `thread` among the ticket's `events`, in their order.
fn conversation(events: &[Event], thread: &str) -> Vec<Entry> {
    events
        .iter()
        .filter_map(|event| {
            let kind = event.kind.strip_prefix("thread.")?;
            let belongs = if kind == "opened" {
                thread_id(event).as_deref() == Some(thread)
            } else {
                event.payload["thread"].as_str() == Some(thread)
            };
            belongs.then(|| Entry {
                author: event.author.as_ref().map_or_else(
                    || "unknown".to_owned(),
                    |author| format!("{} · {}", author.name, author.role),
                ),
                kind: kind.to_owned(),
                body: event.payload["body"].as_str().map(str::to_owned),
                anchor: (kind == "opened").then(|| anchor_text(&event.payload["anchor"])),
            })
        })
        .collect()
}

fn anchor_text(anchor: &serde_json::Value) -> String {
    let field = |name: &str| anchor[name].as_str().unwrap_or_default().to_owned();
    match anchor["kind"].as_str() {
        Some("artifact") => match anchor["quote"].as_str() {
            Some(quote) => format!("artifact {} · “{quote}”", field("artifact")),
            None => format!("artifact {}", field("artifact")),
        },
        Some("code") => format!("code {}", field("ref")),
        Some("change") => format!("change {}", field("ref")),
        Some("transcript") => format!("transcript {}", field("url")),
        _ => "unknown anchor".to_owned(),
    }
}

/// The threads of the open ticket: the list, the conversation, and the reply.
#[component]
pub(crate) fn Threads() -> Element {
    let session = use_context::<Session>();
    let Some(ticket) = (session.ticket)() else {
        return rsx! {
            PageHeader { title: "Threads" }
            Empty { text: "No ticket is open. Open a ticket under Tickets to see its threads." }
        };
    };
    let shown = ticket.clone();
    let state = use_resource(move || {
        let ticket = shown.clone();
        async move {
            let _ = (session.revision)();
            let hone = (session.hone)();
            hone.events(&ticket).await
        }
    });
    let tab = use_signal(|| "conversation".to_owned());
    rsx! {
        PageHeader { context: ticket.clone(), title: "Threads" }
        Tabs { active: tab,
            TabsList { variant: TabsVariant::Line,
                TabsTrigger { value: "conversation", "Conversation" }
                TabsTrigger { value: "open", "Open a thread" }
            }
            TabsContent { value: "conversation",
                match &*state.read() {
                    None => rsx! { Loading {} },
                    Some(Err(failure)) if failure.status == "unavailable" => rsx! {
                        Empty { text: "No threads yet. Open one with Open a thread." }
                    },
                    Some(Err(failure)) => rsx! { FailureAlert { failure: failure.clone() } },
                    Some(Ok(state)) => rsx! {
                        ThreadSplit { threads: state.threads.clone(), events: state.events.clone() }
                    },
                }
            }
            TabsContent { value: "open",
                OpenThread { ticket: ticket.clone(), tab }
            }
        }
    }
}

#[component]
fn ThreadSplit(threads: Vec<ThreadState>, events: Vec<Event>) -> Element {
    let mut session = use_context::<Session>();
    if threads.is_empty() {
        return rsx! { Empty { text: "No threads yet. Open one with Open a thread." } };
    }
    let selected = (session.thread)()
        .filter(|selected| threads.iter().any(|thread| &thread.thread == selected))
        .unwrap_or_else(|| threads[0].thread.clone());
    let current = threads
        .iter()
        .find(|thread| thread.thread == selected)
        .cloned();
    rsx! {
        div { class: "grid grid-cols-[minmax(240px,320px)_1fr] gap-0 border border-line min-h-[420px]",
            ul { class: "m-0 p-0 list-none border-r border-line",
                for thread in threads.clone() {
                    li {
                        button {
                            class: if thread.thread == selected { "w-full text-left p-4 flex flex-col gap-1 bg-surface-2 border-b border-line" } else { "w-full text-left p-4 flex flex-col gap-1 hover:bg-surface border-b border-line" },
                            onclick: {
                                let id = thread.thread.clone();
                                move |_| session.thread.set(Some(id.clone()))
                            },
                            div { class: "flex gap-2 items-center",
                                if thread.blocking {
                                    Badge { class: "font-mono", "BLOCKING" }
                                } else {
                                    Badge { variant: crate::ui::BadgeVariant::Outline, class: "font-mono", "{thread.kind}" }
                                }
                                StatusMark {
                                    mark: if thread.status == "open" { Mark::Open } else { Mark::Done },
                                    word: thread.status.clone(),
                                }
                            }
                            span { class: if thread.status == "open" { "font-mono text-[13px] break-all" } else { "font-mono text-[13px] break-all line-through text-ink-muted" },
                                "{thread.thread}"
                            }
                            span { class: "font-mono text-[12.5px] text-ink-muted", "{thread.opened_by.name} · {thread.opened_by.role}" }
                        }
                    }
                }
            }
            if let Some(thread) = current {
                Conversation { entries: conversation(&events, &thread.thread), thread }
            }
        }
    }
}

#[component]
fn Conversation(thread: ThreadState, entries: Vec<Entry>) -> Element {
    let session = use_context::<Session>();
    let mut body = use_signal(String::new);
    let mut failure = use_signal(|| None);
    let id = thread.thread.clone();
    let send = move |write: Write| {
        spawn(async move {
            let hone = (session.hone)();
            let writer = (session.writer)();
            match hone.write(&writer, &write).await {
                Ok(_) => {
                    failure.set(None);
                    body.set(String::new());
                    session.changed();
                }
                Err(error) => failure.set(Some(error)),
            }
        });
    };
    let reply = send;
    let toggle = send;
    let reply_id = id.clone();
    let open = thread.status == "open";
    let anchor = entries.iter().find_map(|entry| entry.anchor.clone());
    rsx! {
        div { class: "flex flex-col gap-5 p-6 min-w-0",
            h2 { class: "m-0 text-[22px] leading-7 font-bold tracking-[-0.01em]", "{thread.kind}" }
            if let Some(anchor) = anchor {
                div { class: "bg-surface border border-line p-4 font-mono text-[13px] text-ink-muted break-words",
                    "anchored to {anchor}"
                }
            }
            for entry in entries {
                article { class: "flex flex-col gap-1 max-w-[640px]",
                    div { class: "flex gap-2 items-baseline",
                        span { class: "font-mono text-[12.5px] text-ink-muted", "{entry.author}" }
                        if entry.kind != "opened" && entry.kind != "message" {
                            span { class: "font-mono text-[12.5px]", "· {entry.kind}" }
                        }
                    }
                    if let Some(text) = entry.body {
                        p { class: "m-0 text-[15px] leading-[25px] whitespace-pre-wrap break-words", "{text}" }
                    }
                }
            }
            Separator {}
            Field { label: "Reply",
                Textarea { value: body(), rows: 3, oninput: move |value| body.set(value) }
            }
            div { class: "flex flex-wrap gap-2 justify-end items-center",
                span { class: "font-mono text-[12.5px] text-ink-muted mr-auto", "writes thread.message" }
                Button {
                    variant: ButtonVariant::Outline,
                    onclick: move |_| {
                        let thread = id.clone();
                        toggle(if open { Write::Resolve { thread } } else { Write::Reopen { thread } });
                    },
                    if open { "Resolve" } else { "Reopen" }
                }
                Button {
                    onclick: move |_| {
                        if !body().trim().is_empty() {
                            reply(Write::Reply { thread: reply_id.clone(), body: body() });
                        }
                    },
                    "Send"
                }
            }
            if let Some(failure) = failure() {
                FailureAlert { failure }
            }
        }
    }
}

#[component]
fn OpenThread(ticket: String, tab: Signal<String>) -> Element {
    let mut session = use_context::<Session>();
    let mut kind = use_signal(|| "question".to_owned());
    let mut blocking = use_signal(|| false);
    let mut anchor_kind = use_signal(|| "artifact".to_owned());
    let mut target = use_signal(String::new);
    let mut quote = use_signal(String::new);
    let mut body = use_signal(String::new);
    let mut failure = use_signal(|| None);
    let kind_tab = use_signal(|| "artifact".to_owned());
    use_effect(move || anchor_kind.set(kind_tab()));
    let open = move |_| {
        let ticket = ticket.clone();
        spawn(async move {
            let anchor = match anchor_kind().as_str() {
                "code" => Anchor::Code(target()),
                "change" => Anchor::Change(target()),
                _ => Anchor::Artifact {
                    artifact: target(),
                    quote: Some(quote()).filter(|quote| !quote.is_empty()),
                },
            };
            let write = Write::OpenThread {
                ticket: ticket.clone(),
                kind: kind(),
                blocking: blocking(),
                anchor,
                body: body(),
            };
            let hone = (session.hone)();
            let writer = (session.writer)();
            match hone.write(&writer, &write).await {
                Ok(appended) => {
                    failure.set(None);
                    body.set(String::new());
                    session.thread.set(appended.thread);
                    tab.set("conversation".into());
                    session.changed();
                }
                Err(error) => failure.set(Some(error)),
            }
        });
    };
    rsx! {
        section { class: "flex flex-col gap-4 max-w-[720px]",
            SectionLabel { label: "Open a thread" }
            div { class: "flex flex-wrap gap-4 items-end",
                Field { label: "Kind",
                    Input { class: "font-mono w-48", value: kind(), oninput: move |value| kind.set(value) }
                }
                label { class: "h-11 flex items-center gap-2 text-sm",
                    Checkbox { checked: blocking(), aria_label: "Blocking", on_checked_change: move |value| blocking.set(value) }
                    "Blocking: holds no-open-blocking-threads while open"
                }
            }
            Tabs { active: kind_tab,
                TabsList { variant: TabsVariant::Line,
                    TabsTrigger { value: "artifact", "Artifact" }
                    TabsTrigger { value: "code", "Code" }
                    TabsTrigger { value: "change", "Change" }
                }
            }
            div { class: "flex flex-wrap gap-4",
                Field {
                    label: match anchor_kind().as_str() {
                        "code" => "path#symbol",
                        "change" => "Reference",
                        _ => "Artifact ID",
                    },
                    Input { class: "font-mono w-72", value: target(), oninput: move |value| target.set(value) }
                }
                if anchor_kind() == "artifact" {
                    Field { label: "Quote (optional)",
                        Input { class: "w-72", value: quote(), oninput: move |value| quote.set(value) }
                    }
                }
            }
            Field { label: "Text",
                Textarea { value: body(), rows: 4, oninput: move |value| body.set(value) }
            }
            div { class: "flex justify-end items-center gap-2",
                span { class: "font-mono text-[12.5px] text-ink-muted mr-auto", "writes thread.opened" }
                Button { onclick: open, "Open thread" }
            }
            if let Some(failure) = failure() {
                FailureAlert { failure }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::hone::Author;

    fn event(id: &str, kind: &str, payload: serde_json::Value) -> Event {
        Event {
            id: id.into(),
            device: "dev-3k9a5h2t".into(),
            seq: 1,
            kind: kind.into(),
            ticket: Some("tk-9f2x4a8m".into()),
            author: Some(Author {
                name: "alice".into(),
                role: "architect".into(),
            }),
            payload,
            raw: json!({}),
        }
    }

    #[test]
    fn a_conversation_holds_only_its_own_thread() {
        let events = [
            event(
                "ev-0001",
                "thread.opened",
                json!({ "body": "Why?", "anchor": { "kind": "code", "ref": "src/a.rs#A" } }),
            ),
            event("ev-0002", "thread.opened", json!({ "body": "Other" })),
            event(
                "ev-0003",
                "thread.message",
                json!({ "thread": "th-0002", "body": "No" }),
            ),
            event(
                "ev-0004",
                "thread.message",
                json!({ "thread": "th-0001", "body": "Because." }),
            ),
            event("ev-0005", "thread.resolved", json!({ "thread": "th-0001" })),
            event("ev-0006", "step.changed", json!({ "thread": "th-0001" })),
        ];
        let entry = |kind: &str, body: Option<&str>, anchor: Option<&str>| Entry {
            author: "alice · architect".into(),
            kind: kind.into(),
            body: body.map(str::to_owned),
            anchor: anchor.map(str::to_owned),
        };
        assert_eq!(
            conversation(&events, "th-0001"),
            [
                entry("opened", Some("Why?"), Some("code src/a.rs#A")),
                entry("message", Some("Because."), None),
                entry("resolved", None, None),
            ]
        );
    }
}
