//! `hone ticket list`, stored check runs, and `hone runs` (concept Section 12, "Tickets and check
//! runs for a user interface"), on a copy of the example project `examples/payments`.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

use serde_json::{Value, json};

const TICKET: &str = "tk-y4075sb1";
const RECORD: &str = "design/tickets/tk-y4075sb1/ticket.yaml";
const DECISION: &str = "design/decisions/dec-b1b2hjrh-fees-add-without-rounding.md";

fn example() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/payments")
}

// Every regular file of `from` below `to`; the link to the packages is left out, because
// nothing here resolves.
fn copy(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).expect("creatable");
    for entry in std::fs::read_dir(from).expect("readable") {
        let entry = entry.expect("readable");
        let kind = entry.file_type().expect("readable");
        let target = to.join(entry.file_name());
        if kind.is_dir() {
            copy(&entry.path(), &target);
        } else if kind.is_file() {
            std::fs::copy(entry.path(), &target).expect("copyable");
        }
    }
}

// A copy of the example project with a data directory of its own, removed on drop.
struct Project {
    root: PathBuf,
}

impl Project {
    fn new(name: &str) -> Self {
        let root =
            std::env::temp_dir().join(format!("hone-cli-runs-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        copy(&example(), &root.join("project"));
        Self { root }
    }

    fn project(&self) -> PathBuf {
        self.root.join("project")
    }

    fn data(&self) -> PathBuf {
        self.root.join("data")
    }

    fn command(&self, arguments: &[&str]) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_hone"));
        command
            .args(arguments)
            .current_dir(self.project())
            .env("HONE_DATA_DIR", self.data())
            .env("HONE_CONFIG_DIR", self.root.join("config"))
            .env_remove("HONE_AUTHOR")
            .env_remove("HONE_ROLE");
        command
    }

    fn hone(&self, arguments: &[&str]) -> Output {
        self.command(arguments).output().expect("hone runs")
    }

    fn json(&self, arguments: &[&str]) -> (Option<i32>, Value) {
        let mut all = arguments.to_vec();
        all.push("--json");
        let output = self.hone(&all);
        let value = serde_json::from_slice(&output.stdout)
            .unwrap_or_else(|_| panic!("one JSON document: {output:?}"));
        (output.status.code(), value)
    }

    fn sync(&self) {
        let output = self.hone(&["sync", "--folder", "shared"]);
        assert_eq!(output.status.code(), Some(0), "{output:?}");
    }

    fn runs(&self) -> Vec<Value> {
        let (code, value) = self.json(&["runs", "--ticket", TICKET]);
        assert_eq!(code, Some(0), "{value}");
        value["runs"].as_array().expect("runs").clone()
    }

    fn ticket(&self) -> Value {
        let (code, value) = self.json(&["ticket", "list"]);
        assert_eq!(code, Some(0), "{value}");
        let tickets = value["tickets"].as_array().expect("tickets");
        assert_eq!(tickets.len(), 1, "{value}");
        tickets[0].clone()
    }
}

impl Drop for Project {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn names(gates: &Value) -> Vec<String> {
    gates
        .as_array()
        .expect("gates")
        .iter()
        .map(|gate| gate["name"].as_str().expect("name").to_owned())
        .collect()
}

#[test]
fn a_ticket_without_a_store_lists_its_record_only() {
    let project = Project::new("no-store");
    let (code, value) = project.json(&["ticket", "list"]);
    assert_eq!(code, Some(0));
    // The example's profile `standard` requires four steps; `specify` comes first because
    // `decision` and `implementation` require it, and `interfaces` requires `decision`.
    assert_eq!(
        value,
        json!({ "tickets": [{
            "ticket": TICKET,
            "record": true,
            "title": null,
            "external": [{
                "system": "github",
                "key": "42",
                "url": "https://github.com/example/payments/issues/42",
            }],
            "profile": "standard",
            "started": null,
            "authors": [],
            "steps": [
                { "step": "specify", "requirement": "required", "status": "open", "reason": null },
                { "step": "decision", "requirement": "required", "status": "waiting", "reason": null },
                { "step": "implementation", "requirement": "required", "status": "waiting", "reason": null },
                { "step": "interfaces", "requirement": "required", "status": "waiting", "reason": null },
            ],
            "current": "specify",
            "complete": false,
            "threads": { "open": 0, "blocking": 0 },
            "last_event": null,
            "last_run": null,
            "waiting": [{ "kind": "step", "step": "specify", "role": null }],
            "problem": null,
        }]})
    );
    assert!(!project.data().exists(), "listing creates no store");
}

#[test]
fn a_check_stores_its_run_and_the_list_reads_it() {
    let project = Project::new("stored");
    project.sync();
    let (code, report) = project.json(&["check", "--ticket", TICKET, "--source", "pre-push"]);
    assert_eq!(code, Some(1));
    let run = report["run"].as_str().expect("a stored run").to_owned();
    assert!(
        report["gates"]
            .as_array()
            .expect("gates")
            .iter()
            .all(|gate| gate["duration_ms"].is_u64()),
        "{report}"
    );

    let (_, log) = project.json(&["log"]);
    let runs = project.runs();
    assert_eq!(runs.len(), 1);
    let stored = &runs[0];
    assert_eq!(stored["id"], run.as_str());
    assert_eq!(stored["ticket"], TICKET);
    assert_eq!(stored["source"], "pre-push");
    assert_eq!(stored["device"], log["device"]);
    assert!(stored["device"].is_string(), "{log}");
    assert_eq!(
        stored["arguments"],
        json!({ "step": null, "changed": null, "class": null, "base": null, "lock": null })
    );
    assert_eq!(stored["status"], "failed");
    assert!(stored["duration_ms"].is_u64());
    assert_eq!(stored["gates"], report["gates"]);

    // From the README: bob's concern blocks the decision, and without changed paths the
    // implementation and interface gates fail.
    let failed = [
        "decision/no-blockers",
        "implementation/binding",
        "interfaces/public-api",
    ];
    let ticket = project.ticket();
    assert_eq!(
        ticket["last_run"],
        json!({ "id": run, "status": "failed", "failed": failed })
    );
    assert_eq!(
        ticket["started"]["by"],
        json!({ "name": "alice", "role": "architect" })
    );
    assert_eq!(ticket["authors"], json!(["alice", "bob"]));
    assert_eq!(ticket["threads"], json!({ "open": 1, "blocking": 1 }));
    assert_eq!(ticket["current"], "decision");
    assert_eq!(ticket["complete"], false);
    let statuses: Vec<(&str, &str)> = ticket["steps"]
        .as_array()
        .expect("steps")
        .iter()
        .map(|step| {
            (
                step["step"].as_str().expect("step"),
                step["status"].as_str().expect("status"),
            )
        })
        .collect();
    assert_eq!(
        statuses,
        [
            ("specify", "done"),
            ("decision", "open"),
            ("implementation", "open"),
            ("interfaces", "waiting"),
        ]
    );
    let (_, events) = project.json(&["events", "--ticket", TICKET]);
    let open: Vec<&Value> = events["threads"]
        .as_array()
        .expect("threads")
        .iter()
        .filter(|thread| thread["status"] == "open" && thread["blocking"] == true)
        .collect();
    assert_eq!(open.len(), 1, "{events}");
    let thread = open[0]["thread"].clone();
    let newest = events["events"]
        .as_array()
        .expect("events")
        .iter()
        .filter_map(|event| event["id"].as_str())
        .max()
        .expect("an event");
    assert_eq!(ticket["last_event"], newest);
    assert_eq!(
        ticket["waiting"],
        json!([
            // Without changed paths, a failed binding or interface gate is unchecked.
            { "kind": "gates", "gates": ["decision/no-blockers"] },
            { "kind": "threads", "threads": [thread] },
            { "kind": "step", "step": "decision", "role": null },
        ])
    );
}

#[test]
fn a_failed_approval_waits_on_its_role() {
    let project = Project::new("approval");
    project.sync();
    // A changed decision no longer has the approval of its content hash.
    let decision = project.project().join(DECISION);
    let mut text = std::fs::read_to_string(&decision).expect("readable");
    text.push_str("\nOne more sentence.\n");
    std::fs::write(&decision, text).expect("writable");
    let (code, _) = project.json(&["check", "--ticket", TICKET, "--step", "decision"]);
    assert_eq!(code, Some(1));
    let ticket = project.ticket();
    assert_eq!(
        ticket["waiting"],
        json!([
            { "kind": "gates", "gates": ["decision/no-blockers"] },
            { "kind": "threads", "threads": [ticket["waiting"][1]["threads"][0]] },
            { "kind": "approval", "role": "architect", "gate": "decision/approved" },
            { "kind": "step", "step": "decision", "role": null },
        ])
    );
    let runs = project.runs();
    assert_eq!(runs[0]["source"], "cli");
    assert_eq!(runs[0]["arguments"]["step"], "decision");
}

#[test]
fn runs_are_newest_first_and_only_the_newest_hundred_stay() {
    let project = Project::new("hundred");
    project.sync();
    let mut ids = Vec::new();
    // The example has no `exec` gate, so these runs run nothing and pass.
    for _ in 0..101 {
        let (code, report) = project.json(&["check", "--ticket", TICKET, "--class", "exec"]);
        assert_eq!(code, Some(0), "{report}");
        ids.push(report["run"].as_str().expect("run").to_owned());
    }
    let runs = project.runs();
    let stored: Vec<&str> = runs
        .iter()
        .map(|run| run["id"].as_str().expect("id"))
        .collect();
    let mut expected: Vec<&str> = ids.iter().map(String::as_str).collect();
    expected.sort_unstable();
    expected.reverse();
    expected.truncate(100);
    assert_eq!(stored, expected);
    assert_eq!(runs[0]["arguments"]["class"], "exec");
    assert_eq!(runs[0]["gates"], json!([]));
}

#[test]
fn nothing_is_stored_without_a_store_or_when_the_check_cannot_run() {
    let project = Project::new("unstored");
    let (code, report) = project.json(&["check", "--ticket", TICKET, "--class", "exec"]);
    assert_eq!(code, Some(0));
    assert_eq!(report["run"], Value::Null);
    assert!(!project.data().exists(), "a check creates no store");

    project.sync();
    let (code, report) = project.json(&["check", "--ticket", TICKET, "--step", "review"]);
    assert_eq!(code, Some(2));
    assert_eq!(
        report,
        json!({ "status": "error", "message": "the profile `standard` does not list the step `review`" })
    );
    let (code, report) = project.json(&["check", "--ticket", TICKET, "--source", "Pre Push"]);
    assert_eq!(code, Some(2));
    assert_eq!(
        report,
        json!({ "status": "error", "message": "`Pre Push` is not a source: lowercase letters, digits, and `-`, at most 32 characters" })
    );
    let (code, report) = project.json(&["check"]);
    assert_eq!(code, Some(0), "{report}");
    assert_eq!(report["run"], Value::Null);
    assert_eq!(project.runs(), Vec::<Value>::new());
}

#[test]
fn runs_refuse_a_path_as_ticket() {
    let project = Project::new("refuse");
    project.sync();
    let (code, value) = project.json(&["runs", "--ticket", "../tk-y4075sb1"]);
    assert_eq!(code, Some(1));
    assert_eq!(
        value,
        json!({ "status": "invalid", "message": "`../tk-y4075sb1` is not a ticket ID" })
    );
}

#[test]
fn the_agent_hook_stores_its_run() {
    let project = Project::new("hook");
    project.sync();
    let input = json!({
        "tool_name": "Edit",
        "tool_input": { "file_path": project.project().join("src/payments/fee.rs") },
        "cwd": project.project(),
    });
    let mut child = project
        .command(&["hook", "claude-code", "--ticket", TICKET])
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("hone runs");
    child
        .stdin
        .take()
        .expect("stdin")
        .write_all(input.to_string().as_bytes())
        .expect("writable");
    child.wait().expect("hone ends");
    let runs = project.runs();
    assert_eq!(runs.len(), 1);
    assert_eq!(runs[0]["source"], "agent-hook");
    assert_eq!(
        runs[0]["arguments"]["changed"],
        json!(["src/payments/fee.rs"])
    );
    assert_eq!(names(&runs[0]["gates"]), ["implementation/binding"]);
}

#[test]
fn a_ticket_of_the_store_without_a_record_is_listed() {
    let project = Project::new("no-record");
    project.sync();
    std::fs::remove_file(project.project().join(RECORD)).expect("removable");
    let ticket = project.ticket();
    assert_eq!(ticket["record"], false);
    assert_eq!(ticket["title"], Value::Null);
    assert_eq!(ticket["profile"], "standard");
    assert_eq!(
        ticket["steps"],
        json!([{ "step": "specify", "requirement": null, "status": "done", "reason": null }])
    );
    assert_eq!(ticket["current"], Value::Null);
    assert_eq!(ticket["complete"], Value::Null);
    assert_eq!(ticket["waiting"], json!([]));
    assert_eq!(ticket["threads"], json!({ "open": 1, "blocking": 1 }));
}
