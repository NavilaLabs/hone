//! Starting a ticket: attributes, the profile `select` picks for them, an optional override, and
//! the external tickets it belongs to.

use std::time::Duration;

use dioxus::prelude::*;

use super::attributes::{AttributeEditor, Row, parse_rows, rule_keys};
use super::common::{FailureAlert, SectionLabel};
use super::{Project, Session};
use crate::hone::{Failure, ProfileSelection, TicketStart};
use crate::tracker::{Issue, Tracker};
use crate::ui::{Button, ButtonVariant, Choice, Combobox};

/// What a new ticket starts from: external tickets and a title to show.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct Seed {
    /// External tickets as `<system>:<key>[=<url>]`.
    pub(crate) external: Vec<String>,
    /// The title of the tracker ticket, shown above the form.
    pub(crate) title: Option<String>,
}

impl Seed {
    /// A seed from a ticket of `tracker`.
    pub(crate) fn from_issue(tracker: &Tracker, issue: &Issue) -> Self {
        Self {
            external: vec![external_argument(tracker, issue)],
            title: Some(issue.title.clone()),
        }
    }
}

/// The `--external` value for a tracker ticket: `github:8798=<url>` or `jira:PAY-12=<url>`.
pub(crate) fn external_argument(tracker: &Tracker, issue: &Issue) -> String {
    if issue.url.is_empty() {
        format!("{}:{}", tracker.system(), issue.key)
    } else {
        format!("{}:{}={}", tracker.system(), issue.key, issue.url)
    }
}

/// A sentence that explains the selection: which rule matched, or that the default applies.
pub(crate) fn explain(selection: &ProfileSelection) -> String {
    let Some(profile) = &selection.selected else {
        return "select gives no profile for these attributes".to_owned();
    };
    match selection.rule {
        Some(index) => {
            let when = selection.rules[index]["when"]
                .as_object()
                .map(|when| {
                    when.iter()
                        .map(|(key, value)| match value {
                            serde_json::Value::Array(set) => format!(
                                "{key} ∈ {}",
                                set.iter()
                                    .map(ToString::to_string)
                                    .collect::<Vec<_>>()
                                    .join(", ")
                            ),
                            scalar => format!("{key} = {scalar}"),
                        })
                        .collect::<Vec<_>>()
                        .join(" and ")
                })
                .unwrap_or_default();
            format!("{profile}, by rule {} ({when})", index + 1)
        }
        None => format!("{profile}, the default: no rule matches"),
    }
}

