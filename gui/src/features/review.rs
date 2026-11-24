//! Review inline: an artifact's rendered text with the passages its threads quote marked, the
//! threads beside it with their conversation and a reply, a new thread on a passage, the
//! artifact's approvals, and its anchors and links.

use dioxus::prelude::*;
use serde_json::{Map, Value};

use super::artifact::{LinksPanel, ReviewPanel, SourcePanel, links_of};
use super::code::CodeTarget;
use super::common::{FailureAlert, Field, Tag};
use super::threads::{Entry, conversation, kind_choices};
use super::{Project, Session, Tab};
use crate::hone::{Anchor, ArtifactNew, ArtifactSource, Event, Failure, Write};
use crate::markdown::{Quote, preview_quoted};
use crate::ui::{Button, ButtonVariant, Checkbox, Combobox, Input, Textarea};

/// A thread anchored to an artifact, numbered in the order the threads were opened.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct InlineThread {
    pub(crate) number: usize,
    pub(crate) thread: String,
    pub(crate) ticket: String,
    pub(crate) kind: String,
    pub(crate) blocking: bool,
    /// Whether the last of its `thread.resolved` and `thread.reopened` is not a resolution.
    pub(crate) open: bool,
    pub(crate) quote: Option<String>,
    /// The content hash of the artifact when the thread was opened.
    pub(crate) hash: Option<String>,
    pub(crate) entries: Vec<Entry>,
}

impl InlineThread {
    /// The first line of the opening message, or the kind without one.
    pub(crate) fn title(&self) -> String {
        self.entries
            .iter()
            .find_map(|entry| entry.body.as_deref())
            .and_then(|body| body.lines().next())
            .map_or_else(|| self.kind.clone(), str::to_owned)
    }

    /// Whether the artifact changed since the thread was opened (concept Section 10).
    fn outdated(&self, current: Option<&str>) -> bool {
        matches!((self.hash.as_deref(), current), (Some(then), Some(now)) if then != now)
    }
}

/// The threads of `events`, in the order of the log, whose anchor is the artifact `id`.
pub(crate) fn inline_threads(events: &[Event], id: &str) -> Vec<InlineThread> {
    events
        .iter()
        .filter(|event| {
            event.kind == "thread.opened"
                && event.payload["anchor"]["kind"].as_str() == Some("artifact")
                && event.payload["anchor"]["artifact"].as_str() == Some(id)
        })
        .filter_map(|opened| {
            let thread = format!("th-{}", opened.id.strip_prefix("ev-")?);
            let open = events
                .iter()
                .filter(|event| event.payload["thread"].as_str() == Some(thread.as_str()))
                .filter_map(|event| match event.kind.as_str() {
                    "thread.resolved" => Some(false),
                    "thread.reopened" => Some(true),
                    _ => None,
                })
                .next_back()
                .unwrap_or(true);
            let text = |name: &str| opened.payload["anchor"][name].as_str().map(str::to_owned);
            Some(InlineThread {
                number: 0,
                ticket: opened.ticket.clone()?,
                kind: opened.payload["kind"].as_str().unwrap_or("note").to_owned(),
                blocking: opened.payload["blocking"].as_bool().unwrap_or(false),
                open,
                quote: text("quote").filter(|quote| !quote.trim().is_empty()),
                hash: text("hash"),
                entries: conversation(events, &thread),
                thread,
            })
        })
        .enumerate()
        .map(|(index, thread)| InlineThread {
            number: index + 1,
            ..thread
        })
        .collect()
}

/// The thread the review shows open: the session's thread when it is one of `threads`,
/// otherwise the first open one.
pub(crate) fn chosen_thread(threads: &[InlineThread], session: Option<&str>) -> Option<String> {
    threads
        .iter()
        .find(|thread| Some(thread.thread.as_str()) == session)
        .or_else(|| threads.iter().find(|thread| thread.open))
        .map(|thread| thread.thread.clone())
}

