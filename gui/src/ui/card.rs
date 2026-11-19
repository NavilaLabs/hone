use dioxus::prelude::*;
use tw_merge::tw_merge;

/// A framed block, a panel in hone's design.
#[component]
pub fn Card(#[props(into, optional)] class: Option<String>, children: Element) -> Element {
    let merged_class = tw_merge!(
        "bg-card text-card-foreground flex flex-col rounded-xl border shadow-sm",
        "py-6 gap-4",
        class.as_deref().unwrap_or("")
    );
    rsx! {
        div { "data-name": "Card", class: "{merged_class}", {children} }
    }
}

/// The head of a [`Card`].
#[component]
pub fn CardHeader(#[props(into, optional)] class: Option<String>, children: Element) -> Element {
    let merged_class = tw_merge!(
        "@container/card-header flex flex-col items-start gap-1.5 px-6 [.border-b]:pb-6",
        class.as_deref().unwrap_or("")
    );
    rsx! {
        div { "data-name": "CardHeader", class: "{merged_class}", {children} }
    }
}

/// The title in a [`CardHeader`].
#[component]
pub fn CardTitle(#[props(into, optional)] class: Option<String>, children: Element) -> Element {
    let merged_class = tw_merge!("leading-none font-semibold", class.as_deref().unwrap_or(""));
    rsx! {
        h2 { "data-name": "CardTitle", class: "{merged_class}", {children} }
    }
}

/// The body of a [`Card`].
#[component]
pub fn CardContent(#[props(into, optional)] class: Option<String>, children: Element) -> Element {
    let merged_class = tw_merge!("px-6", class.as_deref().unwrap_or(""));
    rsx! {
        div { "data-name": "CardContent", class: "{merged_class}", {children} }
    }
}
