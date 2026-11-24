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
        document::Script { {features::editor::BUNDLE} }
        div { class: "h-screen flex flex-col bg-bg text-ink",
            features::settings::Header {}
            features::search::SearchShortcut {}
            if (session.search_open)() {
                features::search::SearchDialog {}
            }
            if let Some(seed) = (session.new_ticket)() {
                features::new_ticket::NewTicketDialog { seed }
            }
            if (session.new_artifact)() {
                features::new_artifact::NewArtifactDialog {}
            }
            match screen() {
                Screen::Ticket => rsx! { features::tabs::Workspace {} },
                Screen::Tickets => rsx! { features::tickets::Tickets {} },
                other => rsx! {
                    main { class: "flex-1 min-h-0 overflow-y-auto px-10 pt-8 pb-12 flex flex-col gap-8",
                        match other {
                            Screen::Artifacts => match (session.artifact)() {
                                Some(id) => rsx! { features::artifact::ArtifactDetail { key: "{id}", id } },
                                None => rsx! { features::artifacts::Artifacts {} },
                            },
                            Screen::Profiles => rsx! { features::configuration::Profiles {} },
                            Screen::Plugins => rsx! { features::configuration::Plugins {} },
                            Screen::Sync => rsx! {
                                features::sync::Sync {}
                                features::server::Server {}
                                features::events::EventLog {}
                            },
                            _ => rsx! { features::project_settings::ProjectSettingsScreen {} },
                        }
                    }
                },
            }
            features::status::StatusBar {}
        }
    }
}
