//! The Profiles and Plugins screens over `hone resolve --show`: the configuration from its
//! sources, the layers every definition comes from, and the lockfile.

use dioxus::prelude::*;
use serde_json::Value;

use super::common::{
    Empty, FailureAlert, Loading, PageHeader, ProblemAlert, SectionLabel, Tag, WarningNote,
    short_hash,
};
use super::{Project, Session};
use crate::hone::{ConfigView, ShownConfig};
use crate::ui::{
    Button, Table, TableBody, TableCell, TableHead, TableHeader, TableRow, TableWrapper,
};

/// The names of the layers in `origins`, bottom to top: a package by its name, otherwise the
/// kind (`project`, `overrides`).
fn layer_names(origins: &Value) -> Vec<String> {
    origins
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|layer| match layer["kind"].as_str()? {
            "package" => layer["name"].as_str().map(str::to_owned),
            kind => Some(kind.to_owned()),
        })
        .collect()
}

/// Steps against profiles: every step of the configuration, by name, with what each profile
/// asks of it (`required`, `optional`, or nothing).
#[derive(Debug, Clone, PartialEq)]
struct Matrix {
    profiles: Vec<String>,
    rows: Vec<(String, Vec<Option<String>>)>,
}

fn matrix(config: &Value) -> Matrix {
    let names = |section: &str| -> Vec<String> {
        config[section]
            .as_object()
            .into_iter()
            .flatten()
            .map(|(name, _)| name.clone())
            .collect()
    };
    let profiles = names("profiles");
    let rows = names("steps")
        .into_iter()
        .map(|step| {
            let cells = profiles
                .iter()
                .map(|profile| {
                    config["profiles"][profile]["steps"][&step]
                        .as_str()
                        .map(str::to_owned)
                })
                .collect();
            (step, cells)
        })
        .collect();
    Matrix { profiles, rows }
}

/// A plugin of the configuration with its lock entry.
#[derive(Debug, Clone, PartialEq)]
struct PluginRow {
    name: String,
    layers: Vec<String>,
    extensions: Vec<String>,
    hash: String,
    requested: Value,
    approved: Value,
}

