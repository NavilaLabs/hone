//! The assistant panel. The agent behind it (ACP with `hone mcp`, concept Section 12) is not
//! built yet; the panel shows where it will sit and what context a message would carry.

use dioxus::prelude::*;

use super::{Project, Session};
use crate::ui::{Button, ButtonVariant};

/// The panel at the right edge of a ticket's workspace.
#[component]
pub(crate) fn AssistantPanel() -> Element {
    let session = use_context::<Session>();
    let project = use_context::<Project>();
    let mut open = session.assistant_open;
    let ticket = (session.ticket)().unwrap_or_default();
    let current = project.summary(&ticket).and_then(|summary| summary.current);
    rsx! {
        aside { class: "w-[340px] shrink-0 border-l border-line flex flex-col min-h-0",
            div { class: "h-12 px-4 border-b border-line flex items-center gap-2",
                span { class: "font-mono text-[11px] font-semibold tracking-[0.08em] uppercase", "Assistant" }
                span { class: "font-mono text-[11px] text-ink-muted", "via hone mcp" }
                Button { variant: ButtonVariant::Ghost, class: "ml-auto h-8 px-2",
                    title: "Close the assistant",
                    onclick: move |_| open.set(false),
                    "×"
                }
            }
            div { class: "flex-1 min-h-0 overflow-y-auto p-4 flex flex-col gap-3 text-[14px]",
                p { class: "m-0 text-ink-muted",
                    "The assistant is not connected yet. It will talk to an agent of your choice over the Agent Client Protocol, which reaches hone only through hone mcp: it reads the ticket and writes only through the same commands and gates as you."
                }
            }
            div { class: "border-t border-line p-4 flex flex-col gap-3",
                div { class: "flex flex-wrap gap-2 font-mono text-[12px]",
                    span { class: "px-2 h-6 inline-flex items-center border border-line", "{ticket}" }
                    if let Some(step) = current {
                        span { class: "px-2 h-6 inline-flex items-center border border-line", "step {step}" }
                    }
                }
                textarea {
                    class: "h-24 p-3 border border-line bg-bg text-ink text-[14px] resize-none disabled:opacity-60",
                    disabled: true,
                    placeholder: "Ask about this ticket",
                    "aria-label": "Message to the assistant",
                }
                div { class: "flex items-center gap-3",
                    span { class: "font-mono text-[11px] text-ink-muted flex-1", "reads the ticket · writes only via gates" }
                    Button { variant: ButtonVariant::Outline, disabled: true, "Send" }
                }
            }
        }
    }
}
