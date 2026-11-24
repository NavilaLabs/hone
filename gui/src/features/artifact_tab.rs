//! An artifact as a tab: its text in the modes Fields, Source, Split, and Preview, the problems
//! of the unsaved text as it changes, and Save, Discard, and Propose. Unsaved text stays in the
//! session's drafts, so it survives a switch of tabs.

use std::time::Duration;

use dioxus::prelude::*;
use serde_json::{Map, Value};

use super::artifact_form::FieldsForm;
use super::code::CodeTarget;
use super::common::{FailureAlert, Loading, Tag, WarningNote};
use super::editor::CodeEditor;
use super::review::{BindingBar, ResolveThread, ReviewLayout, chosen_thread, inline_threads};
use super::{Draft, Project, Session};
use crate::artifact_text::ArtifactText;
use crate::hone::{ArtifactSource, Failure, Finding};
use crate::markdown::preview;
use crate::ui::{Button, ButtonVariant};

/// How long typing pauses before the text is checked.
const CHECK_PAUSE: Duration = Duration::from_millis(400);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mode {
    Fields,
    Source,
    Split,
    Preview,
}

impl Mode {
    const ALL: [Self; 4] = [Self::Fields, Self::Source, Self::Split, Self::Preview];
    /// The modes of an artifact that may not change: no Fields, because nothing can be edited.
    const READ_ONLY: [Self; 3] = [Self::Source, Self::Split, Self::Preview];

    fn label(self) -> &'static str {
        match self {
            Self::Fields => "Fields",
            Self::Source => "Source",
            Self::Split => "Split",
            Self::Preview => "Preview",
        }
    }
}

/// Whether the saved status `status` binds, so that the artifact may not change (concept
/// Section 6): a change is a new artifact that supersedes it.
pub(crate) fn binding_status(status: Option<&str>, source: &ArtifactSource) -> bool {
    source
        .statuses
        .iter()
        .any(|known| Some(known.name.as_str()) == status && known.binding)
}

/// A problem of the text, with the line its pointer names.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Problem {
    pub(crate) finding: Finding,
    pub(crate) line: Option<usize>,
}

/// The findings of `text` with their lines, errors first.
pub(crate) fn problems(findings: &[Finding], text: &str, markdown: bool) -> Vec<Problem> {
    let file = ArtifactText::parse(text, markdown).ok();
    let mut problems: Vec<Problem> = findings
        .iter()
        .map(|finding| Problem {
            line: file
                .as_ref()
                .zip(finding.pointer.as_deref())
                .and_then(|(file, pointer)| file.line_of(pointer)),
            finding: finding.clone(),
        })
        .collect();
    problems.sort_by_key(|problem| problem.finding.level != "error");
    problems
}

/// Whether Propose is offered: the artifact is in `draft` and its kind has `proposed`.
pub(crate) fn can_propose(status: Option<&str>, source: &ArtifactSource) -> bool {
    status == Some("draft")
        && source
            .statuses
            .iter()
            .any(|status| status.name == "proposed")
}

/// The artifact `id`. `code` is the panel that shows a file outside a ticket.
#[component]
pub(crate) fn ArtifactTab(id: String, code: Signal<Option<CodeTarget>>) -> Element {
    let session = use_context::<Session>();
    let generation = use_signal(|| 0u64);
    let shown = id.clone();
    let source = use_resource(move || {
        let id = shown.clone();
        async move {
            let _ = (session.revision)();
            let loaded = generation();
            (loaded, (session.hone)().artifact(&id).await)
        }
    });
    match &*source.read() {
        None => rsx! { Loading {} },
        Some((_, Err(failure))) => rsx! { FailureAlert { failure: failure.clone() } },
        Some((loaded, Ok(source))) => rsx! {
            Workbench { key: "{loaded}", id: id.clone(), source: source.clone(), generation, code }
        },
    }
}

