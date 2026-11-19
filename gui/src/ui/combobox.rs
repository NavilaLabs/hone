use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use dioxus::prelude::*;
use tw_merge::tw_merge;

/// A value a [`Combobox`] offers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Choice {
    /// What is selected, shown in mono.
    pub value: String,
    /// Text beside the value, for example a title. Searched like the value.
    pub label: Option<String>,
    /// A short note on the right, for example a status or a kind.
    pub note: Option<String>,
}

impl Choice {
    /// A choice that is only its value.
    pub fn plain(value: impl Into<String>) -> Self {
        Self {
            value: value.into(),
            label: None,
            note: None,
        }
    }

    /// The same choice with `label` beside the value.
    #[must_use]
    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.label = Some(label.into()).filter(|label| !label.is_empty());
        self
    }

    /// The same choice with `note` on the right.
    #[must_use]
    pub fn note(mut self, note: impl Into<String>) -> Self {
        self.note = Some(note.into()).filter(|note| !note.is_empty());
        self
    }

    fn matches(&self, query: &str) -> bool {
        let query = query.trim().to_lowercase();
        query.is_empty()
            || [Some(&self.value), self.label.as_ref(), self.note.as_ref()]
                .into_iter()
                .flatten()
                .any(|text| text.to_lowercase().contains(&query))
    }
}

/// The choices that contain `query` in their value, label, or note, ignoring case.
pub fn filter_choices<'a>(choices: &'a [Choice], query: &str) -> Vec<&'a Choice> {
    choices
        .iter()
        .filter(|choice| choice.matches(query))
        .collect()
}

static NEXT_ID: AtomicUsize = AtomicUsize::new(0);

