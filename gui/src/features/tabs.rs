//! The workspace of the open ticket: the explorer, the tabs, and the assistant.

use dioxus::prelude::*;

use super::assistant::AssistantPanel;
use super::explorer::Explorer;
use super::{Session, Tab};

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
    }
}

#[component]
fn TabButton(tab: Tab, active: bool) -> Element {
    let mut session = use_context::<Session>();
    let class = if active {
        "h-full px-4 flex items-center gap-3 border-r border-line bg-bg font-semibold shadow-[inset_0_2px_0_var(--hone-ink)]"
    } else {
        "h-full px-4 flex items-center gap-3 border-r border-line text-ink-muted hover:text-ink"
    };
    let shown = tab.clone();
    let closed = tab.clone();
    rsx! {
        div { class,
            button { role: "tab", "aria-selected": "{active}", class: "h-full",
                onclick: move |_| session.open_tab(shown.clone()),
                {tab.title()}
            }
            if tab != Tab::Overview {
                button {
                    class: "text-ink-muted hover:text-ink",
                    title: "Close",
                    "aria-label": "Close {tab.title()}",
                    onclick: move |_| {
                        let Some(ticket) = (session.ticket)() else { return };
                        if let Some(tabs) = session.tabs.write().get_mut(&ticket) {
                            tabs.close(&closed);
                        }
                    },
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
        Tab::Artifact(id) => rsx! { super::artifact::ArtifactDetail { key: "{id}", id } },
        Tab::Check => rsx! { super::check::CheckRun {} },
        Tab::Graph => rsx! { super::graph::LinkGraph {} },
        Tab::Events => rsx! { super::events::EventLog {} },
        Tab::Threads => rsx! { super::threads::Threads {} },
    }
}
