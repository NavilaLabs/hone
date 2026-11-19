use dioxus::prelude::*;
use tw_merge::tw_merge;

/// The frame around a [`Table`], scrolling when it is too wide.
#[component]
pub fn TableWrapper(#[props(into, optional)] class: Option<String>, children: Element) -> Element {
    let merged = tw_merge!(
        "overflow-auto rounded-md border",
        class.as_deref().unwrap_or("")
    );
    rsx! {
        div { "data-name": "TableWrapper", class: "{merged}", {children} }
    }
}

/// A table.
#[component]
pub fn Table(#[props(into, optional)] class: Option<String>, children: Element) -> Element {
    let merged = tw_merge!(
        "w-full text-sm caption-bottom",
        class.as_deref().unwrap_or("")
    );
    rsx! {
        table { class: "{merged}", {children} }
    }
}

/// The head of a [`Table`].
#[component]
pub fn TableHeader(#[props(into, optional)] class: Option<String>, children: Element) -> Element {
    let merged = tw_merge!(
        "[&_tr]:border-b sticky top-0 z-10 bg-card",
        class.as_deref().unwrap_or("")
    );
    rsx! {
        thead { class: "{merged}", {children} }
    }
}

/// The body of a [`Table`].
#[component]
pub fn TableBody(#[props(into, optional)] class: Option<String>, children: Element) -> Element {
    let merged = tw_merge!("[&_tr:last-child]:border-0", class.as_deref().unwrap_or(""));
    rsx! {
        tbody { class: "{merged}", {children} }
    }
}

/// A row. `selected` marks it with `data-state="selected"`.
#[component]
pub fn TableRow(
    #[props(into, optional)] class: Option<String>,
    #[props(optional)] selected: bool,
    #[props(optional)] onclick: Option<EventHandler<MouseEvent>>,
    children: Element,
) -> Element {
    let merged = tw_merge!(
        "border-b transition-colors data-[state=selected]:bg-muted hover:bg-muted/50",
        class.as_deref().unwrap_or("")
    );
    rsx! {
        tr {
            class: "{merged}",
            "data-state": if selected { "selected" } else { "" },
            "aria-selected": "{selected}",
            onclick: move |event| {
                if let Some(handler) = &onclick {
                    handler.call(event);
                }
            },
            {children}
        }
    }
}

/// A head cell.
#[component]
pub fn TableHead(#[props(into, optional)] class: Option<String>, children: Element) -> Element {
    let merged = tw_merge!(
        "h-10 px-2 text-left align-middle font-medium text-muted-foreground [&:has([role=checkbox])]:pr-0 [&>[role=checkbox]]:translate-y-[2px]",
        class.as_deref().unwrap_or("")
    );
    rsx! {
        th { class: "{merged}", {children} }
    }
}

/// A body cell.
#[component]
pub fn TableCell(
    #[props(into, optional)] class: Option<String>,
    #[props(optional)] colspan: Option<i64>,
    children: Element,
) -> Element {
    // rust-ui's second selector lacks its leading `[` and has no effect; it is fixed here.
    let merged = tw_merge!(
        "p-4 align-middle [&:has([role=checkbox])]:pr-0 [&:has([role=checkbox])]:pl-3",
        class.as_deref().unwrap_or("")
    );
    rsx! {
        td { class: "{merged}", colspan, {children} }
    }
}