#[component]
fn Workbench(
    id: String,
    source: ArtifactSource,
    generation: Signal<u64>,
    code: Signal<Option<CodeTarget>>,
) -> Element {
    let session = use_context::<Session>();
    let project = use_context::<Project>();
    let mut drafts = session.drafts;
    let markdown = source.body.is_some();
    let saved_status = source.data.get("status").and_then(Value::as_str);
    let locked = binding_status(saved_status, &source);
    let mut mode = use_signal(|| if locked { Mode::Preview } else { Mode::Split });
    let mut review_request = session.review;
    let review_id = id.clone();
    use_effect(move || {
        if review_request().as_deref() == Some(review_id.as_str()) {
            mode.set(Mode::Preview);
            review_request.set(None);
        }
    });
    let threads = match &*project.log.read() {
        Some(Ok(log)) => inline_threads(&log.events, &id),
        _ => Vec::new(),
    };
    let chosen = chosen_thread(&threads, (session.thread)().as_deref());
    let chosen_inline = threads
        .iter()
        .find(|thread| Some(&thread.thread) == chosen.as_ref())
        .cloned();
    let mut busy = use_signal(|| false);
    let mut refused = use_signal(|| None::<Failure>);
    let mut not_saved = use_signal(|| false);
    let mut checked = use_signal(|| None::<Result<Vec<Finding>, Failure>>);
    let jump = use_signal(|| None::<usize>);

    let draft = drafts.read().get(&id).cloned();
    let text = draft
        .as_ref()
        .map_or_else(|| source.text.clone(), |draft| draft.text.clone());
    let data: Option<Map<String, Value>> = match &draft {
        Some(draft) => draft.data.clone(),
        None => Some(source.data.clone()),
    };
    let status = data
        .as_ref()
        .and_then(|data| data.get("status"))
        .and_then(Value::as_str)
        .map(str::to_owned);
    let binding = source
        .statuses
        .iter()
        .find(|known| Some(&known.name) == status.as_ref())
        .map(|known| known.binding);
    let body = ArtifactText::parse(&text, markdown)
        .map(|file| file.body().to_owned())
        .unwrap_or_default();

    let set_text = {
        let id = id.clone();
        let original = source.text.clone();
        let path = source.path.clone();
        use_callback(move |(new, data): (String, Option<Map<String, Value>>)| {
            let mut drafts = drafts.write();
            if new == original {
                drafts.remove(&id);
            } else {
                drafts.insert(
                    id.clone(),
                    Draft {
                        path: path.clone(),
                        text: new,
                        data,
                    },
                );
            }
        })
    };
    let current = {
        let id = id.clone();
        let original = source.text.clone();
        let fields = source.data.clone();
        move || match drafts.peek().get(&id) {
            Some(draft) => (draft.text.clone(), draft.data.clone()),
            None => (original.clone(), Some(fields.clone())),
        }
    };
    let set_field = {
        let current = current.clone();
        use_callback(move |(key, value): (String, Option<Value>)| {
            let (text, data) = current();
            let mut data = data.unwrap_or_default();
            let Ok(mut file) = ArtifactText::parse(&text, markdown) else {
                return;
            };
            match &value {
                Some(value) => {
                    file.set(&key, value);
                    data.insert(key, value.clone());
                }
                None => {
                    file.remove(&key);
                    data.remove(&key);
                }
            }
            set_text.call((file.text(), Some(data)));
        })
    };
    let set_body = {
        let current = current.clone();
        use_callback(move |new: String| {
            let (text, data) = current();
            if let Ok(mut file) = ArtifactText::parse(&text, markdown) {
                file.set_body(&new);
                set_text.call((file.text(), data));
            }
        })
    };

    let check_path = source.path.clone();
    let check_id = id.clone();
    let check_original = source.text.clone();
    use_resource(move || {
        let text = drafts
            .read()
            .get(&check_id)
            .map_or_else(|| check_original.clone(), |draft| draft.text.clone());
        let path = check_path.clone();
        async move {
            tokio::time::sleep(CHECK_PAUSE).await;
            let result = (session.hone)().check_artifact(&path, &text).await;
            checked.set(Some(result.map(|checked| checked.findings)));
        }
    });

    let save_path = source.path.clone();
    let save_id = id.clone();
    let save = use_callback(move |text: String| {
        let path = save_path.clone();
        let id = save_id.clone();
        busy.set(true);
        spawn(async move {
            match (session.hone)().save_artifact(&path, &text).await {
                Ok(written) if written.saved => {
                    refused.set(None);
                    not_saved.set(false);
                    drafts.write().remove(&id);
                    session.changed();
                    let mut generation = generation;
                    generation += 1;
                }
                Ok(written) => {
                    refused.set(None);
                    not_saved.set(true);
                    checked.set(Some(Ok(written.findings)));
                }
                Err(failure) => refused.set(Some(failure)),
            }
            busy.set(false);
        });
    });
    let propose_text = ArtifactText::parse(&text, markdown).ok().map(|mut file| {
        file.set("status", &Value::String("proposed".into()));
        file.text()
    });

    let found = match checked() {
        Some(Ok(findings)) => problems(&findings, &text, markdown),
        _ => Vec::new(),
    };
    let errors: Vec<String> = found
        .iter()
        .filter(|problem| problem.finding.level == "error")
        .filter_map(|problem| problem.finding.pointer.clone())
        .collect();
    let error_count = errors.len()
        + found
            .iter()
            .filter(|problem| problem.finding.level == "error" && problem.finding.pointer.is_none())
            .count();
    let changed = draft.is_some();
    let saved_text = text.clone();
    let editor_key = format!("{}-{:?}", generation.peek(), mode());

    rsx! {
        div { class: "flex flex-col gap-4 min-h-0",
            div { class: "flex flex-wrap items-center gap-3",
                span { class: "font-mono text-[13px] text-ink-muted break-all", "{source.path}" }
                match (&status, binding) {
                    _ if locked => rsx! {},
                    (Some(status), Some(true)) => rsx! { Tag { inverted: true, "{status} · binding" } },
                    (Some(status), Some(false)) => rsx! { Tag { "{status} · not binding" } },
                    (Some(status), None) => rsx! { Tag { "{status}" } },
                    (None, _) => rsx! {},
                }
                ProblemCount { checked: checked(), errors: error_count, total: found.len() }
                if changed {
                    span { class: "font-mono text-[12.5px] text-ink-muted", "● unsaved" }
                }
            }
            if locked {
                BindingBar { source: source.clone(), status: saved_status.unwrap_or_default().to_owned() }
            }
            div { class: "flex flex-wrap items-center gap-2",
                div { class: "flex gap-0 border border-line w-fit", role: "tablist", "aria-label": "View",
                    for value in if locked { &Mode::READ_ONLY[..] } else { &Mode::ALL[..] } {
                        {let value = *value; rsx! {
                        button {
                            role: "tab",
                            "aria-selected": if mode() == value { "true" } else { "false" },
                            class: if mode() == value { "h-10 px-4 bg-ink text-bg font-semibold" } else { "h-10 px-4 hover:bg-surface-2" },
                            onclick: move |_| mode.set(value),
                            "{value.label()}"
                        }
                        }}
                    }
                }
                if let Some(proposed) = propose_text.filter(|_| !locked && can_propose(status.as_deref(), &source)) {
                    Button {
                        disabled: busy(),
                        title: "Save with the status proposed",
                        onclick: move |_| save.call(proposed.clone()),
                        "Propose"
                    }
                }
                div { class: "ml-auto flex gap-2",
                    if mode() == Mode::Preview {
                        if let Some(thread) = chosen_inline.clone() {
                            ResolveThread { key: "{thread.thread}", thread }
                        }
                    }
                    if !locked {
                    Button {
                        variant: ButtonVariant::Outline,
                        disabled: busy() || !changed,
                        onclick: {
                            let id = id.clone();
                            move |_| {
                                drafts.write().remove(&id);
                                not_saved.set(false);
                                let mut generation = generation;
                                generation += 1;
                            }
                        },
                        "Discard"
                    }
                    Button { disabled: busy() || !changed, onclick: move |_| save.call(saved_text.clone()), "Save" }
                    }
                }
            }
            if let Some(failure) = refused() {
                FailureAlert { failure }
            } else if not_saved() {
                p { class: "m-0 text-[13px] text-bad font-semibold", role: "alert", "✕ Not saved: the problems below are errors." }
            }
            match mode() {
                Mode::Fields => rsx! {
                    if let Some(data) = data.clone() {
                        FieldsForm { source: source.clone(), data, body: body.clone(), errors: errors.clone(), set_field, set_body, code }
                    } else {
                        WarningNote { text: "The source has changes the fields cannot show. Save them, or discard them, to edit the fields again." }
                    }
                },
                Mode::Source => rsx! {
                    SourceEditor { key: "{editor_key}", text: text.clone(), markdown, read_only: locked, jump, set_text, class: "h-[48vh]" }
                },
                Mode::Split => rsx! {
                    div { class: "grid grid-cols-2 gap-4 items-stretch",
                        SourceEditor { key: "{editor_key}", text: text.clone(), markdown, read_only: locked, jump, set_text, class: "h-[48vh]" }
                        div { class: "border border-line p-5 h-[48vh] overflow-y-auto",
                            ArtifactPreview { data: data.clone(), body: body.clone() }
                        }
                    }
                },
                Mode::Preview => rsx! {
                    ReviewLayout { source: source.clone(), data: data.clone(), body: body.clone(), threads: threads.clone(), chosen: chosen.clone(), code }
                },
            }
            ProblemList { checked: checked(), problems: found, mode, jump }
        }
    }
}

