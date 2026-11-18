//! The replication contract (concept Section 4, "Replication"): the read and sync test cases
//! under `schemas/tests/read/` and `schemas/tests/sync/`, convergence of two devices through a
//! shared folder, and the untrusted paths of a folder.

use std::path::{Path, PathBuf};

use hone_core::{
    Author, Change, Cursor, ProcessError, ProcessStore, StepStatus, SyncCode, SyncReport,
};
use serde_json::{Value, json};

const PROJECT: &str = "prj-8m2k4d7q";
const TICKET: &str = "tk-9f2x4a8m";

fn schemas() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../schemas")
}

// A temporary directory with a project that names the scope of the test cases, removed on drop.
struct Scratch {
    root: PathBuf,
}

impl Scratch {
    fn new(name: &str) -> Self {
        let root =
            std::env::temp_dir().join(format!("hone-replication-{}-{name}", std::process::id()));
        if root.exists() {
            std::fs::remove_dir_all(&root).expect("removable");
        }
        std::fs::create_dir_all(root.join("project/.hone")).expect("creatable");
        std::fs::write(
            root.join("project/.hone/config.yaml"),
            format!("workspace: local\nproject: {PROJECT}\n"),
        )
        .expect("writable");
        Self { root }
    }

    fn data(&self) -> PathBuf {
        self.root.join("data")
    }

    fn folder(&self) -> PathBuf {
        self.root.join("folder")
    }

    fn local_store(&self) -> PathBuf {
        self.data().join("local").join(PROJECT)
    }

    fn store(&self) -> ProcessStore {
        ProcessStore::locate(&self.root.join("project"), Some(&self.data())).expect("locatable")
    }

