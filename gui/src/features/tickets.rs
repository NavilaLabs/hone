//! The Tickets view: every ticket with its steps and what it waits on, a preview, and the
//! tracker's tickets.

use dioxus::prelude::*;

use super::common::{Empty, FailureAlert, Loading, PageHeader, Tag};
use super::{Project, Session};
use crate::hone::{TicketSummary, Waiting};
use crate::ui::{Button, ButtonVariant};

/// The tabs of the ticket list.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Filter {
    /// Started by me, or with an event of mine.
    Mine,
    /// Not complete.
    Open,
    /// Waiting on my role.
    NeedsMe,
    Completed,
    /// The tracker's tickets.
    Tracker,
}

/// Whether `summary` belongs under `filter` for the author `me` (name and role).
pub(crate) fn shows(summary: &TicketSummary, filter: Filter, me: Option<(&str, &str)>) -> bool {
    match filter {
        Filter::Mine => me.is_some_and(|(name, _)| {
            summary
                .started
                .as_ref()
                .is_some_and(|started| started.by.name == name)
                || summary.authors.iter().any(|author| author == name)
        }),
        Filter::Open => summary.complete != Some(true),
        Filter::NeedsMe => me.is_some_and(|(_, role)| {
            !role.is_empty()
                && summary.waiting.iter().any(|reason| match reason {
                    Waiting::Approval { role: wanted, .. } | Waiting::Step { role: wanted, .. } => {
                        wanted.as_deref() == Some(role)
                    }
                    _ => false,
                })
        }),
        Filter::Completed => summary.complete == Some(true),
        Filter::Tracker => false,
    }
}

/// How a "Waiting on" cell reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Tone {
    Bad,
    Me,
    Plain,
    Done,
}

/// The first reason `summary` waits, in words, with `role` as the reader's role.
pub(crate) fn waiting_text(summary: &TicketSummary, role: Option<&str>) -> (String, Tone) {
    if summary.complete == Some(true) {
        return ("■ completed".into(), Tone::Done);
    }
    let plural = |count: usize, word: &str| {
        if count == 1 {
            format!("1 {word}")
        } else {
            format!("{count} {word}s")
        }
    };
    match summary.waiting.first() {
        Some(Waiting::Gates { gates }) => (
            format!("× {} failing", plural(gates.len(), "gate")),
            Tone::Bad,
        ),
        Some(Waiting::Threads { threads }) => (
            format!("▲ {} open", plural(threads.len(), "blocking thread")),
            Tone::Bad,
        ),
        Some(Waiting::Approval { role: wanted, .. })
            if wanted.as_deref() == role && role.is_some() =>
        {
            ("▲ your approval".into(), Tone::Me)
        }
        Some(Waiting::Approval { role: wanted, .. }) => (
            format!("{} approval", wanted.as_deref().unwrap_or("an")),
            Tone::Plain,
        ),
        Some(Waiting::Step { step, role: wanted })
            if wanted.as_deref() == role && role.is_some() =>
        {
            (format!("▲ you · {step}"), Tone::Me)
        }
        Some(Waiting::Step { step, role: wanted }) => (
            format!("{} · {step}", wanted.as_deref().unwrap_or("anyone")),
            Tone::Plain,
        ),
        None if summary.problem.is_some() => ("× record unreadable".into(), Tone::Bad),
        None if !summary.record => ("no record in this checkout".into(), Tone::Plain),
        None => (String::new(), Tone::Plain),
    }
}

/// How long ago the UUIDv7 `id` was written, from `now` in milliseconds since the Unix epoch.
pub(crate) fn ago(id: &str, now: u64) -> Option<String> {
    let hex: String = id
        .split_once('-')?
        .1
        .chars()
        .filter(|c| *c != '-')
        .take(12)
        .collect();
    let then = u64::from_str_radix(&hex, 16).ok()?;
    let minutes = now.saturating_sub(then) / 60_000;
    Some(match minutes {
        0 => "now".into(),
        1..60 => format!("{minutes} min"),
        60..1440 => format!("{} h", minutes / 60),
        1440..2880 => "yesterday".into(),
        _ => format!("{} d", minutes / 1440),
    })
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |since| {
            u64::try_from(since.as_millis()).unwrap_or(u64::MAX)
        })
}