/// The whole file in a source editor. An edit of the source leaves the fields unknown.
#[component]
fn SourceEditor(
    text: String,
    markdown: bool,
    read_only: bool,
    jump: Signal<Option<usize>>,
    set_text: Callback<(String, Option<Map<String, Value>>)>,
    class: String,
) -> Element {
    rsx! {
        CodeEditor {
            doc: text,
            language: if markdown { "markdown" } else { "yaml" },
            read_only,
            line: jump.peek().to_owned(),
            jump,
            onchange: move |new: String| set_text.call((new, None)),
            class,
        }
    }
}

#[component]
fn ProblemCount(
    checked: Option<Result<Vec<Finding>, Failure>>,
    errors: usize,
    total: usize,
) -> Element {
    match checked {
        None => rsx! { span { class: "font-mono text-[12.5px] text-ink-muted", "checking…" } },
        Some(Err(_)) => {
            rsx! { span { class: "font-mono text-[12.5px] text-bad", "✕ not checked" } }
        }
        Some(Ok(_)) if errors > 0 => rsx! {
            span { class: "font-mono text-[12.5px] text-bad", if errors == 1 { "✕ 1 problem" } else { "✕ {errors} problems" } }
        },
        Some(Ok(_)) if total > 0 => rsx! {
            span { class: "font-mono text-[12.5px] text-warn", if total == 1 { "▲ 1 warning" } else { "▲ {total} warnings" } }
        },
        Some(Ok(_)) => rsx! { span { class: "font-mono text-[12.5px] text-ok", "■ no problems" } },
    }
}