/// The dialog that starts a ticket.
#[component]
pub(crate) fn NewTicketDialog(seed: Seed) -> Element {
    let mut session = use_context::<Session>();
    let project = use_context::<Project>();
    let rows = use_signal(|| vec![Row::empty()]);
    let mut external = use_signal(|| seed.external.clone());
    let mut profile = use_signal(|| None::<String>);
    let mut failure = use_signal(|| None::<Failure>);
    let mut busy = use_signal(|| false);
    let attributes = parse_rows(&rows());
    let preview = use_resource(move || async move {
        let attributes = parse_rows(&rows())?;
        // Waits for the typing to pause; a new keystroke restarts the resource.
        tokio::time::sleep(Duration::from_millis(250)).await;
        Some((session.hone)().select(&attributes).await)
    });
    let selection = match &*preview.read() {
        Some(Some(Ok(selection))) => Some(selection.clone()),
        _ => None,
    };
    let keys = selection
        .as_ref()
        .map(|selection| rule_keys(&selection.rules))
        .unwrap_or_default();
    let selected = selection
        .as_ref()
        .and_then(|selection| selection.selected.clone());
    let profile_choices: Vec<Choice> = std::iter::once(Choice::plain("").label("as select gives"))
        .chain(
            selection
                .iter()
                .flat_map(|selection| selection.profiles.clone())
                .map(|name| {
                    let note = if Some(&name) == selected.as_ref() {
                        "selected"
                    } else {
                        ""
                    };
                    Choice::plain(name).note(note)
                }),
        )
        .collect();
    let overridden = profile().filter(|chosen| Some(chosen) != selected.as_ref());
    let writer = (session.writer)();
    let writing_as = if writer.author.is_empty() {
        "the context's author".to_owned()
    } else {
        format!("{} · {}", writer.author, writer.role)
    };
    let start = move |_| {
        let Some(attributes) = parse_rows(&rows()) else {
            return;
        };
        let start = TicketStart {
            attributes,
            external: external(),
            profile: profile(),
        };
        busy.set(true);
        spawn(async move {
            match project
                .resolving((session.hone)())
                .start_ticket(&(session.writer)(), &start)
                .await
            {
                Ok(written) => {
                    failure.set(None);
                    session.new_ticket.set(None);
                    session.changed();
                    session.open_ticket(written.ticket);
                }
                Err(error) => failure.set(Some(error)),
            }
            busy.set(false);
        });
    };
    let _ = project;
    rsx! {
        div {
            class: "fixed inset-0 z-50 flex items-start justify-center pt-[8vh] bg-[color-mix(in_srgb,var(--hone-bg)_70%,transparent)] overflow-y-auto",
            onclick: move |_| session.new_ticket.set(None),
            div {
                class: "w-[min(760px,94vw)] border border-line bg-bg flex flex-col gap-5 p-6 mb-12",
                role: "dialog",
                "aria-modal": "true",
                "aria-label": "Start a ticket",
                onclick: move |event| event.stop_propagation(),
                onkeydown: move |event| {
                    if event.key() == Key::Escape {
                        session.new_ticket.set(None);
                    }
                },
                div { class: "flex flex-col gap-1",
                    span { class: "font-mono text-[13px] text-ink-muted", "new ticket" }
                    h2 { class: "m-0 text-[22px] leading-7 font-bold tracking-[-0.01em]",
                        {seed.title.clone().unwrap_or_else(|| "Start a ticket".to_owned())}
                    }
                }
                if !external().is_empty() {
                    section { class: "flex flex-col gap-2",
                        SectionLabel { label: "External tickets" }
                        for (index , entry) in external().into_iter().enumerate() {
                            div { class: "flex items-center gap-3 border border-line px-3 h-10",
                                span { class: "font-mono text-[13px] truncate min-w-0", "{entry}" }
                                button {
                                    class: "ml-auto text-[13px] underline text-ink-muted hover:text-ink",
                                    onclick: move |_| external.with_mut(|external| { external.remove(index); }),
                                    "Remove"
                                }
                            }
                        }
                    }
                }
                section { class: "flex flex-col gap-2",
                    SectionLabel { label: "Attributes", aside: "they choose the profile" }
                    AttributeEditor { rows, keys }
                }
                section { class: "flex flex-col gap-3",
                    SectionLabel { label: "Profile" }
                    div { class: "border border-line bg-surface px-4 py-3 flex flex-col gap-1",
                        span { class: "text-[13px] text-ink-muted", "select gives" }
                        span { class: "font-mono text-[15px]",
                            match (&attributes, &*preview.read()) {
                                (None, _) => rsx! { span { class: "text-ink-muted", "— fix the attributes first" } },
                                (_, Some(Some(Err(failure)))) => rsx! { span { class: "text-bad", "✕ {failure.message}" } },
                                (_, Some(Some(Ok(selection)))) => rsx! { "{explain(selection)}" },
                                _ => rsx! { span { class: "text-ink-muted", "…" } },
                            }
                        }
                    }
                    div { class: "flex flex-col gap-1 w-80",
                        span { class: "text-[13px] text-ink-muted", "Profile (optional override)" }
                        Combobox {
                            value: profile().unwrap_or_default(),
                            choices: profile_choices,
                            placeholder: "as select gives",
                            label: "Profile",
                            onselect: move |chosen: String| profile.set(Some(chosen).filter(|chosen| !chosen.is_empty())),
                        }
                    }
                    if let Some(chosen) = overridden {
                        div { class: "border-2 border-warn px-4 py-3 text-[13px] flex flex-col gap-1", role: "status",
                            span { class: "font-semibold",
                                span { class: "text-warn", "aria-hidden": "true", "⚠ " }
                                "The ticket will use {chosen}, not {selected.clone().unwrap_or_else(|| \"the selected profile\".to_owned())}."
                            }
                            span { class: "text-ink-muted", "steps-complete keeps this visible as the warning profile-not-selected, so the team sees that the rules were bypassed." }
                        }
                    }
                }
                if let Some(failure) = failure() {
                    FailureAlert { failure: failure.clone() }
                    if failure.message.contains("run hone resolve") {
                        ResolveNow {}
                    }
                }
                div { class: "flex flex-wrap items-center gap-2 justify-end",
                    span { class: "mr-auto text-[13px] text-ink-muted", "Writes the record, the snapshot, and ticket.started as {writing_as}." }
                    Button { variant: ButtonVariant::Outline, onclick: move |_| session.new_ticket.set(None), "Cancel" }
                    Button { disabled: busy() || attributes.is_none(), onclick: start, "Start ticket" }
                }
            }
        }
    }
}

