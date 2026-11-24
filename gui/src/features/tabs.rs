//! The workspace of the open ticket: the explorer, the tabs, and the assistant.

use dioxus::prelude::*;

use super::assistant::AssistantPanel;
use super::code::{CodeTarget, CodeView};
use super::common::FailureAlert;
use super::explorer::Explorer;
use super::{Session, Tab};
use crate::hone::Failure;
use crate::ui::{Button, ButtonVariant};

/// The explorer, the tab bar with the active tab, and the assistant.
#[component]
pub(crate) fn Workspace() -> Element {
    let session = use_context::<Session>();
    let mut assistant = session.assistant_open;
    let Some(ticket) = (session.ticket)() else {
        return rsx! {};
    };
    let tabs = session.ticket_tabs();
    rsx! {
        div { class: "flex-1 min-h-0 flex",
            Explorer {}
            div { class: "flex-1 min-w-0 flex flex-col min-h-0",
                div { class: "h-12 shrink-0 border-b border-line flex items-stretch bg-surface", role: "tablist",
                    for tab in tabs.open.clone() {
                        TabButton { key: "{tab:?}", tab: tab.clone(), active: tab == tabs.active }
                    }
                    if !assistant() {
                        button {
                            class: "ml-auto px-4 border-l border-line text-[14px] hover:bg-surface-2",
                            onclick: move |_| assistant.set(true),
                            "Assistant ‹"
                        }
                    }
                }
                main { class: "flex-1 min-h-0 overflow-y-auto px-10 pt-8 pb-12 flex flex-col gap-8", role: "tabpanel",
                    TabContent { key: "{ticket}-{tabs.active:?}", ticket: ticket.clone(), tab: tabs.active.clone() }
                }
            }
            if assistant() {
                AssistantPanel {}
            }
        }
        if let Some(tab) = (session.closing)() {
            CloseDialog { tab }
        }
    }
}

/// Closes `tab` of the open ticket, or asks first while its artifact has unsaved changes.
fn close(mut session: Session, tab: &Tab) {
    let unsaved = matches!(tab, Tab::Artifact(id) if session.drafts.read().contains_key(id));
    if unsaved {
        session.closing.set(Some(tab.clone()));
        return;
    }
    let Some(ticket) = (session.ticket)() else {
        return;
    };
    if let Some(tabs) = session.tabs.write().get_mut(&ticket) {
        tabs.close(tab);
    }
}

/// Asks whether to save or discard the unsaved changes of `tab` before it closes.
#[component]
fn CloseDialog(tab: Tab) -> Element {
    let mut session = use_context::<Session>();
    let mut failure = use_signal(|| None::<Failure>);
    let mut busy = use_signal(|| false);
    let Tab::Artifact(id) = tab.clone() else {
        return rsx! {};
    };
    let Some(draft) = session.drafts.read().get(&id).cloned() else {
        return rsx! {};
    };
    let finish = {
        let id = id.clone();
        let tab = tab.clone();
        move || {
            session.drafts.write().remove(&id);
            session.closing.set(None);
            close(session, &tab);
        }
    };
    let mut discard = finish.clone();
    let saved = finish;
    let save = move |_| {
        let draft = draft.clone();
        let mut saved = saved.clone();
        busy.set(true);
        spawn(async move {
            match (session.hone)()
                .save_artifact(&draft.path, &draft.text)
                .await
            {
                Ok(written) if written.saved => {
                    session.changed();
                    saved();
                }
                Ok(_) => failure.set(Some(Failure {
                    status: "failed".into(),
                    message: "Not saved: the text has errors. Open the tab to see them.".into(),
                })),
                Err(error) => failure.set(Some(error)),
            }
            busy.set(false);
        });
    };
    rsx! {
        div {
            class: "fixed inset-0 z-50 flex items-start justify-center pt-[16vh] bg-[color-mix(in_srgb,var(--hone-bg)_70%,transparent)]",
            onclick: move |_| session.closing.set(None),
            div {
                class: "w-[min(520px,94vw)] border border-line bg-bg flex flex-col gap-4 p-6",
                role: "dialog",
                "aria-modal": "true",
                "aria-label": "Unsaved changes",
                onclick: move |event| event.stop_propagation(),
                onkeydown: move |event| {
                    if event.key() == Key::Escape {
                        session.closing.set(None);
                    }
                },
                h2 { class: "m-0 text-[18px] font-bold", "{id} has unsaved changes" }
                p { class: "m-0 text-[14px] text-ink-muted", "Save them before the tab closes, or discard them." }
                if let Some(failure) = failure() {
                    FailureAlert { failure }
                }
                div { class: "flex gap-2 justify-end",
                    Button { variant: ButtonVariant::Outline, onclick: move |_| session.closing.set(None), "Cancel" }
                    Button { variant: ButtonVariant::Outline, disabled: busy(), onclick: move |_| discard(), "Discard" }
                    Button { disabled: busy(), onclick: save, "Save" }
                }
            }
        }
    }
}

#[component]
fn TabButton(tab: Tab, active: bool) -> Element {
    let session = use_context::<Session>();
    let class = if active {
        "h-full px-4 flex items-center gap-3 border-r border-line bg-bg font-semibold shadow-[inset_0_2px_0_var(--hone-ink)]"
    } else {
        "h-full px-4 flex items-center gap-3 border-r border-line text-ink-muted hover:text-ink"
    };
    let shown = tab.clone();
    let closed = tab.clone();
    let unsaved = matches!(&tab, Tab::Artifact(id) if session.drafts.read().contains_key(id));
    rsx! {
        div { class,
            button { role: "tab", "aria-selected": "{active}", class: "h-full",
                onclick: move |_| session.open_tab(shown.clone()),
                {tab.title()}
                if unsaved {
                    span { class: "ml-2 text-signal", title: "Unsaved changes", "●" }
                }
            }
            if tab != Tab::Overview {
                button {
                    class: "text-ink-muted hover:text-ink",
                    title: "Close",
                    "aria-label": "Close {tab.title()}",
                    onclick: move |_| close(session, &closed),
                    "×"
                }
            }
        }
    }
}

#[component]
fn TabContent(ticket: String, tab: Tab) -> Element {
    match tab {
        Tab::Overview => rsx! { super::overview::Overview { ticket } },
        Tab::Journal => rsx! { super::journal::Journal { ticket } },
        Tab::Artifact(id) => rsx! { ArtifactInTicket { key: "{id}", id } },
        Tab::Code(target) => rsx! { CodeView { target } },
        Tab::Check => rsx! { super::check::CheckRun {} },
        Tab::Graph => rsx! { super::graph::LinkGraph {} },
        Tab::Events => rsx! { super::events::EventLog {} },
        Tab::Threads => rsx! { super::threads::Threads {} },
    }
}

#[component]
fn ArtifactInTicket(id: String) -> Element {
    // Inside a ticket, code opens as a tab; the panel stays closed.
    let code = use_signal(|| None::<CodeTarget>);
    rsx! { super::artifact_tab::ArtifactTab { id, code } }
}
