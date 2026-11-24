//! The global search: tickets, artifacts, threads, and the tracker's tickets, opened with
//! Ctrl+K or Cmd+K.

use std::time::Duration;

use dioxus::prelude::*;

use super::{Project, Session};
use crate::hone::{Catalog, Event};
use crate::tracker::{Issue, Tracker};

/// What a result opens.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Target {
    Ticket(String),
    Artifact(String),
    /// A thread, and the artifact its anchor names.
    Thread {
        ticket: String,
        thread: String,
        artifact: Option<String>,
    },
    /// A tracker ticket that no record names: start a ticket for it.
    Start(super::new_ticket::Seed),
}

/// A result of the search.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Hit {
    /// `Tickets`, `Artifacts`, `Threads`, or `Tracker`.
    pub(crate) group: &'static str,
    pub(crate) key: String,
    pub(crate) text: String,
    pub(crate) target: Target,
    /// What Enter does when it is not obvious, shown on the right.
    pub(crate) note: Option<String>,
}

const LIMIT: usize = 8;

/// The hits for `query` in the catalog and the log, at most eight per group. Every word of the
/// query must occur in the key or the text, ignoring case.
pub(crate) fn search(catalog: Option<&Catalog>, events: &[Event], query: &str) -> Vec<Hit> {
    let words: Vec<String> = query.split_whitespace().map(str::to_lowercase).collect();
    if words.is_empty() {
        return Vec::new();
    }
    let matches = |key: &str, text: &str| {
        let haystack = format!("{key} {text}").to_lowercase();
        words.iter().all(|word| haystack.contains(word))
    };
    let mut tickets = Vec::new();
    let mut artifacts = Vec::new();
    for entry in catalog
        .map(|catalog| catalog.artifacts.as_slice())
        .unwrap_or_default()
    {
        let Some(id) = entry.id.clone() else {
            continue;
        };
        if entry.kind == "ticket" {
            let text = entry
                .external
                .iter()
                .map(|external| format!("{} {}", external.system, external.key))
                .collect::<Vec<_>>()
                .join(", ");
            if matches(&id, &text) {
                tickets.push(Hit {
                    group: "Tickets",
                    key: id.clone(),
                    text,
                    target: Target::Ticket(id),
                    note: None,
                });
            }
        } else {
            let text = format!(
                "{} · {}",
                entry.kind,
                entry.title.clone().unwrap_or_default()
            );
            if matches(&id, &text) {
                artifacts.push(Hit {
                    group: "Artifacts",
                    key: id.clone(),
                    text: text.trim_end_matches(" · ").to_owned(),
                    target: Target::Artifact(id),
                    note: None,
                });
            }
        }
    }
    let threads = events
        .iter()
        .filter(|event| event.kind == "thread.opened")
        .filter_map(|event| {
            let ticket = event.ticket.clone()?;
            let thread = format!("th-{}", event.id.strip_prefix("ev-")?);
            let line = event.payload["body"]
                .as_str()
                .unwrap_or_default()
                .lines()
                .next()
                .unwrap_or_default()
                .to_owned();
            let artifact = (event.payload["anchor"]["kind"].as_str() == Some("artifact"))
                .then(|| {
                    event.payload["anchor"]["artifact"]
                        .as_str()
                        .map(str::to_owned)
                })
                .flatten();
            matches(&ticket, &line).then(|| Hit {
                group: "Threads",
                key: ticket.clone(),
                text: line,
                target: Target::Thread {
                    ticket,
                    thread,
                    artifact,
                },
                note: None,
            })
        });
    tickets
        .into_iter()
        .take(LIMIT)
        .chain(artifacts.into_iter().take(LIMIT))
        .chain(threads.take(LIMIT))
        .collect()
}

