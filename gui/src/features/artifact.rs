//! One artifact on the Artifacts view, and what the catalog and the log know about it: its
//! anchors, its links both ways, the threads on it, and its review.

use dioxus::prelude::*;

use super::artifact_tab::ArtifactTab;
use super::code::{CodePanel, CodeTarget, open_code};
use super::common::{
    Empty, FailureAlert, Field, Loading, Mark, PageHeader, ProblemAlert, SectionLabel, StatusMark,
    open_file,
};
use super::{Project, Session, Tab};
use crate::hone::{ArtifactSource, CatalogEntry, Event, Failure, ReviewState, Write};
use crate::ui::{Button, ButtonVariant, Card, CardContent, CardHeader, CardTitle, Input};

/// A link between two artifacts, as the detail lists it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Link {
    /// The link type, for example `supersedes`, or `ticket`.
    pub(crate) kind: String,
    /// The ID on the other end.
    pub(crate) other: String,
}

/// The links `entry` stores (its `links` and its ticket), and the links other artifacts store
/// towards it, which are computed (concept Section 5: only forward links are stored).
pub(crate) fn links_of(entry: &CatalogEntry, all: &[CatalogEntry]) -> (Vec<Link>, Vec<Link>) {
    let mut stored: Vec<Link> = entry
        .links
        .iter()
        .flat_map(|(kind, ids)| {
            ids.iter().map(|other| Link {
                kind: kind.clone(),
                other: other.clone(),
            })
        })
        .collect();
    if let Some(ticket) = &entry.ticket {
        stored.push(Link {
            kind: "ticket".into(),
            other: ticket.clone(),
        });
    }
    let Some(id) = entry.id.as_deref() else {
        return (stored, Vec::new());
    };
    let computed = all
        .iter()
        .flat_map(|other| {
            let from = other.id.clone().unwrap_or_default();
            let mut found: Vec<Link> = other
                .links
                .iter()
                .filter(|(_, ids)| ids.iter().any(|target| target == id))
                .map(|(kind, _)| Link {
                    kind: kind.clone(),
                    other: from.clone(),
                })
                .collect();
            if entry.kind == "ticket" && other.ticket.as_deref() == Some(id) {
                found.push(Link {
                    kind: "ticket".into(),
                    other: from,
                });
            }
            found
        })
        .collect();
    (stored, computed)
}

/// The threads of `events` anchored to the artifact `id`: the thread ID and its first line.
fn anchored_threads(events: &[Event], id: &str) -> Vec<(String, String, String)> {
    events
        .iter()
        .filter(|event| {
            event.kind == "thread.opened"
                && event.payload["anchor"]["artifact"].as_str() == Some(id)
        })
        .filter_map(|event| {
            Some((
                event.ticket.clone()?,
                format!("th-{}", event.id.strip_prefix("ev-")?),
                event.payload["body"]
                    .as_str()
                    .unwrap_or_default()
                    .lines()
                    .next()
                    .unwrap_or_default()
                    .to_owned(),
            ))
        })
        .collect()
}

/// The artifact `id` on the Artifacts view: its head and its tab.
#[component]
pub(crate) fn ArtifactDetail(id: String) -> Element {
    let mut session = use_context::<Session>();
    let project = use_context::<Project>();
    let catalog = project.catalog.read().clone();
    let Some(Ok(catalog)) = catalog else {
        return rsx! { Loading {} };
    };
    let Some(entry) = catalog
        .artifacts
        .iter()
        .find(|entry| entry.id.as_deref() == Some(id.as_str()))
        .cloned()
    else {
        return rsx! {
            PageHeader { title: id.clone() }
            ProblemAlert { title: "No such artifact", message: format!("`{id}` is not in the checked directory any more.") }
        };
    };
    let root = session.hone.read().root().to_path_buf();
    let path = entry.path.clone();
    let code = use_signal(|| None::<CodeTarget>);
    rsx! {
        if let Some(target) = code() {
            CodePanel { key: "{target.path}#{target.symbol:?}", target, panel: code }
        }
        PageHeader {
            context: entry.kind.clone(),
            title: entry.title.clone().unwrap_or_else(|| id.clone()),
            actions: rsx! {
                Button { variant: ButtonVariant::Outline, onclick: move |_| session.artifact.set(None), "All artifacts" }
                Button { variant: ButtonVariant::Outline, onclick: move |_| open_file(&root, &path), "Open the file" }
            },
        }
        ArtifactTab { id, code }
    }
}

