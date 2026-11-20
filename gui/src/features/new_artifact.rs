//! The New artifact dialog: `hone artifact new` with a kind, a title, the ticket of a decided
//! kind, and anchors.

use dioxus::prelude::*;

use super::common::{FailureAlert, Field, FindingList, SectionLabel};
use super::{Project, Session};
use crate::hone::{ArtifactNew, ArtifactWritten, Failure, KindInfo};
use crate::ui::{Button, ButtonVariant, Choice, Combobox, Input};

/// The anchors of the form as `<kind>:<ref>`, leaving out rows without a reference.
fn anchor_arguments(rows: &[(String, String)]) -> Vec<String> {
    rows.iter()
        .filter(|(_, reference)| !reference.trim().is_empty())
        .map(|(kind, reference)| format!("{kind}:{}", reference.trim()))
        .collect()
}

/// The dialog, open while `Session::new_artifact` is set.
#[component]
pub(crate) fn NewArtifactDialog() -> Element {
    let mut session = use_context::<Session>();
    let project = use_context::<Project>();
    let kinds = use_resource(move || async move { (session.hone)().kinds().await });
    let mut kind = use_signal(String::new);
    let mut title = use_signal(String::new);
    let mut ticket = use_signal(|| (session.ticket)().unwrap_or_default());
    let mut anchors = use_signal(Vec::<(String, String)>::new);
    let mut outcome = use_signal(|| None::<Result<ArtifactWritten, Failure>>);
    let mut busy = use_signal(|| false);
    let known: Vec<KindInfo> = match &*kinds.read() {
        Some(Ok(kinds)) => kinds.clone(),
        _ => Vec::new(),
    };
    let chosen = known.iter().find(|info| info.kind == kind()).cloned();
    let decided = chosen
        .as_ref()
        .is_some_and(|info| info.lifecycle == "decided");
    let ready =
        chosen.is_some() && !title().trim().is_empty() && (!decided || !ticket().is_empty());
    let create = move |_| {
        let new = ArtifactNew {
            kind: kind(),
            title: title().trim().to_owned(),
            ticket: Some(ticket()).filter(|ticket| decided && !ticket.is_empty()),
            author: Some((session.writer)().author).filter(|author| !author.is_empty()),
            anchors: anchor_arguments(&anchors()),
        };
        busy.set(true);
        spawn(async move {
            let result = (session.hone)().new_artifact(&new).await;
            busy.set(false);
            match result {
                Ok(written) if written.saved => {
                    session.changed();
                    session.new_artifact.set(false);
                    if let Some(id) = written.id {
                        session.open_artifact(id);
                    }
                }
                other => outcome.set(Some(other)),
            }
        });
    };
    let kind_choices: Vec<Choice> = known
        .iter()
        .map(|info| {
            Choice::plain(info.kind.clone())
                .label(info.path.clone())
                .note(info.lifecycle.clone())
        })
        .collect();
    let writer = (session.writer)();
    let author = if writer.author.is_empty() {
        "the context's author".to_owned()
    } else {
        writer.author.clone()
    };
    rsx! {
        div {
            class: "fixed inset-0 z-50 flex items-start justify-center pt-[8vh] bg-[color-mix(in_srgb,var(--hone-bg)_70%,transparent)] overflow-y-auto",
            onclick: move |_| session.new_artifact.set(false),
            div {
                class: "w-[min(720px,94vw)] border border-line bg-bg flex flex-col gap-5 p-6 mb-12",
                role: "dialog",
                "aria-modal": "true",
                "aria-label": "New artifact",
                onclick: move |event| event.stop_propagation(),
                onkeydown: move |event| {
                    if event.key() == Key::Escape {
                        session.new_artifact.set(false);
                    }
                },
                div { class: "flex flex-col gap-1",
                    span { class: "font-mono text-[13px] text-ink-muted", "hone artifact new" }
                    h2 { class: "m-0 text-[22px] leading-7 font-bold tracking-[-0.01em]", "New artifact" }
                }
                if let Some(Err(failure)) = &*kinds.read() {
                    FailureAlert { failure: failure.clone() }
                }
                div { class: "flex flex-wrap gap-4 items-end",
                    div { class: "flex flex-col gap-1 w-64",
                        span { class: "text-[13px] text-ink-muted", "Kind" }
                        Combobox {
                            value: kind(),
                            choices: kind_choices,
                            label: "Kind",
                            onselect: move |chosen: String| kind.set(chosen),
                        }
                    }
                    div { class: "flex-1 min-w-[240px]",
                        Field { label: "Title (the slug of the path and the first heading)",
                            Input { value: title(), placeholder: "Payments run asynchronously", oninput: move |value| title.set(value) }
                        }
                    }
                }
                if decided {
                    div { class: "flex flex-col gap-1 w-80",
                        span { class: "text-[13px] text-ink-muted", "Ticket (a decided kind belongs to one)" }
                        Combobox {
                            value: ticket(),
                            choices: project.ticket_choices(),
                            label: "Ticket",
                            invalid: ticket().is_empty(),
                            onselect: move |chosen: String| ticket.set(chosen),
                        }
                    }
                }
                if let Some(info) = &chosen {
                    p { class: "m-0 font-mono text-[12.5px] text-ink-muted break-all",
                        "{info.path} · prefix {info.id_prefix}"
                        if let Some(status) = &info.first_status {
                            " · starts as {status}"
                        }
                        if !info.required.is_empty() {
                            " · requires {info.required.join(\", \")}"
                        }
                    }
                }
                section { class: "flex flex-col gap-2",
                    SectionLabel { label: "Anchors", aside: "optional; where it lives in the code" }
                    for (index , (anchor_kind , reference)) in anchors().into_iter().enumerate() {
                        div { key: "{index}", class: "flex flex-wrap items-center gap-2",
                            div { class: "w-32",
                                Combobox {
                                    value: anchor_kind,
                                    choices: ["path", "glob", "symbol"].map(Choice::plain).to_vec(),
                                    label: "Anchor kind",
                                    onselect: move |chosen: String| anchors.with_mut(|rows| rows[index].0 = chosen),
                                }
                            }
                            div { class: "flex-1 min-w-[240px]",
                                Input { class: "font-mono", value: reference, placeholder: "src/fee.rs#Fee::add", oninput: move |value: String| anchors.with_mut(|rows| rows[index].1 = value) }
                            }
                            button {
                                class: "text-[13px] underline text-ink-muted hover:text-ink",
                                onclick: move |_| anchors.with_mut(|rows| { rows.remove(index); }),
                                "Remove"
                            }
                        }
                    }
                    div {
                        Button { variant: ButtonVariant::Outline, onclick: move |_| anchors.with_mut(|rows| rows.push(("symbol".into(), String::new()))), "Add an anchor" }
                    }
                }
                match outcome() {
                    Some(Err(failure)) => rsx! { FailureAlert { failure } },
                    Some(Ok(written)) => rsx! {
                        p { class: "m-0 text-[13px] text-bad font-semibold", role: "alert", "✕ Not created: the new file has errors." }
                        FindingList { findings: written.findings }
                    },
                    None => rsx! {},
                }
                div { class: "flex flex-wrap items-center gap-2 justify-end",
                    span { class: "mr-auto text-[13px] text-ink-muted", "Writes the file with {author} in authors; no event." }
                    Button { variant: ButtonVariant::Outline, onclick: move |_| session.new_artifact.set(false), "Cancel" }
                    Button { disabled: busy() || !ready, onclick: create, "Create" }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::anchor_arguments;

    #[test]
    fn anchors_become_arguments_and_empty_rows_are_left_out() {
        let rows = [
            ("symbol".to_owned(), " src/fee.rs#Fee::add ".to_owned()),
            ("path".to_owned(), "  ".to_owned()),
            ("glob".to_owned(), "src/**/*.rs".to_owned()),
        ];
        assert_eq!(
            anchor_arguments(&rows),
            ["symbol:src/fee.rs#Fee::add", "glob:src/**/*.rs"]
        );
    }
}
