//! `append` on a server's store (concept Section 4, "Error codes"): the test cases under
//! `schemas/tests/append/`, and a rejected call that leaves no trace.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use hone_core::{AppendError, Appender, Cursor, HostedStore};
use serde_json::{Value, json};

const WORKSPACE: &str = "local";
const PROJECT: &str = "prj-8m2k4d7q";

fn schemas() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../schemas")
}

// A temporary data directory, removed on drop.
struct Scratch(PathBuf);

impl Scratch {
    fn new(name: &str) -> Self {
        let root = std::env::temp_dir().join(format!("hone-append-{}-{name}", std::process::id()));
        if root.exists() {
            std::fs::remove_dir_all(&root).expect("removable");
        }
        std::fs::create_dir_all(&root).expect("creatable");
        Self(root)
    }

    fn store_directory(&self) -> PathBuf {
        self.0.join(WORKSPACE).join(PROJECT)
    }

    fn store(&self) -> HostedStore {
        HostedStore::new(&self.0, WORKSPACE, PROJECT).expect("valid scope")
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn copy_directory(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).expect("creatable");
    for entry in std::fs::read_dir(from).expect("readable") {
        let entry = entry.expect("readable entry");
        let target = to.join(entry.file_name());
        if entry.file_type().expect("file type").is_dir() {
            copy_directory(&entry.path(), &target);
        } else {
            std::fs::copy(entry.path(), &target).expect("copyable");
        }
    }
}

fn read_json(path: &Path) -> Value {
    serde_json::from_slice(&std::fs::read(path).expect("readable")).expect("JSON")
}

fn actors(value: &Value) -> BTreeMap<String, BTreeSet<String>> {
    serde_json::from_value(value.clone()).expect("actors")
}

// The answer as the transport would send it, without `message`.
fn answer(result: &Result<Cursor, AppendError>) -> Value {
    match result {
        Ok(cursor) => json!({ "status": 200, "cursor": cursor }),
        Err(AppendError::Rejected(rejection)) => {
            let mut body = serde_json::to_value(rejection).expect("serializable");
            body.as_object_mut().expect("an object").remove("message");
            body["status"] = json!(rejection.code.status());
            body
        }
        Err(AppendError::Store(error)) => panic!("the store failed: {error}"),
    }
}

#[test]
fn append_cases_store_and_answer_as_expected() {
    let mut cases: Vec<PathBuf> = std::fs::read_dir(schemas().join("tests/append"))
        .expect("readable")
        .map(|entry| entry.expect("readable entry").path())
        .collect();
    cases.sort();
    assert!(!cases.is_empty(), "no append cases");
    for case in cases {
        let name = case
            .file_name()
            .expect("a name")
            .to_string_lossy()
            .into_owned();
        let scratch = Scratch::new(&name);
        if case.join("store").is_dir() {
            copy_directory(&case.join("store"), &scratch.store_directory());
        }
        let request = read_json(&case.join("request.json"));
        let actors = actors(&request["actors"]);
        let body = std::fs::read_to_string(case.join("events.ndjson")).expect("readable");
        let held_before = scratch.store().read(&Cursor::new()).expect("readable");

        let result = scratch.store().append(
            Appender {
                identity: request["identity"].as_str().expect("an identity"),
                actors: &actors,
            },
            &body,
        );

        assert_eq!(
            answer(&result),
            read_json(&case.join("expected.json")),
            "case {name}"
        );
        // What was stored is exactly the held events plus the accepted lines of the body.
        let held_after = scratch.store().read(&Cursor::new()).expect("readable");
        let expected_cursor: Cursor =
            serde_json::from_value(read_json(&case.join("expected.json"))["cursor"].clone())
                .expect("a cursor");
        assert_eq!(
            scratch.store().cursor().expect("readable"),
            expected_cursor,
            "case {name}"
        );
        for line in &held_before {
            assert!(
                held_after.contains(line),
                "case {name}: a held event is gone"
            );
        }
        for line in held_after.iter().filter(|line| !held_before.contains(line)) {
            assert!(
                body.contains(line.as_str()),
                "case {name}: a stored event is not from the body"
            );
        }
    }
}

#[test]
fn a_rejected_call_into_a_missing_store_creates_nothing() {
    let scratch = Scratch::new("no-trace");
    let body = std::fs::read_to_string(schemas().join("tests/append/seq-gap/events.ndjson"))
        .expect("readable");
    let result = scratch.store().append(
        Appender {
            identity: "alice",
            actors: &BTreeMap::new(),
        },
        &body,
    );
    assert!(matches!(result, Err(AppendError::Rejected(_))));
    assert!(
        !scratch.0.join(WORKSPACE).exists(),
        "the rejected call created a directory"
    );
}

#[test]
fn a_store_needs_scope_ids() {
    let scratch = Scratch::new("scope");
    assert!(HostedStore::new(&scratch.0, "acme", PROJECT).is_err());
    assert!(HostedStore::new(&scratch.0, WORKSPACE, "payments").is_err());
}