/// The artifact `id` for review: its text with the quoted passages, its anchors and links below,
/// and its threads and approvals beside it.
#[component]
pub(crate) fn ReviewLayout(
    source: ArtifactSource,
    data: Option<Map<String, Value>>,
    body: String,
    /// Every thread on the artifact; a resolved one and its quote show on request.
    threads: Vec<InlineThread>,
    chosen: Option<String>,
    code: Signal<Option<CodeTarget>>,
) -> Element {
    let mut session = use_context::<Session>();
    let project = use_context::<Project>();
    let show_resolved = use_signal(|| false);
    let resolved = threads.iter().filter(|thread| !thread.open).count();
    let threads: Vec<InlineThread> = threads
        .into_iter()
        .filter(|thread| {
            thread.open || show_resolved() || chosen.as_deref() == Some(thread.thread.as_str())
        })
        .collect();
    let id = source.id.clone();
    let entry = match &*project.catalog.read() {
        Some(Ok(catalog)) => catalog
            .artifacts
            .iter()
            .find(|entry| entry.id.as_deref() == Some(id.as_str()))
            .cloned()
            .map(|entry| {
                let links = links_of(&entry, &catalog.artifacts);
                (entry, links)
            }),
        _ => None,
    };
    let review_ticket = entry.as_ref().and_then(|(entry, _)| entry.ticket.clone());
    let review_of = (review_ticket.clone(), id.clone());
    let review = use_resource(move || {
        let (ticket, artifact) = review_of.clone();
        async move {
            let _ = (session.revision)();
            match ticket {
                Some(ticket) => Some((session.hone)().review(&ticket, &artifact).await),
                None => None,
            }
        }
    });
    let review_state = review.read().clone().flatten();
    let current_hash = match &review_state {
        Some(Ok(state)) => Some(state.hash.clone()),
        _ => None,
    };
    let quotes: Vec<Quote> = threads
        .iter()
        .filter_map(|thread| {
            Some(Quote {
                number: thread.number,
                text: thread.quote.clone()?,
                chosen: chosen.as_deref() == Some(thread.thread.as_str()),
            })
        })
        .collect();
    let (html, found) = preview_quoted(&body, &quotes);
    let mut meta = vec![id.clone(), source.kind.clone()];
    if let Some(reach) = data
        .as_ref()
        .and_then(|data| data.get("reach"))
        .and_then(Value::as_str)
    {
        meta.push(format!("reach {reach}"));
    }
    let thread_ticket = (session.ticket)().or_else(|| review_ticket.clone());
    rsx! {
        div { class: "grid grid-cols-[minmax(0,1fr)_minmax(320px,400px)] gap-8 items-start",
            div { class: "flex flex-col gap-8 min-w-0",
                div { class: "flex flex-col gap-4",
                    span { class: "font-mono text-[13px] text-ink-muted", {meta.join(" · ")} }
                    if body.trim().is_empty() {
                        p { class: "m-0 text-[13px] text-ink-muted", "No body." }
                    } else {
                        div { class: "hone-prose", dangerous_inner_html: html }
                    }
                }
                SourcePanel { source: source.clone(), code }
                if let Some((_, (stored, computed))) = entry.clone() {
                    LinksPanel { stored, computed }
                }
                if (session.ticket)().is_some() {
                    div {
                        Button {
                            variant: ButtonVariant::Outline,
                            onclick: {
                                let id = id.clone();
                                move |_| {
                                    session.graph_focus.set(Some(id.clone()));
                                    session.open_tab(Tab::Graph);
                                }
                            },
                            "Show in the link graph"
                        }
                    }
                }
            }
            div { class: "flex flex-col gap-6 min-w-0",
                ThreadColumn {
                    threads: threads.clone(),
                    resolved,
                    show_resolved,
                    chosen: chosen.clone(),
                    found,
                    current_hash,
                    artifact: id.clone(),
                    ticket: thread_ticket,
                }
                if let Some(ticket) = review_ticket {
                    ReviewPanel { key: "{ticket}-{id}", ticket, artifact: id.clone(), state: review_state }
                }
            }
        }
    }
}

