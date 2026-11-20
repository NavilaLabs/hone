//! A source file, read-only, at the line of a symbol, and the way into the project's IDE.

use dioxus::prelude::*;

use super::common::{FailureAlert, Loading, ProblemAlert, WarningNote};
use super::editor::{CodeEditor, language_of};
use super::{Project, Screen, Session};
use crate::hone::{Hone, SourceFile};
use crate::ide::Ide;
use crate::ui::{Button, ButtonVariant};

/// A file below the project root and, optionally, a symbol in it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CodeTarget {
    pub(crate) path: String,
    pub(crate) symbol: Option<String>,
}

impl CodeTarget {
    /// The target of an anchor (`path`, `symbol`) or of a code reference (`code`), `None` for
    /// a glob and for a reference into another repository (`repo:path#symbol`).
    pub(crate) fn of(kind: &str, reference: &str) -> Option<Self> {
        if !matches!(kind, "path" | "symbol" | "code") {
            return None;
        }
        let (path, symbol) = match reference.split_once('#') {
            Some((path, symbol)) => (path, Some(symbol.to_owned()).filter(|s| !s.is_empty())),
            None => (reference, None),
        };
        // A path has no `:`, so one names another repository (spec/artifacts.md item 29).
        (!path.is_empty() && !path.contains(':')).then(|| Self {
            path: path.to_owned(),
            symbol,
        })
    }
}

/// Opens `target` in the project's editor at the symbol's line when `in_ide` and an editor is
/// set, otherwise in `panel`.
pub(crate) fn open_code(
    session: Session,
    project: Project,
    target: CodeTarget,
    in_ide: bool,
    mut panel: Signal<Option<CodeTarget>>,
) {
    match project.ide().filter(|_| in_ide) {
        Some(ide) => {
            spawn(async move {
                let hone = (session.hone)();
                let line = line_of(&hone, &target).await;
                if ide.open(hone.root(), &target.path, line).is_err() {
                    // The panel offers the IDE again, and its button shows why it did not start.
                    panel.set(Some(target));
                }
            });
        }
        None => panel.set(Some(target)),
    }
}

async fn line_of(hone: &Hone, target: &CodeTarget) -> usize {
    match &target.symbol {
        Some(symbol) => hone
            .file(&target.path, Some(symbol))
            .await
            .ok()
            .and_then(|file| file.line)
            .unwrap_or(1),
        None => 1,
    }
}

/// Why a symbol has no line, as the Code screen says it.
fn unresolved(file: &SourceFile) -> Option<String> {
    let symbol = file.symbol.as_deref()?;
    let why = match file.resolved.as_deref()? {
        "found" => return None,
        "missing" => format!("The language plugin does not find {symbol} in this file."),
        "unparsable" => "The language plugin cannot parse this file.".to_owned(),
        "unchecked" => format!("No language plugin handles this file, so {symbol} has no line."),
        _ => format!("The language plugin could not look up {symbol}."),
    };
    Some(match &file.message {
        Some(message) => format!("{why} Opened at line 1. ({message})"),
        None => format!("{why} Opened at line 1."),
    })
}