/// What the catalog and the log know about the artifact `id`: its anchors and code references,
/// its links both ways, the threads on it, and its review in its ticket.
#[component]
pub(crate) fn ArtifactFacts(id: String, code: Signal<Option<CodeTarget>>) -> Element {
    let mut session = use_context::<Session>();
    let project = use_context::<Project>();
    let catalog = project.catalog.read().clone();
    let Some(Ok(catalog)) = catalog else {
        return rsx! { Loading {} };
    };
    let Some(entry) = catalog
        .artifacts
        .iter()
        .find(|entry| entry.id.as_deref() == Some(id.as_str()))
        .cloned()
    else {
        return rsx! {};
    };
    let (stored, computed) = links_of(&entry, &catalog.artifacts);
    let threads = match &*project.log.read() {
        Some(Ok(log)) => anchored_threads(&log.events, &id),
        _ => Vec::new(),
    };
    let source_id = id.clone();
    let source = use_resource(move || {
        let id = source_id.clone();
        async move {
            let _ = (session.revision)();
            (session.hone)().artifact(&id).await
        }
    });
    rsx! {
        div { class: "grid grid-cols-[repeat(auto-fit,minmax(340px,1fr))] gap-8 items-start pt-4",
            div { class: "flex flex-col gap-8 min-w-0",
                match &*source.read() {
                    Some(Ok(source)) => rsx! { SourcePanel { source: source.clone(), code } },
                    Some(Err(failure)) => rsx! { FailureAlert { failure: failure.clone() } },
                    None => rsx! { Loading {} },
                }
                if let Some(ticket) = entry.ticket.clone() {
                    ReviewPanel { key: "{ticket}-{entry.path}", ticket, artifact: entry.id.clone().unwrap_or_default() }
                } else {
                    section {
                        SectionLabel { label: "Review" }
                        Empty { text: "This artifact names no ticket. Approvals belong to a ticket, so there is nothing to review here." }
                    }
                }
            }
            div { class: "flex flex-col gap-8 min-w-0",
                if (session.ticket)().is_some() {
                    div {
                        Button {
                            variant: ButtonVariant::Outline,
                            onclick: move |_| {
                                session.graph_focus.set(Some(id.clone()));
                                session.open_tab(Tab::Graph);
                            },
                            "Show in the link graph"
                        }
                    }
                }
                LinksPanel { stored, computed }
                section {
                    SectionLabel { label: "Threads on it", aside: format!("{}", threads.len()) }
                    if threads.is_empty() {
                        Empty { text: "No thread is anchored to this artifact. Open one under Threads with the artifact as anchor." }
                    } else {
                        div { class: "border border-line",
                            for (ticket , thread , line) in threads {
                                button {
                                    class: "w-full text-left px-4 py-3 border-b border-line hover:bg-surface-2 flex flex-col gap-1",
                                    onclick: {
                                        let ticket = ticket.clone();
                                        let thread = thread.clone();
                                        move |_| session.open_thread(ticket.clone(), thread.clone())
                                    },
                                    span { class: "font-semibold", "{line}" }
                                    span { class: "font-mono text-[12.5px] text-ink-muted", "{ticket} · {thread}" }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

/// The anchors (`kind`, `ref`) of an artifact's data, then its `links.code` and `interface.code`
/// as `code`.
fn references_of(data: &serde_json::Map<String, serde_json::Value>) -> Vec<(String, String)> {
    let anchors = data
        .get("anchors")
        .and_then(|anchors| anchors.as_array())
        .into_iter()
        .flatten()
        .filter_map(|anchor| {
            Some((
                anchor["kind"].as_str()?.to_owned(),
                anchor["ref"].as_str()?.to_owned(),
            ))
        });
    // Indexing a `Map` panics on a missing key; indexing a `Value` gives `Null`.
    let code = data
        .get("links")
        .and_then(|links| links["code"].as_array())
        .into_iter()
        .flatten()
        .chain(data.get("interface").map(|interface| &interface["code"]))
        .filter_map(|reference| reference.as_str())
        .map(|reference| ("code".to_owned(), reference.to_owned()));
    anchors.chain(code).collect()
}

/// The anchors and code references of an artifact, each opening its code.
#[component]
fn SourcePanel(source: ArtifactSource, code: Signal<Option<CodeTarget>>) -> Element {
    let session = use_context::<Session>();
    let project = use_context::<Project>();
    let references = references_of(&source.data);
    rsx! {
        section {
            SectionLabel { label: "Anchors and code", aside: "click shows the code, Ctrl+click opens the IDE" }
            if references.is_empty() {
                Empty { text: "No anchors and no code references. Add them on the Edit tab." }
            } else {
                div { class: "border border-line",
                    for (kind , reference) in references {
                        div { class: "px-4 py-2 border-b border-line flex items-baseline gap-3",
                            span { class: "font-mono text-[12.5px] text-ink-muted w-16 shrink-0", "{kind}" }
                            match CodeTarget::of(&kind, &reference) {
                                Some(target) => rsx! {
                                    button {
                                        class: "font-mono text-[13px] underline text-left break-all",
                                        onclick: move |event| {
                                            let modifiers = event.modifiers();
                                            open_code(session, project, target.clone(), modifiers.ctrl() || modifiers.meta(), code);
                                        },
                                        "{reference}"
                                    }
                                },
                                None => rsx! { span { class: "font-mono text-[13px] break-all", "{reference}" } },
                            }
                        }
                    }
                }
            }
        }
    }
}

#[component]
fn LinksPanel(stored: Vec<Link>, computed: Vec<Link>) -> Element {
    rsx! {
        Card { class: "py-0 gap-0",
            CardHeader { class: "bg-surface border-b border-line py-3",
                CardTitle { class: "font-mono text-[11px] tracking-[0.08em] uppercase font-medium", "Links" }
            }
            CardContent { class: "p-0",
                if stored.is_empty() && computed.is_empty() {
                    p { class: "m-0 px-5 py-4 text-[13px] text-ink-muted", "No links in either direction." }
                }
                for link in stored {
                    LinkRow { link, computed: false }
                }
                for link in computed {
                    LinkRow { link, computed: true }
                }
            }
        }
    }
}

#[component]
fn LinkRow(link: Link, computed: bool) -> Element {
    let session = use_context::<Session>();
    let other = link.other.clone();
    rsx! {
        div { class: "px-5 py-3 border-b border-line flex items-baseline gap-3",
            span { class: "font-mono text-[12.5px] text-ink-muted w-28 shrink-0",
                if computed { "← {link.kind}" } else { "{link.kind} →" }
            }
            button {
                class: "font-mono text-[13px] underline text-left break-all",
                onclick: move |_| {
                    if other.starts_with("tk-") {
                        session.open_ticket(other.clone());
                    } else {
                        session.open_artifact(other.clone());
                    }
                },
                "{link.other}"
            }
            if computed {
                span { class: "ml-auto font-mono text-[12px] text-ink-muted", "computed" }
            }
        }
    }
}

/// Who decided on the artifact in its ticket, and the actions of a reviewer.
#[component]
fn ReviewPanel(ticket: String, artifact: String) -> Element {
    let session = use_context::<Session>();
    // The artifact's ticket becomes the open ticket, so that the writer's roles are the roles a
    // review in that ticket is checked against.
    let mut open_ticket = session.ticket;
    let wanted = ticket.clone();
    use_effect(move || {
        if open_ticket.peek().as_deref() != Some(wanted.as_str()) {
            open_ticket.set(Some(wanted.clone()));
        }
    });
    let shown = (ticket.clone(), artifact.clone());
    let state = use_resource(move || {
        let (ticket, artifact) = shown.clone();
        async move {
            let _ = (session.revision)();
            (session.hone)().review(&ticket, &artifact).await
        }
    });
    rsx! {
        section { class: "flex flex-col gap-4",
            SectionLabel { label: "Review", aside: format!("in {ticket}") }
            match &*state.read() {
                None => rsx! { Loading {} },
                Some(Err(failure)) => rsx! { FailureAlert { failure: failure.clone() } },
                Some(Ok(review)) => rsx! {
                    ReviewFacts { review: review.clone() }
                    ReviewActions { ticket: ticket.clone(), artifact: artifact.clone(), review: review.clone() }
                },
            }
        }
    }
}

#[component]
fn ReviewFacts(review: ReviewState) -> Element {
    let approved: Vec<_> = review
        .decisions
        .iter()
        .filter(|decision| decision.current && decision.decision == "approved")
        .cloned()
        .collect();
    let others: Vec<_> = review
        .decisions
        .iter()
        .filter(|decision| !(decision.current && decision.decision == "approved"))
        .cloned()
        .collect();
    let condensed: Vec<String> = review
        .review
        .as_ref()
        .and_then(|block| block["approved_by"].as_array().cloned())
        .unwrap_or_default()
        .iter()
        .map(|entry| {
            format!(
                "{} · {}{}",
                entry["name"].as_str().unwrap_or_default(),
                entry["role"].as_str().unwrap_or_default(),
                if entry["hash"].as_str() == Some(review.hash.as_str()) {
                    ""
                } else {
                    " (older content)"
                }
            )
        })
        .collect();
    rsx! {
        div { class: "border border-line flex flex-col",
            div { class: "px-5 py-3 border-b border-line bg-surface flex flex-wrap gap-x-3 gap-y-1 items-baseline",
                span { class: "font-mono text-[11px] tracking-[0.08em] uppercase", "Current content" }
                span { class: "font-mono text-[12.5px] text-ink-muted break-all", "{review.hash}" }
            }
            if review.decisions.is_empty() {
                p { class: "m-0 px-5 py-4 text-[13px] text-ink-muted", "Nobody has approved or requested changes yet." }
            }
            for decision in approved {
                div { class: "px-5 py-3 border-b border-line flex flex-wrap items-baseline gap-3",
                    StatusMark { mark: Mark::Done, word: "approved" }
                    span { class: "font-semibold", "{decision.name}" }
                    span { class: "font-mono text-[12.5px] text-ink-muted", "{decision.role} · {decision.date}" }
                    if let Some(proof) = decision.proof.clone() {
                        span { class: "text-[13px] text-ink-muted", "proof: {proof}" }
                    }
                }
            }
            for decision in others {
                div { class: "px-5 py-3 border-b border-line flex flex-wrap items-baseline gap-3",
                    if decision.decision == "changes-requested" && decision.current {
                        StatusMark { mark: Mark::Fail, word: "changes requested" }
                    } else if decision.decision == "changes-requested" {
                        StatusMark { mark: Mark::Skip, word: "changes requested · older content" }
                    } else {
                        StatusMark { mark: Mark::Open, word: "approved an older content" }
                    }
                    span { class: "font-semibold", "{decision.name}" }
                    span { class: "font-mono text-[12.5px] text-ink-muted", "{decision.role} · {decision.date}" }
                    if let Some(proof) = decision.proof.clone() {
                        span { class: "text-[13px] text-ink-muted", "proof: {proof}" }
                    }
                }
            }
            div { class: "px-5 py-3 flex flex-col gap-1",
                span { class: "font-mono text-[11px] tracking-[0.08em] uppercase", "In the file" }
                if condensed.is_empty() {
                    span { class: "text-[13px] text-ink-muted", "No condensed approvals in the review block." }
                } else {
                    span { class: "text-[13px]", "approved_by: {condensed.join(\", \")}" }
                }
                for confirmation in review.confirmations.iter() {
                    span { class: "text-[13px] text-ink-muted",
                        {format!("confirmed by {} · {} on {}{}", confirmation.name, confirmation.role, confirmation.date, if confirmation.current { "" } else { " (older content)" })}
                    }
                }
            }
        }
    }
}

#[component]
fn ReviewActions(ticket: String, artifact: String, review: ReviewState) -> Element {
    let session = use_context::<Session>();
    let mut proof = use_signal(String::new);
    let mut failure = use_signal(|| None::<Failure>);
    let mut done = use_signal(|| None::<String>);
    let mut busy = use_signal(|| false);
    let write_ticket = ticket.clone();
    let write_artifact = artifact.clone();
    let write = move |changes_requested: bool| {
        let write = Write::Approve {
            ticket: write_ticket.clone(),
            artifact: write_artifact.clone(),
            changes_requested,
            proof: Some(proof().trim().to_owned()).filter(|proof| !proof.is_empty()),
        };
        busy.set(true);
        spawn(async move {
            let hone = (session.hone)();
            let writer = (session.writer)();
            match hone.write(&writer, &write).await {
                Ok(_) => {
                    failure.set(None);
                    proof.set(String::new());
                    done.set(Some(if changes_requested {
                        "Changes requested.".into()
                    } else {
                        "Approved.".into()
                    }));
                    session.changed();
                }
                Err(error) => failure.set(Some(error)),
            }
            busy.set(false);
        });
    };
    let mut approve = write.clone();
    let mut request = write;
    let confirm_ticket = ticket.clone();
    let confirm_artifact = artifact.clone();
    let mut confirm = move || {
        let write = Write::ConfirmReview {
            ticket: confirm_ticket.clone(),
            artifact: confirm_artifact.clone(),
        };
        busy.set(true);
        spawn(async move {
            match (session.hone)().write(&(session.writer)(), &write).await {
                Ok(_) => {
                    failure.set(None);
                    done.set(Some("Review block confirmed.".into()));
                    session.changed();
                }
                Err(error) => failure.set(Some(error)),
            }
            busy.set(false);
        });
    };
    let condense_ticket = ticket.clone();
    let condense_artifact = artifact.clone();
    let mut condense = move || {
        let (ticket, artifact) = (condense_ticket.clone(), condense_artifact.clone());
        busy.set(true);
        spawn(async move {
            match (session.hone)().condense(&ticket, &artifact).await {
                Ok(path) => {
                    failure.set(None);
                    done.set(Some(format!("Condensed into {path}.")));
                    session.changed();
                }
                Err(error) => failure.set(Some(error)),
            }
            busy.set(false);
        });
    };
    let any_current = review
        .decisions
        .iter()
        .any(|decision| decision.current && decision.decision == "approved");
    let writer = (session.writer)();
    let writing_as = if writer.author.is_empty() {
        "the context's author".to_owned()
    } else {
        format!("{} · {}", writer.author, writer.role)
    };
    rsx! {
        div { class: "flex flex-col gap-3",
            div { class: "flex flex-wrap items-end gap-2",
                Field { label: "Proof (optional, stored with the decision)",
                    Input { class: "w-72", value: proof(), placeholder: "for example: read in the meeting of 18 Nov", oninput: move |value| proof.set(value) }
                }
                Button { variant: ButtonVariant::Outline, disabled: busy(), onclick: move |_| request(true), "Request changes" }
                Button { disabled: busy(), onclick: move |_| approve(false), "Approve" }
            }
            p { class: "m-0 text-[13px] text-ink-muted",
                "Writes approval.given for the current content as {writing_as}. The role must be allowed to approve."
            }
            div { class: "flex flex-wrap items-center gap-2 pt-1",
                Button {
                    variant: ButtonVariant::Outline,
                    disabled: busy() || !any_current,
                    title: if any_current { "Write the approvals of the current content into the file's review block" } else { "Nobody has approved the current content yet" },
                    onclick: move |_| condense(),
                    "Condense into the file"
                }
                Button {
                    variant: ButtonVariant::Outline,
                    disabled: busy() || review.review.is_none(),
                    title: "Confirm the review block for the current content (review.confirmed)",
                    onclick: move |_| confirm(),
                    "Confirm the review block"
                }
                span { class: "text-[13px] text-ink-muted", "Condensing changes the artifact's file, not the log." }
            }
            if let Some(failure) = failure() {
                FailureAlert { failure }
            } else if let Some(text) = done() {
                p { class: "m-0 text-[13px] text-ok", role: "status", "■ {text}" }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;

    fn entry(
        id: &str,
        kind: &str,
        ticket: Option<&str>,
        links: &[(&str, &[&str])],
    ) -> CatalogEntry {
        CatalogEntry {
            id: Some(id.into()),
            kind: kind.into(),
            path: format!("design/{id}.md"),
            ticket: ticket.map(str::to_owned),
            title: None,
            status: None,
            effective: None,
            profile: None,
            external: Vec::new(),
            links: links
                .iter()
                .map(|(kind, ids)| {
                    (
                        (*kind).to_owned(),
                        ids.iter().map(|id| (*id).to_owned()).collect(),
                    )
                })
                .collect::<BTreeMap<_, _>>(),
        }
    }

    fn link(kind: &str, other: &str) -> Link {
        Link {
            kind: kind.into(),
            other: other.into(),
        }
    }

    #[test]
    fn anchors_and_code_references_are_listed_with_or_without_links() {
        let data = serde_json::json!({
            "anchors": [{ "kind": "symbol", "ref": "src/fee.rs#Fee" }, { "kind": "path" }],
            "interface": { "code": "src/fee.rs#Fee::total" }
        });
        let pair = |kind: &str, reference: &str| (kind.to_owned(), reference.to_owned());
        assert_eq!(
            references_of(data.as_object().expect("a map")),
            [
                pair("symbol", "src/fee.rs#Fee"),
                pair("code", "src/fee.rs#Fee::total")
            ]
        );
        let data = serde_json::json!({ "links": { "code": ["a.rs#b"] }, "interface": {} });
        assert_eq!(
            references_of(data.as_object().expect("a map")),
            [pair("code", "a.rs#b")]
        );
    }

    #[test]
    fn links_are_stored_forward_and_computed_backward() {
        let all = [
            entry(
                "dec-1",
                "decision",
                Some("tk-1"),
                &[("affects", &["mod-1"])],
            ),
            entry(
                "dec-2",
                "decision",
                Some("tk-1"),
                &[("supersedes", &["dec-1"])],
            ),
            entry("mod-1", "module", None, &[]),
            entry("tk-1", "ticket", None, &[]),
        ];
        let (stored, computed) = links_of(&all[0], &all);
        assert_eq!(stored, [link("affects", "mod-1"), link("ticket", "tk-1")]);
        assert_eq!(computed, [link("supersedes", "dec-2")]);
        let (stored, computed) = links_of(&all[3], &all);
        assert!(stored.is_empty());
        assert_eq!(computed, [link("ticket", "dec-1"), link("ticket", "dec-2")]);
    }
}
