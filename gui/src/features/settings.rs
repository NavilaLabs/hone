use dioxus::prelude::*;

use super::Session;
use crate::hone::{Hone, Writer};
use crate::ui::{Button, ButtonSize, ButtonVariant, Input};

/// The top bar: the wordmark, the project root, the writer, and the theme.
#[component]
pub(crate) fn Header() -> Element {
    let session = use_context::<Session>();
    let mut hone = session.hone;
    let mut writer = session.writer;
    let mut root = use_signal(|| hone.read().root().display().to_string());
    let mut theme = use_signal(|| "system");
    rsx! {
        header { class: "min-h-14 border-b border-line px-6 py-2 flex flex-wrap items-center gap-6",
            span { class: "text-[22px] font-bold tracking-[-0.01em]",
                "hone"
                span { class: "text-signal", "_" }
            }
            form {
                class: "flex items-center gap-2 flex-1 min-w-[280px]",
                onsubmit: move |event| {
                    event.prevent_default();
                    hone.set(Hone::new(root()));
                    session.changed();
                },
                Input {
                    class: "font-mono h-9",
                    value: root(),
                    placeholder: "Project root",
                    oninput: move |value| root.set(value),
                }
                Button { variant: ButtonVariant::Outline, size: ButtonSize::Sm, class: "h-9",
                    onclick: move |_| {
                        hone.set(Hone::new(root()));
                        session.changed();
                    },
                    "Open"
                }
            }
            div { class: "flex items-center gap-2",
                Input {
                    class: "font-mono h-9 w-36",
                    value: writer.read().author.clone(),
                    placeholder: "author",
                    oninput: move |value| writer.with_mut(|writer: &mut Writer| writer.author = value),
                }
                span { class: "text-ink-muted font-mono", "·" }
                Input {
                    class: "font-mono h-9 w-32",
                    value: writer.read().role.clone(),
                    placeholder: "role",
                    oninput: move |value| writer.with_mut(|writer: &mut Writer| writer.role = value),
                }
            }
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
