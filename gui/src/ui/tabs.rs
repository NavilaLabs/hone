use dioxus::prelude::*;
use tw_merge::tw_merge;

/// The look of a [`TabsList`].
#[derive(Clone, Copy, PartialEq, Eq, Default)]
pub enum TabsVariant {
    #[default]
    Default,
    /// An underline under the active tab, as hone's design asks for.
    Line,
}

#[derive(Clone, Copy)]
struct TabsContext {
    active: Signal<String>,
}

#[derive(Clone, Copy)]
struct TabsListContext {
    variant: TabsVariant,
}

/// Tabs that filter one list. `active` holds the value of the active tab.
#[component]
pub fn Tabs(
    #[props(into, optional)] class: Option<String>,
    active: Signal<String>,
    children: Element,
) -> Element {
    provide_context(TabsContext { active });
    let class = tw_merge!(
        "group/tabs flex gap-2",
        "flex-col",
        class.as_deref().unwrap_or("")
    );
    rsx! {
        div {
            class: "{class}",
            "data-name": "Tabs",
            "data-orientation": "Horizontal",
            {children}
        }
    }
}

/// The row of [`TabsTrigger`]s.
#[component]
pub fn TabsList(
    #[props(into, optional)] class: Option<String>,
    #[props(default)] variant: TabsVariant,
    children: Element,
) -> Element {
    provide_context(TabsListContext { variant });
    let is_line = variant == TabsVariant::Line;
    let class = tw_merge!(
        "group/tabs-list inline-flex w-fit items-center justify-center rounded-lg p-[3px] text-muted-foreground",
        "group-data-[orientation=Horizontal]/tabs:h-8",
        if is_line {
            "gap-1 bg-transparent rounded-none p-0"
        } else {
            "bg-muted"
        },
        class.as_deref().unwrap_or("")
    );
    rsx! {
        div {
            class: "{class}",
            role: "tablist",
            "data-name": "TabsList",
            "data-variant": if is_line { "Line" } else { "Default" },
            {children}
        }
    }
}

/// One tab.
#[component]
pub fn TabsTrigger(
    #[props(into)] value: String,
    #[props(into, optional)] class: Option<String>,
    children: Element,
) -> Element {
    let TabsContext { mut active } = use_context::<TabsContext>();
    let list = use_context::<TabsListContext>();
    let is_line = list.variant == TabsVariant::Line;
    let is_active = active() == value;
    let merged = tw_merge!(
        "relative inline-flex h-[calc(100%-1px)] flex-1 items-center justify-center gap-1.5 rounded-md border border-transparent px-1.5 py-0.5 text-sm font-medium whitespace-nowrap transition-all cursor-pointer select-none",
        "focus-visible:border-ring focus-visible:ring-[3px] focus-visible:ring-ring/50 focus-visible:outline-none",
        "disabled:pointer-events-none disabled:opacity-50",
        "after:absolute after:bg-foreground after:transition-opacity",
        "group-data-[orientation=Horizontal]/tabs:after:inset-x-0 group-data-[orientation=Horizontal]/tabs:after:bottom-[-5px] group-data-[orientation=Horizontal]/tabs:after:h-0.5",
        if is_active {
            "text-foreground"
        } else {
            "text-foreground/60 hover:text-foreground"
        },
        if !is_line && is_active {
            "bg-background shadow-sm dark:border-input dark:bg-input/30"
        } else {
            ""
        },
        if is_line && is_active {
            "after:opacity-100"
        } else {
            "after:opacity-0"
        },
        class.as_deref().unwrap_or("")
    );
    rsx! {
        button {
            class: "{merged}",
            role: "tab",
            "aria-selected": "{is_active}",
            "data-name": "TabsTrigger",
            "data-state": if is_active { "Active" } else { "Inactive" },
            onclick: move |_| active.set(value.clone()),
            {children}
        }
    }
}

/// The content shown while the tab with the same `value` is active.
#[component]
pub fn TabsContent(
    #[props(into)] value: String,
    #[props(into, optional)] class: Option<String>,
    children: Element,
) -> Element {
    let TabsContext { active } = use_context::<TabsContext>();
    let is_active = active() == value;
    let class = tw_merge!(
        "flex-1 text-sm outline-none",
        "group-data-[orientation=Horizontal]/tabs:mt-2",
        if is_active { "" } else { "hidden" },
        class.as_deref().unwrap_or("")
    );
    rsx! {
        div { class: "{class}", "data-name": "TabsContent", {children} }
    }
}
