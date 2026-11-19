use dioxus::prelude::*;
use tw_merge::tw_merge;

/// The look of a [`Badge`].
#[derive(Default, Clone, PartialEq, Eq)]
pub enum BadgeVariant {
    #[default]
    Default,
    Outline,
}

impl BadgeVariant {
    const fn as_str(&self) -> &'static str {
        match self {
            Self::Default => {
                "border-transparent shadow bg-primary text-primary-foreground hover:bg-primary/80"
            }
            Self::Outline => "text-foreground",
        }
    }
}

/// A short label, a tag in hone's design.
#[component]
pub fn Badge(
    #[props(into, optional)] class: Option<String>,
    #[props(default)] variant: BadgeVariant,
    children: Element,
) -> Element {
    let merged = tw_merge!(
        "inline-flex items-center font-semibold rounded-md border transition-colors focus:outline-hidden focus:ring-2 focus:ring-ring focus:ring-offset-2 w-fit",
        variant.as_str(),
        "px-2.5 py-0.5 text-xs",
        class.as_deref().unwrap_or("")
    );
    rsx! {
        span { class: "{merged}", {children} }
    }
}