/// The file of `target`, read-only, in a panel at the right edge of the window, so that the
/// page below keeps its unsaved edits.
#[component]
pub(crate) fn CodePanel(target: CodeTarget, mut panel: Signal<Option<CodeTarget>>) -> Element {
    let mut session = use_context::<Session>();
    let project = use_context::<Project>();
    let mut opened = use_signal(|| None::<Result<(), String>>);
    let shown = target.clone();
    let file = use_resource(move || {
        let target = shown.clone();
        async move {
            (session.hone)()
                .file(&target.path, target.symbol.as_deref())
                .await
        }
    });
    let ide = project.ide();
    let line = match &*file.read() {
        Some(Ok(file)) => file.line.unwrap_or(1),
        _ => 1,
    };
    let open_path = target.path.clone();
    let mut open_in_ide = move |ide: Ide| {
        let root = session.hone.read().root().to_path_buf();
        opened.set(Some(ide.open(&root, &open_path, line)));
    };
    let title = match &target.symbol {
        Some(symbol) => format!("{} # {symbol}", target.path),
        None => target.path.clone(),
    };
    rsx! {
        aside {
            class: "fixed top-0 right-0 bottom-0 z-40 w-[min(820px,92vw)] bg-bg border-l border-line flex flex-col gap-4 p-6 overflow-y-auto",
            role: "dialog",
            "aria-label": "Code",
            onkeydown: move |event| {
                if event.key() == Key::Escape {
                    panel.set(None);
                }
            },
            div { class: "flex flex-wrap items-center gap-2",
                span { class: "font-mono text-[13px] break-all mr-auto", "{title}" }
                match ide {
                    Some(ide) => rsx! {
                        Button {
                            onclick: move |_| open_in_ide(ide.clone()),
                            title: "Ctrl+click an anchor to open it there directly",
                            "Open in IDE"
                        }
                    },
                    None => rsx! {
                        Button {
                            variant: ButtonVariant::Outline,
                            title: "Choose the IDE of this project (leaves this page)",
                            onclick: move |_| session.screen.set(Screen::Settings),
                            "Set an IDE"
                        }
                    },
                }
                Button { variant: ButtonVariant::Outline, onclick: move |_| panel.set(None), "Close" }
            }
            if let Some(Err(message)) = opened() {
                ProblemAlert { title: "The IDE did not open", message }
            }
            match &*file.read() {
                None => rsx! { Loading {} },
                Some(Err(failure)) => rsx! { FailureAlert { failure: failure.clone() } },
                Some(Ok(file)) => rsx! {
                    if let Some(text) = unresolved(file) {
                        WarningNote { text }
                    }
                    CodeEditor {
                        doc: file.text.clone(),
                        language: language_of(&file.path).map(str::to_owned),
                        read_only: true,
                        line: file.line,
                        class: "flex-1 min-h-[60vh]",
                    }
                },
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn target(path: &str, symbol: Option<&str>) -> Option<CodeTarget> {
        Some(CodeTarget {
            path: path.into(),
            symbol: symbol.map(str::to_owned),
        })
    }

    #[test]
    fn anchors_and_code_references_open_their_file_and_symbol() {
        assert_eq!(
            CodeTarget::of("symbol", "src/fee.rs#Fee::add"),
            target("src/fee.rs", Some("Fee::add"))
        );
        assert_eq!(
            CodeTarget::of("code", "src/fee.rs#Fee"),
            target("src/fee.rs", Some("Fee"))
        );
        assert_eq!(
            CodeTarget::of("path", "src/payments"),
            target("src/payments", None)
        );
        assert_eq!(CodeTarget::of("glob", "src/**/*.rs"), None);
        assert_eq!(CodeTarget::of("symbol", "billing:src/fee.rs#Fee"), None);
        assert_eq!(CodeTarget::of("symbol", "#Fee"), None);
    }

    #[test]
    fn a_symbol_without_a_line_says_why() {
        let file = |resolved: &str, message: Option<&str>| SourceFile {
            path: "src/fee.rs".into(),
            text: String::new(),
            symbol: Some("Fee::add".into()),
            language: Some("rust".into()),
            resolved: Some(resolved.into()),
            line: None,
            message: message.map(str::to_owned),
        };
        assert_eq!(unresolved(&file("found", None)), None);
        assert_eq!(
            unresolved(&file("missing", None)).as_deref(),
            Some("The language plugin does not find Fee::add in this file. Opened at line 1.")
        );
        assert_eq!(
            unresolved(&file("unparsable", Some("syntax error at 3:1"))).as_deref(),
            Some(
                "The language plugin cannot parse this file. Opened at line 1. (syntax error at 3:1)"
            )
        );
    }
}