    fn folder_store(&self) -> ProcessStore {
        ProcessStore::locate(&self.root.join("project"), Some(&self.folder())).expect("locatable")
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
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

fn cases(kind: &str) -> Vec<(String, PathBuf)> {
    let mut cases: Vec<(String, PathBuf)> = std::fs::read_dir(schemas().join("tests").join(kind))
        .expect("readable")
        .map(|entry| entry.expect("readable entry"))
        .map(|entry| {
            (
                entry.file_name().into_string().expect("UTF-8"),
                entry.path(),
            )
        })
        .collect();
    cases.sort();
    assert!(!cases.is_empty(), "no {kind} cases");
    cases
}

fn read_json(path: &Path) -> Value {
    serde_json::from_slice(&std::fs::read(path).expect("readable")).expect("JSON")
}

fn cursor_of(value: &Value) -> Cursor {
    serde_json::from_value(value.clone()).expect("a cursor")
}

fn canonical_lines(events: &[Value]) -> String {
    // The fixtures are written by `jq -cS`, which matches RFC 8785 for these events.
    events.iter().map(|event| format!("{event}\n")).collect()
}

#[test]
fn read_cases_return_the_events_after_the_cursor() {
    for (name, case) in cases("read") {
        let scratch = Scratch::new(&format!("read-{name}"));
        copy_directory(&case.join("store"), &scratch.local_store());
        let cursor = cursor_of(&read_json(&case.join("cursor.json")));
        let events = scratch.store().read(&cursor).expect("readable");
        let expected = std::fs::read_to_string(case.join("expected.ndjson")).expect("readable");
        assert_eq!(canonical_lines(&events), expected, "case {name}");
    }
}

fn problem_lines(report: &SyncReport, scratch: &Scratch) -> String {
    let mut lines: Vec<String> = report
        .problems
        .iter()
        .map(|problem| {
            let path = if let Ok(rest) = problem.path.strip_prefix(scratch.folder()) {
                Path::new("folder").join(rest)
            } else if let Ok(rest) = problem.path.strip_prefix(scratch.local_store()) {
                Path::new("local").join(rest)
            } else {
                problem.path.clone()
            };
            let code = serde_json::to_value(problem.code).expect("serializable");
            let line = problem.line.map_or("-".to_owned(), |line| line.to_string());
            format!(
                "{} {} {line}\n",
                code.as_str().expect("a string"),
                path.display()
            )
        })
        .collect();
    lines.sort();
    lines.concat()
}

#[test]
fn sync_cases_reach_the_expected_cursors_and_problems() {
    for (name, case) in cases("sync") {
        let scratch = Scratch::new(&format!("sync-{name}"));
        if case.join("local").exists() {
            copy_directory(&case.join("local"), &scratch.local_store());
        }
        copy_directory(&case.join("folder"), &scratch.folder());
        let store = scratch.store();

        let report = store.sync(&scratch.folder()).expect("syncs");
        let expected = std::fs::read_to_string(case.join("expected.txt")).expect("readable");
        assert_eq!(problem_lines(&report, &scratch), expected, "case {name}");
        let cursors = read_json(&case.join("cursor.json"));
        assert_eq!(
            store.cursor().expect("local cursor"),
            cursor_of(&cursors["local"]),
            "local cursor of case {name}"
        );
        assert_eq!(
            scratch.folder_store().cursor().expect("folder cursor"),
            cursor_of(&cursors["folder"]),
            "folder cursor of case {name}"
        );
        assert!(
            !scratch.local_store().join("device").exists() || case.join("local/device").exists(),
            "case {name}: a sync must not create a device ID"
        );

        let again = store.sync(&scratch.folder()).expect("syncs again");
        assert!(
            again.received.is_empty() && again.sent.is_empty(),
            "case {name}: a second sync moves nothing: {again:?}"
        );
        assert_eq!(again.problems, report.problems, "case {name}");
    }
}

#[test]
fn sync_counts_what_it_moves() {
    let case = schemas().join("tests/sync/exchange");
    let scratch = Scratch::new("counts");
    copy_directory(&case.join("local"), &scratch.local_store());
    copy_directory(&case.join("folder"), &scratch.folder());
    let report = scratch.store().sync(&scratch.folder()).expect("syncs");
    assert_eq!(
        serde_json::to_value(&report.received).expect("JSON"),
        json!({ "dev-7q2m8x4c": 3 })
    );
    assert_eq!(
        serde_json::to_value(&report.sent).expect("JSON"),
        json!({ "dev-3k9a5h2t": 2 })
    );
    let pushed = scratch
        .folder()
        .join("local")
        .join(PROJECT)
        .join("log/dev-3k9a5h2t/1.ndjson");
    assert_eq!(
        std::fs::read(pushed).expect("pushed"),
        std::fs::read(case.join("local/log/dev-3k9a5h2t/1.ndjson")).expect("readable"),
        "the own log is written byte for byte"
    );
}

#[test]
fn a_missing_folder_is_unavailable_and_creates_nothing() {
    let scratch = Scratch::new("missing-folder");
    let outcome = scratch.store().sync(&scratch.folder());
    assert!(
        matches!(&outcome, Err(ProcessError::Unavailable { message }) if message.contains("no shared folder")),
        "{outcome:?}"
    );
    assert!(!scratch.data().exists(), "no local store is created");
    assert!(!scratch.folder().exists());
}

// Two devices with their own data directories and a copy of one gate test project each.
struct Device {
    root: PathBuf,
}

impl Device {
    fn new(scratch: &Scratch, name: &str) -> Self {
        let root = scratch.root.join(name);
        copy_directory(
            &schemas().join("tests/check/steps-complete-incomplete/project"),
            &root.join("project"),
        );
        Self { root }
    }

    fn store(&self) -> ProcessStore {
        ProcessStore::locate(&self.root.join("project"), Some(&self.root.join("data")))
            .expect("locatable")
    }

    fn set_step(&self, author: &str, status: StepStatus) -> Value {
        let author = Author {
            name: author.into(),
            role: "human".into(),
        };
        let change = Change::SetStep {
            ticket: TICKET.into(),
            step: "decision".into(),
            status,
            reason: None,
        };
        self.store()
            .append(&author, change)
            .expect("appended")
            .event
    }

    fn order(&self) -> Vec<Value> {
        self.store()
            .events(TICKET)
            .expect("readable")
            .events
            .iter()
            .map(|event| event["id"].clone())
            .collect()
    }
}

#[test]
fn two_devices_converge_through_a_shared_folder() {
    let scratch = Scratch::new("converge");
    std::fs::create_dir_all(scratch.folder()).expect("creatable");
    let alice = Device::new(&scratch, "alice");
    let bob = Device::new(&scratch, "bob");
    alice.set_step("alice", StepStatus::Done);
    bob.set_step("bob", StepStatus::Skipped);
    alice.set_step("alice", StepStatus::Open);

    assert!(
        alice
            .store()
            .sync(&scratch.folder())
            .expect("syncs")
            .problems
            .is_empty()
    );
    assert!(
        bob.store()
            .sync(&scratch.folder())
            .expect("syncs")
            .problems
            .is_empty()
    );
    let answer = bob.set_step("bob", StepStatus::Done);
    assert!(
        bob.store()
            .sync(&scratch.folder())
            .expect("syncs")
            .problems
            .is_empty()
    );
    assert!(
        alice
            .store()
            .sync(&scratch.folder())
            .expect("syncs")
            .problems
            .is_empty()
    );

    let order = alice.order();
    assert_eq!(order.len(), 4);
    assert_eq!(order, bob.order(), "both devices order the same set alike");
    assert_eq!(
        order.last(),
        Some(&answer["id"]),
        "bob's answer knew every event"
    );
    assert_eq!(
        alice.store().cursor().expect("cursor"),
        bob.store().cursor().expect("cursor")
    );
    let steps = alice.store().events(TICKET).expect("readable").steps;
    assert_eq!(steps["decision"].status, StepStatus::Done);
}

#[cfg(unix)]
mod links {
    use std::os::unix::fs::symlink;

    use super::*;

    fn codes(report: &SyncReport) -> Vec<SyncCode> {
        report.problems.iter().map(|problem| problem.code).collect()
    }

    #[test]
    fn a_linked_segment_is_neither_read_nor_imported() {
        let case = schemas().join("tests/sync/fresh-local-store");
        let scratch = Scratch::new("linked-segment");
        let outside = scratch.root.join("outside");
        copy_directory(
            &case
                .join("folder/local")
                .join(PROJECT)
                .join("log/dev-7q2m8x4c"),
            &outside,
        );
        let device = scratch
            .folder()
            .join("local")
            .join(PROJECT)
            .join("log/dev-7q2m8x4c");
        std::fs::create_dir_all(&device).expect("creatable");
        symlink(outside.join("1.ndjson"), device.join("1.ndjson")).expect("linkable");

        let report = scratch.store().sync(&scratch.folder()).expect("syncs");
        assert_eq!(codes(&report), [SyncCode::UnsafePath]);
        assert_eq!(report.problems[0].path, device.join("1.ndjson"));
        assert!(report.received.is_empty(), "{report:?}");
    }

    #[test]
    fn a_linked_project_directory_is_not_written() {
        let case = schemas().join("tests/sync/exchange");
        let scratch = Scratch::new("linked-project");
        copy_directory(&case.join("local"), &scratch.local_store());
        let outside = scratch.root.join("outside");
        std::fs::create_dir_all(&outside).expect("creatable");
        std::fs::create_dir_all(scratch.folder().join("local")).expect("creatable");
        symlink(&outside, scratch.folder().join("local").join(PROJECT)).expect("linkable");

        let report = scratch.store().sync(&scratch.folder()).expect("syncs");
        assert_eq!(codes(&report), [SyncCode::UnsafePath]);
        assert!(report.sent.is_empty());
        assert_eq!(
            std::fs::read_dir(&outside).expect("readable").count(),
            0,
            "nothing is written outside the folder"
        );
    }

    #[test]
    fn a_linked_own_device_directory_is_not_written() {
        let case = schemas().join("tests/sync/exchange");
        let scratch = Scratch::new("linked-device");
        copy_directory(&case.join("local"), &scratch.local_store());
        let outside = scratch.root.join("outside");
        std::fs::create_dir_all(&outside).expect("creatable");
        let log = scratch.folder().join("local").join(PROJECT).join("log");
        std::fs::create_dir_all(&log).expect("creatable");
        symlink(&outside, log.join("dev-3k9a5h2t")).expect("linkable");

        let report = scratch.store().sync(&scratch.folder()).expect("syncs");
        assert!(codes(&report).contains(&SyncCode::UnsafePath), "{report:?}");
        assert!(report.sent.is_empty());
        assert_eq!(std::fs::read_dir(&outside).expect("readable").count(), 0);
    }

    #[test]
    fn a_hard_linked_own_segment_is_not_written() {
        let case = schemas().join("tests/sync/push-completes-torn-line");
        let scratch = Scratch::new("hard-linked-own");
        copy_directory(&case.join("local"), &scratch.local_store());
        let outside = scratch.root.join("outside.ndjson");
        std::fs::write(&outside, "").expect("writable");
        let log = scratch
            .folder()
            .join("local")
            .join(PROJECT)
            .join("log/dev-3k9a5h2t");
        std::fs::create_dir_all(&log).expect("creatable");
        std::fs::hard_link(&outside, log.join("1.ndjson")).expect("linkable");

        let report = scratch.store().sync(&scratch.folder()).expect("syncs");
        assert!(codes(&report).contains(&SyncCode::UnsafePath), "{report:?}");
        assert!(report.sent.is_empty());
        assert_eq!(std::fs::read(&outside).expect("readable"), b"");
    }

    #[test]
    fn a_hard_linked_foreign_segment_is_not_imported() {
        let case = schemas().join("tests/sync/fresh-local-store");
        let scratch = Scratch::new("hard-linked-foreign");
        let outside = scratch.root.join("outside.ndjson");
        std::fs::copy(
            case.join("folder/local")
                .join(PROJECT)
                .join("log/dev-7q2m8x4c/1.ndjson"),
            &outside,
        )
        .expect("copyable");
        let log = scratch
            .folder()
            .join("local")
            .join(PROJECT)
            .join("log/dev-7q2m8x4c");
        std::fs::create_dir_all(&log).expect("creatable");
        std::fs::hard_link(&outside, log.join("1.ndjson")).expect("linkable");

        let report = scratch.store().sync(&scratch.folder()).expect("syncs");
        assert_eq!(codes(&report), [SyncCode::UnsafePath]);
        assert!(report.received.is_empty(), "{report:?}");
    }
}
