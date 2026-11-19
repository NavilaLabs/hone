//! The process store (concept Section 4): the event fixtures under `schemas/tests/events/`,
//! appending, the reading rules, and the order.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use hone_core::{
    Author, Change, ProcessError, ProcessStore, StepStatus, ThreadAnchor, ThreadStatus,
    TicketEvents, content_hash,
};
use serde_json::{Value, json};

const TICKET: &str = "tk-9f2x4a8m";
const PROJECT: &str = "prj-8m2k4d7q";
const DEVICE_A: &str = "dev-3k9a5h2t";
const DEVICE_B: &str = "dev-7q2m8x4c";

fn schemas() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../schemas")
}

// A copy of a gate test project and an empty data directory, removed on drop.
struct Workspace {
    root: PathBuf,
}

impl Workspace {
    fn new(name: &str) -> Self {
        let root = std::env::temp_dir().join(format!("hone-process-{}-{name}", std::process::id()));
        if root.exists() {
            std::fs::remove_dir_all(&root).expect("removable");
        }
        copy_directory(
            &schemas().join("tests/check/steps-complete-incomplete/project"),
            &root.join("project"),
        );
        Self { root }
    }

    fn project(&self) -> PathBuf {
        self.root.join("project")
    }

    fn data(&self) -> PathBuf {
        self.root.join("data")
    }

    fn store(&self) -> ProcessStore {
        ProcessStore::locate(&self.project(), Some(&self.data())).expect("locatable")
    }

    fn log(&self, device: &str) -> PathBuf {
        self.data()
            .join("local")
            .join(PROJECT)
            .join("log")
            .join(device)
    }

    fn write_segment(&self, device: &str, name: &str, contents: &str) {
        let directory = self.log(device);
        std::fs::create_dir_all(&directory).expect("creatable");
        std::fs::write(directory.join(name), contents).expect("writable");
    }

    fn events(&self) -> TicketEvents {
        self.store().events(TICKET).expect("readable")
    }
}

