use dioxus::prelude::*;

use super::attributes::{AttributeEditor, attribute_changes, parse_rows, rows_of, rule_keys};
use super::common::{
    Empty, FailureAlert, Loading, Mark, PageHeader, SectionLabel, StatusMark, Tag, short_hash,
};
use super::progress::{self, Stage, StepTrack};
use super::{Project, Screen, Session};
use crate::hone::{CatalogEntry, Failure, GateScope, StoreLog, TicketEvents, TicketOverview};
use crate::ui::{
    Badge, Button, ButtonVariant, Combobox, Table, TableBody, TableCell, TableHead, TableHeader,
    TableRow, TableWrapper,
};

/// A row of the ticket list: a ticket record, or a ticket the store knows without one.
#[derive(Debug, Clone, PartialEq, Eq)]
struct TicketRow {
    ticket: String,
    record: Option<CatalogEntry>,
    events: usize,
    last: Option<String>,
}

/// The ticket records in the order of their paths, then the tickets of the store without a
/// record in the order their first event appears in the log, each with its event count and the
/// type of its last event.
fn ticket_rows(records: &[CatalogEntry], log: Option<&StoreLog>) -> Vec<TicketRow> {
    let mut rows: Vec<TicketRow> = records
        .iter()
        .filter_map(|record| {
            Some(TicketRow {
                ticket: record.id.clone()?,
                record: Some(record.clone()),
                events: 0,
                last: None,
            })
        })
        .collect();
    for event in log.map(|log| log.events.as_slice()).unwrap_or_default() {
        let Some(ticket) = &event.ticket else {
            continue;
        };
        match rows.iter_mut().find(|row| &row.ticket == ticket) {
            Some(row) => {
                row.events += 1;
                row.last = Some(event.kind.clone());
            }
            None => rows.push(TicketRow {
                ticket: ticket.clone(),
                record: None,
                events: 1,
                last: Some(event.kind.clone()),
            }),
        }
    }
    rows
}

/// The tickets list, or the open ticket.
#[component]
pub(crate) fn Tickets() -> Element {
    let session = use_context::<Session>();
    match (session.ticket)() {
        Some(ticket) => rsx! { TicketDetail { key: "{ticket}", ticket } },
        None => rsx! { TicketList {} },
    }
}