/// The Tickets view.
#[component]
pub(crate) fn Tickets() -> Element {
    let session = use_context::<Session>();
    let project = use_context::<Project>();
    let mut filter = use_signal(|| Filter::Open);
    let selected = use_signal(|| None::<String>);
    let me = project.me(&session.writer.read());
    let me_ref = me
        .as_ref()
        .map(|(name, role)| (name.as_str(), role.as_str()));
    let list = match &*project.tickets.read() {
        Some(Ok(list)) => Ok(list.tickets.clone()),
        Some(Err(failure)) => Err(Some(failure.clone())),
        None => Err(None),
    };
    let project_id = project.project_id().unwrap_or_default();
    let count = |wanted: Filter| match &list {
        Ok(tickets) => tickets
            .iter()
            .filter(|summary| shows(summary, wanted, me_ref))
            .count(),
        Err(_) => 0,
    };
    let tabs = [
        (Filter::Mine, "Mine", Some(count(Filter::Mine))),
        (Filter::Open, "Open", Some(count(Filter::Open))),
        (Filter::NeedsMe, "Needs me", Some(count(Filter::NeedsMe))),
        (Filter::Completed, "Completed", None),
        (Filter::Tracker, "Tracker", None),
    ];
    let preview = match (&list, selected()) {
        (Ok(tickets), Some(ticket)) => tickets
            .iter()
            .find(|summary| summary.ticket == ticket)
            .cloned(),
        _ => None,
    };
    rsx! {
        div { class: "flex-1 min-h-0 flex",
            main { class: "flex-1 min-w-0 overflow-y-auto px-10 pt-8 pb-12 flex flex-col gap-6",
                PageHeader {
                    context: format!("{project_id} · {} open", count(Filter::Open)),
                    title: "Pick a ticket",
                    actions: rsx! {
                        Button { variant: ButtonVariant::Outline,
                            onclick: move |_| {
                                let mut dialog = session.new_ticket;
                                dialog.set(Some(super::new_ticket::Seed::default()));
                            },
                            "New ticket"
                        }
                    },
                }
                div { class: "flex gap-2 border-b border-line", role: "tablist",
                    for (value , label , number) in tabs {
                        button {
                            role: "tab",
                            "aria-selected": "{filter() == value}",
                            class: if filter() == value { "h-11 px-4 font-semibold shadow-[inset_0_-2px_0_var(--hone-ink)]" } else { "h-11 px-4 text-ink-muted hover:text-ink" },
                            onclick: move |_| filter.set(value),
                            "{label}"
                            if let Some(number) = number {
                                span { class: "ml-2 font-mono text-[12px]", "{number}" }
                            }
                        }
                    }
                }
                if filter() == Filter::Tracker {
                    super::tracker::TrackerScreen {}
                } else {
                    match list {
                        Err(None) => rsx! { Loading {} },
                        Err(Some(failure)) => rsx! { FailureAlert { failure } },
                        Ok(tickets) => {
                            let shown: Vec<TicketSummary> = tickets.into_iter().filter(|summary| shows(summary, filter(), me_ref)).collect();
                            if shown.is_empty() {
                                rsx! { Empty { text: "No ticket here." } }
                            } else {
                                let role = me.as_ref().map(|(_, role)| role.clone()).filter(|role| !role.is_empty());
                                rsx! {
                                    table { class: "w-full border border-line border-collapse",
                                        thead {
                                            tr { class: "text-left text-ink-muted text-[14px] border-b border-line",
                                                th { class: "px-5 h-12 font-normal", "Ticket" }
                                                th { class: "px-5 font-normal", "Profile" }
                                                th { class: "px-5 font-normal", "Steps" }
                                                th { class: "px-5 font-normal", "Waiting on" }
                                                th { class: "px-5 font-normal text-right", "Updated" }
                                            }
                                        }
                                        tbody {
                                            for summary in shown {
                                                Row { key: "{summary.ticket}", summary, role: role.clone(), selected }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
            if let Some(summary) = preview {
                Preview { summary }
            }
        }
    }
}

#[component]
fn Row(summary: TicketSummary, role: Option<String>, selected: Signal<Option<String>>) -> Element {
    let session = use_context::<Session>();
    let (waiting, tone) = waiting_text(&summary, role.as_deref());
    let tone = match tone {
        Tone::Bad => "text-bad",
        Tone::Me => "text-signal",
        Tone::Done => "text-ok",
        Tone::Plain => "text-ink-muted",
    };
    let is_selected = selected().as_deref() == Some(summary.ticket.as_str());
    let external = summary
        .external
        .iter()
        .map(|external| external.key.clone())
        .collect::<Vec<_>>()
        .join(", ");
    let ticket = summary.ticket.clone();
    let opened = summary.ticket.clone();
    let updated = summary
        .last_event
        .as_deref()
        .and_then(|id| ago(id, now_ms()))
        .unwrap_or_default();
    rsx! {
        tr {
            class: if is_selected { "border-b border-line bg-surface-2 cursor-pointer" } else { "border-b border-line hover:bg-surface cursor-pointer" },
            onclick: move |_| selected.set(Some(ticket.clone())),
            ondoubleclick: move |_| session.open_ticket(opened.clone()),
            td { class: "px-5 py-3",
                div { class: "font-semibold", {summary.title.clone().unwrap_or_else(|| summary.ticket.clone())} }
                div { class: "font-mono text-[13px] text-ink-muted",
                    "{summary.ticket}"
                    if !external.is_empty() { " · {external}" }
                }
            }
            td { class: "px-5 font-mono text-[14px]", {summary.profile.clone().unwrap_or_default()} }
            td { class: "px-5", Squares { summary: summary.clone() } }
            td { class: "px-5 font-mono text-[14px] {tone}", "{waiting}" }
            td { class: "px-5 text-right text-ink-muted", "{updated}" }
        }
    }
}

/// One square per step: filled done, dashed skipped, accent current, empty open.
#[component]
fn Squares(summary: TicketSummary) -> Element {
    rsx! {
        span { class: "inline-flex gap-1", "aria-label": "steps",
            for step in summary.steps.iter() {
                {
                    let current = summary.current.as_deref() == Some(step.step.as_str());
                    let class = match step.status.as_str() {
                        "done" => "bg-ink border-ink",
                        "skipped" => "border-dashed border-ink-muted",
                        _ if current => "bg-signal border-signal",
                        _ => "border-ink-muted",
                    };
                    rsx! { span { key: "{step.step}", class: "size-4 border {class}", title: "{step.step}: {step.status}" } }
                }
            }
        }
    }
}

#[component]
fn Preview(summary: TicketSummary) -> Element {
    let session = use_context::<Session>();
    let ticket = summary.ticket.clone();
    rsx! {
        aside { class: "w-[400px] shrink-0 border-l border-line overflow-y-auto px-8 pt-8 pb-12 flex flex-col gap-5",
            span { class: "font-mono text-[12px] tracking-[0.08em] uppercase text-ink-muted", "Preview · {summary.ticket}" }
            h2 { class: "m-0 text-[24px] leading-[30px] font-bold", {summary.title.clone().unwrap_or_else(|| summary.ticket.clone())} }
            div { class: "flex flex-wrap gap-2",
                if let Some(profile) = &summary.profile {
                    Tag { inverted: true, "{profile}" }
                }
                for external in summary.external.iter() {
                    Tag { "{external.system} {external.key}" }
                }
            }
            if let Some(problem) = &summary.problem {
                p { class: "m-0 text-bad text-[14px]", "{problem}" }
            }
            ul { class: "m-0 p-0 list-none border border-line",
                for step in summary.steps.iter() {
                    li { key: "{step.step}", class: "h-11 px-4 flex items-center border-b border-line last:border-b-0 font-mono text-[14px]",
                        span { class: if step.status == "skipped" { "line-through text-ink-muted" } else if summary.current.as_deref() == Some(step.step.as_str()) { "text-signal" } else { "" }, "{step.step}" }
                        span { class: "ml-auto text-ink-muted", "{step.status}" }
                    }
                }
            }
            dl { class: "m-0 grid grid-cols-[auto_1fr] gap-x-6 gap-y-2 text-[14px]",
                dt { class: "text-ink-muted", "Threads" }
                dd { class: "m-0", "{summary.threads.open} open · {summary.threads.blocking} blocking" }
                dt { class: "text-ink-muted", "Last check" }
                dd { class: "m-0",
                    match &summary.last_run {
                        Some(run) if run.failed.is_empty() => rsx! { span { class: "text-ok", "passed · {super::journal::clock(&run.id)}" } },
                        Some(run) => rsx! { span { class: "text-bad", {format!("{} failed · {}", run.failed.len(), super::journal::clock(&run.id))} } },
                        None => rsx! { span { class: "text-ink-muted", "none stored" } },
                    }
                }
            }
            Button { class: "h-12", onclick: move |_| session.open_ticket(ticket.clone()), "Open ticket" }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hone::{Author, Started, ThreadCounts, TicketStep};

    fn summary(waiting: Vec<Waiting>, complete: Option<bool>) -> TicketSummary {
        TicketSummary {
            ticket: "tk-9f2x4a8m".into(),
            record: true,
            title: None,
            external: Vec::new(),
            profile: Some("full".into()),
            started: Some(Started {
                by: Author {
                    name: "alice".into(),
                    role: "architect".into(),
                },
                event: "ev-1".into(),
            }),
            authors: vec!["alice".into(), "bob".into()],
            steps: vec![TicketStep {
                step: "decision".into(),
                requirement: Some("required".into()),
                status: "open".into(),
                reason: None,
            }],
            current: Some("decision".into()),
            complete,
            threads: ThreadCounts {
                open: 0,
                blocking: 0,
            },
            last_event: None,
            last_run: None,
            waiting,
            problem: None,
        }
    }

    #[test]
    fn needs_me_follows_the_role_and_mine_the_name() {
        let ticket = summary(
            vec![Waiting::Step {
                step: "decision".into(),
                role: Some("architect".into()),
            }],
            Some(false),
        );
        assert!(shows(
            &ticket,
            Filter::NeedsMe,
            Some(("carol", "architect"))
        ));
        assert!(!shows(&ticket, Filter::NeedsMe, Some(("alice", "human"))));
        assert!(shows(&ticket, Filter::Mine, Some(("bob", "human"))));
        assert!(!shows(&ticket, Filter::Mine, Some(("carol", "architect"))));
        assert!(shows(&ticket, Filter::Open, None));
        assert!(!shows(&ticket, Filter::Completed, None));
    }

    #[test]
    fn waiting_reads_the_first_reason_and_names_the_reader() {
        let approval = summary(
            vec![Waiting::Approval {
                role: Some("architect".into()),
                gate: "decision/approved".into(),
            }],
            Some(false),
        );
        assert_eq!(
            waiting_text(&approval, Some("architect")),
            ("▲ your approval".into(), Tone::Me)
        );
        assert_eq!(
            waiting_text(&approval, Some("human")),
            ("architect approval".into(), Tone::Plain)
        );
        let gates = summary(
            vec![
                Waiting::Gates {
                    gates: vec!["a/b".into()],
                },
                Waiting::Step {
                    step: "decision".into(),
                    role: None,
                },
            ],
            Some(false),
        );
        assert_eq!(
            waiting_text(&gates, None),
            ("× 1 gate failing".into(), Tone::Bad)
        );
        assert_eq!(
            waiting_text(&summary(Vec::new(), Some(true)), None),
            ("■ completed".into(), Tone::Done)
        );
    }

    #[test]
    fn ages_count_from_the_uuid_v7_time() {
        // 0x019a9da53000 = 1763581440000 ms.
        let id = "ev-019a9da5-3000-7000-8000-000000000000";
        assert_eq!(ago(id, 1_763_581_440_000 + 59_999).as_deref(), Some("now"));
        assert_eq!(
            ago(id, 1_763_581_440_000 + 120_000).as_deref(),
            Some("2 min")
        );
        assert_eq!(
            ago(id, 1_763_581_440_000 + 3 * 3_600_000).as_deref(),
            Some("3 h")
        );
        assert_eq!(
            ago(id, 1_763_581_440_000 + 30 * 3_600_000).as_deref(),
            Some("yesterday")
        );
        assert_eq!(
            ago(id, 1_763_581_440_000 + 5 * 86_400_000).as_deref(),
            Some("5 d")
        );
    }
}
