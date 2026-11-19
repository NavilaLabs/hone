//! The GUI's interface to hone: the real command line, called through `hone_gui::hone`. A change
//! to the JSON of the command line turns this test red (concept Section 12, "Desktop GUI").
//!
//! It needs the `hone` binary: `HONE_BIN`, otherwise `target/debug/hone` of the main workspace
//! (`cargo build -p hone-cli` at the repository root).

use std::path::{Path, PathBuf};

use hone_gui::hone::{Anchor, Hone, Peer, StepStatus, Write, Writer};

const TICKET: &str = "tk-9f2x4a8m";
const THREAD: &str = "th-0193a8f0-9b2c-7e4d-a1f3-000000000001";

fn repository() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("..")
}

fn binary() -> PathBuf {
    let binary = std::env::var_os("HONE_BIN").map_or_else(
        || {
            repository()
                .join("target/debug")
                .join(format!("hone{}", std::env::consts::EXE_SUFFIX))
        },
        PathBuf::from,
    );
    assert!(
        binary.is_file(),
        "{} is missing: run `cargo build -p hone-cli` at the repository root or set HONE_BIN",
        binary.display()
    );
    binary
}

// A temporary directory with a copy of a gate test case's project and process store.
struct Scratch {
    root: PathBuf,
}

impl Scratch {
    fn new(name: &str) -> Self {
        let root = std::env::temp_dir().join(format!("hone-gui-{}-{name}", std::process::id()));
        if root.exists() {
            std::fs::remove_dir_all(&root).expect("removable");
        }
        let case = repository().join("schemas/tests/check/no-open-blocking-threads-open");
        copy(&case.join("project"), &root.join("project"));
        Self { root }
    }

    fn with_store(self) -> Self {
        let case = repository().join("schemas/tests/check/no-open-blocking-threads-open");
        copy(&case.join("data"), &self.root.join("data"));
        self
    }

