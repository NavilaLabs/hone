//! The Edit tab of an artifact: the front matter as fields, the Markdown body in a source
//! editor beside its preview, and the whole file as source. Every save goes through
//! `hone artifact save`, which checks the file and writes it only without an error.

use dioxus::prelude::*;
use serde_json::{Map, Value};

use super::code::{CodeTarget, open_code};
use super::common::{Empty, FailureAlert, Field, FindingList, Loading, SectionLabel, WarningNote};
use super::editor::CodeEditor;
use super::{Project, Session};
use crate::artifact_text::ArtifactText;
use crate::hone::{ArtifactSource, ArtifactWritten, Failure};
use crate::markdown::preview;
use crate::ui::{Button, ButtonVariant, Checkbox, Choice, Combobox, Input, Textarea};

/// Fields that the path fixes or that only `hone review condense` writes.
const FIXED: [&str; 4] = ["id", "type", "ticket", "review"];
const ANCHOR_KINDS: [&str; 3] = ["path", "glob", "symbol"];

/// What a save left to show: its result, kept across the reload that follows it.
type Saved = Option<Result<ArtifactWritten, Failure>>;

/// The Edit tab of the artifact `id`. `code` is the panel that shows a file.
#[component]
pub(crate) fn ArtifactEditor(id: String, code: Signal<Option<CodeTarget>>) -> Element {
    let session = use_context::<Session>();
    let generation = use_signal(|| 0u64);
    let saved = use_signal(|| None);
    let source = use_resource(move || {
        let id = id.clone();
        async move {
            let loaded = generation();
            (loaded, (session.hone)().artifact(&id).await)
        }
    });
    match &*source.read() {
        None => rsx! { Loading {} },
        Some((_, Err(failure))) => rsx! { FailureAlert { failure: failure.clone() } },
        Some((loaded, Ok(source))) => rsx! {
            EditorBody { key: "{loaded}", source: source.clone(), generation, saved, code }
        },
    }
}

/// The pointers of the error findings of the last refused save.
fn error_pointers(saved: &Saved) -> Vec<String> {
    match saved {
        Some(Ok(written)) if !written.saved => written
            .findings
            .iter()
            .filter(|finding| finding.level == "error")
            .filter_map(|finding| finding.pointer.clone())
            .collect(),
        _ => Vec::new(),
    }
}

/// Whether one of `pointers` points into the field `pointer`.
fn hits(pointers: &[String], pointer: &str) -> bool {
    pointers.iter().any(|hit| {
        hit == pointer
            || hit
                .strip_prefix(pointer)
                .is_some_and(|rest| rest.starts_with('/'))
    })
}

/// The fields of the form in the order of the file, then the fields the kind's schema names,
/// then `status`, `links`, and `anchors` if the file has none yet.
fn field_order(keys: &[String], source: &ArtifactSource) -> Vec<String> {
    let mut order: Vec<String> = keys
        .iter()
        .filter(|key| !FIXED.contains(&key.as_str()))
        .cloned()
        .collect();
    let schema_keys = source
        .schema
        .as_ref()
        .and_then(|schema| schema["properties"].as_object())
        .map(|properties| properties.keys().cloned().collect::<Vec<_>>())
        .unwrap_or_default();
    let status = (!source.statuses.is_empty()).then(|| "status".to_owned());
    for key in schema_keys
        .into_iter()
        .chain(status)
        .chain(["links".to_owned(), "anchors".to_owned()])
    {
        if !order.contains(&key) && !FIXED.contains(&key.as_str()) {
            order.push(key);
        }
    }
    order
}