#[component]
fn TicketList() -> Element {
    let mut session = use_context::<Session>();
    let project = use_context::<Project>();
    let tracker = project.tracker();
    rsx! {
        PageHeader {
            context: project.project_id().unwrap_or_default(),
            title: "Tickets",
            actions: rsx! {
                Button {
                    onclick: move |_| session.new_ticket.set(Some(super::new_ticket::Seed::default())),
                    "New ticket"
                }
                Button {
                    variant: ButtonVariant::Outline,
                    onclick: move |_| session.screen.set(Screen::Tracker),
                    if let Some(tracker) = &tracker { "Browse {tracker.describe()}" } else { "Set up a tracker" }
                }
            },
        }
        div { class: "flex flex-col gap-1 max-w-xl",
            span { class: "text-[13px] text-ink-muted", "Open a ticket" }
            Combobox {
                value: "",
                choices: project.ticket_choices(),
                placeholder: "Search by ID, external key, or profile",
                label: "Open a ticket",
                empty: "No ticket matches.",
                onselect: move |ticket: String| session.open_ticket(ticket),
            }
        }
        match (&*project.catalog.read(), &*project.log.read()) {
            (None, _) | (_, None) => rsx! { Loading {} },
            (Some(Err(failure)), _) => rsx! { FailureAlert { failure: failure.clone() } },
            (Some(Ok(catalog)), Some(log)) => {
                let records: Vec<CatalogEntry> = catalog.tickets().cloned().collect();
                let log = log.as_ref().ok();
                let rows = ticket_rows(&records, log);
                if rows.is_empty() {
                    rsx! { Empty { text: "No ticket yet. A ticket appears here once its record exists under the ticket path of the configuration." } }
                } else {
                    rsx! {
                        TableWrapper {
                            Table {
                                TableHeader { class: "bg-surface",
                                    TableRow {
                                        TableHead { "Ticket" }
                                        TableHead { "External" }
                                        TableHead { "Profile" }
                                        TableHead { class: "w-[260px]", "Progress" }
                                        TableHead { "Last event" }
                                    }
                                }
                                TableBody {
                                    for row in rows {
                                        TicketListRow { key: "{row.ticket}", row }
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
fn TicketListRow(row: TicketRow) -> Element {
    let session = use_context::<Session>();
    let ticket = row.ticket.clone();
    let shown = ticket.clone();
    let has_record = row.record.is_some();
    let progress = use_resource(move || {
        let ticket = shown.clone();
        async move {
            let _ = (session.revision)();
            if !has_record {
                return None;
            }
            let hone = (session.hone)();
            let overview = hone.ticket(&ticket).await.ok()?;
            let state = hone.events(&ticket).await.ok();
            Some(progress::progress(&overview, state.as_ref(), None))
        }
    });
    let external = row
        .record
        .as_ref()
        .map(|record| {
            record
                .external
                .iter()
                .map(|external| format!("{} {}", external.system, external.key))
                .collect::<Vec<_>>()
                .join(", ")
        })
        .unwrap_or_default();
    rsx! {
        TableRow { class: "cursor-pointer", onclick: move |_| session.open_ticket(ticket.clone()),
            TableCell {
                button { class: "font-mono underline text-left", "{row.ticket}" }
            }
            TableCell { class: "font-mono text-[12.5px] text-ink-muted", "{external}" }
            TableCell { class: "font-mono text-[12.5px]",
                match row.record.as_ref().and_then(|record| record.profile.clone()) {
                    Some(profile) => rsx! { "{profile}" },
                    None => rsx! { span { class: "text-ink-muted", "no record" } },
                }
            }
            TableCell {
                match &*progress.read() {
                    Some(Some(steps)) => rsx! { MiniTrack { steps: steps.clone() } },
                    Some(None) => rsx! { span { class: "text-ink-muted text-[13px]", "—" } },
                    None => rsx! { span { class: "text-ink-muted text-[13px]", "…" } },
                }
            }
            TableCell { class: "font-mono text-[12.5px] text-ink-muted",
                {row.last.clone().map_or_else(|| "no events".to_owned(), |last| format!("{last} · {} event{}", row.events, if row.events == 1 { "" } else { "s" }))}
            }
        }
    }
}

/// The steps as small segments, for a table row: filled when done, dashed when skipped, accent
/// for the current step, empty when open.
#[component]
fn MiniTrack(steps: Vec<progress::StepProgress>) -> Element {
    let finished = steps
        .iter()
        .filter(|step| matches!(step.stage, Stage::Done | Stage::Skipped))
        .count();
    let names: Vec<String> = steps
        .iter()
        .map(|step| {
            let state = match step.stage {
                Stage::Done => "done",
                Stage::Skipped => "skipped",
                Stage::Ready => "open",
                Stage::Blocked => "waiting",
            };
            format!("{} {state}", step.step)
        })
        .collect();
    rsx! {
        div { class: "flex items-center gap-3", title: "{names.join(\", \")}",
            div { class: "flex gap-[3px] flex-1",
                for step in steps.iter() {
                    span {
                        class: match (step.stage, step.current) {
                            (Stage::Done, _) => "h-2 flex-1 bg-ink",
                            (Stage::Skipped, _) => "h-2 flex-1 border border-dashed border-ink-muted",
                            (_, true) => "h-2 flex-1 bg-signal",
                            _ => "h-2 flex-1 bg-surface-2",
                        },
                    }
                }
            }
            span { class: "font-mono text-[12.5px] whitespace-nowrap", "{finished}/{steps.len()}" }
        }
    }
}

#[component]
fn TicketDetail(ticket: String) -> Element {
    let mut session = use_context::<Session>();
    let project = use_context::<Project>();
    let shown = ticket.clone();
    let check = use_resource(move || {
        let ticket = shown.clone();
        async move {
            let _ = (session.revision)();
            let hone = (session.hone)();
            hone.check(Some(&ticket), GateScope::Pure).await
        }
    });
    let check_ticket = ticket.clone();
    let state: Option<TicketEvents> = match &*project.state.read() {
        Some(Some(Ok(state))) => Some(state.clone()),
        _ => None,
    };
    let state_failure = match &*project.state.read() {
        Some(Some(Err(failure))) if failure.status != "unavailable" => Some(failure.clone()),
        _ => None,
    };
    let tracker = project.tracker();
    // The title lives in the tracker: the ticket record has none.
    let linked = use_resource(move || async move {
        let tracker = project.tracker()?;
        let key = match &*project.overview.read() {
            Some(Some(Ok(overview))) => overview
                .external
                .iter()
                .find(|external| external.system.eq_ignore_ascii_case(tracker.system()))?
                .key
                .clone(),
            _ => return None,
        };
        tracker.issue(&key).await.ok().flatten()
    });
    let linked_issue = linked.read().clone().flatten();
    rsx! {
        match &*project.overview.read() {
            Some(Some(Ok(overview))) => {
                let external: Vec<String> = overview
                    .external
                    .iter()
                    .map(|external| format!("{} {}", external.system, external.key))
                    .collect();
                let context = std::iter::once(ticket.clone()).chain(external).collect::<Vec<_>>().join(" · ");
                rsx! {
                    PageHeader {
                        context,
                        title: linked_issue.as_ref().map_or_else(|| ticket.clone(), |issue| issue.title.clone()),
                        actions: rsx! {
                            Button { variant: ButtonVariant::Outline, onclick: move |_| session.ticket.set(None), "All tickets" }
                            Button {
                                variant: ButtonVariant::Outline,
                                title: "Every gate, exec gates included, on the Checks screen",
                                onclick: move |_| {
                                    session.ticket.set(Some(check_ticket.clone()));
                                    session.screen.set(Screen::Check);
                                },
                                "Run full hone check"
                            }
                        },
                    }
                    div { class: "flex flex-wrap gap-2 -mt-5",
                        for (name , value) in overview.attributes.iter() {
                            Tag {
                                bad: name == "risk" && value.as_str() == Some("high"),
                                "{name}: {value.as_str().map(str::to_owned).unwrap_or_else(|| value.to_string())}"
                            }
                        }
                        Tag { inverted: true, "profile: {overview.profile}" }
                        Tag { muted: true, "config {short_hash(&overview.config)}" }
                        for external in overview.external.iter() {
                            if let Some(url) = external.url.clone().or_else(|| tracker.as_ref().filter(|tracker| tracker.system() == external.system).map(|tracker| tracker.issue_url(&external.key))) {
                                ExternalLink { label: format!("{} {}", external.system, external.key), url }
                            }
                        }
                    }
                    if overview.selected.as_ref() != Some(&overview.profile) {
                        div { class: "border-2 border-warn px-5 py-4 flex flex-col gap-1 -mt-3", role: "status",
                            span { class: "font-semibold",
                                span { class: "text-warn", "aria-hidden": "true", "⚠ " }
                                "profile-not-selected: this ticket uses {overview.profile}, but select gives "
                                span { class: "font-mono", {overview.selected.clone().unwrap_or_else(|| "no profile".to_owned())} }
                                " for its attributes."
                            }
                            span { class: "text-[13px] text-ink-muted", "The profile was chosen by hand, or the attributes changed after the start. steps-complete reports it as a warning; a new ticket gets the selected profile." }
                        }
                    }
                    TicketSettings { ticket: ticket.clone(), overview: overview.clone() }
                    StepTrack {
                        ticket: ticket.clone(),
                        overview: overview.clone(),
                        state: state.clone(),
                        run: check.read().clone(),
                        checking: check.read().is_none(),
                    }
                }
            }
            Some(Some(Err(failure))) => rsx! {
                PageHeader {
                    context: "ticket",
                    title: ticket.clone(),
                    actions: rsx! {
                        Button { variant: ButtonVariant::Outline, onclick: move |_| session.ticket.set(None), "All tickets" }
                    },
                }
                FailureAlert { failure: failure.clone() }
            },
            _ => rsx! {
                PageHeader { context: "ticket", title: ticket.clone() }
                Loading {}
            },
        }
        if let Some(failure) = state_failure {
            FailureAlert { failure }
        }
        div { class: "grid grid-cols-[repeat(auto-fit,minmax(380px,1fr))] gap-8 items-start",
            TicketArtifacts { ticket: ticket.clone() }
            ThreadTable { ticket: ticket.clone(), state: state.clone() }
        }
        if let Some(state) = state.filter(|state| !state.invalid.is_empty()) {
            super::events::InvalidPanel { invalid: state.invalid.clone() }
        }
    }
}

/// The ticket's attributes to edit, and the switch to the current rules.
#[component]
fn TicketSettings(ticket: String, overview: TicketOverview) -> Element {
    let session = use_context::<Session>();
    let project = use_context::<Project>();
    let mut editing = use_signal(|| false);
    let mut rows = use_signal(|| rows_of(&overview.attributes));
    let mut outcome = use_signal(|| None::<Result<String, Failure>>);
    let mut busy = use_signal(|| false);
    let selection = use_resource(move || async move { (session.hone)().select(&[]).await.ok() });
    let keys = match &*selection.read() {
        Some(Some(selection)) => rule_keys(&selection.rules),
        _ => Vec::new(),
    };
    let original = overview.attributes.clone();
    let save_ticket = ticket.clone();
    let save = move |_| {
        let Some(wanted) = parse_rows(&rows()) else {
            return;
        };
        let (set, unset) = attribute_changes(&original, &wanted);
        if set.is_empty() && unset.is_empty() {
            outcome.set(Some(Ok("Nothing changed.".to_owned())));
            editing.set(false);
            return;
        }
        let ticket = save_ticket.clone();
        busy.set(true);
        spawn(async move {
            let result = (session.hone)()
                .set_attributes(&(session.writer)(), &ticket, &set, &unset)
                .await
                .map(|written| {
                    outcome_text(
                        "Attributes saved",
                        &written.profile,
                        written.selected.as_deref(),
                    )
                });
            if result.is_ok() {
                editing.set(false);
                session.changed();
            }
            outcome.set(Some(result));
            busy.set(false);
        });
    };
    let switch_ticket = ticket.clone();
    let switch = move |_| {
        let ticket = switch_ticket.clone();
        busy.set(true);
        spawn(async move {
            let result = project
                .resolving((session.hone)())
                .switch_config(&(session.writer)(), &ticket)
                .await
                .map(|written| {
                    outcome_text(
                        &format!(
                            "Switched to the current rules ({})",
                            short_hash(&written.config)
                        ),
                        &written.profile,
                        written.selected.as_deref(),
                    )
                });
            if result.is_ok() {
                session.changed();
            }
            outcome.set(Some(result));
            busy.set(false);
        });
    };
    let reset_rows = overview.attributes.clone();
    rsx! {
        div { class: "flex flex-col gap-3 -mt-3",
            div { class: "flex flex-wrap gap-2",
                Button {
                    variant: ButtonVariant::Outline,
                    disabled: busy(),
                    onclick: move |_| {
                        rows.set(rows_of(&reset_rows));
                        editing.toggle();
                    },
                    if editing() { "Close the attributes" } else { "Edit attributes" }
                }
                Button {
                    variant: ButtonVariant::Outline,
                    disabled: busy(),
                    title: "Freeze the current resolved configuration for this ticket (ticket.config_changed); the profile stays",
                    onclick: switch,
                    "Switch to current rules"
                }
            }
            if editing() {
                div { class: "border border-line p-5 flex flex-col gap-4",
                    AttributeEditor { rows, keys }
                    div { class: "flex flex-wrap items-center gap-2",
                        span { class: "mr-auto text-[13px] text-ink-muted", "Writes the record and ticket.attributes_changed. The profile stays, even if select now gives another one." }
                        Button { disabled: busy() || parse_rows(&rows()).is_none(), onclick: save, "Save attributes" }
                    }
                }
            }
            match outcome() {
                Some(Ok(text)) => rsx! { p { class: "m-0 text-[13px] text-ok", role: "status", "■ {text}" } },
                Some(Err(failure)) => rsx! {
                    FailureAlert { failure: failure.clone() }
                    if failure.message.contains("run hone resolve") {
                        super::new_ticket::ResolveNow {}
                    }
                },
                None => rsx! {},
            }
        }
    }
}

/// What a ticket command did, with the profile and what `select` gives now.
fn outcome_text(done: &str, profile: &str, selected: Option<&str>) -> String {
    match selected {
        Some(selected) if selected == profile => {
            format!("{done}. The profile {profile} is what select gives.")
        }
        Some(selected) => {
            format!("{done}. The profile stays {profile}; select now gives {selected}.")
        }
        None => format!("{done}. The profile stays {profile}; select gives no profile."),
    }
}

#[component]
fn ExternalLink(label: String, url: String) -> Element {
    rsx! {
        button {
            class: "h-7 px-2 border border-line font-mono text-[12.5px] underline hover:bg-surface-2",
            title: "Open {url} in the browser",
            onclick: move |_| super::common::open_in_browser(&url),
            "{label} ↗"
        }
    }
}

#[component]
fn TicketArtifacts(ticket: String) -> Element {
    let session = use_context::<Session>();
    let project = use_context::<Project>();
    let artifacts: Vec<CatalogEntry> = match &*project.catalog.read() {
        Some(Ok(catalog)) => catalog
            .artifacts
            .iter()
            .filter(|entry| entry.ticket.as_deref() == Some(ticket.as_str()))
            .cloned()
            .collect(),
        _ => Vec::new(),
    };
    rsx! {
        section {
            SectionLabel { label: "Artifacts", aside: format!("{} of this ticket", artifacts.len()) }
            if artifacts.is_empty() {
                Empty { text: "No artifact names this ticket yet. Artifacts appear here once their file is written." }
            } else {
                TableWrapper {
                    Table {
                        TableHeader { class: "bg-surface",
                            TableRow {
                                TableHead { "ID" }
                                TableHead { "Kind" }
                                TableHead { "Status" }
                            }
                        }
                        TableBody {
                            for artifact in artifacts {
                                TableRow {
                                    TableCell { class: "font-mono",
                                        button {
                                            class: "font-mono underline text-left",
                                            title: "{artifact.path}",
                                            onclick: {
                                                let id = artifact.id.clone().unwrap_or_default();
                                                move |_| session.open_artifact(id.clone())
                                            },
                                            {artifact.id.clone().unwrap_or_default()}
                                        }
                                        if let Some(title) = artifact.title.clone() {
                                            span { class: "block font-sans text-[13px] text-ink-muted", "{title}" }
                                        }
                                    }
                                    TableCell { class: "font-mono text-[12.5px] text-ink-muted", "{artifact.kind}" }
                                    TableCell { super::artifacts::EffectiveStatus { entry: artifact.clone() } }
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
fn ThreadTable(ticket: String, state: Option<TicketEvents>) -> Element {
    let mut session = use_context::<Session>();
    let threads = state.map(|state| state.threads).unwrap_or_default();
    let open = threads
        .iter()
        .filter(|thread| thread.status == "open")
        .count();
    let open_ticket = ticket.clone();
    rsx! {
        section {
            SectionLabel { label: "Threads", aside: format!("{open} open of {}", threads.len()) }
            if threads.is_empty() {
                div { class: "flex flex-col gap-3 items-start",
                    Empty { text: "No threads yet. Open one to ask, object, or note something on this ticket." }
                    Button {
                        variant: ButtonVariant::Outline,
                        onclick: move |_| {
                            session.ticket.set(Some(open_ticket.clone()));
                            session.screen.set(Screen::Threads);
                        },
                        "Open a thread"
                    }
                }
            } else {
                TableWrapper {
                    Table {
                        TableHeader { class: "bg-surface",
                            TableRow {
                                TableHead { "Kind" }
                                TableHead { "Status" }
                                TableHead { "Opened by" }
                            }
                        }
                        TableBody {
                            for thread in threads {
                                TableRow {
                                    class: "cursor-pointer",
                                    onclick: {
                                        let ticket = ticket.clone();
                                        let id = thread.thread.clone();
                                        move |_| session.open_thread(ticket.clone(), id.clone())
                                    },
                                    TableCell {
                                        span { class: "font-mono underline", "{thread.kind}" }
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
                                    TableCell { class: "font-mono text-[12.5px]", "{thread.opened_by.name} · {thread.opened_by.role}" }
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

    fn event(id: &str, kind: &str, ticket: &str) -> Event {
        Event {
            id: id.into(),
            device: "dev-3k9a5h2t".into(),
            seq: 1,
            kind: kind.into(),
            ticket: Some(ticket.into()),
            author: None,
            payload: json!({}),
            raw: json!({}),
        }
    }

    fn record(id: &str) -> CatalogEntry {
        CatalogEntry {
            id: Some(id.into()),
            kind: "ticket".into(),
            path: format!("design/tickets/{id}/ticket.yaml"),
            ticket: None,
            title: None,
            status: None,
            effective: None,
            profile: Some("standard".into()),
            external: Vec::new(),
            links: std::collections::BTreeMap::new(),
        }
    }

    #[test]
    fn records_come_first_then_tickets_known_only_to_the_store() {
        let log = StoreLog {
            cursor: BTreeMap::new(),
            events: vec![
                event("ev-1", "thread.opened", "tk-b"),
                event("ev-2", "step.changed", "tk-a"),
                event("ev-3", "thread.message", "tk-b"),
            ],
        };
        let rows = ticket_rows(&[record("tk-a"), record("tk-c")], Some(&log));
        let summary: Vec<(&str, bool, usize, Option<&str>)> = rows
            .iter()
            .map(|row| {
                (
                    row.ticket.as_str(),
                    row.record.is_some(),
                    row.events,
                    row.last.as_deref(),
                )
            })
            .collect();
        assert_eq!(
            summary,
            [
                ("tk-a", true, 1, Some("step.changed")),
                ("tk-c", true, 0, None),
                ("tk-b", false, 2, Some("thread.message")),
            ]
        );
    }

    #[test]
    fn an_outcome_says_whether_select_agrees_with_the_profile() {
        assert_eq!(
            outcome_text("Attributes saved", "standard", Some("standard")),
            "Attributes saved. The profile standard is what select gives."
        );
        assert_eq!(
            outcome_text("Attributes saved", "standard", Some("full")),
            "Attributes saved. The profile stays standard; select now gives full."
        );
    }

    #[test]
    fn a_hash_is_shortened_to_eight_digits() {
        assert_eq!(short_hash("sha256:4c076d82c93f2647"), "sha256:4c076d82…");
    }
}
