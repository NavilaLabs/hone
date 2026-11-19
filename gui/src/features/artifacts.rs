use dioxus::prelude::*;

use super::common::{Empty, FailureAlert, Loading, Mark, PageHeader, StatusMark};
use super::{Project, Session};
use crate::hone::CatalogEntry;
use crate::ui::{
    Card, CardContent, CardHeader, CardTitle, Choice, Combobox, Input, Table, TableBody, TableCell,
    TableHead, TableHeader, TableRow, TableWrapper,
};

/// The kinds among `artifacts` with their counts, in the order of their names, ticket records
/// left out.
fn kinds(artifacts: &[CatalogEntry]) -> Vec<(String, usize)> {
    let mut kinds: std::collections::BTreeMap<String, usize> = std::collections::BTreeMap::new();
    for entry in artifacts.iter().filter(|entry| entry.kind != "ticket") {
        *kinds.entry(entry.kind.clone()).or_default() += 1;
    }
    kinds.into_iter().collect()
}

/// The artifacts of `kind` (every kind for `None`) with the effective status `status` (every
/// status for an empty one) whose ID, title, path, or ticket contains `text`.
fn matching<'a>(
    artifacts: &'a [CatalogEntry],
    kind: Option<&str>,
    status: &str,
    text: &str,
) -> Vec<&'a CatalogEntry> {
    let text = text.trim().to_lowercase();
    artifacts
        .iter()
        .filter(|entry| entry.kind != "ticket")
        .filter(|entry| kind.is_none_or(|kind| entry.kind == kind))
        .filter(|entry| status.is_empty() || entry.effective.as_deref() == Some(status))
        .filter(|entry| {
            text.is_empty()
                || [
                    entry.id.as_deref(),
                    entry.title.as_deref(),
                    Some(entry.path.as_str()),
                    entry.ticket.as_deref(),
                ]
                .into_iter()
                .flatten()
                .any(|field| field.to_lowercase().contains(&text))
        })
        .collect()
}