#[component]
fn EditorBody(
    source: ArtifactSource,
    generation: Signal<u64>,
    saved: Signal<Saved>,
    code: Signal<Option<CodeTarget>>,
) -> Element {
    let session = use_context::<Session>();
    let markdown = source.body.is_some();
    let original = source.text.clone();
    let mut text = use_signal(|| source.text.clone());
    let mut data = use_signal(|| source.data.clone());
    let mut body = use_signal(|| source.body.clone().unwrap_or_default());
    let mut mode = use_signal(|| "form");
    let mut source_edited = use_signal(|| false);
    let mut busy = use_signal(|| false);
    let order = ArtifactText::parse(&source.text, markdown)
        .map(|file| field_order(&file.keys(), &source))
        .unwrap_or_default();
    let set_field = use_callback(move |(key, value): (String, Option<Value>)| {
        data.with_mut(|data| match &value {
            Some(value) => {
                data.insert(key.clone(), value.clone());
            }
            None => {
                data.remove(&key);
            }
        });
        let current = text.peek().clone();
        if let Ok(mut file) = ArtifactText::parse(&current, markdown) {
            match &value {
                Some(value) => file.set(&key, value),
                None => file.remove(&key),
            }
            text.set(file.text());
        }
    });
    let set_body = move |new: String| {
        let current = text.peek().clone();
        if let Ok(mut file) = ArtifactText::parse(&current, markdown) {
            file.set_body(&new);
            text.set(file.text());
        }
        body.set(new);
    };
    let path = source.path.clone();
    let save = move |_| {
        let path = path.clone();
        busy.set(true);
        spawn(async move {
            let result = (session.hone)().save_artifact(&path, &text.peek()).await;
            let written = matches!(&result, Ok(written) if written.saved);
            saved.set(Some(result));
            busy.set(false);
            if written {
                session.changed();
                generation += 1;
            }
        });
    };
    let changed = text() != original;
    let errors = error_pointers(&saved());
    rsx! {
        div { class: "flex flex-col gap-6",
            div { class: "flex flex-wrap items-center gap-2",
                div { class: "flex gap-0 border border-line w-fit", role: "tablist",
                    for (value , label) in [("form", "Fields and body"), ("source", "Source")] {
                        button {
                            role: "tab",
                            "aria-selected": if mode() == value { "true" } else { "false" },
                            class: if mode() == value { "h-10 px-4 bg-ink text-bg font-semibold" } else { "h-10 px-4 hover:bg-surface-2" },
                            onclick: move |_| mode.set(value),
                            "{label}"
                        }
                    }
                }
                span { class: "font-mono text-[12.5px] text-ink-muted ml-2", "{source.path}" }
                div { class: "ml-auto flex gap-2",
                    Button {
                        variant: ButtonVariant::Outline,
                        disabled: busy() || !changed,
                        onclick: move |_| {
                            saved.set(None);
                            generation += 1;
                        },
                        "Discard changes"
                    }
                    Button { disabled: busy() || !changed, onclick: save, "Save" }
                }
            }
            match saved() {
                Some(Err(failure)) => rsx! { FailureAlert { failure } },
                Some(Ok(written)) => rsx! {
                    if written.saved {
                        p { class: "m-0 text-[13px] text-ok", role: "status", "■ Saved {written.path}." }
                    } else {
                        p { class: "m-0 text-[13px] text-bad font-semibold", role: "alert", "✕ Not saved: hone artifact save found errors." }
                    }
                    if !written.findings.is_empty() {
                        FindingList { findings: written.findings.clone() }
                    }
                },
                None => rsx! {},
            }
            if mode() == "source" {
                p { class: "m-0 text-[13px] text-ink-muted",
                    "The whole file. Saving checks it like schema-valid: the YAML profile, the path against the content, the kind, the status, and the schema."
                }
                CodeEditor {
                    doc: text(),
                    language: if markdown { "markdown" } else { "yaml" },
                    onchange: move |new: String| {
                        text.set(new);
                        source_edited.set(true);
                    },
                    class: "h-[65vh]",
                }
            } else if source_edited() {
                WarningNote { text: "The source has changes the fields cannot show. Save them, or discard them, to edit the fields again." }
            } else {
                section { class: "flex flex-col gap-4",
                    SectionLabel { label: "Front matter", aside: "id, type, ticket, and review are fixed here" }
                    for key in order {
                        FormField {
                            key: "{key}",
                            name: key.clone(),
                            value: data.read().get(&key).cloned(),
                            source: source.clone(),
                            invalid: hits(&errors, &format!("/{key}")),
                            set_field,
                            code,
                        }
                    }
                }
                if markdown {
                    section { class: "flex flex-col gap-3",
                        SectionLabel { label: "Body", aside: "Markdown, with a live preview" }
                        div { class: "grid grid-cols-[repeat(auto-fit,minmax(360px,1fr))] gap-4 items-stretch",
                            CodeEditor {
                                doc: body.peek().clone(),
                                language: "markdown",
                                onchange: set_body,
                                class: "h-[60vh]",
                            }
                            div {
                                class: "hone-prose border border-line p-5 h-[60vh] overflow-y-auto",
                                dangerous_inner_html: preview(&body()),
                            }
                        }
                    }
                }
            }
        }
    }
}