/// The problems of the text. A click on one with a line shows that line in the source.
#[component]
fn ProblemList(
    checked: Option<Result<Vec<Finding>, Failure>>,
    problems: Vec<Problem>,
    mode: Signal<Mode>,
    jump: Signal<Option<usize>>,
) -> Element {
    let mut mode = mode;
    let mut jump = jump;
    rsx! {
        section { class: "border border-line",
            div { class: "px-4 py-2 border-b border-line bg-surface flex items-baseline gap-3",
                span { class: "font-mono text-[11px] tracking-[0.08em] uppercase", "Problems" }
                span { class: "font-mono text-[12px] text-ink-muted", "{problems.len()} · the text as it is now: schema-valid and links-resolve" }
            }
            match checked {
                Some(Err(failure)) => rsx! { div { class: "p-4", FailureAlert { failure } } },
                _ if problems.is_empty() => rsx! {
                    p { class: "m-0 px-4 py-3 text-[13px] text-ink-muted", "No problems." }
                },
                _ => rsx! {
                    for problem in problems {
                        button {
                            class: "w-full text-left px-4 py-2 border-b border-line last:border-b-0 hover:bg-surface-2 grid grid-cols-[1.5rem_11rem_5rem_1fr] gap-3 items-baseline",
                            disabled: problem.line.is_none(),
                            onclick: move |_| {
                                if let Some(line) = problem.line {
                                    if matches!(mode(), Mode::Fields | Mode::Preview) {
                                        mode.set(Mode::Split);
                                    }
                                    jump.set(Some(line));
                                }
                            },
                            if problem.finding.level == "error" {
                                span { class: "text-bad", "✕" }
                            } else {
                                span { class: "text-warn", "▲" }
                            }
                            span { class: "font-mono text-[12.5px]", "{problem.finding.code}" }
                            span { class: "font-mono text-[12.5px] text-ink-muted",
                                {problem.line.map(|line| format!("line {line}")).unwrap_or_default()}
                            }
                            span { class: "text-[13px]", "{problem.finding.message}" }
                        }
                    }
                },
            }
        }
    }
}

