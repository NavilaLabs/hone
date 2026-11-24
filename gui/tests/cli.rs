//! The GUI's interface to hone: the real command line, called through `hone_gui::hone`. A change
//! to the JSON of the command line turns this test red (concept Section 12, "Desktop GUI").
//!
//! It needs the `hone` binary: `HONE_BIN`, otherwise `target/debug/hone` of the main workspace
//! (`cargo build -p hone-cli` at the repository root).

use std::path::{Path, PathBuf};

use hone_gui::hone::{
    Anchor, AttributeValue, ConfigView, GateScope, Hone, Peer, StepStatus, TicketStart, Write,
    Writer,
};

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
    let run = scratch
        .hone()
        .check(Some(TICKET), GateScope::All)
        .await
        .expect("ran");
    assert_eq!(run.status, "failed");
    let failed: Vec<_> = run.gates.iter().filter(|gate| !gate.ok).collect();
    assert_eq!(failed.len(), 1);
    assert_eq!(failed[0].name, "implementation/binding");
    assert_eq!(failed[0].gate, "change-needs-binding");
    assert_eq!(failed[0].findings[0].code, "changed-paths-missing");
    assert_eq!(failed[0].findings[0].level, "error");

    let elsewhere = Hone::with_binary(binary(), scratch.root.join("nowhere"));
    let failure = elsewhere
        .check(None, GateScope::All)
        .await
        .expect_err("could not run");
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
async fn the_lists_to_choose_from_come_from_the_command_line() {
    let scratch = Scratch::new("lists");
    let hone = scratch.hone();

    let catalog = hone.artifacts().await.expect("listed");
    assert_eq!(catalog.project, "prj-8m2k4d7q");
    let ids: Vec<(Option<&str>, &str)> = catalog
        .artifacts
        .iter()
        .map(|entry| (entry.id.as_deref(), entry.kind.as_str()))
        .collect();
    assert_eq!(
        ids,
        [
            (Some("dec-7k3m2q8p"), "decision"),
            (Some("mod-4t8w2n6c"), "module"),
            (Some("spec-3b8n1c5w"), "spec"),
            (Some(TICKET), "ticket"),
        ]
    );
    let record = catalog.tickets().next().expect("a ticket record");
    assert_eq!(record.profile.as_deref(), Some("standard"));
    assert_eq!(catalog.artifacts[0].effective.as_deref(), Some("accepted"));

    let overview = hone.ticket(TICKET).await.expect("shown");
    assert_eq!(overview.roles, ["architect", "human"]);
    let steps: Vec<(&str, &str)> = overview
        .steps
        .iter()
        .map(|step| (step.step.as_str(), step.requirement.as_str()))
        .collect();
    assert_eq!(
        steps,
        [
            ("completion", "optional"),
            ("decision", "required"),
            ("implementation", "required"),
            ("interfaces", "optional"),
            ("settled", "optional"),
            ("specify", "required"),
        ]
    );
    let failure = hone.ticket("tk-2b7n5k1q").await.expect_err("no record");
    assert_eq!(failure.status, "error");
}

#[tokio::test]
async fn the_context_author_comes_from_the_bound_context() {
    let scratch = Scratch::new("context");
    let config = scratch.root.join("config");
    let hone = scratch.hone().with_variable("HONE_CONFIG_DIR", &config);
    let unbound = hone.context().await.expect_err("no binding");
    assert_eq!(unbound.status, "invalid");

    let project = scratch.root.join("project");
    for arguments in [
        vec![
            "context", "set", "acme", "--folder", "/tmp", "--author", "carol", "--role", "human",
        ],
        vec!["context", "use", "acme", "--root"],
    ] {
        let mut command = std::process::Command::new(binary());
        command.args(&arguments).env("HONE_CONFIG_DIR", &config);
        if arguments[1] == "use" {
            command.arg(&project);
        }
        let status = command.status().expect("hone runs");
        assert!(status.success(), "{arguments:?}");
    }
    let bound = hone.context().await.expect("bound");
    assert_eq!(bound.context, "acme");
    let author = bound.author.expect("an author");
    assert_eq!(
        (author.name.as_str(), author.role.as_str()),
        ("carol", "human")
    );
}