/// One top-level field, shaped by its value or, while absent, by the kind's schema.
#[component]
fn FormField(
    name: String,
    value: Option<Value>,
    source: ArtifactSource,
    invalid: bool,
    set_field: Callback<(String, Option<Value>)>,
    code: Signal<Option<CodeTarget>>,
) -> Element {
    let schema = source
        .schema
        .as_ref()
        .map(|schema| schema["properties"][&name].clone())
        .unwrap_or(Value::Null);
    match name.as_str() {
        "status" => rsx! {
            StatusField { value: value.and_then(|value| value.as_str().map(str::to_owned)), source, invalid, set_field }
        },
        "links" => rsx! {
            LinksField { links: value.and_then(|value| value.as_object().cloned()).unwrap_or_default(), source, set_field, code }
        },
        "anchors" => rsx! {
            AnchorsField { anchors: value.and_then(|value| value.as_array().cloned()).unwrap_or_default(), invalid, set_field, code }
        },
        _ => rsx! { ValueField { name, value, schema, invalid, set_field, code } },
    }
}

#[component]
fn StatusField(
    value: Option<String>,
    source: ArtifactSource,
    invalid: bool,
    set_field: Callback<(String, Option<Value>)>,
) -> Element {
    let choices: Vec<Choice> = source
        .statuses
        .iter()
        .map(|status| {
            Choice::plain(status.name.clone()).note(if status.binding { "binding" } else { "" })
        })
        .collect();
    rsx! {
        div { class: "flex flex-col gap-1 w-72",
            span { class: "text-[13px] text-ink-muted", "status" }
            Combobox {
                value: value.unwrap_or_default(),
                choices,
                label: "Status",
                invalid,
                onselect: move |status: String| set_field.call(("status".into(), Some(Value::String(status)))),
            }
        }
    }
}

/// How a field is edited.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Shape {
    Text,
    Integer,
    Boolean,
    /// A list of strings, one per line.
    Lines,
    /// A map of strings, one field per key.
    Texts,
    /// Anything else: shown, edited in the source.
    Other,
}

fn shape(value: Option<&Value>, schema: &Value) -> Shape {
    match value {
        Some(Value::String(_)) => Shape::Text,
        Some(Value::Number(_)) => Shape::Integer,
        Some(Value::Bool(_)) => Shape::Boolean,
        Some(Value::Array(items)) if items.iter().all(Value::is_string) => Shape::Lines,
        Some(Value::Object(map)) if map.values().all(Value::is_string) => Shape::Texts,
        Some(_) => Shape::Other,
        None => match schema["type"].as_str() {
            Some("string") => Shape::Text,
            Some("integer") => Shape::Integer,
            Some("boolean") => Shape::Boolean,
            Some("array") if schema["items"]["type"] == "string" => Shape::Lines,
            Some("object") => Shape::Texts,
            None if schema.get("$ref").is_some() => Shape::Text,
            _ => Shape::Other,
        },
    }
}

