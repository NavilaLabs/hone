use std::path::PathBuf;

use dioxus::desktop::{Config, WindowBuilder};
use dioxus::prelude::*;

use crate::features::{self, Screen, Session};

const STYLE: &str = include_str!("../assets/hone.css");

pub(crate) fn launch(root: PathBuf) {
    let window = WindowBuilder::new()
        .with_title("hone")
        .with_inner_size(dioxus::desktop::LogicalSize::new(1280.0, 840.0));
    LaunchBuilder::desktop()
        .with_cfg(Config::new().with_window(window).with_menu(None))
        .with_context(Root(root))
        .launch(App);
}

#[derive(Clone)]
struct Root(PathBuf);

#[component]
fn App() -> Element {
    let Root(root) = use_context::<Root>();
    let session = use_context_provider(|| Session::new(root));
    let screen = session.screen;
    rsx! {
        document::Style { {STYLE} }
        div { class: "min-h-screen flex flex-col bg-bg text-ink",
            features::settings::Header {}
            div { class: "flex flex-wrap flex-1",
                Navigation {}
                main { class: "flex-1 min-w-0 px-10 pt-8 pb-12 flex flex-col gap-8",
                    match screen() {
                        Screen::Tickets => rsx! { features::tickets::Tickets {} },
                        Screen::Threads => rsx! { features::threads::Threads {} },
                        Screen::Events => rsx! { features::events::EventLog {} },
                        Screen::Check => rsx! { features::check::CheckRun {} },
                        Screen::Sync => rsx! { features::sync::Sync {} },
                        Screen::Server => rsx! { features::server::Server {} },
                    }
                }
            }
        }
    }
}

#[component]
fn Navigation() -> Element {
    rsx! {
        nav { class: "basis-[232px] grow-0 shrink-0 border-r border-line py-6 flex flex-col",
            NavGroup { label: "Workspace",
                NavItem { screen: Screen::Tickets, "Tickets" }
                NavItem { screen: Screen::Threads, "Threads" }
                NavItem { screen: Screen::Check, "Checks" }
                NavItem { screen: Screen::Events, "Event log" }
            }
            NavGroup { label: "Project",
                NavItem { screen: Screen::Sync, "Sync" }
                NavItem { screen: Screen::Server, "Server" }
            }
        }
    }
}

#[component]
fn NavGroup(label: String, children: Element) -> Element {
    rsx! {
        div { class: "flex flex-col pb-6",
            span { class: "px-4 pb-2 font-mono text-[11px] leading-4 font-medium tracking-[0.08em] uppercase text-ink-muted",
                "{label}"
            }
            {children}
        }
    }
}

#[component]
fn NavItem(screen: Screen, children: Element) -> Element {
    let mut current = use_context::<Session>().screen;
    let active = current() == screen;
    let class = if active {
        "h-11 px-4 flex items-center text-left bg-ink text-bg font-semibold"
    } else {
        "h-11 px-4 flex items-center text-left hover:bg-surface-2"
    };
    rsx! {
        button {
            class,
            "aria-current": if active { "page" } else { "false" },
            onclick: move |_| current.set(screen),
            {children}
        }
    }
}