/// The plugins of the configuration with their lock entries, and the names of the plugins the
/// lockfile approves that the configuration does not use.
fn plugins(shown: &ShownConfig) -> (Vec<PluginRow>, Vec<String>) {
    let configured = shown.config["plugins"]
        .as_object()
        .cloned()
        .unwrap_or_default();
    let rows = configured
        .iter()
        .map(|(name, plugin)| PluginRow {
            name: name.clone(),
            layers: layer_names(&shown.origins["plugins"][name]),
            extensions: plugin["language"]["extensions"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(Value::as_str)
                .map(str::to_owned)
                .collect(),
            hash: plugin["hash"].as_str().unwrap_or_default().to_owned(),
            requested: plugin["capabilities"].clone(),
            approved: shown.lock["plugins"][name]["capabilities"].clone(),
        })
        .collect();
    let unused = shown.lock["plugins"]
        .as_object()
        .into_iter()
        .flatten()
        .map(|(name, _)| name.clone())
        .filter(|name| !configured.contains_key(name))
        .collect();
    (rows, unused)
}

/// Capabilities in one line: `read project, docs · net none · 5000 ms · 256 pages`.
fn capabilities(capabilities: &Value) -> String {
    let list = |key: &str| {
        let items: Vec<&str> = capabilities[key]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .collect();
        if items.is_empty() {
            "none".to_owned()
        } else {
            items.join(", ")
        }
    };
    if capabilities.is_null() {
        return "not approved".to_owned();
    }
    format!(
        "read {} · net {} · {} ms · {} pages",
        list("read"),
        list("net"),
        capabilities["timeout_ms"],
        capabilities["memory_pages"]
    )
}

/// Loads `hone resolve --show` again after every write and every resolve.
fn use_config() -> Resource<Result<ConfigView, crate::hone::Failure>> {
    let session = use_context::<Session>();
    let project = use_context::<Project>();
    use_resource(move || async move {
        let _ = (session.revision)();
        let hone = project.resolving((session.hone)());
        hone.show_config().await
    })
}

/// Runs `hone resolve` and reads the project again; a failure shows below the button.
#[component]
fn ResolveButton() -> Element {
    let session = use_context::<Session>();
    let project = use_context::<Project>();
    let mut failure = use_signal(|| None::<crate::hone::Failure>);
    let mut running = use_signal(|| false);
    let resolve = move |_| {
        running.set(true);
        spawn(async move {
            let hone = project.resolving((session.hone)());
            match hone.resolve().await {
                Ok(_) => {
                    failure.set(None);
                    session.changed();
                }
                Err(error) => failure.set(Some(error)),
            }
            running.set(false);
        });
    };
    rsx! {
        div { class: "flex flex-col gap-2",
            Button { disabled: running(), onclick: resolve, "Run hone resolve" }
            if let Some(failure) = failure() {
                FailureAlert { failure }
            }
        }
    }
}

const STALE: &str = ".hone/resolved.json does not hold this configuration: the checks and new tickets still use the old one.";

/// The state of `resolved.json` and, while it is out of date, the action that writes it.
#[component]
fn FileState(shown: ShownConfig) -> Element {
    rsx! {
        div { class: "flex flex-wrap items-center gap-2",
            Tag { muted: true, "config {short_hash(&shown.hash)}" }
            if shown.file == "up-to-date" {
                Tag { muted: true, ".hone/resolved.json is up to date" }
            }
        }
        if shown.file != "up-to-date" {
            WarningNote { text: STALE }
            div { ResolveButton {} }
        }
    }
}

/// The problems of a failed resolution, framed in `bad`.
#[component]
fn ResolveProblems(problems: Vec<Value>) -> Element {
    rsx! {
        div { class: "flex flex-col gap-2",
            for problem in problems {
                ProblemAlert {
                    title: format!("resolve failed ({})", problem["category"].as_str().unwrap_or("problem")),
                    message: problem["message"].as_str().map_or_else(|| problem.to_string(), str::to_owned),
                }
            }
        }
    }
}

#[component]
fn Layers(names: Vec<String>) -> Element {
    rsx! {
        span { class: "font-mono text-[12px] text-ink-muted",
            if names.is_empty() { "defaults" } else { {names.join(" → ")} }
        }
    }
}

/// A selection rule's condition: each attribute with its value or list as JSON, so that a text
/// `"3"` and a number `3` read apart.
fn condition(when: &Value) -> String {
    when.as_object()
        .into_iter()
        .flatten()
        .map(|(name, value)| format!("{name}: {value}"))
        .collect::<Vec<_>>()
        .join(", ")
}

/// Every profile against every step, with the layers each comes from.
#[component]
pub(crate) fn Profiles() -> Element {
    let config = use_config();
    match &*config.read() {
        Some(Ok(ConfigView::Shown(shown))) => rsx! { ProfileMatrix { shown: shown.clone() } },
        other => rsx! {
            PageHeader { context: "hone resolve --show", title: "Profiles" }
            match other {
                None => rsx! { Loading {} },
                Some(Err(failure)) => rsx! { FailureAlert { failure: failure.clone() } },
                Some(Ok(ConfigView::Failed { problems, .. })) => rsx! { ResolveProblems { problems: problems.clone() } },
                Some(Ok(ConfigView::Shown(_))) => rsx! {},
            }
        },
    }
}

#[component]
fn ProfileMatrix(shown: ShownConfig) -> Element {
    let matrix = matrix(&shown.config);
    let select = &shown.config["select"];
    let rules = select["rules"].as_array().cloned().unwrap_or_default();
    let up_to_date = shown.file == "up-to-date";
    let state = if up_to_date {
        "up to date"
    } else {
        "out of date"
    };
    rsx! {
        div { class: "flex flex-col gap-3",
            PageHeader {
                context: format!(".hone/resolved.json · {state} · config {}", short_hash(&shown.hash)),
                title: "Profiles",
                actions: rsx! {
                    if !up_to_date {
                        ResolveButton {}
                    }
                },
            }
            p { class: "m-0 text-[14px] text-ink-muted",
                "Project settings apply to new tickets. Running tickets keep their frozen configuration."
            }
        }
        if !up_to_date {
            WarningNote { text: STALE }
        }
        if matrix.profiles.is_empty() {
            Empty { text: "The configuration has no profile. A profile is a file under .hone/profiles/ or in a package." }
        } else {
            TableWrapper { class: "shrink-0",
                Table {
                    TableHeader { class: "bg-surface",
                        TableRow {
                            TableHead { class: "align-top py-4", "Step" }
                            for profile in matrix.profiles.clone() {
                                TableHead { class: "align-top py-4",
                                    div { class: "flex flex-col gap-1",
                                        span { class: "font-mono font-bold text-[15px] text-ink", "{profile}" }
                                        Layers { names: layer_names(&shown.origins["profiles"][&profile]) }
                                        span { class: "font-mono text-[12px] text-ink-muted",
                                            {format!(
                                                "retention {} · review {}",
                                                shown.config["profiles"][&profile]["retention"].as_str().unwrap_or("-"),
                                                shown.config["profiles"][&profile]["review_summary"].as_str().unwrap_or("-"),
                                            )}
                                        }
                                    }
                                }
                            }
                        }
                    }
                    TableBody {
                        for (step, cells) in matrix.rows.clone() {
                            TableRow {
                                TableCell { class: "py-3",
                                    div { class: "flex flex-col gap-1",
                                        span { class: "font-semibold text-[15px]", "{step}" }
                                        Layers { names: layer_names(&shown.origins["steps"][&step]) }
                                    }
                                }
                                for cell in cells {
                                    TableCell { class: "py-3 font-mono text-[13px]",
                                        match cell.as_deref() {
                                            Some("required") => rsx! {
                                                span { span { "aria-hidden": "true", "■ " } "required" }
                                            },
                                            Some(other) => rsx! {
                                                span { class: "text-ink-muted", span { "aria-hidden": "true", "□ " } "{other}" }
                                            },
                                            None => rsx! { span { class: "text-ink-muted", "aria-label": "not in this profile", "–" } },
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
        div { class: "grid grid-cols-[repeat(auto-fill,minmax(360px,1fr))] gap-6 items-start",
            section { class: "border border-line",
                div { class: "px-5 py-3 bg-surface border-b border-line flex items-baseline justify-between gap-4",
                    h2 { class: "m-0 font-mono text-[11px] leading-4 font-medium tracking-[0.08em] uppercase",
                        "Selection rules · first match wins"
                    }
                    Layers { names: layer_names(&shown.origins["select"]) }
                }
                ol { class: "m-0 p-0 list-none font-mono text-[13px]",
                    for rule in rules {
                        li { class: "px-5 py-3 border-b border-line flex justify-between gap-4",
                            span { class: "break-all", {condition(&rule["when"])} }
                            span { class: "whitespace-nowrap", "→ {rule[\"profile\"].as_str().unwrap_or_default()}" }
                        }
                    }
                    li { class: "px-5 py-3 flex justify-between gap-4",
                        span { class: "text-ink-muted", "default" }
                        span { class: "whitespace-nowrap", "→ {select[\"default\"].as_str().unwrap_or_default()}" }
                    }
                }
            }
        }
    }
}

/// The plugins of the configuration, their capabilities against the approved ones, and what
/// else the lockfile approves.
#[component]
pub(crate) fn Plugins() -> Element {
    let config = use_config();
    rsx! {
        PageHeader { context: "hone resolve --show", title: "Plugins" }
        match &*config.read() {
            None => rsx! { Loading {} },
            Some(Err(failure)) => rsx! { FailureAlert { failure: failure.clone() } },
            Some(Ok(ConfigView::Failed { problems, lock })) => rsx! {
                ResolveProblems { problems: problems.clone() }
                if let Some(lock) = lock.clone() {
                    LockTable { lock }
                }
            },
            Some(Ok(ConfigView::Shown(shown))) => rsx! { PluginList { shown: shown.clone() } },
        }
    }
}

#[component]
fn PluginList(shown: ShownConfig) -> Element {
    let (rows, unused) = plugins(&shown);
    let commands = shown.lock["x-hone-exec"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    rsx! {
        FileState { shown: shown.clone() }
        section {
            SectionLabel { label: "Plugins of the configuration", aside: format!("{}", rows.len()) }
            if rows.is_empty() {
                Empty { text: "The configuration has no plugin. A plugin is a file under .hone/plugins/ or in a package, approved in .hone/lock.yaml." }
            } else {
                TableWrapper {
                    Table {
                        TableHeader { class: "bg-surface",
                            TableRow {
                                TableHead { "Plugin" }
                                TableHead { "Language" }
                                TableHead { "Capabilities" }
                                TableHead { "Approved" }
                                TableHead { "Hash" }
                            }
                        }
                        TableBody {
                            for row in rows {
                                TableRow {
                                    TableCell {
                                        div { class: "flex flex-col gap-1",
                                            span { class: "font-mono whitespace-nowrap", "{row.name}" }
                                            Layers { names: row.layers.clone() }
                                        }
                                    }
                                    TableCell { class: "font-mono text-ink-muted whitespace-nowrap",
                                        if row.extensions.is_empty() { "–" } else { {row.extensions.join(", ")} }
                                    }
                                    TableCell { class: "font-mono text-[12.5px]", {capabilities(&row.requested)} }
                                    TableCell { class: "font-mono text-[12.5px] text-ink-muted", {capabilities(&row.approved)} }
                                    TableCell { class: "font-mono text-[12.5px] text-ink-muted",
                                        span { title: "{row.hash}", {short_hash(&row.hash)} }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
        if !unused.is_empty() {
            section {
                SectionLabel { label: "Approved, not used", aside: format!("{}", unused.len()) }
                div { class: "flex flex-wrap gap-2",
                    for name in unused {
                        Tag { muted: true, "{name}" }
                    }
                }
            }
        }
        if !commands.is_empty() {
            section {
                SectionLabel { label: "Approved commands", aside: format!("{}", commands.len()) }
                div { class: "flex flex-col gap-2",
                    for command in commands {
                        div { class: "font-mono text-[13px]", {command_line(&command)} }
                    }
                }
            }
        }
    }
}

/// The lockfile's plugin approvals alone, shown while the resolution fails.
#[component]
fn LockTable(lock: Value) -> Element {
    let approvals: Vec<(String, Value)> = lock["plugins"]
        .as_object()
        .into_iter()
        .flatten()
        .map(|(name, entry)| (name.clone(), entry.clone()))
        .collect();
    rsx! {
        section {
            SectionLabel { label: "Approved in .hone/lock.yaml", aside: format!("{}", approvals.len()) }
            if approvals.is_empty() {
                Empty { text: "The lockfile approves no plugin." }
            } else {
                TableWrapper {
                    Table {
                        TableHeader { class: "bg-surface",
                            TableRow {
                                TableHead { "Plugin" }
                                TableHead { "Approved" }
                                TableHead { "Hash" }
                            }
                        }
                        TableBody {
                            for (name, entry) in approvals {
                                TableRow {
                                    TableCell { class: "font-mono", "{name}" }
                                    TableCell { class: "font-mono text-[12.5px]", {capabilities(&entry["capabilities"])} }
                                    TableCell { class: "font-mono text-[12.5px] text-ink-muted", {entry["hash"].as_str().unwrap_or_default().to_owned()} }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

/// An approved command as it would be typed, with its working directory and the variables it
/// may see.
fn command_line(command: &Value) -> String {
    let words: Vec<&str> = command["command"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .collect();
    let environment: Vec<&str> = command["env"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .collect();
    format!(
        "{} (in {}, environment: {})",
        words.join(" "),
        command["cwd"].as_str().unwrap_or("."),
        if environment.is_empty() {
            "none".to_owned()
        } else {
            environment.join(", ")
        }
    )
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn layers_name_packages_by_their_name() {
        let origins = json!([
            { "kind": "package", "name": "hone/standard" },
            { "kind": "project" },
            { "kind": "overrides" },
        ]);
        assert_eq!(
            layer_names(&origins),
            ["hone/standard", "project", "overrides"]
        );
        assert!(layer_names(&json!([])).is_empty());
    }

    #[test]
    fn the_matrix_has_every_step_and_profile() {
        let config = json!({
            "steps": { "decision": {}, "implementation": {}, "review": {} },
            "profiles": {
                "lite": { "steps": { "implementation": "required" } },
                "standard": { "steps": { "decision": "required", "review": "optional" } },
            },
        });
        assert_eq!(
            matrix(&config),
            Matrix {
                profiles: vec!["lite".into(), "standard".into()],
                rows: vec![
                    ("decision".into(), vec![None, Some("required".into())]),
                    ("implementation".into(), vec![Some("required".into()), None]),
                    ("review".into(), vec![None, Some("optional".into())]),
                ],
            }
        );
    }

    #[test]
    fn plugins_pair_with_their_lock_entries() {
        let shown = ShownConfig {
            hash: "sha256:00".into(),
            file: "up-to-date".into(),
            config: json!({ "plugins": { "lang-rust": {
                "capabilities": { "read": ["project"], "net": [], "timeout_ms": 5000, "memory_pages": 256 },
                "hash": "sha256:aa",
                "language": { "extensions": ["rs"] },
                "wasm": ".hone/plugins/lang-rust.wasm",
            } } }),
            origins: [(
                "plugins".to_owned(),
                json!({ "lang-rust": [{ "kind": "package", "name": "hone/lang-rust" }] }),
            )]
            .into(),
            lock: json!({ "plugins": {
                "lang-rust": { "hash": "sha256:aa", "capabilities": { "read": ["project", "docs"], "net": [], "timeout_ms": 10000, "memory_pages": 256 } },
                "probe": { "hash": "sha256:bb", "capabilities": { "read": [], "net": [], "timeout_ms": 1000, "memory_pages": 64 } },
            } }),
        };
        let (rows, unused) = plugins(&shown);
        assert_eq!(unused, ["probe"]);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].layers, ["hone/lang-rust"]);
        assert_eq!(rows[0].extensions, ["rs"]);
        assert_eq!(
            capabilities(&rows[0].requested),
            "read project · net none · 5000 ms · 256 pages"
        );
        assert_eq!(
            capabilities(&rows[0].approved),
            "read project, docs · net none · 10000 ms · 256 pages"
        );
        assert_eq!(capabilities(&Value::Null), "not approved");
    }

    #[test]
    fn a_condition_keeps_text_and_numbers_apart() {
        assert_eq!(
            condition(
                &serde_json::json!({ "risk": "high", "size": 3, "type": ["fix", "feature"] })
            ),
            "risk: \"high\", size: 3, type: [\"fix\",\"feature\"]"
        );
    }

    #[test]
    fn an_approved_command_reads_as_typed() {
        let command =
            json!({ "command": ["cargo", "--version"], "cwd": ".", "env": ["HOME", "PATH"] });
        assert_eq!(
            command_line(&command),
            "cargo --version (in ., environment: HOME, PATH)"
        );
    }
}