#[component]
fn ValueField(
    name: String,
    value: Option<Value>,
    schema: Value,
    invalid: bool,
    set_field: Callback<(String, Option<Value>)>,
    code: Signal<Option<CodeTarget>>,
) -> Element {
    let shape = shape(value.as_ref(), &schema);
    let mut raw = use_signal(|| match &value {
        Some(Value::Array(items)) => items
            .iter()
            .filter_map(Value::as_str)
            .collect::<Vec<_>>()
            .join("\n"),
        Some(Value::Number(number)) => number.to_string(),
        _ => String::new(),
    });
    let mut bad_number = use_signal(|| false);
    let key = name.clone();
    let label = match schema["description"].as_str() {
        Some(description) => format!("{name} · {description}"),
        None => name.clone(),
    };
    match shape {
        Shape::Text => {
            let text = value
                .as_ref()
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned();
            let update = move |new: String| {
                set_field.call((
                    key.clone(),
                    Some(new).filter(|new| !new.is_empty()).map(Value::String),
                ));
            };
            rsx! {
                Field { label,
                    if text.contains('\n') {
                        Textarea { value: text, oninput: update }
                    } else {
                        Input { value: text, invalid, oninput: update }
                    }
                }
            }
        }
        Shape::Integer => rsx! {
            Field { label,
                Input {
                    class: "font-mono w-48",
                    value: raw(),
                    invalid: invalid || bad_number(),
                    oninput: move |new: String| {
                        raw.set(new.clone());
                        let trimmed = new.trim();
                        match trimmed.parse::<i64>() {
                            Ok(number) => {
                                bad_number.set(false);
                                set_field.call((key.clone(), Some(Value::from(number))));
                            }
                            Err(_) if trimmed.is_empty() => {
                                bad_number.set(false);
                                set_field.call((key.clone(), None));
                            }
                            Err(_) => bad_number.set(true),
                        }
                    },
                }
            }
        },
        Shape::Boolean => {
            let checked = value.as_ref().and_then(Value::as_bool).unwrap_or(false);
            rsx! {
                label { class: "flex items-center gap-2 text-[13px] text-ink-muted",
                    Checkbox {
                        checked,
                        aria_label: name.clone(),
                        on_checked_change: move |checked: bool| set_field.call((key.clone(), Some(Value::Bool(checked)))),
                    }
                    "{label}"
                }
            }
        }
        Shape::Lines => rsx! {
            Field { label: format!("{label} (one per line)"),
                Textarea {
                    value: raw(),
                    rows: 3,
                    oninput: move |new: String| {
                        raw.set(new.clone());
                        let items: Vec<Value> = new
                            .lines()
                            .map(str::trim)
                            .filter(|line| !line.is_empty())
                            .map(|line| Value::String(line.to_owned()))
                            .collect();
                        set_field.call((key.clone(), Some(Value::Array(items)).filter(|items| items != &Value::Array(Vec::new()))));
                    },
                }
            }
        },
        Shape::Texts => {
            let map = value
                .as_ref()
                .and_then(Value::as_object)
                .cloned()
                .unwrap_or_default();
            let mut names: Vec<String> = map.keys().cloned().collect();
            if let Some(properties) = schema["properties"].as_object() {
                for property in properties.keys() {
                    if !names.contains(property) {
                        names.push(property.clone());
                    }
                }
            }
            rsx! {
                fieldset { class: "border border-line p-4 flex flex-col gap-3 min-w-0",
                    legend { class: "px-1 text-[13px] text-ink-muted", "{label}" }
                    for inner in names {
                        TextsEntry {
                            key: "{inner}",
                            outer: name.clone(),
                            inner: inner.clone(),
                            map: map.clone(),
                            reference: inner == "code" || schema["properties"][&inner]["$ref"].as_str().is_some_and(|reference| reference.contains("CodeRef")),
                            set_field,
                            code,
                        }
                    }
                }
            }
        }
        Shape::Other => rsx! {
            div { class: "flex flex-col gap-1",
                span { class: "text-[13px] text-ink-muted", "{label} · edit it under Source" }
                pre { class: "m-0 border border-line bg-surface p-3 font-mono text-[12.5px] overflow-x-auto",
                    {value.map(|value| serde_json::to_string_pretty(&value).unwrap_or_default()).unwrap_or_else(|| "not set".to_owned())}
                }
            }
        },
    }
}

/// One entry of a map of strings, for example `interface.code`.
#[component]
fn TextsEntry(
    outer: String,
    inner: String,
    map: Map<String, Value>,
    reference: bool,
    set_field: Callback<(String, Option<Value>)>,
    code: Signal<Option<CodeTarget>>,
) -> Element {
    let text = map
        .get(&inner)
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned();
    let target = reference.then(|| CodeTarget::of("code", &text)).flatten();
    let key = inner.clone();
    rsx! {
        div { class: "flex items-end gap-2",
            div { class: "flex-1 min-w-0",
                Field { label: "{inner}",
                    Input {
                        class: if reference { "font-mono" } else { "" },
                        value: text,
                        oninput: move |new: String| {
                            let mut map = map.clone();
                            if new.is_empty() {
                                map.remove(&key);
                            } else {
                                map.insert(key.clone(), Value::String(new));
                            }
                            set_field.call((outer.clone(), Some(Value::Object(map)).filter(|map| map != &Value::Object(Map::new()))));
                        },
                    }
                }
            }
            if let Some(target) = target {
                OpenCode { target, code }
            }
        }
    }
}