#[tokio::test]
async fn the_gui_check_is_stored_and_the_ticket_list_reads_it() {
    let scratch = Scratch::new("runs").with_store();
    let hone = scratch.hone();
    let before = hone.ticket_list().await.expect("readable");
    assert_eq!(before.tickets.len(), 1);
    assert_eq!(before.tickets[0].ticket, TICKET);
    assert_eq!(before.tickets[0].last_run, None);
    assert_eq!(before.tickets[0].threads.blocking, 1);

    let run = hone
        .check(Some(TICKET), GateScope::Pure)
        .await
        .expect("ran");
    let id = run.run.clone().expect("a stored run");
    let runs = hone.runs(TICKET).await.expect("readable");
    assert_eq!(runs.len(), 1);
    assert_eq!(
        (runs[0].id.as_str(), runs[0].source.as_str()),
        (id.as_str(), "gui")
    );
    assert_eq!(runs[0].arguments["class"], "pure");
    assert_eq!(runs[0].gates, run.gates);

    let after = hone.ticket_list().await.expect("readable");
    let last = after.tickets[0].last_run.clone().expect("the run");
    assert_eq!(last.id, id);
    let failed: Vec<String> = run
        .gates
        .iter()
        .filter(|gate| !gate.ok)
        .map(|gate| gate.name.clone())
        .collect();
    assert_eq!(last.failed, failed);
    let log = hone.log().await.expect("readable");
    assert_eq!(log.device.as_deref(), Some(runs[0].device.as_str()));
}

#[tokio::test]
async fn a_pure_check_runs_no_exec_gate() {
    let scratch = Scratch::new("pure").with_store();
    let run = scratch
        .hone()
        .check(Some(TICKET), GateScope::Pure)
        .await
        .expect("ran");
    assert!(run.gates.iter().all(|gate| gate.gate != "exec"));
    assert!(
        run.gates
            .iter()
            .any(|gate| gate.name == "implementation/binding")
    );
}

#[tokio::test]
async fn a_review_follows_approvals_condensing_and_confirming() {
    let scratch = Scratch::new("review").with_store();
    let hone = scratch.hone();
    let before = hone.review(TICKET, "dec-7k3m2q8p").await.expect("shown");
    assert!(before.decisions.is_empty());
    assert_eq!(
        before.path,
        "design/decisions/dec-7k3m2q8p-payment-async.md"
    );

    let approve = |changes_requested: bool, proof: Option<&str>| Write::Approve {
        ticket: TICKET.into(),
        artifact: "dec-7k3m2q8p".into(),
        changes_requested,
        proof: proof.map(str::to_owned),
    };
    hone.write(&alice(), &approve(false, Some("read twice")))
        .await
        .expect("approved");
    let bob = Writer {
        author: "bob".into(),
        role: "human".into(),
    };
    hone.write(&bob, &approve(true, None))
        .await
        .expect("changes requested");
    let refused = hone
        .write(
            &Writer {
                author: "bob".into(),
                role: "architect".into(),
            },
            &approve(false, None),
        )
        .await
        .expect_err("bob is no architect");
    assert_eq!(refused.status, "invalid");

    let review = hone.review(TICKET, "dec-7k3m2q8p").await.expect("shown");
    let decisions: Vec<(&str, &str, bool, Option<&str>)> = review
        .decisions
        .iter()
        .map(|decision| {
            (
                decision.name.as_str(),
                decision.decision.as_str(),
                decision.current,
                decision.proof.as_deref(),
            )
        })
        .collect();
    assert_eq!(
        decisions,
        [
            ("alice", "approved", true, Some("read twice")),
            ("bob", "changes-requested", true, None),
        ]
    );

    let path = hone
        .condense(TICKET, "dec-7k3m2q8p")
        .await
        .expect("condensed");
    assert_eq!(path, "design/decisions/dec-7k3m2q8p-payment-async.md");
    let confirm = Write::ConfirmReview {
        ticket: TICKET.into(),
        artifact: "dec-7k3m2q8p".into(),
    };
    hone.write(&alice(), &confirm).await.expect("confirmed");
    let after = hone.review(TICKET, "dec-7k3m2q8p").await.expect("shown");
    assert_eq!(after.hash, review.hash, "condensing keeps the content hash");
    assert!(
        after
            .confirmations
            .iter()
            .all(|confirmation| confirmation.current)
    );
    assert_eq!(after.confirmations.len(), 1);
    assert_eq!(
        after.review.expect("a review block")["approved_by"][0]["name"],
        "alice"
    );
}

#[tokio::test]
async fn the_catalog_carries_the_links_of_an_artifact() {
    let scratch = Scratch::new("links");
    let catalog = scratch.hone().artifacts().await.expect("listed");
    let decision = &catalog.artifacts[0];
    assert_eq!(decision.id.as_deref(), Some("dec-7k3m2q8p"));
    assert_eq!(
        decision.links.get("affects").map(Vec::as_slice),
        Some(["mod-4t8w2n6c".to_owned()].as_slice())
    );
}

