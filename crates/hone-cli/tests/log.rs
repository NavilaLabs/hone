//! `hone log --json`, the event list a GUI reads (concept Section 12, "Desktop GUI").

use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::{Value, json};

const CASE: &str = "schemas/tests/check/no-open-blocking-threads-open";

fn repository() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn log(data_directory: &Path) -> (Option<i32>, Value) {
    let output = Command::new(env!("CARGO_BIN_EXE_hone"))
        .args(["log", "--json", "--root"])
        .arg(repository().join(CASE).join("project"))
        .env("HONE_DATA_DIR", data_directory)
        .output()
        .expect("hone runs");
    let value = serde_json::from_slice(&output.stdout).expect("one JSON document");
    (output.status.code(), value)
}

#[test]
fn log_lists_every_event_and_the_cursor() {
    let data = repository().join(CASE).join("data");
    let segment = data.join("local/prj-8m2k4d7q/log/dev-3k9a5h2t/1.ndjson");
    let event: Value =
        serde_json::from_slice(&std::fs::read(segment).expect("readable")).expect("one event");

    let (code, value) = log(&data);
    assert_eq!(code, Some(0));
    assert_eq!(
        value,
        json!({ "cursor": { "dev-3k9a5h2t": 1 }, "events": [event], "device": null })
    );
}

#[test]
fn log_without_a_store_is_unavailable() {
    let missing = std::env::temp_dir().join(format!("hone-cli-log-{}-missing", std::process::id()));
    let (code, value) = log(&missing);
    assert_eq!(code, Some(1));
    assert_eq!(value["status"], "unavailable");
    assert!(!missing.exists(), "reading creates no store");
}