/// The button that shows a file in the panel, or with Ctrl (Cmd) in the IDE.
#[component]
fn OpenCode(target: CodeTarget, code: Signal<Option<CodeTarget>>) -> Element {
    let session = use_context::<Session>();
    let project = use_context::<Project>();
    rsx! {
        button {
            class: "h-9 px-3 border border-line font-mono text-[12.5px] underline hover:bg-surface-2 shrink-0",
            title: "Show the code; Ctrl+click opens it in the IDE",
            onclick: move |event| {
                let modifiers = event.modifiers();
                let in_ide = modifiers.ctrl() || modifiers.meta();
                open_code(session, project, target.clone(), in_ide, code);
            },
            "Code ↗"
        }
    }
}

#[component]
fn LinksField(
    links: Map<String, Value>,
    source: ArtifactSource,
    set_field: Callback<(String, Option<Value>)>,
    code: Signal<Option<CodeTarget>>,
) -> Element {
    let session = use_context::<Session>();
    let project = use_context::<Project>();
    let catalog = match &*project.catalog.read() {
        Some(Ok(catalog)) => catalog.artifacts.clone(),
        _ => Vec::new(),
    };
    let write = move |links: Map<String, Value>| {
        let links: Map<String, Value> = links
            .into_iter()
            .filter(|(_, ids)| ids.as_array().is_none_or(|ids| !ids.is_empty()))
            .collect();
        set_field.call((
            "links".into(),
            Some(Value::Object(links)).filter(|links| links != &Value::Object(Map::new())),
        ));
    };
    let ids_of = |links: &Map<String, Value>, kind: &str| -> Vec<String> {
        links
            .get(kind)
            .and_then(Value::as_array)
            .map(|ids| {
                ids.iter()
                    .filter_map(Value::as_str)
                    .map(str::to_owned)
                    .collect()
            })
            .unwrap_or_default()
    };
    let code_refs = ids_of(&links, "code");
    let mut new_code = use_signal(String::new);
    let kept: Vec<String> = links
        .keys()
        .filter(|kind| !source.link_types.contains(kind) && kind.as_str() != "code")
        .cloned()
        .collect();
    rsx! {
        fieldset { class: "border border-line p-4 flex flex-col gap-3 min-w-0",
            legend { class: "px-1 text-[13px] text-ink-muted", "links" }
            for kind in source.link_types.clone() {
                {
                    let ids = ids_of(&links, &kind);
                    let choices: Vec<Choice> = catalog
                        .iter()
                        .filter(|entry| entry.kind != "ticket")
                        .filter_map(|entry| {
                            let id = entry.id.clone()?;
                            (id != source.id && !ids.contains(&id)).then(|| {
                                Choice::plain(id)
                                    .label(entry.title.clone().unwrap_or_default())
                                    .note(entry.kind.clone())
                            })
                        })
                        .collect();
                    let add_links = links.clone();
                    let add_kind = kind.clone();
                    rsx! {
                        div { key: "{kind}", class: "flex flex-wrap items-center gap-2",
                            span { class: "font-mono text-[12.5px] text-ink-muted w-28 shrink-0", "{kind} →" }
                            for id in ids.clone() {
                                {
                                    let remove_links = links.clone();
                                    let remove_kind = kind.clone();
                                    let remove_id = id.clone();
                                    let open_id = id.clone();
                                    rsx! {
                                        span { key: "{id}", class: "h-8 pl-2 inline-flex items-center gap-1 border border-line font-mono text-[12.5px]",
                                            button { class: "underline", onclick: move |_| session.open_artifact(open_id.clone()), "{id}" }
                                            button {
                                                class: "h-8 w-8 text-ink-muted hover:text-ink",
                                                "aria-label": "Remove {id}",
                                                onclick: move |_| {
                                                    let mut links = remove_links.clone();
                                                    if let Some(Value::Array(ids)) = links.get_mut(&remove_kind) {
                                                        ids.retain(|value| value.as_str() != Some(remove_id.as_str()));
                                                    }
                                                    write(links);
                                                },
                                                "×"
                                            }
                                        }
                                    }
                                }
                            }
                            div { class: "w-64",
                                Combobox {
                                    value: "",
                                    choices,
                                    placeholder: "add an artifact",
                                    label: "Add a {kind} link",
                                    onselect: move |id: String| {
                                        if id.is_empty() {
                                            return;
                                        }
                                        let mut links = add_links.clone();
                                        let ids = links.entry(add_kind.clone()).or_insert_with(|| Value::Array(Vec::new()));
                                        if let Value::Array(ids) = ids {
                                            ids.push(Value::String(id));
                                        }
                                        write(links);
                                    },
                                }
                            }
                        }
                    }
                }
            }
            div { class: "flex flex-col gap-2",
                span { class: "font-mono text-[12.5px] text-ink-muted", "code → (path#symbol)" }
                for reference in code_refs.clone() {
                    {
                        let remove_links = links.clone();
                        let remove_reference = reference.clone();
                        rsx! {
                            div { key: "{reference}", class: "flex items-center gap-2",
                                span { class: "font-mono text-[13px] break-all", "{reference}" }
                                if let Some(target) = CodeTarget::of("code", &reference) {
                                    OpenCode { target, code }
                                }
                                button {
                                    class: "text-[13px] underline text-ink-muted hover:text-ink",
                                    onclick: move |_| {
                                        let mut links = remove_links.clone();
                                        if let Some(Value::Array(references)) = links.get_mut("code") {
                                            references.retain(|value| value.as_str() != Some(remove_reference.as_str()));
                                        }
                                        write(links);
                                    },
                                    "Remove"
                                }
                            }
                        }
                    }
                }
                div { class: "flex items-center gap-2",
                    Input { class: "font-mono w-96", value: new_code(), placeholder: "src/fee.rs#Fee::add", oninput: move |value| new_code.set(value) }
                    Button {
                        variant: ButtonVariant::Outline,
                        disabled: new_code().trim().is_empty(),
                        onclick: move |_| {
                            let mut links = links.clone();
                            let references = links.entry("code").or_insert_with(|| Value::Array(Vec::new()));
                            if let Value::Array(references) = references {
                                references.push(Value::String(new_code().trim().to_owned()));
                            }
                            write(links);
                            new_code.set(String::new());
                        },
                        "Add"
                    }
                }
            }
            if !kept.is_empty() {
                span { class: "text-[13px] text-ink-muted", "Kept as they are (edit under Source): {kept.join(\", \")}" }
            }
        }
    }
}