/// The tracker's tickets as hits: a ticket that a ticket record names opens that hone ticket,
/// any other one the Tracker screen.
pub(crate) fn tracker_hits(
    tracker: &Tracker,
    issues: &[Issue],
    catalog: Option<&Catalog>,
) -> Vec<Hit> {
    issues
        .iter()
        .take(LIMIT)
        .map(|issue| {
            let linked = catalog.and_then(|catalog| {
                catalog.tickets().find_map(|record| {
                    record
                        .external
                        .iter()
                        .any(|external| tracker.names(&external.system, &external.key, &issue.key))
                        .then(|| record.id.clone())
                        .flatten()
                })
            });
            let key = match tracker {
                Tracker::Github { .. } => format!("#{}", issue.key),
                Tracker::Jira { .. } => issue.key.clone(),
            };
            Hit {
                group: "Tracker",
                text: format!("{} · {}", issue.title, issue.state),
                note: Some(match &linked {
                    Some(ticket) => format!("opens {ticket}"),
                    None => "starts a ticket".to_owned(),
                }),
                target: linked.map_or_else(
                    || Target::Start(super::new_ticket::Seed::from_issue(tracker, issue)),
                    Target::Ticket,
                ),
                key,
            }
        })
        .collect()
}

/// Opens the search on Ctrl+K or Cmd+K anywhere in the window.
#[component]
pub(crate) fn SearchShortcut() -> Element {
    let mut session = use_context::<Session>();
    use_future(move || async move {
        // The key listener lives in the page; it tells the app through the evaluation's channel.
        let mut listener = document::eval(
            r#"document.addEventListener('keydown', (event) => {
                if ((event.ctrlKey || event.metaKey) && event.key.toLowerCase() === 'k') {
                    event.preventDefault();
                    dioxus.send(true);
                }
            });"#,
        );
        while listener.recv::<bool>().await.is_ok() {
            session.search_open.set(true);
        }
    });
    rsx! {}
}

