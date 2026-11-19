use dioxus::prelude::*;
use tw_merge::tw_merge;

/// A checkbox. `on_checked_change` receives the new state.
#[component]
pub fn Checkbox(
    #[props(into, optional)] class: Option<String>,
    #[props(default = false)] checked: bool,
    #[props(default = false)] disabled: bool,
    #[props(optional)] on_checked_change: Option<EventHandler<bool>>,
    #[props(default = "Checkbox".to_string(), into)] aria_label: String,
) -> Element {
    let checked_state = if checked { "checked" } else { "unchecked" };
    let merged = tw_merge!(
        "peer border-input dark:bg-input/30 data-[state=checked]:bg-primary data-[state=checked]:text-primary-foreground dark:data-[state=checked]:bg-primary data-[state=checked]:border-primary focus-visible:border-ring focus-visible:ring-ring/50 aria-invalid:ring-destructive/20 dark:aria-invalid:ring-destructive/40 aria-invalid:border-destructive size-4 shrink-0 rounded-[4px] border shadow-xs transition-shadow outline-none focus-visible:ring-[3px] disabled:cursor-not-allowed disabled:opacity-50",
        class.as_deref().unwrap_or("")
    );
    rsx! {
        button {
            "data-name": "Checkbox",
            class: "{merged}",
            "data-state": checked_state,
            r#type: "button",
            role: "checkbox",
            "aria-checked": "{checked}",
            "aria-label": "{aria_label}",
            disabled,
            onclick: move |_| {
                if !disabled && let Some(handler) = &on_checked_change {
                    handler.call(!checked);
                }
            },
            span {
                "data-name": "CheckboxIndicator",
                class: "flex justify-center items-center text-current transition-none",
                if checked {
                    svg {
                        class: "size-3.5",
                        view_box: "0 0 24 24",
                        fill: "none",
                        stroke: "currentColor",
                        stroke_width: "3",
                        path { d: "M20 6 9 17l-5-5" }
                    }
                }
            }
        }
    }
}