#[tokio::test]
async fn a_ticket_starts_changes_and_refuses_a_stale_configuration() {
    let root = std::env::temp_dir().join(format!("hone-gui-{}-start", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    copy(
        &repository().join("schemas/tests/resolve/defaults/project"),
        &root.join("project"),
    );
    let hone = Hone::with_binary(binary(), root.join("project"))
        .with_variable("HONE_DATA_DIR", root.join("data"));
    let writer = Writer {
        author: "bob".into(),
        role: "human".into(),
    };
    let start = TicketStart {
        attributes: vec![
            ("size".into(), AttributeValue::Text("3".into())),
            ("count".into(), AttributeValue::Integer(3)),
        ],
        external: vec!["github:12=https://github.com/acme/payments/issues/12".into()],
        profile: None,
    };

    let stale = hone
        .start_ticket(&writer, &start)
        .await
        .expect_err("no resolved.json yet");
    assert_eq!(stale.status, "invalid");
    assert!(
        stale.message.contains("run hone resolve"),
        "{}",
        stale.message
    );

    let hash = hone.resolve().await.expect("resolved");
    let selection = hone.select(&start.attributes).await.expect("selected");
    assert_eq!(
        (selection.selected.as_deref(), selection.rule),
        (Some("only"), None)
    );

    let started = hone.start_ticket(&writer, &start).await.expect("started");
    assert_eq!(started.profile, "only");
    assert_eq!(started.config, hash);
    assert_eq!(
        serde_json::Value::Object(started.attributes.clone()),
        serde_json::json!({ "size": "3", "count": 3 }),
        "a text that looks like a number stays text"
    );
    let catalog = hone.artifacts().await.expect("listed");
    let record = catalog.tickets().next().expect("the new record");
    assert_eq!(record.id.as_deref(), Some(started.ticket.as_str()));
    assert_eq!(record.external[0].key, "12");

    let changed = hone
        .set_attributes(
            &writer,
            &started.ticket,
            &[("urgent".into(), AttributeValue::Boolean(true))],
            &["count".into()],
        )
        .await
        .expect("changed");
    assert_eq!(
        serde_json::Value::Object(changed.attributes),
        serde_json::json!({ "size": "3", "urgent": true })
    );
    let unchanged = hone
        .switch_config(&writer, &started.ticket)
        .await
        .expect_err("already current");
    assert_eq!(
        unchanged.message,
        format!(
            "the ticket `{}` already uses the current configuration",
            started.ticket
        )
    );
    let overview = hone.ticket(&started.ticket).await.expect("shown");
    assert_eq!(overview.selected.as_deref(), Some("only"));
    std::fs::remove_dir_all(&root).expect("removable");
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

#[tokio::test]
async fn an_artifact_is_shown_saved_checked_and_created_through_the_command_line() {
    let scratch = Scratch::new("editing");
    let hone = scratch.hone();
    let decision = "design/decisions/dec-7k3m2q8p-payment-async.md";
    let file = scratch.root.join("project").join(decision);
    let original = std::fs::read_to_string(&file).expect("readable");

    let kinds = hone.kinds().await.expect("kinds");
    let names: Vec<(&str, &str)> = kinds
        .iter()
        .map(|kind| (kind.kind.as_str(), kind.lifecycle.as_str()))
        .collect();
    assert_eq!(
        names,
        [
            ("decision", "decided"),
            ("module", "living"),
            ("spec", "decided")
        ]
    );

    let source = hone.artifact("dec-7k3m2q8p").await.expect("shown");
    assert_eq!(source.text, original);
    assert_eq!(source.path, decision);
    assert_eq!(source.data["status"], "accepted");
    assert!(source.body.is_some());
    let statuses: Vec<&str> = source
        .statuses
        .iter()
        .map(|status| status.name.as_str())
        .collect();
    assert_eq!(statuses, ["accepted", "proposed", "rejected"]);

    let refused = original.replace("status: \"accepted\"", "status: \"bogus\"");
    let written = hone.save_artifact(decision, &refused).await.expect("ran");
    assert!(!written.saved);
    let codes: Vec<(&str, Option<&str>)> = written
        .findings
        .iter()
        .map(|finding| (finding.code.as_str(), finding.pointer.as_deref()))
        .collect();
    assert_eq!(codes, [("status", Some("/status"))]);
    assert_eq!(std::fs::read_to_string(&file).expect("readable"), original);

    let unresolved = refused.replace("[\"mod-4t8w2n6c\"]", "[\"mod-9c3d6h4v\"]");
    let checked = hone
        .check_artifact(decision, &unresolved)
        .await
        .expect("ran");
    assert_eq!(checked.path, decision);
    let codes: Vec<(&str, Option<&str>)> = checked
        .findings
        .iter()
        .map(|finding| (finding.code.as_str(), finding.pointer.as_deref()))
        .collect();
    assert_eq!(
        codes,
        [
            ("status", Some("/status")),
            ("link-unresolved", Some("/links/affects/0"))
        ]
    );
    assert_eq!(std::fs::read_to_string(&file).expect("readable"), original);

    let changed = original.replace("status: \"accepted\"", "status: \"proposed\"");
    let written = hone.save_artifact(decision, &changed).await.expect("ran");
    assert!(written.saved, "{:?}", written.findings);
    assert_eq!(std::fs::read_to_string(&file).expect("readable"), changed);

    let outside = hone.save_artifact("../escape.md", &changed).await;
    assert_eq!(
        outside.map_err(|failure| failure.status),
        Err("error".to_owned())
    );

    let new = hone_gui::hone::ArtifactNew {
        kind: "module".into(),
        title: "Fee rules".into(),
        anchors: vec!["path:src/payments".into()],
        ..Default::default()
    };
    let created = hone.new_artifact(&new).await.expect("ran");
    assert!(created.saved, "{:?}", created.findings);
    let id = created.id.expect("an ID");
    assert_eq!(created.path, format!("design/modules/{id}-fee-rules.yaml"));
    let shown = hone.artifact(&id).await.expect("shown");
    assert_eq!(
        shown.data["anchors"],
        serde_json::json!([{ "kind": "path", "ref": "src/payments" }])
    );
}

#[tokio::test]
async fn a_file_is_shown_at_the_line_of_a_symbol() {
    let root = repository().join("schemas/tests/check/module-anchors-resolve-resolved/project");
    let hone = Hone::with_binary(binary(), root);
    let found = hone
        .file("src/payments/fee.rs", Some("Fee::total"))
        .await
        .expect("shown");
    assert_eq!(
        (found.resolved.as_deref(), found.line),
        (Some("found"), Some(7))
    );
    assert!(found.text.starts_with("pub struct Fee {\n"));
    let missing = hone
        .file("src/payments/fee.rs", Some("Nope"))
        .await
        .expect("shown");
    assert_eq!(
        (missing.resolved.as_deref(), missing.line),
        (Some("missing"), None)
    );
    let plain = hone.file("src/payments/fee.rs", None).await.expect("shown");
    assert_eq!((plain.resolved, plain.line), (None, None));
    let outside = hone.file("../../escape.rs", None).await;
    assert_eq!(
        outside.map_err(|failure| failure.status),
        Err("error".to_owned())
    );
}

#[tokio::test]
async fn show_config_gives_the_layers_and_the_lockfile() {
    let case = repository().join("schemas/tests/resolve/null-removal");
    let hone = Hone::with_binary(binary(), case.join("project"))
        .with_packages(Some(case.join("packages")));
    let ConfigView::Shown(shown) = hone.show_config().await.expect("hone runs") else {
        panic!("the case resolves");
    };
    assert_eq!(
        shown.hash,
        "sha256:06425703822e17cfb9101e73544c0549a2454a3d8f1b38a321f5682fbc8d0fd7"
    );
    assert_eq!(shown.file, "out-of-date");
    assert_eq!(
        shown.origins["steps"]["decision"],
        serde_json::json!([{ "kind": "package", "name": "acme/base" }, { "kind": "project" }])
    );
    assert_eq!(shown.lock["packages"]["acme/base"]["version"], "1.0.0");
    assert!(!case.join("project/.hone/resolved.json").exists());

    let case = repository().join("schemas/tests/resolve/error-plugin-hash");
    let hone = Hone::with_binary(binary(), case.join("project"))
        .with_packages(Some(case.join("packages")));
    let ConfigView::Failed { problems, lock } = hone.show_config().await.expect("hone runs") else {
        panic!("the case fails");
    };
    assert_eq!(problems[0]["category"], "plugin-hash");
    assert_eq!(
        lock.expect("readable")["plugins"]["api-check"]["hash"],
        "sha256:1111111111111111111111111111111111111111111111111111111111111111"
    );
}
