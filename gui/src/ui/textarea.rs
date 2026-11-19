use dioxus::prelude::*;
use tw_merge::tw_merge;

/// A multi-line text field. `oninput` receives the new value.
#[component]
pub fn Textarea(
    #[props(into, optional)] class: Option<String>,
    #[props(into, optional)] id: Option<String>,
    #[props(into, optional)] placeholder: Option<String>,
    #[props(into)] value: String,
    #[props(default = 4)] rows: i64,
    #[props(optional)] oninput: Option<EventHandler<String>>,
) -> Element {
    let class = tw_merge!(
        "border-input placeholder:text-muted-foreground focus-visible:border-ring focus-visible:ring-ring/50 aria-invalid:ring-destructive/20 dark:aria-invalid:ring-destructive/40 aria-invalid:border-destructive dark:bg-input/30 flex field-sizing-content min-h-16 w-full rounded-md border bg-transparent px-3 py-2 text-base shadow-xs transition-[color,box-shadow] outline-none focus-visible:ring-2 disabled:cursor-not-allowed disabled:opacity-50 md:text-sm",
        class.as_deref().unwrap_or("")
    );
    rsx! {
        textarea {
            class: "{class}",
            id: id.as_deref(),
            placeholder: placeholder.as_deref().unwrap_or(""),
            rows,
            value,
            oninput: move |event| {
                if let Some(handler) = &oninput {
                    handler.call(event.value());
                }
            },
        }
    }
}
