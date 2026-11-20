use std::path::PathBuf;

use dioxus::desktop::tao::window::Icon;
use dioxus::desktop::{Config, WindowBuilder};
use dioxus::prelude::*;

use crate::features::{self, Project, Screen, Session};

const STYLE: &str = include_str!("../assets/hone.css");
const ICON: &[u8] = include_bytes!("../assets/icon.png");

pub(crate) fn launch(root: PathBuf) {
    let window = WindowBuilder::new()
        .with_title("hone")
        .with_window_icon(window_icon())
        .with_inner_size(dioxus::desktop::LogicalSize::new(1280.0, 840.0));
    LaunchBuilder::desktop()
        .with_cfg(Config::new().with_window(window).with_menu(None))
        .with_context(Root(root))
        .launch(App);
}

// The window icon is the logo's dark variant: its blue reads on light and dark task bars.
fn window_icon() -> Option<Icon> {
    let mut reader = png::Decoder::new(std::io::Cursor::new(ICON))
        .read_info()
        .ok()?;
    let mut pixels = vec![0; reader.output_buffer_size()?];
    let frame = reader.next_frame(&mut pixels).ok()?;
    if frame.color_type != png::ColorType::Rgba || frame.bit_depth != png::BitDepth::Eight {
        return None;
    }
    pixels.truncate(frame.buffer_size());
    Icon::from_rgba(pixels, frame.width, frame.height).ok()
}

#[derive(Clone)]
struct Root(PathBuf);

#[component]
fn App() -> Element {
    let Root(root) = use_context::<Root>();
    let session = use_context_provider(|| Session::new(root));
    let project = Project::use_project(session);
    use_context_provider(|| project);
    let screen = session.screen;
    rsx! {
        document::Style { {STYLE} }
        div { class: "min-h-screen flex flex-col bg-bg text-ink",
            features::settings::Header {}
            features::search::SearchShortcut {}
            if (session.search_open)() {
                features::search::SearchDialog {}
            }
            if let Some(seed) = (session.new_ticket)() {
                features::new_ticket::NewTicketDialog { seed }
            }
            div { class: "flex flex-wrap flex-1",
                Navigation {}
                main { class: "flex-1 min-w-0 px-10 pt-8 pb-12 flex flex-col gap-8",
                    match screen() {
                        Screen::Tickets => rsx! { features::tickets::Tickets {} },
                        Screen::Artifacts => match (session.artifact)() {
                            Some(id) => rsx! { features::artifact::ArtifactDetail { key: "{id}", id } },
                            None => rsx! { features::artifacts::Artifacts {} },
                        },
                        Screen::Graph => rsx! { features::graph::LinkGraph {} },
                        Screen::Threads => rsx! { features::threads::Threads {} },
                        Screen::Events => rsx! { features::events::EventLog {} },
                        Screen::Check => rsx! { features::check::CheckRun {} },
                        Screen::Sync => rsx! { features::sync::Sync {} },
                        Screen::Server => rsx! { features::server::Server {} },
                        Screen::Tracker => rsx! { features::tracker::TrackerScreen {} },
                    }
                }
            }
        }
    }
}

#[component]
fn Navigation() -> Element {
    let session = use_context::<Session>();
    let project = use_context::<Project>();
    let (tickets, artifacts) = match &*project.catalog.read() {
        Some(Ok(catalog)) => (
            Some(catalog.tickets().count()),
            Some(
                catalog
                    .artifacts
                    .iter()
                    .filter(|entry| entry.kind != "ticket")
                    .count(),
            ),
        ),
        _ => (None, None),
    };
    let open_threads = match &*project.state.read() {
        Some(Some(Ok(state))) => Some(
            state
                .threads
                .iter()
                .filter(|thread| thread.status == "open")
                .count(),
        ),
        _ => None,
    };
    let failed = match &*session.last_check.read() {
        Some(features::LastCheck {
            outcome: Ok(run), ..
        }) => Some(run.gates.iter().filter(|gate| !gate.ok).count()),
        _ => None,
    };
    rsx! {
        nav { class: "basis-[232px] grow-0 shrink-0 border-r border-line py-6 flex flex-col",
            NavGroup { label: "Workspace",
                NavItem { screen: Screen::Tickets, label: "Tickets", count: tickets.map(|count| count.to_string()) }
                NavItem { screen: Screen::Artifacts, label: "Artifacts", count: artifacts.map(|count| count.to_string()) }
                NavItem { screen: Screen::Graph, label: "Link graph" }
                NavItem { screen: Screen::Threads, label: "Threads", count: open_threads.filter(|count| *count > 0).map(|count| format!("{count} open")) }
                NavItem {
                    screen: Screen::Check,
                    label: "Checks",
                    count: failed.filter(|count| *count > 0).map(|count| format!("{count} fail")),
                    bad: true,
                }
                NavItem { screen: Screen::Events, label: "Event log" }
            }
            NavGroup { label: "Project",
                NavItem { screen: Screen::Tracker, label: "Tracker" }
                NavItem { screen: Screen::Sync, label: "Sync" }
                NavItem { screen: Screen::Server, label: "Server" }
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
fn NavItem(
    screen: Screen,
    label: String,
    #[props(optional)] count: Option<String>,
    #[props(optional)] bad: bool,
) -> Element {
    let session = use_context::<Session>();
    let mut current = session.screen;
    let mut artifact = session.artifact;
    let active = current() == screen;
    let class = if active {
        "h-11 px-4 flex items-center justify-between gap-2 text-left bg-ink text-bg font-semibold"
    } else {
        "h-11 px-4 flex items-center justify-between gap-2 text-left hover:bg-surface-2"
    };
    let count_class = match (active, bad) {
        (true, _) => "font-mono text-[12px]",
        (false, true) => "font-mono text-[12px] text-bad",
        (false, false) => "font-mono text-[12px] text-ink-muted",
    };
    rsx! {
        button {
            class,
            "aria-current": if active { "page" } else { "false" },
            onclick: move |_| {
                if screen == Screen::Artifacts {
                    artifact.set(None);
                }
                current.set(screen);
            },
            span { "{label}" }
            if let Some(count) = count {
                span { class: count_class, "{count}" }
            }
        }
    }
}
