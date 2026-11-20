use std::collections::BTreeMap;
use std::path::Path;

use serde_json::{Map, Value, json};

use crate::canonical::sha256;
use crate::error::{Category, Origin, Problem, file_problem, pointer_problem};
use crate::manifest::Lock;
use crate::pointer::Pointer;

pub(crate) fn embed_schemas(tree: &mut Map<String, Value>, schemas: &BTreeMap<String, Value>) {
    let Some(Value::Object(kinds)) = tree.get_mut("kinds") else {
        return;
    };
    for (name, kind) in kinds {
        if let Some(schema) = kind.get_mut("schema").filter(|schema| schema.is_string())
            && let Some(content) = schemas.get(name)
        {
            *schema = content.clone();
        }
    }
}

pub(crate) fn complete_plugins(
    tree: &mut Map<String, Value>,
    hone: &Path,
    lock: &Lock,
) -> Vec<Problem> {
    let mut problems = Vec::new();
    let Some(Value::Object(plugins)) = tree.get_mut("plugins") else {
        return problems;
    };
    for (name, plugin) in plugins.iter_mut() {
        let Value::Object(plugin) = plugin else {
            continue;
        };
        let at = Pointer::from_segments(["plugins", name]);
        for key in plugin.keys() {
            if key != "capabilities" && key != "language" && !key.starts_with("x-") {
                problems.push(pointer_problem(
                    Category::Shape,
                    &at.child(key),
                    "a plugin definition holds only `capabilities` and `language`, `resolve` writes the rest",
                ));
            }
        }
        if let Some(Value::Object(capabilities)) = plugin.get_mut("capabilities") {
            capabilities.entry("net").or_insert_with(|| json!([]));
        }

        let wasm = format!("plugins/{name}.wasm");
        let hash = match std::fs::read(hone.join(&wasm)) {
            Ok(bytes) => sha256(&bytes),
            Err(_) => {
                problems.push(file_problem(
                    Category::PluginHash,
                    &Origin::Project,
                    &wasm,
                    "missing",
                ));
                continue;
            }
        };
        let Some(approved) = lock.plugins.get(name) else {
            problems.push(file_problem(
                Category::PluginHash,
                &Origin::Project,
                "lock.yaml",
                format!("plugin `{name}` has no entry in lock.yaml"),
            ));
            continue;
        };
        if approved.hash != hash {
            problems.push(file_problem(
                Category::PluginHash,
                &Origin::Project,
                &wasm,
                format!("hash {hash} differs from {} in lock.yaml", approved.hash),
            ));
        }
        if let Some(Value::Object(capabilities)) = plugin.get("capabilities") {
            check_capabilities(
                capabilities,
                &approved.capabilities,
                &at.child("capabilities"),
                &mut problems,
            );
        }
        plugin.insert("wasm".into(), Value::String(format!(".hone/{wasm}")));
        plugin.insert("hash".into(), Value::String(hash));
    }
    language_conflicts(plugins, &mut problems);
    problems
}

// A file belongs to exactly one language plugin, so two plugins must not claim one extension.
fn language_conflicts(plugins: &Map<String, Value>, problems: &mut Vec<Problem>) {
    let mut claimed: std::collections::BTreeMap<&str, &str> = std::collections::BTreeMap::new();
    for (name, plugin) in plugins {
        let extensions = plugin["language"]["extensions"]
            .as_array()
            .into_iter()
            .flatten();
        for (index, extension) in extensions.enumerate() {
            let Some(extension) = extension.as_str() else {
                continue;
            };
            if let Some(first) = claimed.insert(extension, name) {
                let at = Pointer::from_segments(["plugins", name, "language", "extensions"])
                    .child(index.to_string());
                problems.push(pointer_problem(
                    Category::LanguageConflict,
                    &at,
                    format!("the extension `{extension}` is claimed by `{first}` and `{name}`"),
                ));
            }
        }
    }
}

fn check_capabilities(
    requested: &Map<String, Value>,
    approved: &Map<String, Value>,
    at: &Pointer,
    problems: &mut Vec<Problem>,
) {
    for (key, message) in unapproved_capabilities(requested, approved) {
        problems.push(pointer_problem(
            Category::PluginCapabilities,
            &at.child(key),
            message,
        ));
    }
}

// The capabilities beyond the approved ones (concept Section 8, "Plugins"), as the key and a
// message per excess.
pub(crate) fn unapproved_capabilities(
    requested: &Map<String, Value>,
    approved: &Map<String, Value>,
) -> Vec<(&'static str, String)> {
    let mut excess = Vec::new();
    let mut deny = |key: &'static str, message: String| excess.push((key, message));
    for key in ["read", "net"] {
        let approved = approved
            .get(key)
            .and_then(Value::as_array)
            .map_or(&[][..], Vec::as_slice);
        if let Some(Value::Array(requested)) = requested.get(key) {
            for item in requested.iter().filter(|item| !approved.contains(item)) {
                deny(key, format!("{item} is not approved in the lockfile"));
            }
        }
    }
    if requested
        .get("net")
        .and_then(Value::as_array)
        .is_some_and(|net| !net.is_empty())
    {
        deny(
            "net",
            "plugins that `check` runs are pure, `net` must be `[]`".into(),
        );
    }
    for key in ["timeout_ms", "memory_pages"] {
        let Some(requested) = requested.get(key).and_then(Value::as_i64) else {
            continue;
        };
        match approved.get(key).and_then(Value::as_i64) {
            Some(approved) if requested > approved => {
                deny(key, format!("{requested} exceeds the approved {approved}"));
            }
            Some(_) => {}
            None => deny(key, "the lockfile approves no value".into()),
        }
    }
    excess
}

pub(crate) fn write_defaults(tree: &mut Map<String, Value>) {
    tree.insert("protocol".into(), json!(1));
    for key in ["kinds", "roles", "actors", "steps", "profiles"] {
        tree.entry(key).or_insert_with(|| json!({}));
    }
    if tree
        .get("plugins")
        .and_then(Value::as_object)
        .is_some_and(Map::is_empty)
    {
        tree.remove("plugins");
    }
    fill(tree, "roles", &[("kind", json!("human"))]);
    fill(
        tree,
        "profiles",
        &[
            ("retention", json!("archive")),
            ("review_summary", json!("full")),
        ],
    );
    fill(
        tree,
        "steps",
        &[
            ("acts", json!([])),
            ("inputs", json!([])),
            ("outputs", json!([])),
            ("requires", json!([])),
            ("gates", json!({})),
        ],
    );
}

fn fill(tree: &mut Map<String, Value>, section: &str, defaults: &[(&str, Value)]) {
    let Some(Value::Object(definitions)) = tree.get_mut(section) else {
        return;
    };
    for definition in definitions.values_mut() {
        if let Value::Object(definition) = definition {
            for (key, value) in defaults {
                definition.entry(*key).or_insert_with(|| value.clone());
            }
        }
    }
}