#[component]
fn ThreadColumn(
    threads: Vec<InlineThread>,
    resolved: usize,
    show_resolved: Signal<bool>,
    chosen: Option<String>,
    found: Vec<usize>,
    current_hash: Option<String>,
    artifact: String,
    ticket: Option<String>,
) -> Element {
    let mut show_resolved = show_resolved;
    let shown = threads.clone();
    let open_count = threads.iter().filter(|thread| thread.open).count();
    rsx! {
        section { class: "flex flex-col gap-3",
            div { class: "flex items-baseline gap-3",
                span { class: "font-mono text-[12px] tracking-[0.08em] uppercase font-semibold", "Threads · {open_count}" }
                if resolved > 0 {
                    button {
                        class: "ml-auto font-mono text-[12px] text-ink-muted underline",
                        onclick: move |_| show_resolved.toggle(),
                        if show_resolved() { "hide {resolved} resolved" } else { "{resolved} resolved hidden" }
                    }
                }
            }
            if shown.is_empty() {
                p { class: "m-0 text-[13px] text-ink-muted", "No open thread on this artifact." }
            }
            for thread in shown {
                ThreadCard {
                    key: "{thread.thread}",
                    open: chosen.as_deref() == Some(thread.thread.as_str()),
                    quote_found: found.contains(&thread.number),
                    outdated: thread.outdated(current_hash.as_deref()),
                    thread,
                }
            }
            OpenInline { artifact, ticket }
        }
    }
}

#[component]
fn ThreadCard(thread: InlineThread, open: bool, quote_found: bool, outdated: bool) -> Element {
    let mut session = use_context::<Session>();
    let title = thread.title();
    let messages = thread
        .entries
        .iter()
        .filter(|entry| entry.body.is_some())
        .count();
    let opened_by = thread
        .entries
        .first()
        .map(|entry| entry.author.clone())
        .unwrap_or_default();
    let mut notes = Vec::new();
    if thread.quote.is_some() && !quote_found {
        notes.push("quote not found in the text".to_owned());
    }
    if outdated {
        notes.push("on an older content".to_owned());
    }
    if !thread.open {
        notes.push("resolved".to_owned());
    }
    let border = if open {
        "border-2 border-signal"
    } else {
        "border border-line"
    };
    rsx! {
        article { class: "{border} flex flex-col",
            button {
                class: "w-full text-left px-4 py-3 flex flex-col gap-1 hover:bg-surface-2",
                "aria-expanded": if open { "true" } else { "false" },
                onclick: {
                    let id = thread.thread.clone();
                    move |_| session.thread.set(Some(id.clone()))
                },
                div { class: "flex items-baseline gap-2",
                    span { class: "font-semibold text-signal", "{thread.number}" }
                    span { class: if thread.open { "font-semibold break-words" } else { "font-semibold break-words line-through text-ink-muted" }, "{title}" }
                    if thread.blocking && thread.open {
                        span { class: "ml-auto shrink-0 px-2 font-mono text-[11px] bg-signal text-on-signal", "BLOCKING" }
                    }
                }
                if !open {
                    span { class: "text-[13px] text-ink-muted",
                        {format!("{opened_by} · {} · {} message{}", thread.kind, messages, if messages == 1 { "" } else { "s" })}
                    }
                }
                if !notes.is_empty() {
                    span { class: "font-mono text-[12px] text-warn", {notes.join(" · ")} }
                }
            }
            if open {
                Conversation { thread: thread.clone() }
            }
        }
    }
}

#[component]
fn Conversation(thread: InlineThread) -> Element {
    let session = use_context::<Session>();
    let mut body = use_signal(String::new);
    let mut failure = use_signal(|| None::<Failure>);
    let mut busy = use_signal(|| false);
    let id = thread.thread.clone();
    let reply = move |_| {
        let text = body();
        if text.trim().is_empty() {
            return;
        }
        let write = Write::Reply {
            thread: id.clone(),
            body: text,
        };
        busy.set(true);
        spawn(async move {
            match (session.hone)().write(&(session.writer)(), &write).await {
                Ok(_) => {
                    failure.set(None);
                    body.set(String::new());
                    session.changed();
                }
                Err(error) => failure.set(Some(error)),
            }
            busy.set(false);
        });
    };
    rsx! {
        div { class: "border-t border-line px-4 py-3 flex flex-col gap-3",
            for entry in thread.entries.clone() {
                div { class: "flex flex-col gap-0.5",
                    div { class: "flex gap-2 items-baseline flex-wrap",
                        span { class: "font-semibold", "{entry.author}" }
                        span { class: "font-mono text-[12px] text-ink-muted", "{entry.role}" }
                        if let Some(time) = entry.time.clone() {
                            span { class: "font-mono text-[12px] text-ink-muted", "· {time}" }
                        }
                        if entry.kind != "opened" && entry.kind != "message" {
                            span { class: "font-mono text-[12px]", "· {entry.kind}" }
                        }
                    }
                    if let Some(text) = entry.body.clone() {
                        p { class: "m-0 text-[14px] leading-[22px] whitespace-pre-wrap break-words", "{text}" }
                    }
                }
            }
        }
        div { class: "border-t border-line px-4 py-3 flex flex-col gap-2",
            Textarea { value: body(), rows: 2, placeholder: "Reply", oninput: move |value| body.set(value) }
            div { class: "flex justify-end",
                Button { variant: ButtonVariant::Outline, disabled: busy() || body().trim().is_empty(), onclick: reply, "Reply" }
            }
            if let Some(failure) = failure() {
                FailureAlert { failure }
            }
        }
    }
}

