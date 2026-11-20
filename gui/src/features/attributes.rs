//! Editing a ticket's attributes: rows of key, type, and value, checked before they reach the
//! command line.

use dioxus::prelude::*;
use serde_json::{Map, Value};

use crate::hone::AttributeValue;
use crate::ui::{Choice, Combobox, Input};

/// The type of an attribute value, as the protocol allows them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Kind {
    Text,
    Integer,
    Boolean,
}

/// One row of the editor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Row {
    pub(crate) key: String,
    pub(crate) kind: Kind,
    /// The text of a text or integer value, `true` or `false` for a boolean.
    pub(crate) value: String,
}

impl Row {
    pub(crate) fn empty() -> Self {
        Self {
            key: String::new(),
            kind: Kind::Text,
            value: String::new(),
        }
    }

    fn of(key: &str, value: &Value) -> Option<Self> {
        Some(match AttributeValue::from_json(value)? {
            AttributeValue::Text(text) => Self {
                key: key.to_owned(),
                kind: Kind::Text,
                value: text,
            },
            AttributeValue::Integer(number) => Self {
                key: key.to_owned(),
                kind: Kind::Integer,
                value: number.to_string(),
            },
            AttributeValue::Boolean(flag) => Self {
                key: key.to_owned(),
                kind: Kind::Boolean,
                value: flag.to_string(),
            },
        })
    }
}

/// The rows of a record's attributes, in the order of their names.
pub(crate) fn rows_of(attributes: &Map<String, Value>) -> Vec<Row> {
    attributes
        .iter()
        .filter_map(|(key, value)| Row::of(key, value))
        .collect()
}

/// Why a row cannot be used, or `None` when it can. A row without key and value is ignored.
pub(crate) fn row_problem(rows: &[Row], index: usize) -> Option<String> {
    let row = &rows[index];
    let key = row.key.trim();
    if key.is_empty() {
        return (!row.value.trim().is_empty()).then(|| "a value needs a name".to_owned());
    }
    if key.contains('=') {
        return Some("a name cannot contain =".to_owned());
    }
    if rows[..index].iter().any(|other| other.key.trim() == key) {
        return Some(format!("`{key}` is set twice"));
    }
    if row.kind == Kind::Integer && row.value.trim().parse::<i64>().is_err() {
        return Some("a whole number, like 3 or -2".to_owned());
    }
    None
}

/// The attributes of the rows, or `None` when a row has a problem. Rows without a key are left
/// out.
pub(crate) fn parse_rows(rows: &[Row]) -> Option<Vec<(String, AttributeValue)>> {
    if (0..rows.len()).any(|index| row_problem(rows, index).is_some()) {
        return None;
    }
    Some(
        rows.iter()
            .filter(|row| !row.key.trim().is_empty())
            .map(|row| {
                let value = match row.kind {
                    Kind::Text => AttributeValue::Text(row.value.clone()),
                    Kind::Integer => {
                        AttributeValue::Integer(row.value.trim().parse().unwrap_or_default())
                    }
                    Kind::Boolean => AttributeValue::Boolean(row.value == "true"),
                };
                (row.key.trim().to_owned(), value)
            })
            .collect(),
    )
}

/// What to send to `hone ticket set-attributes` to turn `original` into `wanted`: the
/// attributes that are new or changed, and the names that are gone.
pub(crate) fn attribute_changes(
    original: &Map<String, Value>,
    wanted: &[(String, AttributeValue)],
) -> (Vec<(String, AttributeValue)>, Vec<String>) {
    let set = wanted
        .iter()
        .filter(|(key, value)| original.get(key) != Some(&value.to_json()))
        .cloned()
        .collect();
    let unset = original
        .keys()
        .filter(|key| !wanted.iter().any(|(wanted, _)| wanted == *key))
        .cloned()
        .collect();
    (set, unset)
}

