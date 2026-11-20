//! A source editor in the window: CodeMirror from `assets/editor.js`, bundled so that nothing
//! loads from the network.

use std::sync::atomic::{AtomicU64, Ordering};

use dioxus::prelude::*;
use serde_json::json;

/// The bundle that defines `window.honeEditor`, added to the page once.
pub(crate) const BUNDLE: &str = include_str!("../../assets/editor.js");

/// The language of the editor for the file `path`, by its extension.
pub(crate) fn language_of(path: &str) -> Option<&'static str> {
    let extension = path.rsplit_once('.')?.1.to_ascii_lowercase();
    Some(match extension.as_str() {
        "rs" => "rust",
        "ts" | "tsx" | "mts" | "cts" => "typescript",
        "js" | "jsx" | "mjs" | "cjs" => "javascript",
        "py" | "pyi" => "python",
        "php" => "php",
        "dart" => "dart",
        "md" => "markdown",
        "yaml" | "yml" => "yaml",
        "json" => "json",
        _ => return None,
    })
}

/// An editor over `doc`. The props are read once: give the component a new `key` to show other
/// text. `onchange` receives the whole text after every change.
#[component]
pub(crate) fn CodeEditor(
    doc: String,
    #[props(into, optional)] language: Option<String>,
    #[props(optional)] read_only: bool,
    /// The line to show and mark, from 1.
    #[props(optional)]
    line: Option<usize>,
    #[props(optional)] onchange: Option<EventHandler<String>>,
    #[props(into, optional)] class: Option<String>,
) -> Element {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let id = use_hook(|| format!("hone-editor-{}", NEXT.fetch_add(1, Ordering::Relaxed)));
    let mount_id = id.clone();
    use_hook(move || {
        let options = json!({
            "doc": doc,
            "language": language,
            "readOnly": read_only,
            "line": line,
        });
        spawn(async move {
            // The bundle and the element may not be in the page yet when this runs.
            let mut eval = document::eval(
                r"
                const [id, options] = await dioxus.recv();
                while (!window.honeEditor || !document.getElementById(id)) {
                    await new Promise((resolve) => setTimeout(resolve, 20));
                }
                window.honeEditor.mount(id, options, (text) => dioxus.send(text));
                await new Promise(() => {});
                ",
            );
            if eval.send((mount_id, options)).is_err() {
                return;
            }
            while let Ok(text) = eval.recv::<String>().await {
                if let Some(onchange) = &onchange {
                    onchange.call(text);
                }
            }
        });
    });
    let destroy_id = id.clone();
    use_drop(move || {
        document::eval(&format!(
            "window.honeEditor && window.honeEditor.destroy({})",
            json!(destroy_id)
        ));
    });
    let class = class.unwrap_or_else(|| "h-[480px]".to_owned());
    rsx! {
        div { id: "{id}", class: "border border-line overflow-hidden {class}" }
    }
}

#[cfg(test)]
mod tests {
    use super::language_of;

    #[test]
    fn the_extension_picks_the_language() {
        assert_eq!(language_of("src/fee.rs"), Some("rust"));
        assert_eq!(language_of("web/App.TSX"), Some("typescript"));
        assert_eq!(language_of("lib/fee.dart"), Some("dart"));
        assert_eq!(language_of("design/dec-1.md"), Some("markdown"));
        assert_eq!(language_of("Makefile"), None);
        assert_eq!(language_of("notes.txt"), None);
    }
}
