//! A test plugin for the gate test cases of hone. The gate parameter `mode` (`x-mode` when the
//! plugin evaluates an `exec` gate) selects what `check` does:
//!
//! - `findings`: returns the parameter `findings` as its findings, none without it.
//! - `echo`: reports what it received as warnings, one code per fact: `artifact-<status>` (or
//!   `artifact-none`) with the artifact's path, `changed` with each changed path, `ticket-<id>`,
//!   `step-<step>`, `base-present` or `base-absent`, and `exit-<code>` for a `tool_output`.
//! - `dump`: the warning `input` whose message is the whole input as JSON.
//! - `evaluate`: the error `exit-nonzero` when `tool_output.exit` is not 0, and the warning
//!   `truncated` when `tool_output.truncated` is `true`.
//! - `read`: the error `unreadable` when the file `path` (a path in the sandbox) cannot be read.
//! - `write`: the error `written` when the file `path` can be written.
//! - `trap`, `spin`, `grow`, `error`: panics, loops forever, allocates without bound, or returns
//!   an error.
//! - `garbage`, `invalid`: returns bytes that are not JSON, or JSON outside the output schema.
//!
//! As a language plugin, the first entry of the input selects what `resolve` and `public_api` do:
//! its symbol for `resolve`, the name of its file without the extension for `public_api`.
//!
//! - `trap`: panics.
//! - `extra`: returns one result more than the input has entries.
//! - anything else: every symbol is `found`, every file has no public declaration.

use extism_pdk::{FnResult, WithReturnCode, plugin_fn};
use serde_json::{Value, json};

/// The entry point `describe` of the plugin contract.
#[plugin_fn]
pub fn describe() -> FnResult<String> {
    Ok(json!({
        "name": "probe",
        "version": env!("CARGO_PKG_VERSION"),
        "protocol": 1,
        "entry_points": ["describe", "check", "resolve", "public_api"],
        "capabilities": { "read": ["project"], "net": [], "timeout_ms": 1000, "memory_pages": 64 },
    })
    .to_string())
}

/// The entry point `check` of the plugin contract.
#[plugin_fn]
pub fn check(input: String) -> FnResult<String> {
    let input: Value = serde_json::from_str(&input)?;
    let params = &input["params"];
    // The parameters of an `exec` gate allow further keys only as `x-<name>`.
    let mode = params["mode"].as_str().or(params["x-mode"].as_str());
    let findings = match mode.unwrap_or_default() {
        "findings" if params["findings"].is_null() => json!([]),
        "findings" => params["findings"].clone(),
        "echo" => Value::Array(echo(&input)),
        "dump" => json!([{ "level": "warning", "code": "input", "message": input.to_string() }]),
        "evaluate" => Value::Array(evaluate(&input["tool_output"])),
        "read" => {
            let path = params["path"].as_str().unwrap_or_default();
            match std::fs::read(path) {
                Ok(_) => json!([]),
                Err(error) => json!([error_finding("unreadable", &format!("{path}: {error}"))]),
            }
        }
        "write" => {
            let path = params["path"].as_str().unwrap_or_default();
            match std::fs::write(path, b"written") {
                Ok(()) => json!([error_finding("written", path)]),
                Err(_) => json!([]),
            }
        }
        "trap" => panic!("the probe traps on purpose"),
        "spin" => loop {
            std::hint::spin_loop();
        },
        "grow" => {
            let mut blocks: Vec<Vec<u8>> = Vec::new();
            loop {
                blocks.push(vec![1; 1024 * 1024]);
                std::hint::black_box(&blocks);
            }
        }
        "error" => {
            return Err(WithReturnCode::new(
                extism_pdk::Error::msg("the probe fails on purpose"),
                1,
            ));
        }
        "garbage" => return Ok("not json".into()),
        "invalid" => return Ok(json!({ "ok": true, "findings": [] }).to_string()),
        other => json!([error_finding("unknown-mode", other)]),
    };
    Ok(json!({ "findings": findings }).to_string())
}

/// The entry point `resolve` of the language plugin contract.
#[plugin_fn]
pub fn resolve(input: String) -> FnResult<String> {
    let input: Value = serde_json::from_str(&input)?;
    let symbols = input["symbols"].as_array().cloned().unwrap_or_default();
    let mode = symbols.first().and_then(|first| first["symbol"].as_str());
    Ok(language_output(
        mode,
        symbols.len(),
        json!({ "status": "found" }),
    ))
}

/// The entry point `public_api` of the language plugin contract.
#[plugin_fn]
pub fn public_api(input: String) -> FnResult<String> {
    let input: Value = serde_json::from_str(&input)?;
    let paths = input["paths"].as_array().cloned().unwrap_or_default();
    let mode = paths
        .first()
        .and_then(Value::as_str)
        .map(std::path::Path::new)
        .and_then(std::path::Path::file_stem)
        .and_then(|stem| stem.to_str());
    Ok(language_output(
        mode,
        paths.len(),
        json!({ "status": "ok", "symbols": [] }),
    ))
}

fn language_output(mode: Option<&str>, entries: usize, result: Value) -> String {
    let count = match mode {
        Some("trap") => panic!("the probe traps on purpose"),
        Some("extra") => entries + 1,
        _ => entries,
    };
    json!({ "results": vec![result; count] }).to_string()
}

fn echo(input: &Value) -> Vec<Value> {
    let mut findings = Vec::new();
    for artifact in input["artifacts"].as_array().into_iter().flatten() {
        let status = artifact["status"].as_str().unwrap_or("none");
        findings.push(json!({
            "level": "warning",
            "code": format!("artifact-{status}"),
            "path": artifact["path"],
            "message": "",
        }));
    }
    for path in input["changed_paths"].as_array().into_iter().flatten() {
        findings
            .push(json!({ "level": "warning", "code": "changed", "path": path, "message": "" }));
    }
    let mut code = |code: String| findings.push(warning(&code));
    code(format!(
        "ticket-{}",
        input["ticket"]["id"].as_str().unwrap_or("none")
    ));
    code(format!("step-{}", input["step"].as_str().unwrap_or("none")));
    let base = if std::fs::metadata("/base").is_ok() {
        "base-present"
    } else {
        "base-absent"
    };
    code(base.to_owned());
    if let Some(exit) = input["tool_output"]["exit"].as_i64() {
        code(format!("exit-{exit}"));
    }
    findings
}

fn evaluate(output: &Value) -> Vec<Value> {
    let mut findings = Vec::new();
    if output["exit"].as_i64() != Some(0) {
        findings.push(error_finding("exit-nonzero", &output["exit"].to_string()));
    }
    if output["truncated"] == Value::Bool(true) {
        findings.push(warning("truncated"));
    }
    findings
}

fn error_finding(code: &str, message: &str) -> Value {
    json!({ "level": "error", "code": code, "message": message })
}

fn warning(code: &str) -> Value {
    json!({ "level": "warning", "code": code, "message": "" })
}
