use dioxus::prelude::*;

use super::{Project, Screen, Session};
use crate::hone::{Hone, Writer};
use crate::ui::{Button, ButtonSize, ButtonVariant, Combobox, Input};

const LOGO_LIGHT: &str = include_str!("../../assets/logo-light.svg");
const LOGO_DARK: &str = include_str!("../../assets/logo-dark.svg");

/// The header: the wordmark, the project switcher, the views outside a ticket, the open ticket,
/// the search, and the writer.
#[component]
pub(crate) fn Header() -> Element {
    let session = use_context::<Session>();
    let project = use_context::<Project>();
    let mut hone = session.hone;
    let mut menu = use_signal(|| false);
    let mut root = use_signal(|| hone.read().root().display().to_string());
    let shown_root = hone.read().root().display().to_string();
    let folder = hone
        .read()
        .root()
        .canonicalize()
        .ok()
        .and_then(|path| {
            path.file_name()
                .map(|name| name.to_string_lossy().into_owned())
        })
        .unwrap_or_else(|| shown_root.clone());
    let mut open_root = move || {
        hone.set(Hone::new(root()));
        let mut ticket = session.ticket;
        ticket.set(None);
        let mut screen = session.screen;
        screen.set(Screen::Tickets);
        menu.set(false);
        session.changed();
    };
    let views = [
        (Screen::Tickets, "Tickets"),
        (Screen::Artifacts, "Artifacts"),
        (Screen::Profiles, "Profiles"),
        (Screen::Plugins, "Plugins"),
        (Screen::Sync, "Sync"),
    ];
    let current = (session.screen)();
    let ticket = (session.ticket)();
    let summary = ticket.as_deref().and_then(|ticket| project.summary(ticket));
    rsx! {
        header { class: "h-16 shrink-0 border-b border-line px-6 flex items-center gap-6 relative",
            span { class: "flex items-center gap-2 text-[22px] font-bold tracking-[-0.01em]",
                span { class: "hone-logo-light size-6 [&>svg]:size-6", "aria-hidden": "true", dangerous_inner_html: LOGO_LIGHT }
                span { class: "hone-logo-dark size-6 [&>svg]:size-6", "aria-hidden": "true", dangerous_inner_html: LOGO_DARK }
                "hone"
            }
            button {
                class: "h-10 px-3 border border-line font-mono text-[14px] flex items-center gap-2 hover:bg-surface-2",
                title: "{shown_root}",
                "aria-expanded": "{menu}",
                onclick: move |_| {
                    root.set(hone.read().root().display().to_string());
                    menu.toggle();
                },
                "{folder}"
                span { "aria-hidden": "true", "▾" }
            }
            if menu() {
                div { class: "absolute top-16 left-24 z-20 w-[480px] bg-bg border border-line p-4 flex flex-col gap-3 shadow-none",
                    span { class: "font-mono text-[12px] text-ink-muted",
                        {project.project_id().unwrap_or_else(|| "no project".to_owned())}
                    }
                    form {
                        class: "flex items-center gap-2",
                        onsubmit: move |event| {
                            event.prevent_default();
                            open_root();
                        },
                        Input {
                            class: "font-mono h-9",
                            value: root(),
                            placeholder: "Project root, the directory that contains .hone/",
                            oninput: move |value| root.set(value),
                        }
                        Button { variant: ButtonVariant::Default, size: ButtonSize::Sm, class: "h-9",
                            onclick: move |_| open_root(),
                            "Open"
                        }
                    }
                    Button { variant: ButtonVariant::Outline,
                        onclick: move |_| {
                            let mut screen = session.screen;
                            screen.set(Screen::Settings);
                            menu.set(false);
                        },
                        "Project settings: IDE, packages, tracker"
                    }
                }
            }
            nav { class: "flex items-stretch h-full", "aria-label": "Views",
                for (screen , label) in views {
                    button {
                        class: if current == screen { "px-4 font-semibold shadow-[inset_0_-3px_0_var(--hone-ink)]" } else { "px-4 text-ink-muted hover:text-ink" },
                        "aria-current": if current == screen { "page" } else { "false" },
                        onclick: move |_| {
                            let mut target = session.screen;
                            if screen == Screen::Artifacts {
                                let mut artifact = session.artifact;
                                artifact.set(None);
                            }
                            target.set(screen);
                        },
                        "{label}"
                    }
                }
            }
            div { class: "ml-auto flex items-center gap-4",
                if let Some(ticket) = ticket {
                    {
                        let finished = summary.as_ref().map_or(0, |summary| summary.steps.iter().filter(|step| step.status == "done" || step.status == "skipped").count());
                        let total = summary.as_ref().map_or(0, |summary| summary.steps.len());
                        let title = summary.as_ref().and_then(|summary| summary.title.clone()).unwrap_or_default();
                        let chip = if current == Screen::Ticket { "h-10 px-4 flex items-center gap-3 bg-ink text-bg max-w-[460px]" } else { "h-10 px-4 flex items-center gap-3 border border-line hover:bg-surface-2 max-w-[460px]" };
                        let opened = ticket.clone();
                        rsx! {
                            button { class: chip, title: "Open the ticket's workspace", onclick: move |_| session.open_ticket(opened.clone()),
                                span { class: "font-mono text-[13px] opacity-80", "{ticket}" }
                                if !title.is_empty() {
                                    span { class: "font-semibold truncate", "{title}" }
                                }
                                span { class: "px-1.5 bg-signal text-on-signal font-mono text-[12px]", "{finished}/{total}" }
                            }
                        }
                    }
                }
                button {
                    class: "h-10 px-3 flex items-center gap-3 border border-line text-[14px] text-ink-muted hover:bg-surface-2 min-w-[200px]",
                    title: "Search tickets, artifacts, threads, and the tracker",
                    onclick: move |_| {
                        let mut open = session.search_open;
                        open.set(true);
                    },
                    "Search"
                    span { class: "ml-auto font-mono text-[11px] border border-line px-1", "Ctrl K" }
                }
                WriterPicker {}
            }
        }
    }
}