impl Drop for Workspace {
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

// The canonical form of RFC 8785 for the ASCII-only, float-free data of these tests: sorted
// keys, no whitespace. Written here so that the tests do not depend on the implementation.
fn canonical(value: &Value) -> String {
    match value {
        Value::Object(map) => {
            let sorted: BTreeMap<&String, &Value> = map.iter().collect();
            let members: Vec<String> = sorted
                .into_iter()
                .map(|(key, value)| format!("{}:{}", Value::String(key.clone()), canonical(value)))
                .collect();
            format!("{{{}}}", members.join(","))
        }
        Value::Array(items) => {
            let items: Vec<String> = items.iter().map(canonical).collect();
            format!("[{}]", items.join(","))
        }
        scalar => scalar.to_string(),
    }
}

fn event_id(number: u64) -> String {
    format!("ev-0193a8f0-9b2c-7e4d-a1f3-{number:012}")
}

fn event(device: &str, seq: u64, number: u64) -> Value {
    json!({
        "id": event_id(number),
        "project": PROJECT,
        "ticket": TICKET,
        "device": device,
        "seq": seq,
        "type": "step.changed",
        "author": { "name": "alice", "role": "human" },
        "payload": { "step": "decision", "status": "done" },
    })
}

fn lines(events: &[Value]) -> String {
    events
        .iter()
        .map(|event| format!("{}\n", canonical(event)))
        .collect()
}

fn ids(events: &TicketEvents) -> Vec<String> {
    events
        .events
        .iter()
        .map(|event| event["id"].as_str().expect("id").to_owned())
        .collect()
}

fn messages(events: &TicketEvents) -> Vec<String> {
    events
        .invalid
        .iter()
        .map(|invalid| {
            let line = invalid
                .line
                .map_or(String::new(), |line| format!(":{line}"));
            format!("{}{line} {}", invalid.path, invalid.message)
        })
        .collect()
}

fn bob() -> Author {
    Author {
        name: "bob".into(),
        role: "human".into(),
    }
}

fn alice() -> Author {
    Author {
        name: "alice".into(),
        role: "human".into(),
    }
}

fn code_thread(blocking: bool) -> Change {
    Change::OpenThread {
        ticket: TICKET.into(),
        kind: "concern".into(),
        blocking,
        anchor: ThreadAnchor::Code {
            reference: "src/ledger.rs#Ledger".into(),
        },
        body: "What happens when the queue fails?".into(),
    }
}

fn invalid_message(error: ProcessError) -> String {
    match error {
        ProcessError::Invalid { message } => message,
        other => panic!("expected an invalid request, got {other:?}"),
    }
}

#[test]
fn event_fixtures() {
    let workspace = Workspace::new("fixtures");
    let expected = std::fs::read_to_string(schemas().join("tests/events/invalid/expected.txt"))
        .expect("expected.txt");
    let expected: BTreeMap<&str, &str> = expected
        .lines()
        .map(|line| line.split_once(' ').expect("`<file> <reason>`"))
        .collect();
    let mut checked = 0;
    for (kind, valid) in [("valid", true), ("invalid", false)] {
        let mut files: Vec<PathBuf> = std::fs::read_dir(schemas().join("tests/events").join(kind))
            .expect("fixture directory")
            .map(|entry| entry.expect("entry").path())
            .filter(|path| {
                path.extension()
                    .is_some_and(|extension| extension == "json")
            })
            .collect();
        files.sort();
        for file in files {
            let name = file.file_name().and_then(|n| n.to_str()).expect("name");
            let fixture: Value =
                serde_json::from_slice(&std::fs::read(&file).expect("readable")).expect("JSON");
            let device = fixture["device"]
                .as_str()
                .filter(|device| device.starts_with("dev-"))
                .unwrap_or(DEVICE_A)
                .to_owned();
            let _ = std::fs::remove_dir_all(workspace.data());
            workspace.write_segment(&device, "1.ndjson", &lines(&[fixture]));
            let events = workspace.events();
            if valid {
                assert_eq!(messages(&events), Vec::<String>::new(), "{name}");
                assert_eq!(events.events.len(), 1, "{name}");
            } else {
                let reason = expected
                    .get(name)
                    .unwrap_or_else(|| panic!("{name} has no line in expected.txt"));
                let found = messages(&events);
                assert!(
                    found.len() == 1 && found[0].contains(reason),
                    "{name}: expected one invalid event because of `{reason}`, found {found:?}"
                );
                assert!(events.events.is_empty(), "{name}");
            }
            checked += 1;
        }
    }
    assert_eq!(checked, expected.len() + 17, "every fixture was checked");
}

#[test]
fn append_writes_canonical_lines_and_computes_the_thread() {
    let workspace = Workspace::new("append");
    let store = workspace.store();
    let opened = store.append(&bob(), code_thread(true)).expect("opened");
    let thread = opened.thread.clone().expect("thread ID");
    assert_eq!(
        thread,
        opened.event["id"]
            .as_str()
            .expect("id")
            .replacen("ev-", "th-", 1)
    );
    store
        .append(
            &alice(),
            Change::Reply {
                thread: thread.clone(),
                body: "Fall back to synchronous processing.".into(),
            },
        )
        .expect("replied");
    store
        .append(
            &alice(),
            Change::Resolve {
                thread: thread.clone(),
            },
        )
        .expect("resolved");

    let device = std::fs::read_to_string(store.directory().join("device")).expect("device");
    let device = device.trim();
    assert!(device.len() == 12 && device.starts_with("dev-"), "{device}");
    let segment = std::fs::read_to_string(workspace.log(device).join("1.ndjson")).expect("segment");
    let written: Vec<&str> = segment.lines().collect();
    assert_eq!(written.len(), 3);
    assert!(segment.ends_with('\n'));
    for (index, line) in written.iter().enumerate() {
        let event: Value = serde_json::from_str(line).expect("JSON");
        assert_eq!(&canonical(&event), line, "line {index} is canonical");
        assert_eq!(event["seq"], index as u64 + 1);
        assert_eq!(event["device"], device);
        assert!(event.get("after").is_none(), "no other device, no `after`");
    }

    let events = workspace.events();
    assert_eq!(events.events.len(), 3);
    assert_eq!(events.threads.len(), 1);
    let state = &events.threads[0];
    assert_eq!(state.thread, thread);
    assert_eq!(state.status, ThreadStatus::Resolved);
    assert!(state.answered && state.blocking);
    assert_eq!(state.opened_by, bob());
}

#[test]
fn append_points_after_to_the_newest_event_of_every_other_device() {
    let workspace = Workspace::new("after");
    workspace.write_segment(
        DEVICE_B,
        "1.ndjson",
        &lines(&[event(DEVICE_B, 1, 7), event(DEVICE_B, 2, 3)]),
    );
    let appended = workspace
        .store()
        .append(
            &alice(),
            Change::SetStep {
                ticket: TICKET.into(),
                step: "specify".into(),
                status: StepStatus::Skipped,
                reason: Some("no new terms".into()),
            },
        )
        .expect("appended");
    assert_eq!(appended.event["after"], json!([event_id(3)]));
    let events = workspace.events();
    assert_eq!(
        ids(&events).last().map(String::as_str),
        appended.event["id"].as_str()
    );
    assert_eq!(events.steps["specify"].status, StepStatus::Skipped);
    assert_eq!(
        events.steps["specify"].reason.as_deref(),
        Some("no new terms")
    );
}

#[test]
fn append_continues_the_sequence_and_closes_a_full_segment() {
    let workspace = Workspace::new("segments");
    std::fs::create_dir_all(workspace.data().join("local").join(PROJECT)).expect("creatable");
    std::fs::write(
        workspace.data().join("local").join(PROJECT).join("device"),
        format!("{DEVICE_A}\n"),
    )
    .expect("writable");
    let full: Vec<Value> = (1..=1000).map(|seq| event(DEVICE_A, seq, seq)).collect();
    workspace.write_segment(DEVICE_A, "1.ndjson", &lines(&full));
    let appended = workspace
        .store()
        .append(&bob(), code_thread(false))
        .expect("appended");
    assert_eq!(appended.event["seq"], 1001);
    let second = std::fs::read_to_string(workspace.log(DEVICE_A).join("2.ndjson")).expect("2");
    assert_eq!(second.lines().count(), 1);
    let events = workspace.events();
    assert_eq!(messages(&events), Vec::<String>::new());
    assert_eq!(events.events.len(), 1001);
}

#[test]
fn append_cuts_off_an_interrupted_line() {
    let workspace = Workspace::new("interrupted");
    std::fs::create_dir_all(workspace.data().join("local").join(PROJECT)).expect("creatable");
    std::fs::write(
        workspace.data().join("local").join(PROJECT).join("device"),
        format!("{DEVICE_A}\n"),
    )
    .expect("writable");
    let torn = format!("{}{{\"id\":\"ev-0193", lines(&[event(DEVICE_A, 1, 1)]));
    workspace.write_segment(DEVICE_A, "1.ndjson", &torn);
    assert_eq!(
        messages(&workspace.events()),
        Vec::<String>::new(),
        "ignored while open"
    );
    let appended = workspace
        .store()
        .append(&bob(), code_thread(false))
        .expect("appended");
    assert_eq!(appended.event["seq"], 2);
    let segment = std::fs::read_to_string(workspace.log(DEVICE_A).join("1.ndjson")).expect("1");
    assert_eq!(segment.lines().count(), 2);
    assert!(!segment.contains("{\"id\":\"ev-0193\n"));
    assert_eq!(workspace.events().events.len(), 2);
}

#[test]
fn reading_rejects_events_that_break_the_rules() {
    let workspace = Workspace::new("rules");
    let mut foreign = event(DEVICE_B, 2, 20);
    foreign["project"] = json!("prj-0000aaaa");
    workspace.write_segment(
        DEVICE_A,
        "1.ndjson",
        &format!(
            "{}{}not json\n{}{{ \"id\": 1 }}\n{}",
            lines(&[event(DEVICE_A, 1, 1)]),
            lines(&[event(DEVICE_B, 2, 2)]),
            lines(&[event(DEVICE_A, 4, 4)]),
            lines(&[event(DEVICE_A, 9, 9)]),
        ),
    );
    workspace.write_segment(
        DEVICE_A,
        "2.ndjson",
        &format!(
            "{}{}",
            lines(&[event(DEVICE_A, 7, 10)]),
            canonical(&event(DEVICE_A, 8, 11))
        ),
    );
    workspace.write_segment(DEVICE_A, "3.ndjson", "");
    workspace.write_segment(DEVICE_A, "4.ndjson.zst", "");
    workspace.write_segment(DEVICE_B, "1.ndjson", &lines(&[foreign]));
    let mut conflicting = event("dev-9c3d6h4v", 1, 1);
    conflicting["payload"]["status"] = json!("skipped");
    workspace.write_segment("dev-9c3d6h4v", "1.ndjson", &lines(&[conflicting]));
    workspace.write_segment(
        "dev-4t8w2n6c",
        "3.ndjson",
        &lines(&[
            event("dev-4t8w2n6c", 2001, 31),
            event("dev-4t8w2n6c", 2002, 32),
        ]),
    );

    let events = workspace.events();
    assert_eq!(
        messages(&events),
        [
            "log/dev-3k9a5h2t/1.ndjson:1 another event carries the ID `ev-0193a8f0-9b2c-7e4d-a1f3-000000000001` with different content",
            "log/dev-3k9a5h2t/1.ndjson:2 the event of `dev-7q2m8x4c` lies in the log of `dev-3k9a5h2t`",
            "log/dev-3k9a5h2t/1.ndjson:3 the line is not JSON",
            "log/dev-3k9a5h2t/1.ndjson:5 the line is not in the canonical form of RFC 8785",
            "log/dev-3k9a5h2t/1.ndjson:6 `seq` is 9, expected 6",
            "log/dev-3k9a5h2t/2.ndjson:2 the last line of a closed segment does not end with a newline",
            "log/dev-3k9a5h2t/4.ndjson.zst compressed segments are not supported yet",
            "log/dev-7q2m8x4c/1.ndjson:1 the event belongs to the project `prj-0000aaaa`, not `prj-8m2k4d7q`",
            "log/dev-9c3d6h4v/1.ndjson:1 another event carries the ID `ev-0193a8f0-9b2c-7e4d-a1f3-000000000001` with different content",
        ]
    );
    let valid = ids(&events);
    assert!(
        valid.contains(&event_id(4)),
        "line 4 keeps its place: {valid:?}"
    );
    assert!(
        valid.contains(&event_id(10)),
        "segment 2 continues at 7: {valid:?}"
    );
    assert!(
        valid.contains(&event_id(31)) && valid.contains(&event_id(32)),
        "a gap where whole segments are missing is allowed: {valid:?}"
    );
}

#[test]
fn order_follows_after_and_seq_before_id() {
    let workspace = Workspace::new("order");
    let mut late = event(DEVICE_B, 1, 2);
    late["after"] = json!([event_id(8), "ev-0193a8f0-0000-7000-8000-000000000099"]);
    workspace.write_segment(
        DEVICE_A,
        "1.ndjson",
        &lines(&[event(DEVICE_A, 1, 9), event(DEVICE_A, 2, 1)]),
    );
    workspace.write_segment(DEVICE_B, "1.ndjson", &lines(&[late, event(DEVICE_B, 2, 5)]));
    let mut first_cycle = event("dev-9c3d6h4v", 1, 7);
    first_cycle["after"] = json!([event_id(6)]);
    let mut second_cycle = event("dev-4t8w2n6c", 1, 6);
    second_cycle["after"] = json!([event_id(7)]);
    workspace.write_segment("dev-9c3d6h4v", "1.ndjson", &lines(&[first_cycle]));
    workspace.write_segment("dev-4t8w2n6c", "1.ndjson", &lines(&[second_cycle]));
    workspace.write_segment(
        "dev-1a2b3c4d",
        "1.ndjson",
        &lines(&[event("dev-1a2b3c4d", 1, 8)]),
    );
    let numbers: Vec<String> = ids(&workspace.events())
        .iter()
        .map(|id| {
            id.trim_start_matches("ev-0193a8f0-9b2c-7e4d-a1f3-")
                .trim_start_matches('0')
                .to_owned()
        })
        .collect();
    // 9 before 1: same device, seq wins over id. 2 after 8: `after`, the unknown reference does
    // not count. 6 and 7 wait on each other: once nothing else is ready, the cycle is broken by
    // id.
    assert_eq!(numbers, ["8", "2", "5", "9", "1", "6", "7"]);
}

#[test]
fn a_missing_store_is_unavailable_and_an_empty_one_has_no_events() {
    let workspace = Workspace::new("availability");
    let store = workspace.store();
    assert!(matches!(
        store.events(TICKET),
        Err(ProcessError::Unavailable { .. })
    ));
    std::fs::create_dir_all(store.directory()).expect("creatable");
    let events = store.events(TICKET).expect("readable");
    assert!(events.events.is_empty() && events.threads.is_empty() && events.invalid.is_empty());
}

#[test]
fn append_rejects_changes_that_do_not_fit_the_project() {
    let workspace = Workspace::new("rejected");
    let store = workspace.store();
    let step = |step: &str| Change::SetStep {
        ticket: TICKET.into(),
        step: step.into(),
        status: StepStatus::Done,
        reason: None,
    };
    let reason = |author: &Author, change| {
        invalid_message(store.append(author, change).expect_err("rejected"))
    };
    assert!(reason(&alice(), step("domain")).contains("does not list the step `domain`"));
    let critic = Author {
        name: "critic-bot".into(),
        role: "critic".into(),
    };
    assert!(reason(&critic, step("decision")).contains("the role `critic` does not exist"));
    let unknown_thread = Change::Resolve {
        thread: "th-0193a8f0-9b2c-7e4d-a1f3-000000000001".into(),
    };
    assert!(reason(&alice(), unknown_thread).contains("no thread"));
    let unknown_artifact = Change::OpenThread {
        ticket: TICKET.into(),
        kind: "concern".into(),
        blocking: false,
        anchor: ThreadAnchor::Artifact {
            artifact: "dec-00000000".into(),
            quote: None,
        },
        body: "Where is it?".into(),
    };
    assert!(reason(&alice(), unknown_artifact).contains("no artifact `dec-00000000`"));
    let empty_body = Change::OpenThread {
        ticket: TICKET.into(),
        kind: "concern".into(),
        blocking: false,
        anchor: ThreadAnchor::Code {
            reference: "src/ledger.rs".into(),
        },
        body: String::new(),
    };
    assert!(reason(&alice(), empty_body).contains("/payload/body"));
    assert!(
        !store.directory().exists(),
        "a rejected change creates no store"
    );
    let unknown_ticket = Change::SetStep {
        ticket: "tk-00000000".into(),
        step: "decision".into(),
        status: StepStatus::Done,
        reason: None,
    };
    assert!(matches!(
        store.append(&alice(), unknown_ticket),
        Err(ProcessError::IoError { .. })
    ));
}

#[test]
fn an_artifact_anchor_carries_the_content_hash_of_the_file() {
    let workspace = Workspace::new("anchor");
    let path = "design/decisions/dec-7k3m2q8p-payment-async.md";
    let bytes = std::fs::read(workspace.project().join(path)).expect("decision");
    let appended = workspace
        .store()
        .append(
            &bob(),
            Change::OpenThread {
                ticket: TICKET.into(),
                kind: "concern".into(),
                blocking: true,
                anchor: ThreadAnchor::Artifact {
                    artifact: "dec-7k3m2q8p".into(),
                    quote: Some("asynchronously".into()),
                },
                body: "What happens when the queue fails?".into(),
            },
        )
        .expect("appended");
    let anchor = &appended.event["payload"]["anchor"];
    assert_eq!(anchor["hash"], content_hash(path, &bytes).expect("hash"));
    assert_eq!(anchor["quote"], "asynchronously");
}

#[test]
fn locate_needs_valid_scope_ids() {
    let workspace = Workspace::new("scope");
    let config = workspace.project().join(".hone/config.yaml");
    std::fs::write(&config, "workspace: local\nproject: prj-ABC\n").expect("writable");
    assert!(matches!(
        ProcessStore::locate(&workspace.project(), Some(&workspace.data())),
        Err(ProcessError::IoError { message }) if message.contains("no valid `project`")
    ));
    std::fs::write(&config, "workspace: ws-3k9a5h2t\nproject: prj-8m2k4d7q\n").expect("writable");
    let store = ProcessStore::locate(&workspace.project(), Some(&workspace.data())).expect("ok");
    assert_eq!(
        store.directory(),
        workspace.data().join("ws-3k9a5h2t").join(PROJECT)
    );
}

fn architect() -> Author {
    Author {
        name: "alice".into(),
        role: "architect".into(),
    }
}

const DECISION: &str = "design/decisions/dec-7k3m2q8p-payment-async.md";

fn approval(seq: u64, number: u64, author: (&str, &str), hash: &str, decision: &str) -> Value {
    json!({
        "id": event_id(number),
        "project": PROJECT,
        "ticket": TICKET,
        "device": DEVICE_A,
        "seq": seq,
        "type": "approval.given",
        "author": { "name": author.0, "role": author.1 },
        "payload": { "artifact": "dec-7k3m2q8p", "hash": hash, "decision": decision },
    })
}

#[test]
fn approve_binds_the_current_hash_and_checks_the_role() {
    let workspace = Workspace::new("approve");
    let bytes = std::fs::read(workspace.project().join(DECISION)).expect("decision");
    let hash = content_hash(DECISION, &bytes).expect("hash");
    let approve = |proof: Option<&str>| Change::Approve {
        ticket: TICKET.into(),
        artifact: "dec-7k3m2q8p".into(),
        changes_requested: false,
        proof: proof.map(str::to_owned),
    };

    let appended = workspace
        .store()
        .append(&architect(), approve(Some("signed")))
        .expect("appended");
    assert_eq!(appended.event["type"], "approval.given");
    assert_eq!(
        appended.event["payload"],
        json!({ "artifact": "dec-7k3m2q8p", "hash": hash, "decision": "approved", "proof": "signed" })
    );

    let not_an_actor = Author {
        name: "bob".into(),
        role: "architect".into(),
    };
    let error = workspace
        .store()
        .append(&not_an_actor, approve(None))
        .expect_err("bob is no architect");
    assert_eq!(
        invalid_message(error),
        "`actors` does not give `bob` the role `architect`"
    );

    let error = workspace
        .store()
        .append(
            &not_an_actor,
            Change::ConfirmReview {
                ticket: TICKET.into(),
                artifact: "dec-7k3m2q8p".into(),
            },
        )
        .expect_err("bob is no architect");
    assert_eq!(
        invalid_message(error),
        "`actors` does not give `bob` the role `architect`"
    );

    let confirmed = workspace
        .store()
        .append(
            &alice(),
            Change::ConfirmReview {
                ticket: TICKET.into(),
                artifact: "dec-7k3m2q8p".into(),
            },
        )
        .expect("no step names `decides`, and human may approve");
    assert_eq!(
        confirmed.event["payload"],
        json!({ "artifact": "dec-7k3m2q8p", "hash": hash })
    );
}

#[test]
fn condense_takes_the_last_decision_of_every_person_for_the_current_hash() {
    let workspace = Workspace::new("condense");
    let file = workspace.project().join(DECISION);
    let original = std::fs::read_to_string(&file).expect("decision");
    let hash = content_hash(DECISION, original.as_bytes()).expect("hash");
    let stale = format!("sha256:{}", "0".repeat(64));
    workspace.write_segment(
        DEVICE_A,
        "1.ndjson",
        &lines(&[
            approval(1, 1, ("bob", "human"), &hash, "approved"),
            approval(2, 2, ("alice", "architect"), &hash, "approved"),
            approval(3, 3, ("bob", "human"), &hash, "changes-requested"),
            approval(4, 4, ("alice", "human"), &stale, "approved"),
        ]),
    );
    let summary = workspace.root.join("summary.yaml");
    std::fs::write(
        &summary,
        "- q: \"Why \\\"async\\\"?\"\n  a: \"Because.\"\n- rejected: \"Outbox\"\n",
    )
    .expect("writable");

    let condensed = workspace
        .store()
        .condense(TICKET, "dec-7k3m2q8p", Some(&summary))
        .expect("condensed");
    assert_eq!(condensed.path, DECISION);

    // 0x0193a8f0 9b2c: `date -u -d @1733706160 +%F` is 2024-12-09.
    let before_review = &original[..original.find("review:\n").expect("a review block")];
    let expected = format!(
        "{before_review}review:\n  approved_by:\n    - {{ name: \"alice\", role: \"architect\", hash: \"{hash}\", date: \"2024-12-09\" }}\n  summary:\n    - {{ q: \"Why \\\"async\\\"?\", a: \"Because.\" }}\n    - {{ rejected: \"Outbox\" }}\n---\nText.\n"
    );
    assert_eq!(std::fs::read_to_string(&file).expect("condensed"), expected);
    assert_eq!(
        content_hash(DECISION, expected.as_bytes()).expect("hash"),
        hash,
        "the review block is outside the content hash"
    );
}

#[test]
fn condense_refuses_a_malformed_summary_and_leaves_the_file() {
    let workspace = Workspace::new("condense-summary");
    let file = workspace.project().join(DECISION);
    let original = std::fs::read(&file).expect("decision");
    let summary = workspace.root.join("summary.yaml");
    std::fs::write(&summary, "- q: \"Only a question\"\n").expect("writable");

    let error = workspace
        .store()
        .condense(TICKET, "dec-7k3m2q8p", Some(&summary))
        .expect_err("malformed");
    assert_eq!(
        invalid_message(error),
        "summary entry 1 must be `{ q, a }` or `{ rejected }` with text values"
    );
    assert_eq!(std::fs::read(&file).expect("decision"), original);
    assert_eq!(
        std::fs::read_dir(file.parent().expect("a directory"))
            .expect("readable")
            .count(),
        2,
        "no temporary file is left"
    );
}