/// Offers `hone resolve` after a command refused a stale `.hone/resolved.json`.
#[component]
pub(crate) fn ResolveNow() -> Element {
    let session = use_context::<Session>();
    let project = use_context::<Project>();
    let mut outcome = use_signal(|| None::<Result<String, Failure>>);
    let mut busy = use_signal(|| false);
    rsx! {
        div { class: "flex flex-wrap items-center gap-3",
            span { class: "text-[13px] text-ink-muted", "The resolved configuration is older than its sources. hone resolve writes it again; review the change before you commit it." }
            Button {
                variant: ButtonVariant::Outline,
                disabled: busy(),
                onclick: move |_| {
                    busy.set(true);
                    spawn(async move {
                        let result = project.resolving((session.hone)()).resolve().await;
                        if result.is_ok() {
                            session.changed();
                        }
                        outcome.set(Some(result));
                        busy.set(false);
                    });
                },
                "Run hone resolve"
            }
        }
        match outcome() {
            Some(Ok(hash)) => rsx! { p { class: "m-0 text-[13px] text-ok", role: "status", "■ .hone/resolved.json written ({hash}). Try again." } },
            Some(Err(failure)) => rsx! { FailureAlert { failure } },
            None => rsx! {},
        }
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn selection(rule: Option<usize>) -> ProfileSelection {
        ProfileSelection {
            selected: Some(if rule.is_some() { "full" } else { "standard" }.into()),
            rule,
            rules: vec![
                json!({ "when": { "risk": ["high", "critical"], "type": "feature" }, "profile": "full" }),
            ],
            default: Some("standard".into()),
            profiles: vec!["full".into(), "standard".into()],
        }
    }

    #[test]
    fn the_explanation_names_the_rule_or_the_default() {
        assert_eq!(
            explain(&selection(Some(0))),
            "full, by rule 1 (risk ∈ \"high\", \"critical\" and type = \"feature\")"
        );
        assert_eq!(
            explain(&selection(None)),
            "standard, the default: no rule matches"
        );
    }

    #[test]
    fn a_tracker_ticket_becomes_an_external_argument() {
        let github = Tracker::Github {
            owner: "acme".into(),
            repo: "payments".into(),
            token_env: None,
        };
        let issue = Issue {
            key: "8798".into(),
            title: "Batches".into(),
            state: "open".into(),
            url: "https://github.com/acme/payments/issues/8798".into(),
        };
        assert_eq!(
            Seed::from_issue(&github, &issue),
            Seed {
                external: vec!["github:8798=https://github.com/acme/payments/issues/8798".into()],
                title: Some("Batches".into()),
            }
        );
    }
}