    fn hone(&self) -> Hone {
        Hone::with_binary(binary(), self.root.join("project"))
            .with_variable("HONE_DATA_DIR", self.root.join("data"))
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn copy(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).expect("creatable");
    for entry in std::fs::read_dir(from).expect("readable") {
        let entry = entry.expect("readable");
        let target = to.join(entry.file_name());
        if entry.file_type().expect("typed").is_dir() {
            copy(&entry.path(), &target);
        } else {
            std::fs::copy(entry.path(), target).expect("copyable");
        }
    }
}

fn alice() -> Writer {
    Writer {
        author: "alice".into(),
        role: "architect".into(),
    }
}

#[tokio::test]
async fn reading_follows_the_store() {
    let scratch = Scratch::new("read");
    let failure = scratch.hone().log().await.expect_err("no store yet");
    assert_eq!(failure.status, "unavailable");

    let scratch = scratch.with_store();
    let log = scratch.hone().log().await.expect("readable");
    assert_eq!(
        log.cursor.into_iter().collect::<Vec<_>>(),
        [("dev-3k9a5h2t".to_owned(), 1)]
    );
    assert_eq!(log.events.len(), 1);
    assert_eq!(log.events[0].kind, "thread.opened");
    assert_eq!(log.events[0].ticket.as_deref(), Some(TICKET));
    assert_eq!(log.events[0].raw["seq"], 1);

    let state = scratch.hone().events(TICKET).await.expect("readable");
    assert_eq!(state.threads.len(), 1);
    let thread = &state.threads[0];
    assert_eq!(
        (
            thread.thread.as_str(),
            thread.status.as_str(),
            thread.blocking,
            thread.answered
        ),
        (THREAD, "open", true, false)
    );
    assert_eq!(thread.opened_by.name, "bob");
    assert!(state.steps.is_empty() && state.invalid.is_empty());
}

#[tokio::test]
async fn writing_appends_and_rejects_with_the_reason() {
    let scratch = Scratch::new("write").with_store();
    let hone = scratch.hone();

    let reply = Write::Reply {
        thread: THREAD.into(),
        body: "It retries.".into(),
    };
    let appended = hone.write(&alice(), &reply).await.expect("written");
    assert_eq!(appended.event.kind, "thread.message");
    assert_eq!(appended.event.payload["thread"], THREAD);
    assert_eq!(appended.thread.as_deref(), Some(THREAD));

    let open = Write::OpenThread {
        ticket: TICKET.into(),
        kind: "question".into(),
        blocking: false,
        anchor: Anchor::Code("src/ledger.rs#Ledger".into()),
        body: "Why async?".into(),
    };
    let opened = hone.write(&alice(), &open).await.expect("written");
    let uuid = opened.event.id.strip_prefix("ev-").expect("an event ID");
    assert_eq!(opened.thread, Some(format!("th-{uuid}")));

    let unknown = Write::SetStep {
        ticket: TICKET.into(),
        step: "nope".into(),
        status: StepStatus::Done,
        reason: None,
    };
    let failure = hone
        .write(&alice(), &unknown)
        .await
        .expect_err("not in the profile");
    assert_eq!(failure.status, "invalid");
    assert_eq!(
        failure.message,
        "the profile `standard` of the ticket does not list the step `nope`"
    );

    let skipped = Write::SetStep {
        ticket: TICKET.into(),
        step: "interfaces".into(),
        status: StepStatus::Skipped,
        reason: Some("no new terms".into()),
    };
    hone.write(&alice(), &skipped).await.expect("written");
    let state = hone.events(TICKET).await.expect("readable");
    let step = &state.steps["interfaces"];
    assert_eq!(
        (step.status.as_str(), step.reason.as_deref()),
        ("skipped", Some("no new terms"))
    );
    assert!(state.threads[0].answered);
}

#[tokio::test]
async fn a_failed_gate_is_a_result() {
    let scratch = Scratch::new("check").with_store();
    let run = scratch.hone().check(Some(TICKET)).await.expect("ran");
    assert_eq!(run.status, "failed");
    let failed: Vec<_> = run.gates.iter().filter(|gate| !gate.ok).collect();
    assert_eq!(failed.len(), 1);
    assert_eq!(failed[0].name, "implementation/binding");
    assert_eq!(failed[0].gate, "change-needs-binding");
    assert_eq!(failed[0].findings[0].code, "changed-paths-missing");
    assert_eq!(failed[0].findings[0].level, "error");

    let elsewhere = Hone::with_binary(binary(), scratch.root.join("nowhere"));
    let failure = elsewhere.check(None).await.expect_err("could not run");
    assert_eq!(failure.status, "error");
}

#[tokio::test]
async fn sync_reports_what_it_moved_and_a_missing_folder_fails() {
    let scratch = Scratch::new("sync").with_store();
    let hone = scratch.hone();
    let reply = Write::Reply {
        thread: THREAD.into(),
        body: "It retries.".into(),
    };
    let own = hone
        .write(&alice(), &reply)
        .await
        .expect("written")
        .event
        .device;

    let folder = scratch.root.join("folder");
    std::fs::create_dir_all(&folder).expect("creatable");
    let peer = Peer::Folder(folder.display().to_string());
    let report = hone.sync(&peer).await.expect("synced");
    assert_eq!(report.sent.into_iter().collect::<Vec<_>>(), [(own, 1)]);
    assert!(report.received.is_empty() && report.problems.is_empty());

    let missing = Peer::Folder(scratch.root.join("missing").display().to_string());
    let failure = hone.sync(&missing).await.expect_err("no folder");
    assert_eq!(failure.status, "unavailable");
    assert!(!scratch.root.join("missing").exists());
}

#[tokio::test]
async fn a_missing_binary_is_not_started() {
    let scratch = Scratch::new("missing-binary");
    let hone = Hone::with_binary(
        scratch.root.join("no-such-hone"),
        scratch.root.join("project"),
    );
    let failure = hone.log().await.expect_err("not started");
    assert_eq!(failure.status, "not-started");
}

#[tokio::test]
async fn a_server_that_cannot_start_reports_and_ends() {
    let scratch = Scratch::new("serve");
    let config = scratch.root.join("server.yaml");
    std::fs::write(&config, "listen: [not, an, address]\n").expect("writable");
    let mut server = scratch.hone().serve(&config).expect("spawned");
    let mut lines = Vec::new();
    while let Some(line) = server.next_line().await {
        lines.push(line);
    }
    assert!(!lines.is_empty(), "the server says why it did not start");
    assert!(lines.join("\n").contains("server.yaml"), "{lines:?}");
}