/// A new thread on the artifact, its quote taken from the text selected in the window.
#[component]
fn OpenInline(artifact: String, ticket: Option<String>) -> Element {
    let mut session = use_context::<Session>();
    let project = use_context::<Project>();
    let mut shown = use_signal(|| false);
    let mut kind = use_signal(|| "question".to_owned());
    let mut blocking = use_signal(|| false);
    let mut quote = use_signal(String::new);
    let mut body = use_signal(String::new);
    let mut failure = use_signal(|| None::<Failure>);
    let mut busy = use_signal(|| false);
    let Some(ticket) = ticket else {
        return rsx! {
            p { class: "m-0 text-[13px] text-ink-muted", "A thread belongs to a ticket: open one to start a thread here." }
        };
    };
    let seen: Vec<String> = match &*project.log.read() {
        Some(Ok(log)) => log
            .events
            .iter()
            .filter(|event| event.kind == "thread.opened")
            .filter_map(|event| event.payload["kind"].as_str().map(str::to_owned))
            .collect(),
        _ => Vec::new(),
    };
    let take_selection = move |_| {
        spawn(async move {
            let mut eval = document::eval("dioxus.send(String(window.getSelection() || ''));");
            if let Ok(text) = eval.recv::<String>().await {
                let text = text.split_whitespace().collect::<Vec<_>>().join(" ");
                if !text.is_empty() {
                    quote.set(text);
                }
            }
        });
    };
    let writes_in = ticket.clone();
    let open = move |_| {
        let write = Write::OpenThread {
            ticket: ticket.clone(),
            kind: kind(),
            blocking: blocking(),
            anchor: Anchor::Artifact {
                artifact: artifact.clone(),
                quote: Some(quote().trim().to_owned()).filter(|quote| !quote.is_empty()),
            },
            body: body(),
        };
        busy.set(true);
        spawn(async move {
            match (session.hone)().write(&(session.writer)(), &write).await {
                Ok(appended) => {
                    failure.set(None);
                    body.set(String::new());
                    quote.set(String::new());
                    shown.set(false);
                    session.thread.set(appended.thread);
                    session.changed();
                }
                Err(error) => failure.set(Some(error)),
            }
            busy.set(false);
        });
    };
    if !shown() {
        return rsx! {
            div {
                Button {
                    variant: ButtonVariant::Outline,
                    title: "Select a passage in the text first to quote it",
                    onclick: move |event| {
                        take_selection(event);
                        shown.set(true);
                    },
                    "Open thread"
                }
            }
        };
    }
    rsx! {
        section { class: "border border-line p-4 flex flex-col gap-3",
            div { class: "flex flex-wrap gap-3 items-end",
                Field { label: "Kind",
                    div { class: "w-44",
                        Combobox {
                            value: kind(),
                            choices: kind_choices(&seen),
                            label: "Kind",
                            custom: true,
                            empty: "Type a new kind and press Enter.",
                            onselect: move |value: String| kind.set(value),
                        }
                    }
                }
                label { class: "h-11 flex items-center gap-2 text-sm",
                    Checkbox { checked: blocking(), aria_label: "Blocking", on_checked_change: move |value| blocking.set(value) }
                    "Blocking"
                }
            }
            Field { label: "Quote (optional)",
                div { class: "flex gap-2",
                    Input { class: "flex-1", value: quote(), oninput: move |value| quote.set(value) }
                    Button { variant: ButtonVariant::Outline, title: "Quote the text selected in the window", onclick: take_selection, "Selection" }
                }
            }
            Field { label: "Text",
                Textarea { value: body(), rows: 3, oninput: move |value| body.set(value) }
            }
            div { class: "flex items-center gap-2",
                span { class: "font-mono text-[12px] text-ink-muted mr-auto", "writes thread.opened in {writes_in}" }
                Button { variant: ButtonVariant::Outline, onclick: move |_| shown.set(false), "Cancel" }
                Button { disabled: busy() || body().trim().is_empty(), onclick: open, "Open thread" }
            }
            if let Some(failure) = failure() {
                FailureAlert { failure }
            }
        }
    }
}

