//! A ticket's attributes to edit, and the switch to the current rules.

use dioxus::prelude::*;

use super::attributes::{AttributeEditor, attribute_changes, parse_rows, rows_of, rule_keys};
use super::common::{FailureAlert, short_hash};
use super::{Project, Session};
use crate::hone::{Failure, TicketOverview};
use crate::ui::{Button, ButtonVariant};

#[component]
pub(crate) fn TicketSettings(ticket: String, overview: TicketOverview) -> Element {
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
pub(crate) fn outcome_text(done: &str, profile: &str, selected: Option<&str>) -> String {
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
pub(crate) fn ExternalLink(label: String, url: String) -> Element {
    rsx! {
        button {
            class: "h-7 px-2 border border-line font-mono text-[12.5px] underline hover:bg-surface-2",
            title: "Open {url} in the browser",
            onclick: move |_| super::common::open_in_browser(&url),
            "{label} ↗"
        }
    }
}

#[cfg(test)]
mod tests {
    use super::outcome_text;

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
}
