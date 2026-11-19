use dioxus::prelude::*;
use tw_merge::tw_merge;

/// A horizontal hairline.
#[component]
pub fn Separator(#[props(into, optional)] class: Option<String>) -> Element {
    let merged_class = tw_merge!(
        "shrink-0 bg-border",
        "w-full h-[1px]",
        class.as_deref().unwrap_or("")
    );
    rsx! {
        div { class: "{merged_class}", role: "separator" }
    }
}