/// A text field with a list of choices that filters as one types. Arrow keys move through the
/// list, Enter selects, Escape closes. With `custom`, Enter on text that is no choice selects
/// that text, so a new value can be entered.
#[component]
pub fn Combobox(
    #[props(into)] value: String,
    choices: Vec<Choice>,
    #[props(into, optional)] placeholder: Option<String>,
    #[props(into, optional)] class: Option<String>,
    #[props(into, optional)] label: Option<String>,
    #[props(optional)] custom: bool,
    /// What the list says when nothing matches.
    #[props(into, optional)]
    empty: Option<String>,
    onselect: EventHandler<String>,
) -> Element {
    let id = use_hook(|| format!("combobox-{}", NEXT_ID.fetch_add(1, Ordering::Relaxed)));
    let mut query = use_signal(|| None::<String>);
    let mut open = use_signal(|| false);
    let mut active = use_signal(|| 0usize);
    let typed = query().unwrap_or_default();
    let shown: Vec<Choice> = filter_choices(&choices, &typed)
        .into_iter()
        .cloned()
        .collect();
    let new_value = (custom
        && !typed.trim().is_empty()
        && !choices.iter().any(|choice| choice.value == typed.trim()))
    .then(|| typed.trim().to_owned());
    let rows = shown.len() + usize::from(new_value.is_some());
    let highlighted = if rows == 0 { 0 } else { active().min(rows - 1) };
    let mut choose = move |chosen: String| {
        query.set(None);
        open.set(false);
        onselect.call(chosen);
    };
    let keyed_shown = shown.clone();
    let keyed_new = new_value.clone();
    let text = query().unwrap_or_else(|| value.clone());
    let selected_index = choices.iter().position(|choice| choice.value == value);
    let mut reveal = move || {
        if !open() {
            active.set(selected_index.unwrap_or(0));
            open.set(true);
        }
    };
    let list = format!("{id}-list");
    let field = tw_merge!(
        "h-9 w-full min-w-0 border border-input bg-transparent px-3 pr-8 font-mono text-[13px] text-foreground placeholder:text-muted-foreground outline-none dark:bg-input/30",
        class.as_deref().unwrap_or("")
    );
    rsx! {
        div {
            class: "relative min-w-0",
            onfocusout: move |_| {
                // A click on a choice blurs the field first; closing later lets the click land.
                spawn(async move {
                    tokio::time::sleep(Duration::from_millis(150)).await;
                    open.set(false);
                    query.set(None);
                });
            },
            input {
                r#type: "text",
                role: "combobox",
                class: "{field}",
                id: "{id}",
                autocomplete: "off",
                spellcheck: "false",
                "aria-label": label,
                "aria-expanded": if open() { "true" } else { "false" },
                "aria-controls": "{list}",
                "aria-autocomplete": "list",
                "aria-activedescendant": if open() && rows > 0 { format!("{id}-{highlighted}") } else { String::new() },
                placeholder,
                value: "{text}",
                onfocus: move |_| reveal(),
                onclick: move |_| reveal(),
                oninput: move |event| {
                    query.set(Some(event.value()));
                    active.set(0);
                    open.set(true);
                },
                onkeydown: move |event| match event.key() {
                    Key::ArrowDown => {
                        event.prevent_default();
                        if open() && rows > 0 {
                            active.set((highlighted + 1) % rows);
                        }
                        open.set(true);
                    }
                    Key::ArrowUp => {
                        event.prevent_default();
                        if rows > 0 {
                            active.set((highlighted + rows - 1) % rows);
                        }
                        open.set(true);
                    }
                    Key::Enter => {
                        event.prevent_default();
                        if !open() {
                            open.set(true);
                        } else if let Some(choice) = keyed_shown.get(highlighted) {
                            choose(choice.value.clone());
                        } else if let Some(new) = keyed_new.clone() {
                            choose(new);
                        }
                    }
                    Key::Escape => {
                        open.set(false);
                        query.set(None);
                    }
                    Key::Tab => open.set(false),
                    _ => {}
                },
            }
            span {
                class: "pointer-events-none absolute right-3 top-1/2 -translate-y-1/2 text-ink-muted text-[10px]",
                "aria-hidden": "true",
                "▾"
            }
            if open() {
                ul {
                    id: "{list}",
                    role: "listbox",
                    class: "absolute left-0 top-full z-30 mt-1 max-h-72 min-w-full w-max max-w-[min(560px,90vw)] overflow-y-auto border border-line bg-bg p-0 m-0 list-none",
                    for (index , choice) in shown.into_iter().enumerate() {
                        li {
                            key: "{choice.value}",
                            id: "{id}-{index}",
                            role: "option",
                            "aria-selected": if index == highlighted { "true" } else { "false" },
                            class: if index == highlighted { "px-3 py-2 flex items-baseline gap-3 cursor-pointer bg-signal-muted border-l-2 border-signal" } else { "px-3 py-2 flex items-baseline gap-3 cursor-pointer border-l-2 border-transparent hover:bg-surface-2" },
                            onmousedown: move |event| event.prevent_default(),
                            onclick: {
                                let chosen = choice.value.clone();
                                move |_| choose(chosen.clone())
                            },
                            span { class: if choice.value == value { "font-mono text-[13px] font-semibold shrink-0" } else { "font-mono text-[13px] shrink-0" },
                                "{choice.value}"
                            }
                            if let Some(label) = choice.label.clone() {
                                span { class: "text-[13px] text-ink-muted truncate min-w-0", "{label}" }
                            }
                            if let Some(note) = choice.note.clone() {
                                span { class: "ml-auto font-mono text-[12px] text-ink-muted shrink-0", "{note}" }
                            }
                        }
                    }
                    if let Some(new) = new_value {
                        li {
                            id: "{id}-{rows - 1}",
                            role: "option",
                            "aria-selected": if highlighted == rows - 1 { "true" } else { "false" },
                            class: if highlighted == rows - 1 { "px-3 py-2 cursor-pointer bg-signal-muted border-l-2 border-signal text-[13px]" } else { "px-3 py-2 cursor-pointer border-l-2 border-transparent hover:bg-surface-2 text-[13px]" },
                            onmousedown: move |event| event.prevent_default(),
                            onclick: move |_| choose(new.clone()),
                            "Use "
                            span { class: "font-mono", "“{new}”" }
                        }
                    }
                    if rows == 0 {
                        li { class: "px-3 py-2 text-[13px] text-ink-muted",
                            {empty.clone().unwrap_or_else(|| "Nothing matches.".to_owned())}
                        }
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_query_matches_value_label_and_note_ignoring_case() {
        let choices = [
            Choice::plain("tk-9f2x4a8m").label("Payments run asynchronously"),
            Choice::plain("tk-2b7n5k1q").note("PAY-124"),
            Choice::plain("dec-7k3m2q8p"),
        ];
        let values = |query: &str| -> Vec<&str> {
            filter_choices(&choices, query)
                .into_iter()
                .map(|choice| choice.value.as_str())
                .collect()
        };
        assert_eq!(values("ASYNC"), ["tk-9f2x4a8m"]);
        assert_eq!(values("pay-124"), ["tk-2b7n5k1q"]);
        assert_eq!(values("tk-"), ["tk-9f2x4a8m", "tk-2b7n5k1q"]);
        assert_eq!(values("  "), ["tk-9f2x4a8m", "tk-2b7n5k1q", "dec-7k3m2q8p"]);
    }

    #[test]
    fn empty_labels_and_notes_are_left_out() {
        let choice = Choice::plain("alice").label("").note("");
        assert_eq!(choice, Choice::plain("alice"));
    }
}