#[component]
fn AnchorsField(
    anchors: Vec<Value>,
    invalid: bool,
    set_field: Callback<(String, Option<Value>)>,
    code: Signal<Option<CodeTarget>>,
) -> Element {
    let write = move |anchors: Vec<Value>| {
        set_field.call((
            "anchors".into(),
            Some(Value::Array(anchors)).filter(|anchors| anchors != &Value::Array(Vec::new())),
        ));
    };
    let added = anchors.clone();
    rsx! {
        fieldset { class: if invalid { "border border-bad p-4 flex flex-col gap-3 min-w-0" } else { "border border-line p-4 flex flex-col gap-3 min-w-0" },
            legend { class: "px-1 text-[13px] text-ink-muted", "anchors · where it lives in the code" }
            if anchors.is_empty() {
                Empty { text: "No anchors. A symbol anchor is path#symbol, for example src/fee.rs#Fee::add." }
            }
            for (index , anchor) in anchors.iter().enumerate() {
                {
                    let kind = anchor["kind"].as_str().unwrap_or_default().to_owned();
                    let reference = anchor["ref"].as_str().unwrap_or_default().to_owned();
                    let target = CodeTarget::of(&kind, &reference);
                    let kind_anchors = anchors.clone();
                    let reference_anchors = anchors.clone();
                    let remove_anchors = anchors.clone();
                    rsx! {
                        div { key: "{index}", class: "flex flex-wrap items-center gap-2",
                            div { class: "w-32",
                                Combobox {
                                    value: kind.clone(),
                                    choices: ANCHOR_KINDS.iter().map(|kind| Choice::plain(*kind)).collect::<Vec<_>>(),
                                    label: "Anchor kind",
                                    onselect: move |kind: String| {
                                        let mut anchors = kind_anchors.clone();
                                        anchors[index]["kind"] = Value::String(kind);
                                        write(anchors);
                                    },
                                }
                            }
                            div { class: "flex-1 min-w-[240px]",
                                Input {
                                    class: "font-mono",
                                    value: reference.clone(),
                                    placeholder: if kind == "symbol" { "src/fee.rs#Fee::add" } else { "src/payments" },
                                    oninput: move |value: String| {
                                        let mut anchors = reference_anchors.clone();
                                        anchors[index]["ref"] = Value::String(value);
                                        write(anchors);
                                    },
                                }
                            }
                            if let Some(target) = target {
                                OpenCode { target, code }
                            }
                            button {
                                class: "text-[13px] underline text-ink-muted hover:text-ink",
                                onclick: move |_| {
                                    let mut anchors = remove_anchors.clone();
                                    anchors.remove(index);
                                    write(anchors);
                                },
                                "Remove"
                            }
                        }
                    }
                }
            }
            div {
                Button {
                    variant: ButtonVariant::Outline,
                    onclick: move |_| {
                        let mut anchors = added.clone();
                        anchors.push(serde_json::json!({ "kind": "symbol", "ref": "" }));
                        write(anchors);
                    },
                    "Add an anchor"
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::hone::KindStatus;

    fn source(schema: Value) -> ArtifactSource {
        ArtifactSource {
            id: "mod-4t8w2n6c".into(),
            kind: "module".into(),
            path: "design/modules/mod-4t8w2n6c-fees.yaml".into(),
            text: String::new(),
            data: Map::new(),
            body: None,
            lifecycle: "living".into(),
            statuses: vec![KindStatus {
                name: "active".into(),
                binding: true,
            }],
            schema: Some(schema),
            link_types: vec!["affects".into()],
        }
    }

    #[test]
    fn fields_follow_the_file_then_the_schema() {
        let source = source(json!({ "properties": { "name": {}, "secrets": {}, "anchors": {} } }));
        let keys = ["id", "type", "name", "x-team", "review"].map(str::to_owned);
        assert_eq!(
            field_order(&keys, &source),
            ["name", "x-team", "anchors", "secrets", "status", "links"]
        );
    }

    #[test]
    fn a_value_or_its_schema_decides_how_it_is_edited() {
        let none = Value::Null;
        assert_eq!(shape(Some(&json!("Fees")), &none), Shape::Text);
        assert_eq!(shape(Some(&json!(3)), &none), Shape::Integer);
        assert_eq!(shape(Some(&json!(["a", "b"])), &none), Shape::Lines);
        assert_eq!(
            shape(Some(&json!([{ "name": "alice" }])), &none),
            Shape::Other
        );
        assert_eq!(
            shape(Some(&json!({ "code": "src/fee.rs#Fee" })), &none),
            Shape::Texts
        );
        assert_eq!(
            shape(None, &json!({ "$ref": "../common.schema.json#/$defs/Id" })),
            Shape::Text
        );
        assert_eq!(
            shape(
                None,
                &json!({ "type": "array", "items": { "type": "string" } })
            ),
            Shape::Lines
        );
        assert_eq!(shape(None, &json!({ "type": "boolean" })), Shape::Boolean);
        assert_eq!(shape(None, &json!({ "minItems": 1 })), Shape::Other);
    }

    #[test]
    fn an_error_marks_the_field_it_points_into() {
        let pointers = ["/anchors/0/ref".to_owned(), "/status".to_owned()];
        assert!(hits(&pointers, "/anchors"));
        assert!(hits(&pointers, "/status"));
        assert!(!hits(&pointers, "/statuses"));
        assert!(!hits(&pointers, "/links"));
    }
}
