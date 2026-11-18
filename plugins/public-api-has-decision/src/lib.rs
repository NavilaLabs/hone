//! hone plugin gate `public-api-has-decision` (concept Section 9).
//!
//! With the parameters `{ anchored, binding }` (for example `{ anchored: module, binding:
//! decision }`), every changed `.rs` file that an anchor of an artifact of kind `anchored` hits is
//! parsed in the checked state (`/project`) and in the base state (`/base`). If its public API
//! differs, an artifact of kind `binding` with a binding effective status must list the anchored
//! artifact in `links.affects`, otherwise the gate reports `public-api-unbound`.
//!
//! The public API of a file is the set of its `pub` items: function signatures, structs with
//! their `pub` fields, enums, traits without the bodies of default methods, type aliases,
//! constants and statics with their types, `pub use`, the `pub` functions of inherent `impl`
//! blocks, and the headers of trait `impl` blocks, each inside its `pub mod` path. Bodies, private
//! items, and doc comments do not count. A file missing in a state has no public API there.
//!
//! Other codes: `changed-paths-missing` and `base-missing` when the call lacks them, and
//! `unparsable` when a changed file in the checked state is not valid Rust.

mod anchor;
mod api;

use std::collections::BTreeSet;

use extism_pdk::{FnResult, plugin_fn};
use serde_json::{Value, json};

/// The entry point `describe` of the plugin contract.
#[plugin_fn]
pub fn describe() -> FnResult<String> {
    Ok(json!({
        "name": "public-api-has-decision",
        "version": env!("CARGO_PKG_VERSION"),
        "protocol": 1,
        "entry_points": ["describe", "check"],
        "capabilities": { "read": ["project"], "net": [], "timeout_ms": 5000, "memory_pages": 256 },
    })
    .to_string())
}

/// The entry point `check` of the plugin contract.
#[plugin_fn]
pub fn check(input: String) -> FnResult<String> {
    let input: Value = serde_json::from_str(&input)?;
    Ok(json!({ "findings": findings(&input) }).to_string())
}

fn findings(input: &Value) -> Vec<Value> {
    let Some(changed) = input["changed_paths"].as_array() else {
        return vec![error(
            "changed-paths-missing",
            "the call supplied no changed paths",
            None,
        )];
    };
    if std::fs::metadata("/base").is_err() {
        return vec![error(
            "base-missing",
            "the call supplied no base state to compare the public API with",
            None,
        )];
    }
    let anchored = input["params"]["anchored"].as_str().unwrap_or_default();
    let binding = input["params"]["binding"].as_str().unwrap_or_default();
    let artifacts = input["artifacts"].as_array().map_or(&[][..], Vec::as_slice);
    let mut findings = Vec::new();
    for path in changed.iter().filter_map(Value::as_str) {
        if !path.ends_with(".rs") || path.contains(':') {
            continue;
        }
        let hit: Vec<&str> = artifacts
            .iter()
            .filter(|artifact| artifact["data"]["type"] == anchored)
            .filter(|artifact| {
                artifact["data"]["anchors"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .any(|anchor| anchor::hits(anchor, path))
            })
            .filter_map(|artifact| artifact["data"]["id"].as_str())
            .collect();
        if hit.is_empty() {
            continue;
        }
        let checked = match api::public_items(&format!("/project/{path}")) {
            Ok(items) => items,
            Err(message) => {
                findings.push(error("unparsable", &message, Some(path)));
                continue;
            }
        };
        // A base that does not parse counts as an empty API: every public item is then new.
        let base = api::public_items(&format!("/base/{path}")).unwrap_or_default();
        if checked == base {
            continue;
        }
        let unbound: Vec<&str> = hit
            .into_iter()
            .filter(|id| !is_bound(input, artifacts, binding, id))
            .collect();
        if !unbound.is_empty() {
            findings.push(error(
                "public-api-unbound",
                &format!(
                    "the public API changed ({}) without a binding `{binding}` that affects {}",
                    difference(&checked, &base),
                    unbound.join(", ")
                ),
                Some(path),
            ));
        }
    }
    findings
}

fn is_bound(input: &Value, artifacts: &[Value], binding: &str, id: &str) -> bool {
    let statuses = &input["config"]["kinds"][binding]["statuses"];
    artifacts.iter().any(|artifact| {
        let status = artifact["status"].as_str().unwrap_or_default();
        artifact["data"]["type"] == binding
            && statuses[status]["binding"] == Value::Bool(true)
            && artifact["data"]["links"]["affects"]
                .as_array()
                .is_some_and(|affects| affects.iter().any(|target| target == id))
    })
}

fn difference(checked: &BTreeSet<String>, base: &BTreeSet<String>) -> String {
    let mut parts: Vec<String> = Vec::new();
    for item in checked.difference(base).take(3) {
        parts.push(format!("added `{item}`"));
    }
    for item in base.difference(checked).take(3) {
        parts.push(format!("removed `{item}`"));
    }
    parts.join(", ")
}

fn error(code: &str, message: &str, path: Option<&str>) -> Value {
    let mut finding = json!({ "level": "error", "code": code, "message": message });
    if let Some(path) = path {
        finding["path"] = json!(path);
    }
    finding
}
