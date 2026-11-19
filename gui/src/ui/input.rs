use dioxus::prelude::*;
use tw_merge::tw_merge;

/// A one-line text field. `oninput` receives the new value.
#[component]
pub fn Input(
    #[props(into, optional)] class: Option<String>,
    #[props(into, optional)] placeholder: Option<String>,
    #[props(into, optional)] id: Option<String>,
    #[props(into)] value: String,
    #[props(optional)] disabled: bool,
    #[props(optional)] oninput: Option<EventHandler<String>>,
) -> Element {
    let merged_class = tw_merge!(
        "text-foreground file:text-foreground placeholder:text-muted-foreground selection:bg-primary selection:text-primary-foreground dark:bg-input/30 border-input flex h-9 w-full min-w-0 rounded-md border bg-transparent px-3 py-1 text-base shadow-xs transition-[color,box-shadow] outline-none file:inline-flex file:h-7 file:border-0 file:bg-transparent file:text-sm file:font-medium disabled:pointer-events-none disabled:cursor-not-allowed disabled:opacity-50 md:text-sm",
        "focus-visible:border-ring focus-visible:ring-ring/50",
        "focus-visible:ring-2",
        "aria-invalid:ring-destructive/20 dark:aria-invalid:ring-destructive/40 aria-invalid:border-destructive",
        "read-only:bg-muted",
        class.as_deref().unwrap_or("")
    );
    rsx! {
        input {
            "data-name": "Input",
            r#type: "text",
            class: "{merged_class}",
            placeholder,
            value,
            id,
            disabled,
            oninput: move |event| {
                if let Some(handler) = &oninput {
                    handler.call(event.value());
                }
            },
        }
    }
}
