//! hone plugin gate `module-anchors-resolve` (concept Section 5, "Modules").
//!
//! With the parameter `{ kind }` (default `module`), every anchor of every artifact of that kind
//! and its `interface.code` must resolve in the checked state (`/project`): a `path` anchor names
//! a file or directory, a `glob` anchor matches at least one file, and a `symbol` anchor
//! `path#symbol` names a file in which the symbol exists. Symbols are resolved in Rust files: an
//! item, an item of an inline `mod`, a variant of an enum, an item of a trait, a field of a
//! struct, or an item of an `impl` block of a type, written `a::b`.
//!
//! Codes: `anchor-missing` (no file, directory, or match), `symbol-missing` (the file exists, the
//! symbol does not), `unparsable` (a Rust file that does not parse), and the warnings
//! `symbol-unchecked` (a symbol in a file of another language, whose file exists) and
//! `other-repository` (an anchor `repository:path`, which this state cannot see). Every finding
//! names the artifact's path and the pointer of the anchor.

mod files;
mod symbol;

use extism_pdk::{FnResult, plugin_fn};
use serde_json::{Value, json};

/// The entry point `describe` of the plugin contract.
#[plugin_fn]
pub fn describe() -> FnResult<String> {
    Ok(json!({
        "name": "module-anchors-resolve",
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
    let kind = input["params"]["kind"].as_str().unwrap_or("module");
    let mut findings = Vec::new();
    for artifact in input["artifacts"].as_array().into_iter().flatten() {
        if artifact["data"]["type"] != kind {
            continue;
        }
        let path = artifact["path"].as_str().unwrap_or_default();
        let anchors = artifact["data"]["anchors"].as_array().into_iter().flatten();
        for (index, anchor) in anchors.enumerate() {
            let (Some(anchor_kind), Some(reference)) =
                (anchor["kind"].as_str(), anchor["ref"].as_str())
            else {
                continue;
            };
            let pointer = format!("/anchors/{index}");
            if let Some(finding) = resolve(anchor_kind, reference) {
                findings.push(finding.at(path, &pointer));
            }
        }
        if let Some(reference) = artifact["data"]["interface"]["code"].as_str()
            && let Some(finding) = resolve("symbol", reference)
        {
            findings.push(finding.at(path, "/interface/code"));
        }
    }
    findings
}

struct Problem {
    level: &'static str,
    code: &'static str,
    message: String,
}

impl Problem {
    fn error(code: &'static str, message: String) -> Self {
        Self {
            level: "error",
            code,
            message,
        }
    }

    fn warning(code: &'static str, message: String) -> Self {
        Self {
            level: "warning",
            code,
            message,
        }
    }

    fn at(self, path: &str, pointer: &str) -> Value {
        json!({
            "level": self.level,
            "code": self.code,
            "message": self.message,
            "path": path,
            "pointer": pointer,
        })
    }
}

fn resolve(kind: &str, reference: &str) -> Option<Problem> {
    if reference.contains(':') && !reference.contains("::")
        || reference.split('#').next()?.contains(':')
    {
        return Some(Problem::warning(
            "other-repository",
            format!("`{reference}` lies in another repository, which this check cannot see"),
        ));
    }
    match kind {
        "path" => (!files::exists(reference)).then(|| {
            Problem::error(
                "anchor-missing",
                format!("no file or directory `{reference}`"),
            )
        }),
        "glob" => (!files::glob_matches(reference))
            .then(|| Problem::error("anchor-missing", format!("no file matches `{reference}`"))),
        "symbol" => {
            let (file, name) = reference.split_once('#').unwrap_or((reference, ""));
            if !files::is_file(file) {
                return Some(Problem::error(
                    "anchor-missing",
                    format!("no file `{file}`"),
                ));
            }
            if name.is_empty() {
                return None;
            }
            if !file.ends_with(".rs") {
                return Some(Problem::warning(
                    "symbol-unchecked",
                    format!("`{name}` in `{file}` is not checked: only Rust symbols are resolved"),
                ));
            }
            match symbol::exists(&files::project(file), name) {
                Ok(true) => None,
                Ok(false) => Some(Problem::error(
                    "symbol-missing",
                    format!("no symbol `{name}` in `{file}`"),
                )),
                Err(message) => Some(Problem::error("unparsable", format!("`{file}`: {message}"))),
            }
        }
        _ => None,
    }
}