/// Every artifact of the checked directory, by kind, with its declared and effective status.
#[component]
pub(crate) fn Artifacts() -> Element {
    let session = use_context::<Session>();
    let project = use_context::<Project>();
    let mut kind = use_signal(|| None::<String>);
    let mut status = use_signal(String::new);
    let mut text = use_signal(String::new);
    rsx! {
        PageHeader {
            context: session.hone.read().root().display().to_string(),
            title: "Artifacts",
        }
        match &*project.catalog.read() {
            None => rsx! { Loading {} },
            Some(Err(failure)) => rsx! { FailureAlert { failure: failure.clone() } },
            Some(Ok(catalog)) => {
                let kinds = kinds(&catalog.artifacts);
                let total: usize = kinds.iter().map(|(_, count)| count).sum();
                let mut statuses: Vec<String> = catalog
                    .artifacts
                    .iter()
                    .filter_map(|entry| entry.effective.clone())
                    .collect();
                statuses.sort();
                statuses.dedup();
                let status_choices: Vec<Choice> = std::iter::once(Choice::plain("").label("every status"))
                    .chain(statuses.into_iter().map(Choice::plain))
                    .collect();
                let shown: Vec<CatalogEntry> = matching(&catalog.artifacts, kind().as_deref(), &status(), &text())
                    .into_iter()
                    .cloned()
                    .collect();
                rsx! {
                    div { class: "flex flex-wrap border-b border-line", role: "tablist",
                        KindTab { label: "All".to_owned(), count: total, active: kind().is_none(), onclick: move |_| kind.set(None) }
                        for (name , count) in kinds {
                            KindTab {
                                label: name.clone(),
                                count,
                                active: kind().as_deref() == Some(name.as_str()),
                                onclick: move |_| kind.set(Some(name.clone())),
                            }
                        }
                    }
                    div { class: "flex flex-wrap items-end gap-4",
                        label { class: "flex flex-col gap-1 text-[13px] text-ink-muted",
                            "Search"
                            Input { class: "w-72", value: text(), placeholder: "ID, title, path, or ticket", oninput: move |value| text.set(value) }
                        }
                        div { class: "flex flex-col gap-1 text-[13px] text-ink-muted w-56",
                            "Effective status"
                            Combobox {
                                value: status(),
                                choices: status_choices,
                                placeholder: "every status",
                                label: "Effective status",
                                onselect: move |value: String| status.set(value),
                            }
                        }
                    }
                    if shown.is_empty() {
                        Empty { text: "No artifact matches. Artifacts appear here once their file exists under a path of the configuration." }
                    } else {
                        TableWrapper {
                            Table {
                                TableHeader { class: "bg-surface",
                                    TableRow {
                                        TableHead { "ID" }
                                        TableHead { "Kind" }
                                        TableHead { "Title" }
                                        TableHead { "Ticket" }
                                        TableHead { "Declared" }
                                        TableHead { "Effective" }
                                    }
                                }
                                TableBody {
                                    for entry in shown.iter().cloned() {
                                        ArtifactRow { key: "{entry.path}", entry }
                                    }
                                }
                            }
                        }
                    }
                    p { class: "m-0 text-[13px] text-ink-muted",
                        "Showing {shown.len()} of {total}. "
                        "superseded is computed from the links of the checked directory, never written."
                    }
                    if !catalog.unreadable.is_empty() {
                        Card { class: "py-0 gap-0 border-bad",
                            CardHeader { class: "bg-bad text-bg py-2",
                                CardTitle { class: "font-mono text-[11px] tracking-[0.08em] uppercase font-medium", "✕ unreadable files" }
                            }
                            CardContent { class: "p-0",
                                for file in catalog.unreadable.clone() {
                                    div { class: "px-5 py-3 border-b border-line flex flex-col gap-1",
                                        span { class: "font-mono text-[13px] break-all", "{file.path}" }
                                        span { class: "text-[13px]", "{file.message}" }
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
fn KindTab(
    label: String,
    count: usize,
    active: bool,
    onclick: EventHandler<MouseEvent>,
) -> Element {
    rsx! {
        button {
            role: "tab",
            "aria-selected": if active { "true" } else { "false" },
            class: if active { "h-11 px-4 -mb-px border-b-[3px] border-signal font-semibold flex items-center gap-2" } else { "h-11 px-4 -mb-px border-b-[3px] border-transparent text-ink-muted hover:text-ink flex items-center gap-2" },
            onclick: move |event| onclick.call(event),
            "{label}"
            span { class: "font-mono text-[12px] text-ink-muted", "{count}" }
        }
    }
}

#[component]
fn ArtifactRow(entry: CatalogEntry) -> Element {
    let session = use_context::<Session>();
    let ticket = entry.ticket.clone();
    rsx! {
        TableRow {
            TableCell { class: "font-mono",
                match entry.id.clone() {
                    Some(id) => rsx! {
                        button {
                            class: "font-mono underline text-left",
                            title: "{entry.path}",
                            onclick: move |_| session.open_artifact(id.clone()),
                            "{id}"
                        }
                    },
                    None => rsx! { span { title: "{entry.path}", "—" } },
                }
            }
            TableCell { class: "font-mono text-[12.5px] text-ink-muted", "{entry.kind}" }
            TableCell { {entry.title.clone().unwrap_or_default()} }
            TableCell {
                match ticket {
                    Some(ticket) => rsx! {
                        button {
                            class: "font-mono text-[12.5px] underline",
                            onclick: move |_| session.open_ticket(ticket.clone()),
                            "{ticket}"
                        }
                    },
                    None => rsx! { span { class: "text-ink-muted", "—" } },
                }
            }
            TableCell { class: "font-mono text-[12.5px] text-ink-muted", {entry.status.clone().unwrap_or_else(|| "—".into())} }
            TableCell { EffectiveStatus { entry: entry.clone() } }
        }
    }
}

/// The effective status of an artifact; `superseded` is struck through, `living` artifacts
/// without a status show `living`.
#[component]
pub(crate) fn EffectiveStatus(entry: CatalogEntry) -> Element {
    match entry.effective.as_deref() {
        Some("superseded") => rsx! {
            StatusMark { mark: Mark::Skip, word: "superseded" }
        },
        Some(status) => rsx! {
            span { class: "font-mono text-[12.5px]", "□ {status}" }
        },
        None => rsx! {
            span { class: "font-mono text-[12.5px] text-ink-muted", "living" }
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(id: &str, kind: &str, effective: Option<&str>, title: Option<&str>) -> CatalogEntry {
        CatalogEntry {
            id: Some(id.into()),
            kind: kind.into(),
            path: format!("design/{id}.md"),
            ticket: None,
            title: title.map(str::to_owned),
            status: effective.map(str::to_owned),
            effective: effective.map(str::to_owned),
            profile: None,
            external: Vec::new(),
            links: std::collections::BTreeMap::new(),
        }
    }

    #[test]
    fn kinds_are_counted_without_ticket_records() {
        let artifacts = [
            entry("dec-1", "decision", Some("accepted"), None),
            entry("dec-2", "decision", Some("superseded"), None),
            entry("mod-1", "module", None, None),
            entry("tk-1", "ticket", None, None),
        ];
        assert_eq!(
            kinds(&artifacts),
            [("decision".to_owned(), 2), ("module".to_owned(), 1)]
        );
    }

    #[test]
    fn filters_combine_kind_status_and_text() {
        let artifacts = [
            entry(
                "dec-1",
                "decision",
                Some("accepted"),
                Some("Payments run async"),
            ),
            entry("dec-2", "decision", Some("superseded"), None),
            entry("spec-1", "spec", Some("accepted"), None),
            entry("tk-1", "ticket", None, None),
        ];
        let ids = |kind: Option<&str>, status: &str, text: &str| -> Vec<String> {
            matching(&artifacts, kind, status, text)
                .into_iter()
                .filter_map(|entry| entry.id.clone())
                .collect()
        };
        assert_eq!(ids(None, "", ""), ["dec-1", "dec-2", "spec-1"]);
        assert_eq!(ids(Some("decision"), "accepted", ""), ["dec-1"]);
        assert_eq!(ids(None, "accepted", "ASYNC"), ["dec-1"]);
        assert_eq!(ids(None, "", "spec-"), ["spec-1"]);
    }
}