/// The attribute names and values the selection rules test, as choices for the editor.
pub(crate) fn rule_keys(rules: &[Value]) -> Vec<Choice> {
    let mut keys: Vec<(String, Vec<String>)> = Vec::new();
    for rule in rules {
        for (key, wanted) in rule["when"].as_object().into_iter().flatten() {
            let values: Vec<String> = match wanted {
                Value::Array(set) => set.iter().map(Value::to_string).collect(),
                scalar => vec![scalar.to_string()],
            };
            match keys.iter_mut().find(|(known, _)| known == key) {
                Some((_, known)) => {
                    for value in values {
                        if !known.contains(&value) {
                            known.push(value);
                        }
                    }
                }
                None => keys.push((key.clone(), values)),
            }
        }
    }
    keys.into_iter()
        .map(|(key, values)| {
            Choice::plain(key)
                .label(values.join(", "))
                .note("used by select")
        })
        .collect()
}

/// The rows of attributes with a field per name, type, and value.
#[component]
pub(crate) fn AttributeEditor(rows: Signal<Vec<Row>>, keys: Vec<Choice>) -> Element {
    let mut rows = rows;
    let current = rows();
    rsx! {
        div { class: "flex flex-col gap-2",
            if !current.is_empty() {
                div { class: "grid grid-cols-[minmax(160px,1fr)_196px_minmax(160px,1fr)_72px] gap-2 text-[13px] text-ink-muted",
                    span { "Name" }
                    span { "Type" }
                    span { "Value" }
                    span {}
                }
            }
            for (index , row) in current.iter().cloned().enumerate() {
                {
                    let problem = row_problem(&current, index);
                    rsx! {
                        div { key: "{index}", class: "grid grid-cols-[minmax(160px,1fr)_196px_minmax(160px,1fr)_72px] gap-2 items-center",
                            Combobox {
                                value: row.key.clone(),
                                choices: keys.clone(),
                                placeholder: "name, for example risk",
                                label: "Attribute name",
                                custom: true,
                                invalid: problem.is_some() && !row.key.trim().is_empty() || problem.as_deref() == Some("a value needs a name"),
                                onselect: move |key: String| rows.with_mut(|rows| rows[index].key = key),
                            }
                            div { class: "flex border border-line", role: "radiogroup", "aria-label": "Type",
                                for (kind , label) in [(Kind::Text, "text"), (Kind::Integer, "number"), (Kind::Boolean, "yes/no")] {
                                    button {
                                        role: "radio",
                                        "aria-checked": if row.kind == kind { "true" } else { "false" },
                                        class: if row.kind == kind { "h-9 px-3 bg-ink text-bg text-[13px] font-semibold" } else { "h-9 px-3 text-[13px] hover:bg-surface-2" },
                                        onclick: move |_| rows.with_mut(|rows| {
                                            rows[index].kind = kind;
                                            if kind == Kind::Boolean && rows[index].value != "true" {
                                                rows[index].value = "false".into();
                                            }
                                        }),
                                        "{label}"
                                    }
                                }
                            }
                            if row.kind == Kind::Boolean {
                                div { class: "flex border border-line w-fit", role: "radiogroup", "aria-label": "Value",
                                    for flag in ["true", "false"] {
                                        button {
                                            role: "radio",
                                            "aria-checked": if row.value == flag { "true" } else { "false" },
                                            class: if row.value == flag { "h-9 px-4 bg-ink text-bg font-mono text-[13px]" } else { "h-9 px-4 font-mono text-[13px] hover:bg-surface-2" },
                                            onclick: move |_| rows.with_mut(|rows| rows[index].value = flag.into()),
                                            "{flag}"
                                        }
                                    }
                                }
                            } else {
                                Input {
                                    class: "font-mono",
                                    value: row.value.clone(),
                                    placeholder: if row.kind == Kind::Integer { "a number, like 3" } else { "a value, like high" },
                                    invalid: problem.is_some() && row.kind == Kind::Integer && !row.key.trim().is_empty(),
                                    oninput: move |value: String| rows.with_mut(|rows| rows[index].value = value),
                                }
                            }
                            button {
                                class: "h-9 px-3 text-[13px] underline text-ink-muted hover:text-ink",
                                title: "Remove this attribute",
                                onclick: move |_| rows.with_mut(|rows| { rows.remove(index); }),
                                "Remove"
                            }
                        }
                        if let Some(problem) = problem {
                            p { class: "m-0 -mt-1 text-[13px] text-bad", role: "alert", "✕ {problem}" }
                        }
                    }
                }
            }
            div {
                button {
                    class: "h-9 px-3 border border-dashed border-line text-[13px] hover:bg-surface-2",
                    onclick: move |_| rows.with_mut(|rows| rows.push(Row::empty())),
                    "+ Add attribute"
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn row(key: &str, kind: Kind, value: &str) -> Row {
        Row {
            key: key.into(),
            kind,
            value: value.into(),
        }
    }

    #[test]
    fn rows_are_checked_before_they_are_sent() {
        let rows = [
            row("risk", Kind::Text, "high"),
            row("size", Kind::Integer, "three"),
            row("risk", Kind::Text, "low"),
            row("", Kind::Text, "orphan"),
            row("", Kind::Text, ""),
        ];
        assert_eq!(row_problem(&rows, 0), None);
        assert_eq!(
            row_problem(&rows, 1).as_deref(),
            Some("a whole number, like 3 or -2")
        );
        assert_eq!(
            row_problem(&rows, 2).as_deref(),
            Some("`risk` is set twice")
        );
        assert_eq!(
            row_problem(&rows, 3).as_deref(),
            Some("a value needs a name")
        );
        assert_eq!(row_problem(&rows, 4), None);
        assert_eq!(parse_rows(&rows), None);
    }

    #[test]
    fn rows_become_typed_attributes() {
        let rows = [
            row("risk", Kind::Text, "high"),
            row("size", Kind::Integer, " 3 "),
            row("urgent", Kind::Boolean, "true"),
            row("", Kind::Text, ""),
        ];
        assert_eq!(
            parse_rows(&rows),
            Some(vec![
                ("risk".into(), AttributeValue::Text("high".into())),
                ("size".into(), AttributeValue::Integer(3)),
                ("urgent".into(), AttributeValue::Boolean(true)),
            ])
        );
    }

    #[test]
    fn changes_name_what_is_new_changed_or_gone() {
        let original = json!({ "risk": "high", "size": 3, "type": "bug" });
        let original = original.as_object().expect("a map");
        let wanted = vec![
            ("risk".to_owned(), AttributeValue::Text("high".into())),
            ("size".to_owned(), AttributeValue::Text("3".into())),
            ("team".to_owned(), AttributeValue::Text("payments".into())),
        ];
        let (set, unset) = attribute_changes(original, &wanted);
        assert_eq!(
            set,
            [
                ("size".to_owned(), AttributeValue::Text("3".into())),
                ("team".to_owned(), AttributeValue::Text("payments".into())),
            ]
        );
        assert_eq!(unset, ["type"]);
        assert_eq!(rows_of(original).len(), 3);
    }

    #[test]
    fn rule_keys_collect_the_values_select_tests() {
        let rules = [
            json!({ "when": { "type": "feature", "risk": ["high", "critical"] }, "profile": "full" }),
            json!({ "when": { "risk": "low" }, "profile": "quick" }),
        ];
        let keys: Vec<(String, Option<String>)> = rule_keys(&rules)
            .into_iter()
            .map(|choice| (choice.value, choice.label))
            .collect();
        assert_eq!(
            keys,
            [
                (
                    "risk".to_owned(),
                    Some("\"high\", \"critical\", \"low\"".to_owned())
                ),
                ("type".to_owned(), Some("\"feature\"".to_owned())),
            ]
        );
    }
}