/// The front matter's reach and links as chips over the rendered body. Without `data` (an edit
/// of the source) only the body.
#[component]
fn ArtifactPreview(data: Option<Map<String, Value>>, body: String) -> Element {
    let mut chips = Vec::new();
    if let Some(data) = &data {
        if let Some(reach) = data.get("reach").and_then(Value::as_str) {
            chips.push(format!("reach: {reach}"));
        }
        if let Some(links) = data.get("links").and_then(Value::as_object) {
            for (kind, targets) in links {
                for target in targets.as_array().into_iter().flatten() {
                    if let Some(target) = target.as_str() {
                        chips.push(format!("{kind}: {target}"));
                    }
                }
            }
        }
    }
    rsx! {
        div { class: "flex flex-col gap-4",
            if !chips.is_empty() {
                div { class: "flex flex-wrap gap-2",
                    for chip in chips {
                        Tag { muted: true, "{chip}" }
                    }
                }
            }
            if body.trim().is_empty() {
                p { class: "m-0 text-[13px] text-ink-muted", "No body." }
            } else {
                div { class: "hone-prose", dangerous_inner_html: preview(&body) }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hone::KindStatus;

    fn finding(level: &str, code: &str, pointer: Option<&str>) -> Finding {
        Finding {
            code: code.into(),
            level: level.into(),
            message: String::new(),
            location: None,
            path: None,
            pointer: pointer.map(str::to_owned),
        }
    }

    #[test]
    fn problems_name_the_line_of_their_pointer_and_errors_come_first() {
        let text =
            "---\nid: \"dec-1\"\nstatus: \"draft\"\nlinks:\n  affects: [\"mod-1\"]\n---\nText.\n";
        let found = problems(
            &[
                finding("warning", "unknown-field", Some("/mood")),
                finding("error", "status", Some("/status")),
                finding("error", "link-unresolved", Some("/links/affects/0")),
            ],
            text,
            true,
        );
        let lines: Vec<(&str, Option<usize>)> = found
            .iter()
            .map(|problem| (problem.finding.code.as_str(), problem.line))
            .collect();
        assert_eq!(
            lines,
            [
                ("status", Some(3)),
                ("link-unresolved", Some(5)),
                ("unknown-field", None)
            ]
        );
    }

    #[test]
    fn propose_needs_a_draft_and_a_kind_with_proposed() {
        let mut source = ArtifactSource {
            id: "dec-1".into(),
            kind: "decision".into(),
            path: "design/dec-1.md".into(),
            text: String::new(),
            data: Map::new(),
            body: Some(String::new()),
            lifecycle: "decided".into(),
            statuses: ["draft", "proposed"]
                .map(|name| KindStatus {
                    name: name.into(),
                    binding: false,
                })
                .to_vec(),
            schema: None,
            link_types: Vec::new(),
        };
        assert!(can_propose(Some("draft"), &source));
        assert!(!can_propose(Some("proposed"), &source));
        assert!(!can_propose(None, &source));
        source.statuses.retain(|status| status.name == "draft");
        assert!(!can_propose(Some("draft"), &source));
    }
}
