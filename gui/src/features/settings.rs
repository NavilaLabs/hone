use dioxus::prelude::*;

use super::{Project, Session};
use crate::hone::{Hone, Writer};
use crate::ui::{Button, ButtonSize, ButtonVariant, Combobox, Input};

const LOGO_LIGHT: &str = include_str!("../../assets/logo-light.svg");
const LOGO_DARK: &str = include_str!("../../assets/logo-dark.svg");

/// The top bar: the logo and wordmark, the project, the writer, and the theme.
#[component]
pub(crate) fn Header() -> Element {
    let session = use_context::<Session>();
    let project = use_context::<Project>();
    let mut hone = session.hone;
    let mut editing = use_signal(|| false);
    let mut root = use_signal(|| hone.read().root().display().to_string());
    let mut theme = use_signal(|| "system");
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
        editing.set(false);
        session.changed();
    };
    rsx! {
        header { class: "min-h-14 border-b border-line px-6 py-2 flex flex-wrap items-center gap-x-6 gap-y-2",
            span { class: "flex items-center gap-2 text-[22px] font-bold tracking-[-0.01em]",
                span { class: "hone-logo-light size-6 [&>svg]:size-6", "aria-hidden": "true", dangerous_inner_html: LOGO_LIGHT }
                span { class: "hone-logo-dark size-6 [&>svg]:size-6", "aria-hidden": "true", dangerous_inner_html: LOGO_DARK }
                "hone"
            }
            if editing() {
                form {
                    class: "flex items-center gap-2 flex-1 min-w-[280px]",
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
                    Button { variant: ButtonVariant::Ghost, size: ButtonSize::Sm, class: "h-9",
                        onclick: move |_| editing.set(false),
                        "Cancel"
                    }
                }
            } else {
                div { class: "flex items-baseline gap-2 font-mono text-[13px] text-ink-muted min-w-0 flex-1",
                    span { {project.project_id().unwrap_or_else(|| "no project".to_owned())} }
                    span { "/" }
                    span { class: "text-ink truncate", title: "{shown_root}", "{folder}" }
                    button {
                        class: "ml-2 font-sans text-[13px] underline text-ink-muted hover:text-ink",
                        onclick: move |_| {
                            root.set(hone.read().root().display().to_string());
                            editing.set(true);
                        },
                        "Change"
                    }
                }
            }
            WriterPicker {}
            Button {
                variant: ButtonVariant::Ghost,
                size: ButtonSize::Sm,
                title: "Switch between system, light, and dark",
                onclick: move |_| {
                    let next = match theme() {
                        "system" => "light",
                        "light" => "dark",
                        _ => "system",
                    };
                    theme.set(next);
                    let script = if next == "system" {
                        "document.documentElement.removeAttribute('data-theme')".to_owned()
                    } else {
                        format!("document.documentElement.setAttribute('data-theme', '{next}')")
                    };
                    document::eval(&script);
                },
                "Theme: {theme}"
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
            span { class: "text-[13px] text-ink-muted", "Writing as" }
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