/// The author and role the GUI writes as. Empty fields leave the author to the project's
/// context.
#[component]
fn WriterPicker() -> Element {
    let session = use_context::<Session>();
    let project = use_context::<Project>();
    let mut writer = session.writer;
    let context_author = match &*project.context.read() {
        Some(Ok(context)) => context.author.clone(),
        _ => None,
    };
    let placeholder_author = context_author
        .as_ref()
        .map_or_else(|| "author".to_owned(), |author| author.name.clone());
    let placeholder_role = context_author
        .as_ref()
        .map_or_else(|| "role".to_owned(), |author| author.role.clone());
    let roles = project.role_choices();
    let role = writer.read().role.clone();
    let unknown_role =
        !role.is_empty() && !roles.is_empty() && !roles.iter().any(|choice| choice.value == role);
    // A write needs both or neither: with one alone, the context's author would write.
    let half_author = writer.read().author.is_empty() && !role.is_empty();
    let half_role = !writer.read().author.is_empty() && role.is_empty();
    rsx! {
        div { class: "flex items-center gap-2",
            div { class: "w-40",
                Combobox {
                    value: writer.read().author.clone(),
                    choices: project.author_choices(),
                    placeholder: placeholder_author,
                    label: "Author",
                    custom: true,
                    invalid: half_author,
                    empty: "Type a new handle and press Enter.",
                    onselect: move |name: String| {
                        // A known author brings the role they last wrote with, when it fits the ticket.
                        let role = project.last_role(&name).filter(|role| {
                            writer.read().role.is_empty()
                                && project.role_choices().iter().any(|choice| &choice.value == role)
                        });
                        writer.with_mut(|writer: &mut Writer| {
                            writer.author = name;
                            if let Some(role) = role {
                                writer.role = role;
                            }
                        });
                    },
                }
            }
            span { class: "text-ink-muted font-mono", "·" }
            div { class: "w-36",
                Combobox {
                    value: role.clone(),
                    choices: roles,
                    placeholder: placeholder_role,
                    label: "Role",
                    custom: true,
                    invalid: unknown_role || half_role,
                    empty: "Open a ticket to see the roles of its configuration.",
                    onselect: move |role: String| writer.with_mut(|writer: &mut Writer| writer.role = role),
                }
            }
            if !writer.read().author.is_empty() || !writer.read().role.is_empty() {
                button {
                    class: "text-[13px] underline text-ink-muted hover:text-ink",
                    title: "Write as the author of the project's context",
                    onclick: move |_| writer.set(Writer { author: String::new(), role: String::new() }),
                    "Clear"
                }
            }
        }
    }
}