/// The head of a binding artifact: read-only, and Supersede, which creates an artifact of the
/// same kind in the open ticket that supersedes it.
#[component]
pub(crate) fn BindingBar(source: ArtifactSource, status: String) -> Element {
    let session = use_context::<Session>();
    let mut asked = use_signal(|| false);
    let mut title = use_signal(String::new);
    let mut failure = use_signal(|| None::<Failure>);
    let mut refused = use_signal(Vec::new);
    let mut busy = use_signal(|| false);
    let ticket = (session.ticket)();
    let decided = source.lifecycle == "decided";
    let create = {
        let ticket = ticket.clone();
        let source = source.clone();
        move |_| {
            let new = ArtifactNew {
                kind: source.kind.clone(),
                title: title().trim().to_owned(),
                ticket: ticket.clone(),
                author: Some((session.writer)().author).filter(|author| !author.is_empty()),
                anchors: Vec::new(),
                supersedes: vec![source.id.clone()],
            };
            busy.set(true);
            spawn(async move {
                match (session.hone)().new_artifact(&new).await {
                    Ok(written) if written.saved => {
                        failure.set(None);
                        asked.set(false);
                        title.set(String::new());
                        session.changed();
                        if let Some(id) = written.id {
                            session.open_artifact(id);
                        }
                    }
                    Ok(written) => refused.set(written.findings),
                    Err(error) => failure.set(Some(error)),
                }
                busy.set(false);
            });
        }
    };
    rsx! {
        div { class: "flex flex-col gap-3",
            div { class: "flex flex-wrap items-center gap-3",
                Tag { inverted: true, "{status.to_uppercase()} · binding" }
                span { class: "font-mono text-[12.5px] text-ink-muted",
                    if decided { "read-only · change it with a superseding {source.kind}" } else { "read-only" }
                }
                if decided {
                    Button {
                        class: "ml-auto",
                        variant: ButtonVariant::Outline,
                        disabled: ticket.is_none(),
                        title: if ticket.is_none() { "Open the ticket that changes it" } else { "Create a {source.kind} in the open ticket that supersedes this one" },
                        onclick: move |_| asked.toggle(),
                        "Supersede"
                    }
                }
            }
            if asked() {
                div { class: "border border-line p-4 flex flex-wrap items-end gap-3",
                    Field { label: "Title of the superseding {source.kind}",
                        Input { class: "w-96", value: title(), oninput: move |value| title.set(value) }
                    }
                    span { class: "font-mono text-[12px] text-ink-muted", {format!("in {} · supersedes {}", ticket.clone().unwrap_or_default(), source.id)} }
                    div { class: "ml-auto flex gap-2",
                        Button { variant: ButtonVariant::Outline, onclick: move |_| asked.set(false), "Cancel" }
                        Button { disabled: busy() || title().trim().is_empty(), onclick: create, "Create" }
                    }
                }
                for finding in refused() {
                    p { class: "m-0 text-[13px] text-bad", role: "alert", "✕ {finding.code}: {finding.message}" }
                }
                if let Some(failure) = failure() {
                    FailureAlert { failure }
                }
            }
        }
    }
}

