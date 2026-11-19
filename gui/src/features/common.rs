use dioxus::prelude::*;

use crate::hone::Failure;
use crate::ui::{Alert, AlertDescription, AlertTitle, Skeleton};

/// The head of a screen: a context line, the title, and the actions on the right.
#[component]
pub(crate) fn PageHeader(
    #[props(into, optional)] context: Option<String>,
    #[props(into)] title: String,
    #[props(optional)] actions: Option<Element>,
) -> Element {
    rsx! {
        header { class: "flex flex-wrap items-end justify-between gap-6",
            div { class: "flex flex-col gap-2 min-w-0",
                if let Some(context) = context {
                    span { class: "font-mono text-[13px] text-ink-muted break-all", "{context}" }
                }
                h1 { class: "m-0 text-[32px] leading-[37px] font-bold tracking-[-0.02em] break-words",
                    "{title}"
                }
            }
            if let Some(actions) = actions {
                div { class: "flex flex-wrap gap-2", {actions} }
            }
        }
    }
}

/// An uppercase section label with optional text on the right.
#[component]
pub(crate) fn SectionLabel(
    #[props(into)] label: String,
    #[props(optional)] aside: Option<String>,
) -> Element {
    rsx! {
        div { class: "flex items-baseline justify-between gap-4 pb-3",
            h2 { class: "m-0 font-mono text-[11px] leading-4 font-medium tracking-[0.08em] uppercase",
                "{label}"
            }
            if let Some(aside) = aside {
                span { class: "text-[13px] text-ink-muted", "{aside}" }
            }
        }
    }
}

/// The meaning a [`StatusMark`] carries.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Mark {
    Done,
    Open,
    Fail,
    Skip,
}

/// A state as a glyph and a word; colour only reinforces it.
#[component]
pub(crate) fn StatusMark(mark: Mark, #[props(into)] word: String) -> Element {
    let (glyph, colour) = match mark {
        Mark::Done => ("■", "text-ok"),
        Mark::Open => ("□", "text-ink"),
        Mark::Fail => ("✕", "text-bad"),
        Mark::Skip => ("⁄", "text-ink-muted line-through"),
    };
    rsx! {
        span { class: "font-mono text-[12.5px] leading-[18px] inline-flex gap-1 items-baseline {colour}",
            span { "aria-hidden": "true", "{glyph}" }
            "{word}"
        }
    }
}

/// A failed call of the command line: what did not run and why.
#[component]
pub(crate) fn FailureAlert(failure: Failure) -> Element {
    rsx! {
        Alert { class: "border-2 border-signal",
            AlertTitle { "hone could not run ({failure.status})" }
            AlertDescription { class: "font-mono text-[13px] break-words", "{failure.message}" }
        }
    }
}

/// Rows of full height while data loads.
#[component]
pub(crate) fn Loading() -> Element {
    rsx! {
        div { class: "flex flex-col gap-2",
            Skeleton { class: "h-11 w-full" }
            Skeleton { class: "h-11 w-full" }
            Skeleton { class: "h-11 w-full" }
        }
    }
}

/// A sentence saying what will appear here and what makes it appear.
#[component]
pub(crate) fn Empty(#[props(into)] text: String) -> Element {
    rsx! {
        p { class: "text-ink-muted m-0", "{text}" }
    }
}

/// A labelled field.
#[component]
pub(crate) fn Field(#[props(into)] label: String, children: Element) -> Element {
    rsx! {
        label { class: "flex flex-col gap-1 min-w-0 text-[13px] text-ink-muted",
            "{label}"
            {children}
        }
    }
}
