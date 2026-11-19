use dioxus::prelude::*;
use tw_merge::tw_merge;

/// A message that needs attention, with `role="alert"`.
#[component]
pub fn Alert(#[props(into, optional)] class: Option<String>, children: Element) -> Element {
    let class = tw_merge!(
        "relative w-full rounded-lg border px-4 py-3 text-sm [&>svg+div]:translate-y-[-3px] [&>svg]:absolute [&>svg]:left-4 [&>svg]:top-4 [&>svg]:text-foreground [&>svg~*]:pl-7",
        "bg-background text-foreground",
        class.as_deref().unwrap_or("")
    );
    rsx! {
        div { role: "alert", class: "{class}", {children} }
    }
}

/// The title of an [`Alert`].
#[component]
pub fn AlertTitle(#[props(into, optional)] class: Option<String>, children: Element) -> Element {
    let class = tw_merge!(
        "mb-1 font-medium tracking-tight leading-none",
        class.as_deref().unwrap_or("")
    );
    rsx! {
        h4 { class: "{class}", {children} }
    }
}

/// The text of an [`Alert`].
#[component]
pub fn AlertDescription(
    #[props(into, optional)] class: Option<String>,
    children: Element,
) -> Element {
    let class = tw_merge!(
        "text-sm [&_p]:leading-relaxed",
        class.as_deref().unwrap_or("")
    );
    rsx! {
        p { class: "{class}", {children} }
    }
}