/// Resolves the chosen thread, or reopens it when it is resolved.
#[component]
pub(crate) fn ResolveThread(thread: InlineThread) -> Element {
    let session = use_context::<Session>();
    let mut failure = use_signal(|| None::<Failure>);
    let open = thread.open;
    let id = thread.thread.clone();
    rsx! {
        Button {
            variant: if open { ButtonVariant::Default } else { ButtonVariant::Outline },
            title: "{thread.number} {thread.title()}",
            onclick: move |_| {
                let thread = id.clone();
                let write = if open { Write::Resolve { thread } } else { Write::Reopen { thread } };
                spawn(async move {
                    match (session.hone)().write(&(session.writer)(), &write).await {
                        Ok(_) => {
                            failure.set(None);
                            session.changed();
                        }
                        Err(error) => failure.set(Some(error)),
                    }
                });
            },
            if open { "Resolve thread" } else { "Reopen thread" }
        }
        if let Some(failure) = failure() {
            FailureAlert { failure }
        }
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::hone::Author;

    fn event(id: &str, kind: &str, ticket: &str, payload: Value) -> Event {
        Event {
            id: id.into(),
            device: "dev-3k9a5h2t".into(),
            seq: 1,
            kind: kind.into(),
            ticket: Some(ticket.into()),
            author: Some(Author {
                name: "bob".into(),
                role: "human".into(),
            }),
            payload,
            raw: json!({}),
        }
    }

    fn on(artifact: &str, quote: Option<&str>) -> Value {
        json!({
            "kind": "question",
            "blocking": true,
            "anchor": { "kind": "artifact", "artifact": artifact, "hash": "sha256:aa", "quote": quote },
            "body": "How long?\nSupport needs a number.",
        })
    }

    #[test]
    fn the_threads_of_an_artifact_are_numbered_with_their_state() {
        let events = [
            event(
                "ev-0001",
                "thread.opened",
                "tk-1",
                on("dec-7k3m2q8p", Some("pending state")),
            ),
            event("ev-0002", "thread.opened", "tk-1", on("dec-2h6r9t1x", None)),
            event(
                "ev-0003",
                "thread.opened",
                "tk-2",
                json!({ "kind": "concern", "anchor": { "kind": "code", "ref": "dec-7k3m2q8p" }, "body": "x" }),
            ),
            event(
                "ev-0004",
                "thread.opened",
                "tk-2",
                on("dec-7k3m2q8p", Some(" ")),
            ),
            event(
                "ev-0005",
                "thread.resolved",
                "tk-1",
                json!({ "thread": "th-0001" }),
            ),
            event(
                "ev-0006",
                "thread.reopened",
                "tk-1",
                json!({ "thread": "th-0001" }),
            ),
            event(
                "ev-0007",
                "thread.resolved",
                "tk-2",
                json!({ "thread": "th-0004" }),
            ),
        ];
        let threads = inline_threads(&events, "dec-7k3m2q8p");
        let summary: Vec<_> = threads
            .iter()
            .map(|thread| {
                (
                    thread.number,
                    thread.thread.as_str(),
                    thread.ticket.as_str(),
                    thread.open,
                    thread.quote.as_deref(),
                    thread.entries.len(),
                )
            })
            .collect();
        assert_eq!(
            summary,
            [
                (1, "th-0001", "tk-1", true, Some("pending state"), 3),
                (2, "th-0004", "tk-2", false, None, 2),
            ]
        );
        assert_eq!(threads[0].title(), "How long?");
        assert!(threads[0].blocking);
        assert!(threads[0].outdated(Some("sha256:bb")));
        assert!(!threads[0].outdated(Some("sha256:aa")));
        assert!(!threads[0].outdated(None));
    }

    #[test]
    fn the_chosen_thread_is_the_sessions_or_the_first_open_one() {
        let events = [
            event("ev-0001", "thread.opened", "tk-1", on("dec-7k3m2q8p", None)),
            event("ev-0002", "thread.opened", "tk-1", on("dec-7k3m2q8p", None)),
            event(
                "ev-0003",
                "thread.resolved",
                "tk-1",
                json!({ "thread": "th-0001" }),
            ),
        ];
        let threads = inline_threads(&events, "dec-7k3m2q8p");
        assert_eq!(chosen_thread(&threads, None).as_deref(), Some("th-0002"));
        assert_eq!(
            chosen_thread(&threads, Some("th-0001")).as_deref(),
            Some("th-0001")
        );
        assert_eq!(
            chosen_thread(&threads, Some("th-9999")).as_deref(),
            Some("th-0002")
        );
        assert_eq!(chosen_thread(&threads[..1], None), None);
    }
}