/// The search dialog over the current screen.
#[component]
pub(crate) fn SearchDialog() -> Element {
    let mut session = use_context::<Session>();
    let project = use_context::<Project>();
    let mut query = use_signal(String::new);
    let mut active = use_signal(|| 0usize);
    let tracker = project.tracker();
    let tracker_results = use_resource(move || async move {
        let query = query();
        let tracker = project.tracker()?;
        if query.trim().chars().count() < 2 {
            return None;
        }
        // Waits for the typing to pause; a new keystroke restarts the resource.
        tokio::time::sleep(Duration::from_millis(350)).await;
        Some(tracker.search(query.trim()).await)
    });
    let events = match &*project.log.read() {
        Some(Ok(log)) => log.events.clone(),
        _ => Vec::new(),
    };
    let catalog = match &*project.catalog.read() {
        Some(Ok(catalog)) => Some(catalog.clone()),
        _ => None,
    };
    let mut hits = search(catalog.as_ref(), &events, &query());
    let tracker_note = match &*tracker_results.read() {
        Some(Some(Ok(issues))) => {
            if let Some(tracker) = &tracker {
                hits.extend(tracker_hits(tracker, issues, catalog.as_ref()));
            }
            None
        }
        Some(Some(Err(message))) => Some(format!("✕ the tracker could not be searched: {message}")),
        Some(None) => None,
        None if tracker.is_some() && query().trim().chars().count() >= 2 => {
            Some("searching the tracker…".to_owned())
        }
        None => None,
    };
    let highlighted = if hits.is_empty() {
        0
    } else {
        active().min(hits.len() - 1)
    };
    let mut open = move |target: Target| {
        session.search_open.set(false);
        match target {
            Target::Ticket(ticket) => session.open_ticket(ticket),
            Target::Artifact(id) => session.open_artifact(id),
            Target::Thread {
                ticket,
                thread,
                artifact,
            } => session.open_thread(ticket, thread, artifact),
            Target::Start(seed) => session.new_ticket.set(Some(seed)),
        }
    };
    let keyed = hits.clone();
    rsx! {
        div {
            class: "fixed inset-0 z-50 flex items-start justify-center pt-[12vh] bg-[color-mix(in_srgb,var(--hone-bg)_70%,transparent)]",
            onclick: move |_| session.search_open.set(false),
            div {
                class: "w-[min(680px,92vw)] border border-line bg-bg flex flex-col",
                role: "dialog",
                "aria-modal": "true",
                "aria-label": "Search",
                onclick: move |event| event.stop_propagation(),
                input {
                    r#type: "search",
                    class: "h-12 px-4 border-0 border-b border-line bg-transparent text-[15px] outline-none text-ink placeholder:text-ink-muted",
                    placeholder: if tracker.is_some() { "Search tickets, artifacts, threads, and the tracker" } else { "Search tickets, artifacts, and threads" },
                    autofocus: true,
                    role: "combobox",
                    "aria-expanded": "true",
                    "aria-controls": "hone-search-results",
                    "aria-activedescendant": if hits.is_empty() { String::new() } else { format!("hone-search-{highlighted}") },
                    value: "{query}",
                    onmounted: move |element| async move {
                        let _ = element.set_focus(true).await;
                    },
                    oninput: move |event| {
                        query.set(event.value());
                        active.set(0);
                    },
                    onkeydown: move |event| match event.key() {
                        Key::ArrowDown => {
                            event.prevent_default();
                            if !keyed.is_empty() {
                                active.set((highlighted + 1) % keyed.len());
                            }
                        }
                        Key::ArrowUp => {
                            event.prevent_default();
                            if !keyed.is_empty() {
                                active.set((highlighted + keyed.len() - 1) % keyed.len());
                            }
                        }
                        Key::Enter => {
                            if let Some(hit) = keyed.get(highlighted) {
                                open(hit.target.clone());
                            }
                        }
                        Key::Escape => session.search_open.set(false),
                        _ => {}
                    },
                }
                ul { id: "hone-search-results", role: "listbox", class: "m-0 p-0 list-none max-h-[60vh] overflow-y-auto",
                    if query().trim().is_empty() {
                        li { class: "px-4 py-3 text-[13px] text-ink-muted", "Type an ID, a title, words of a thread, or a tracker key. ↑ ↓ move, Enter opens, Esc closes." }
                    } else if hits.is_empty() && tracker_note.is_none() {
                        li { class: "px-4 py-3 text-[13px] text-ink-muted", "Nothing matches." }
                    }
                    for (index , hit) in hits.iter().cloned().enumerate() {
                        if index == 0 || hits[index - 1].group != hit.group {
                            li { class: "px-4 pt-3 pb-1 font-mono text-[11px] tracking-[0.08em] uppercase text-ink-muted", role: "presentation", "{hit.group}" }
                        }
                        li {
                            id: "hone-search-{index}",
                            role: "option",
                            "aria-selected": if index == highlighted { "true" } else { "false" },
                            class: if index == highlighted { "px-4 py-2 flex items-baseline gap-3 cursor-pointer bg-signal-muted border-l-2 border-signal" } else { "px-4 py-2 flex items-baseline gap-3 cursor-pointer border-l-2 border-transparent hover:bg-surface-2" },
                            onmousedown: move |event| event.prevent_default(),
                            onclick: {
                                let target = hit.target.clone();
                                move |_| open(target.clone())
                            },
                            span { class: "font-mono text-[13px] shrink-0", "{hit.key}" }
                            span { class: "text-[13px] text-ink-muted truncate min-w-0", "{hit.text}" }
                            if let Some(note) = hit.note.clone() {
                                span { class: "ml-auto pl-3 font-mono text-[12px] text-ink-muted shrink-0 whitespace-nowrap", "↵ {note}" }
                            }
                        }
                    }
                    if let Some(note) = tracker_note {
                        li { class: if note.starts_with('✕') { "px-4 py-3 text-[13px] text-bad" } else { "px-4 py-3 text-[13px] text-ink-muted" }, "{note}" }
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
    use crate::hone::{CatalogEntry, ExternalTicket};

    fn entry(id: &str, kind: &str, title: Option<&str>) -> CatalogEntry {
        CatalogEntry {
            id: Some(id.into()),
            kind: kind.into(),
            path: String::new(),
            ticket: None,
            title: title.map(str::to_owned),
            status: None,
            effective: None,
            profile: None,
            external: if kind == "ticket" {
                vec![ExternalTicket {
                    system: "jira".into(),
                    key: "PAY-123".into(),
                    url: None,
                }]
            } else {
                Vec::new()
            },
            links: BTreeMap::new(),
        }
    }

    fn catalog() -> Catalog {
        Catalog {
            project: "prj-8m2k4d7q".into(),
            artifacts: vec![
                entry(
                    "dec-7k3m2q8p",
                    "decision",
                    Some("Payments run asynchronously"),
                ),
                entry("spec-3b8n1c5w", "spec", Some("Async charge flow")),
                entry("tk-9f2x4a8m", "ticket", None),
            ],
            unreadable: Vec::new(),
        }
    }

    fn thread(body: &str) -> Event {
        Event {
            id: "ev-0193a8f0-9b2c-7e4d-a1f3-000000000001".into(),
            device: "dev-3k9a5h2t".into(),
            seq: 1,
            kind: "thread.opened".into(),
            ticket: Some("tk-9f2x4a8m".into()),
            author: None,
            payload: json!({ "body": body }),
            raw: json!({}),
        }
    }

    #[test]
    fn every_word_must_match_and_groups_keep_their_order() {
        let events = [thread("What happens when the queue fails?\nDetails.")];
        let keys = |query: &str| -> Vec<(&'static str, String)> {
            search(Some(&catalog()), &events, query)
                .into_iter()
                .map(|hit| (hit.group, hit.key))
                .collect()
        };
        assert_eq!(
            keys("async"),
            [
                ("Artifacts", "dec-7k3m2q8p".to_owned()),
                ("Artifacts", "spec-3b8n1c5w".to_owned())
            ]
        );
        assert_eq!(keys("pay-123"), [("Tickets", "tk-9f2x4a8m".to_owned())]);
        assert_eq!(keys("QUEUE fails"), [("Threads", "tk-9f2x4a8m".to_owned())]);
        assert!(keys("queue async").is_empty());
        assert!(keys("   ").is_empty());
    }

    #[test]
    fn a_linked_tracker_ticket_opens_the_hone_ticket() {
        let tracker = Tracker::Jira {
            base_url: "https://acme.atlassian.net".into(),
            project_key: "PAY".into(),
            email: None,
            token_env: None,
        };
        let issue = |key: &str| Issue {
            key: key.into(),
            title: "Async".into(),
            state: "open".into(),
            url: String::new(),
        };
        let hits = tracker_hits(
            &tracker,
            &[issue("PAY-123"), issue("PAY-124")],
            Some(&catalog()),
        );
        assert_eq!(hits[0].target, Target::Ticket("tk-9f2x4a8m".into()));
        assert_eq!(hits[0].note.as_deref(), Some("opens tk-9f2x4a8m"));
        assert_eq!(hits[1].note.as_deref(), Some("starts a ticket"));
        assert_eq!(
            hits[1].target,
            Target::Start(super::super::new_ticket::Seed {
                external: vec!["jira:PAY-124".into()],
                title: Some("Async".into()),
            })
        );
    }

    #[test]
    fn a_thread_hit_opens_its_thread() {
        let hits = search(None, &[thread("Why async?")], "why");
        assert_eq!(
            hits[0].target,
            Target::Thread {
                ticket: "tk-9f2x4a8m".into(),
                thread: "th-0193a8f0-9b2c-7e4d-a1f3-000000000001".into(),
                artifact: None,
            }
        );
        assert_eq!(hits[0].text, "Why async?");
    }
}
