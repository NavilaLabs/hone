use std::path::{Path, PathBuf};

use rmcp::ServiceExt;
use rmcp::model::CallToolRequestParams;
use rmcp::service::{RoleClient, RunningService, ServiceError};
use serde_json::{Value, json};

use hone_core::Author;

use crate::{Project, Server, SyncPeer};

const TICKET: &str = "tk-9f2x4a8m";

fn schemas() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../schemas")
}

fn check_case(name: &str) -> PathBuf {
    schemas().join("tests/check").join(name).join("project")
}

fn project(root: PathBuf) -> Project {
    Project {
        root,
        packages: None,
        data_directory: None,
        author: None,
        sync_peer: None,
    }
}

// A fresh copy of a resolve test case, because `resolve` writes into the project.
fn resolve_case(name: &str) -> Project {
    let case = schemas().join("tests/resolve").join(name);
    let root = std::env::temp_dir().join(format!("hone-mcp-{}-{name}", std::process::id()));
    if root.exists() {
        std::fs::remove_dir_all(&root).expect("removable");
    }
    copy_directory(&case.join("project"), &root);
    let packages = case.join("packages");
    Project {
        root,
        packages: packages.exists().then_some(packages),
        data_directory: None,
        author: None,
        sync_peer: None,
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

async fn connect(project: Project) -> RunningService<RoleClient, ()> {
    let (client, server) = tokio::io::duplex(1 << 16);
    tokio::spawn(async move {
        Server::new(project)
            .serve(server)
            .await
            .expect("server starts")
            .waiting()
            .await
    });
    ().serve(client).await.expect("client connects")
}

struct Reply {
    is_error: bool,
    content: Value,
}

async fn call(project: Project, tool: &'static str, arguments: Value) -> Reply {
    let result = try_call(project, tool, arguments)
        .await
        .unwrap_or_else(|error| panic!("{tool}: {error}"));
    Reply {
        is_error: result.is_error == Some(true),
        content: result.structured_content.expect("structured content"),
    }
}

async fn try_call(
    project: Project,
    tool: &'static str,
    arguments: Value,
) -> Result<rmcp::model::CallToolResult, ServiceError> {
    let client = connect(project).await;
    let Value::Object(arguments) = arguments else {
        panic!("arguments are an object");
    };
    let result = client
        .call_tool(CallToolRequestParams::new(tool).with_arguments(arguments))
        .await;
    client.cancel().await.expect("client stops");
    result
}

// Arguments that do not match the input schema come back as an error result, so that the
// agent reads the reason and can correct the call.
async fn invalid_arguments(project: Project, tool: &'static str, arguments: Value) -> String {
    let result = try_call(project, tool, arguments)
        .await
        .unwrap_or_else(|error| panic!("{tool}: {error}"));
    assert_eq!(result.is_error, Some(true), "{result:?}");
    result
        .content
        .iter()
        .filter_map(|content| content.as_text().map(|text| text.text.clone()))
        .collect()
}

fn codes(findings: &Value) -> Vec<String> {
    findings
        .as_array()
        .expect("a list of findings")
        .iter()
        .map(|finding| format!("{} {}", finding["level"], finding["code"]).replace('"', ""))
        .collect()
}

fn gate_codes(content: &Value) -> Vec<String> {
    content["gates"]
        .as_array()
        .expect("a list of gates")
        .iter()
        .flat_map(|gate| codes(&gate["findings"]))
        .collect()
}

fn read_json(path: &Path) -> Value {
    serde_json::from_slice(&std::fs::read(path).expect("readable")).expect("JSON")
}

#[tokio::test]
async fn lists_exactly_the_tools() {
    let client = connect(project(check_case("pass-ticket"))).await;
    let mut names: Vec<String> = client
        .list_all_tools()
        .await
        .expect("tools")
        .into_iter()
        .map(|tool| tool.name.into_owned())
        .collect();
    client.cancel().await.expect("client stops");
    names.sort();
    assert_eq!(
        names,
        [
            "catalog",
            "check",
            "check_artifact",
            "check_edit",
            "content_hash",
            "events",
            "resolve",
            "runs",
            "step_set",
            "sync",
            "thread_open",
            "thread_reopen",
            "thread_reply",
            "thread_resolve",
            "ticket_list",
            "ticket_overview",
            "ticket_start",
        ]
    );
}

#[tokio::test]
async fn catalog_and_ticket_overview_return_the_json_of_the_command_line() {
    let root = check_case("no-open-blocking-threads-open");
    let catalog = call(project(root.clone()), "catalog", json!({})).await;
    assert!(!catalog.is_error);
    assert_eq!(catalog.content["project"], "prj-8m2k4d7q");
    let ids: Vec<&str> = catalog.content["artifacts"]
        .as_array()
        .expect("artifacts")
        .iter()
        .filter_map(|artifact| artifact["id"].as_str())
        .collect();
    assert_eq!(
        ids,
        [
            "dec-7k3m2q8p",
            "mod-4t8w2n6c",
            "spec-3b8n1c5w",
            "tk-9f2x4a8m"
        ]
    );

    let overview = call(
        project(root.clone()),
        "ticket_overview",
        json!({ "ticket": TICKET }),
    )
    .await;
    assert!(!overview.is_error);
    assert_eq!(overview.content["profile"], "standard");
    assert_eq!(overview.content["roles"], json!(["architect", "human"]));

    let malformed = call(
        project(root),
        "ticket_overview",
        json!({ "ticket": "PAY-123" }),
    )
    .await;
    assert!(malformed.is_error);
    assert_eq!(
        malformed.content,
        json!({ "status": "error", "message": "`PAY-123` is not a ticket ID" })
    );
}

#[tokio::test]
async fn resolve_writes_then_compares() {
    let project = resolve_case("extends-order");
    assert!(project.packages.is_some(), "the case uses packages");
    let expected = read_json(&schemas().join("tests/resolve/extends-order/expected.json"));
    let resolved = project.root.join(".hone/resolved.json");

    let before = call(project.clone(), "resolve", json!({ "check": true })).await;
    assert!(!before.is_error);
    assert_eq!(before.content["status"], "out-of-date");
    assert!(!resolved.exists(), "`check` writes nothing");

    let written = call(project.clone(), "resolve", json!({})).await;
    assert!(!written.is_error);
    assert_eq!(written.content["status"], "written");
    assert_eq!(read_json(&resolved), expected);

    let after = call(project.clone(), "resolve", json!({ "check": true })).await;
    assert_eq!(after.content["status"], "up-to-date");
    assert_eq!(after.content["hash"], written.content["hash"]);
    std::fs::remove_dir_all(&project.root).expect("removable");
}

#[tokio::test]
async fn resolve_shows_the_layers_and_writes_nothing() {
    let project = resolve_case("null-removal");
    let shown = call(project.clone(), "resolve", json!({ "show": true })).await;
    assert!(!shown.is_error);
    assert_eq!(shown.content["status"], "shown");
    assert_eq!(shown.content["file"], "out-of-date");
    assert_eq!(
        shown.content["origins"]["kinds"]["decision"],
        json!([{ "kind": "package", "name": "acme/base" }, { "kind": "project" }])
    );
    assert!(!project.root.join(".hone/resolved.json").exists());

    for (input, message) in [
        (
            json!({ "show": true, "check": true }),
            "`show` and `check` exclude each other",
        ),
        (json!({ "ticket": TICKET }), "`ticket` needs `show`"),
    ] {
        let reply = call(project.clone(), "resolve", input.clone()).await;
        assert!(reply.is_error, "{input}");
        assert_eq!(reply.content["status"], "invalid", "{input}");
        assert_eq!(reply.content["message"], message, "{input}");
    }
    assert!(!project.root.join(".hone/resolved.json").exists());
    std::fs::remove_dir_all(&project.root).expect("removable");
}

#[tokio::test]
async fn resolve_failure_is_an_error_with_problems() {
    let project = resolve_case("error-shape");
    let reply = call(project.clone(), "resolve", json!({})).await;
    assert!(reply.is_error);
    assert_eq!(reply.content["status"], "failed");
    let problems = reply.content["problems"].as_array().expect("problems");
    assert_eq!(problems.len(), 1, "{problems:?}");
    assert_eq!(problems[0]["category"], "shape");
    assert!(!project.root.join(".hone/resolved.json").exists());
    std::fs::remove_dir_all(&project.root).expect("removable");
}

#[tokio::test]
async fn check_passes() {
    let reply = call(
        project(check_case("pass-ticket")),
        "check",
        json!({ "ticket": TICKET, "changed_paths": ["src/payments/fee.rs"] }),
    )
    .await;
    assert!(!reply.is_error);
    assert_eq!(reply.content["status"], "passed");
    let gates: Vec<&Value> = reply.content["gates"]
        .as_array()
        .expect("gates")
        .iter()
        .map(|gate| &gate["gate"])
        .collect();
    assert!(gates.contains(&&json!("change-needs-binding")), "{gates:?}");
}

#[tokio::test]
async fn check_failed_gate_is_a_normal_result() {
    let reply = call(
        project(check_case("change-needs-binding-missing")),
        "check",
        json!({
            "ticket": TICKET,
            "step": "implementation",
            "changed_paths": ["src/payments/fee.rs"],
        }),
    )
    .await;
    assert!(!reply.is_error);
    assert_eq!(reply.content["status"], "failed");
    assert_eq!(gate_codes(&reply.content), ["error binding-missing"]);
}

#[tokio::test]
async fn check_takes_the_lockfile_relative_to_the_root() {
    let case = "exec-unapproved-base-lock";
    let arguments = json!({ "ticket": TICKET, "step": "interfaces" });
    let own = call(project(check_case(case)), "check", arguments.clone()).await;
    assert_eq!(own.content["status"], "passed", "{:?}", own.content);
    let mut arguments = arguments;
    arguments["lock"] = json!("../lock.yaml");
    let base = call(project(check_case(case)), "check", arguments).await;
    assert!(!base.is_error);
    assert_eq!(gate_codes(&base.content), ["error exec-unapproved"]);
}

#[tokio::test]
async fn check_takes_the_base_relative_to_the_root() {
    let arguments = json!({
        "ticket": TICKET,
        "step": "interfaces",
        "changed_paths": ["src/payments/fee.rs"],
        "base": "../base",
    });
    let reply = call(
        project(check_case("plugin-public-api-unbound")),
        "check",
        arguments,
    )
    .await;
    assert_eq!(gate_codes(&reply.content), ["error public-api-unbound"]);
}

#[tokio::test]
async fn check_that_cannot_run_is_an_error() {
    let reply = call(project(schemas().join("tests/check")), "check", json!({})).await;
    assert!(reply.is_error);
    assert_eq!(reply.content["status"], "error");
    let message = reply.content["message"].as_str().expect("message");
    assert!(message.contains("resolved.json"), "{message}");
}

#[tokio::test]
async fn check_step_without_ticket_is_an_error() {
    let reply = call(
        project(check_case("pass-ticket")),
        "check",
        json!({ "step": "implementation" }),
    )
    .await;
    assert!(reply.is_error);
    assert_eq!(reply.content["status"], "error");
    let message = reply.content["message"].as_str().expect("message");
    assert!(message.contains("`step` requires a ticket"), "{message}");
}

#[tokio::test]
async fn check_unknown_class_is_invalid() {
    let message = invalid_arguments(
        project(check_case("pass-ticket")),
        "check",
        json!({ "class": "fast" }),
    )
    .await;
    assert!(message.contains("unknown variant `fast`"), "{message}");
}

#[tokio::test]
async fn check_edit_passes_with_a_binding_decision() {
    let reply = call(
        project(check_case("pass-ticket")),
        "check_edit",
        json!({ "ticket": TICKET, "path": "src/payments/fee.rs" }),
    )
    .await;
    assert!(!reply.is_error);
    assert_eq!(reply.content["status"], "passed");
    let gates: Vec<&Value> = reply.content["gates"]
        .as_array()
        .expect("gates")
        .iter()
        .map(|gate| &gate["gate"])
        .collect();
    assert_eq!(gates, [&json!("change-needs-binding")]);
}

#[tokio::test]
async fn check_edit_fails_without_a_binding_decision() {
    let reply = call(
        project(check_case("change-needs-binding-missing")),
        "check_edit",
        json!({ "ticket": TICKET, "path": "src/payments/fee.rs" }),
    )
    .await;
    assert!(!reply.is_error);
    assert_eq!(reply.content["status"], "failed");
    assert_eq!(gate_codes(&reply.content), ["error binding-missing"]);
}

#[tokio::test]
async fn check_edit_never_blocks_artifact_files() {
    let reply = call(
        project(check_case("change-needs-binding-missing")),
        "check_edit",
        json!({ "ticket": TICKET, "path": "design/modules/mod-4t8w2n6c-fee.yaml" }),
    )
    .await;
    assert!(!reply.is_error);
    assert_eq!(reply.content["status"], "passed");
    assert_eq!(reply.content["gates"], json!([]));
}

#[tokio::test]
async fn check_edit_unknown_ticket_is_an_error() {
    let reply = call(
        project(check_case("pass-ticket")),
        "check_edit",
        json!({ "ticket": "tk-00000000", "path": "src/payments/fee.rs" }),
    )
    .await;
    assert!(reply.is_error);
    assert_eq!(reply.content["status"], "error");
}

#[tokio::test]
async fn check_edit_without_path_is_invalid() {
    let message = invalid_arguments(
        project(check_case("pass-ticket")),
        "check_edit",
        json!({ "ticket": TICKET }),
    )
    .await;
    assert!(message.contains("missing field `path`"), "{message}");
}

fn artifact(case: &str, path: &str) -> Value {
    let content = std::fs::read_to_string(check_case(case).join(path)).expect("readable");
    json!({ "path": path, "content": content })
}

#[tokio::test]
async fn check_artifact_passes_with_warnings_only() {
    let path = "design/decisions/dec-4h7r2x9b-extra.md";
    let reply = call(
        project(check_case("schema-valid-unknown-field")),
        "check_artifact",
        artifact("schema-valid-unknown-field", path),
    )
    .await;
    assert!(!reply.is_error);
    assert_eq!(reply.content["status"], "passed");
    assert_eq!(
        codes(&reply.content["findings"]),
        ["warning unknown-field", "warning unknown-field"]
    );
}

#[tokio::test]
async fn check_artifact_fails_on_an_error() {
    let path = "design/decisions/dec-4h7r2x9b-no-authors.md";
    let reply = call(
        project(check_case("schema-valid-schema")),
        "check_artifact",
        artifact("schema-valid-schema", path),
    )
    .await;
    assert!(!reply.is_error);
    assert_eq!(reply.content["status"], "failed");
    assert_eq!(codes(&reply.content["findings"]), ["error schema"]);
    assert_eq!(reply.content["findings"][0]["path"], path);
}

#[tokio::test]
async fn check_artifact_reports_a_link_to_a_missing_artifact() {
    let path = "design/decisions/dec-7k3m2q8p-payment-async.md";
    let reply = call(
        project(check_case("links-resolve-affects")),
        "check_artifact",
        artifact("links-resolve-affects", path),
    )
    .await;
    assert!(!reply.is_error);
    assert_eq!(reply.content["status"], "failed");
    assert_eq!(reply.content["id"], "dec-7k3m2q8p");
    assert_eq!(reply.content["path"], path);
    assert_eq!(codes(&reply.content["findings"]), ["error link-unresolved"]);
    assert_eq!(reply.content["findings"][0]["pointer"], "/links/affects/0");
}

#[tokio::test]
async fn check_artifact_checks_the_text_in_place_of_the_file() {
    let path = "design/decisions/dec-7k3m2q8p-payment-async.md";
    let mut input = artifact("links-resolve-affects", path);
    let fixed = input["content"]
        .as_str()
        .expect("text")
        .replace("mod-9z9z9z9z", "mod-4t8w2n6c");
    input["content"] = json!(fixed);
    let reply = call(
        project(check_case("links-resolve-affects")),
        "check_artifact",
        input,
    )
    .await;
    assert!(!reply.is_error);
    assert_eq!(reply.content["status"], "passed");
    assert_eq!(reply.content["findings"], json!([]));
}

#[tokio::test]
async fn check_artifact_refuses_a_ticket_record() {
    let path = "design/tickets/tk-9f2x4a8m/ticket.yaml";
    let reply = call(
        project(check_case("links-resolve-affects")),
        "check_artifact",
        artifact("links-resolve-affects", path),
    )
    .await;
    assert!(reply.is_error);
    assert_eq!(reply.content["status"], "error");
    assert_eq!(
        reply.content["message"],
        "`design/tickets/tk-9f2x4a8m/ticket.yaml` is a ticket record: change it with hone ticket set-attributes"
    );
}

#[tokio::test]
async fn check_artifact_without_resolved_configuration_is_an_error() {
    let path = "design/decisions/dec-4h7r2x9b-extra.md";
    let reply = call(
        project(schemas().join("tests/check")),
        "check_artifact",
        artifact("schema-valid-unknown-field", path),
    )
    .await;
    assert!(reply.is_error);
    assert_eq!(reply.content["status"], "error");
}

#[tokio::test]
async fn check_artifact_without_content_is_invalid() {
    let message = invalid_arguments(
        project(check_case("pass-ticket")),
        "check_artifact",
        json!({ "path": "design/decisions/dec-7k3m2q8p-payment-async.md" }),
    )
    .await;
    assert!(message.contains("missing field `content`"), "{message}");
}

#[tokio::test]
async fn content_hash_matches_the_stored_approval() {
    let content =
        std::fs::read_to_string(schemas().join("tests/valid/decision-accepted.md")).expect("file");
    let reply = call(
        project(check_case("pass-ticket")),
        "content_hash",
        json!({ "path": "decision.md", "content": content }),
    )
    .await;
    assert!(!reply.is_error);
    assert_eq!(
        reply.content["hash"],
        "sha256:6707fdc5fdd423057322a6f4d934968ab60848484affd95ee104d31c38905d6e"
    );
}

#[tokio::test]
async fn content_hash_outside_the_profile_is_an_error() {
    let content = "---\nid: &anchor dec-4h7r2x9b\n---\n";
    let reply = call(
        project(check_case("pass-ticket")),
        "content_hash",
        json!({ "path": "decision.md", "content": content }),
    )
    .await;
    assert!(reply.is_error);
    assert_eq!(reply.content["status"], "error");
    let violations = reply.content["violations"].as_array().expect("violations");
    assert_eq!(violations.len(), 1, "{violations:?}");
    assert!(
        violations[0].as_str().expect("text").contains("anchor"),
        "{violations:?}"
    );
}

#[tokio::test]
async fn content_hash_without_path_is_invalid() {
    let message = invalid_arguments(
        project(check_case("pass-ticket")),
        "content_hash",
        json!({ "content": "---\n---\n" }),
    )
    .await;
    assert!(message.contains("missing field `path`"), "{message}");
}

// A fresh copy of a gate test project with its own data directory, written as `author`.
fn process_case(name: &str, author: Option<(&str, &str)>) -> Project {
    let root = std::env::temp_dir().join(format!("hone-mcp-{}-{name}", std::process::id()));
    if root.exists() {
        std::fs::remove_dir_all(&root).expect("removable");
    }
    copy_directory(
        &check_case("no-open-blocking-threads-process-store-unavailable"),
        &root.join("project"),
    );
    Project {
        root: root.join("project"),
        packages: None,
        data_directory: Some(root.join("data")),
        author: author.map(|(name, role)| hone_core::Author {
            name: name.into(),
            role: role.into(),
        }),
        sync_peer: None,
    }
}

fn as_author(project: &Project, name: &str, role: &str) -> Project {
    Project {
        author: Some(hone_core::Author {
            name: name.into(),
            role: role.into(),
        }),
        ..project.clone()
    }
}

fn remove(project: &Project) {
    let root = project.root.parent().expect("temporary root");
    std::fs::remove_dir_all(root).expect("removable");
}

#[tokio::test]
async fn process_tools_write_the_events_the_gates_read() {
    let bob = process_case("process-flow", Some(("bob", "human")));
    let alice = as_author(&bob, "alice", "human");
    let completion = json!({ "ticket": TICKET, "step": "completion" });

    let opened = call(
        bob.clone(),
        "thread_open",
        json!({
            "ticket": TICKET,
            "kind": "concern",
            "blocking": true,
            "anchor": { "kind": "code", "ref": "src/ledger.rs#Ledger" },
            "body": "What happens when the queue fails?",
        }),
    )
    .await;
    assert!(!opened.is_error, "{:?}", opened.content);
    assert_eq!(
        opened.content["event"]["author"],
        json!({ "name": "bob", "role": "human" })
    );
    let thread = opened.content["thread"]
        .as_str()
        .expect("thread")
        .to_owned();

    let blocked = call(bob.clone(), "check", completion.clone()).await;
    assert_eq!(blocked.content["status"], "failed");
    assert_eq!(gate_codes(&blocked.content), ["error thread-blocking-open"]);

    let reply = json!({ "thread": thread, "body": "Fall back to synchronous processing." });
    assert!(!call(alice.clone(), "thread_reply", reply).await.is_error);
    let resolved = call(alice.clone(), "thread_resolve", json!({ "thread": thread })).await;
    assert_eq!(resolved.content["event"]["type"], "thread.resolved");
    let passed = call(alice.clone(), "check", completion).await;
    assert_eq!(passed.content["status"], "passed", "{:?}", passed.content);

    let step = json!({ "ticket": TICKET, "step": "decision", "status": "skipped", "reason": "no new decision" });
    assert!(!call(alice.clone(), "step_set", step).await.is_error);
    let reopened = call(bob.clone(), "thread_reopen", json!({ "thread": thread })).await;
    assert!(!reopened.is_error);

    let events = call(bob.clone(), "events", json!({ "ticket": TICKET })).await;
    assert!(!events.is_error);
    assert_eq!(events.content["events"].as_array().map(Vec::len), Some(5));
    assert_eq!(events.content["threads"][0]["status"], "open");
    assert_eq!(events.content["threads"][0]["answered"], true);
    assert_eq!(
        events.content["steps"]["decision"],
        json!({ "status": "skipped", "reason": "no new decision" })
    );
    remove(&bob);
}

#[tokio::test]
async fn check_stores_its_run_for_runs_and_ticket_list() {
    let bob = process_case("runs", Some(("bob", "human")));
    let opened = call(
        bob.clone(),
        "thread_open",
        json!({
            "ticket": TICKET,
            "kind": "concern",
            "blocking": true,
            "anchor": { "kind": "code", "ref": "src/ledger.rs#Ledger" },
            "body": "What happens when the queue fails?",
        }),
    )
    .await;
    assert!(!opened.is_error, "{:?}", opened.content);
    let checked = call(
        bob.clone(),
        "check",
        json!({ "ticket": TICKET, "step": "completion" }),
    )
    .await;
    let run = checked.content["run"].as_str().expect("a stored run");

    let runs = call(bob.clone(), "runs", json!({ "ticket": TICKET })).await;
    assert!(!runs.is_error, "{:?}", runs.content);
    assert_eq!(runs.content["runs"].as_array().map(Vec::len), Some(1));
    assert_eq!(runs.content["runs"][0]["id"], run);
    assert_eq!(runs.content["runs"][0]["source"], "mcp");

    let list = call(bob.clone(), "ticket_list", json!({})).await;
    assert!(!list.is_error, "{:?}", list.content);
    let ticket = &list.content["tickets"][0];
    assert_eq!(ticket["ticket"], TICKET);
    assert_eq!(ticket["last_run"]["id"], run);
    assert_eq!(ticket["threads"], json!({ "open": 1, "blocking": 1 }));
    assert_eq!(ticket["authors"], json!(["bob"]));

    let refused = call(bob.clone(), "runs", json!({ "ticket": "../x" })).await;
    assert!(refused.is_error);
    assert_eq!(
        refused.content,
        json!({ "status": "invalid", "message": "`../x` is not a ticket ID" })
    );
    remove(&bob);
}

#[tokio::test]
async fn writing_without_an_author_is_an_error() {
    let project = process_case("process-no-author", None);
    let reply = call(
        project.clone(),
        "step_set",
        json!({ "ticket": TICKET, "step": "decision", "status": "done" }),
    )
    .await;
    assert!(reply.is_error);
    assert_eq!(reply.content["status"], "invalid");
    assert!(
        reply.content["message"]
            .as_str()
            .is_some_and(|message| message.contains("--author")),
        "{:?}",
        reply.content
    );
    remove(&project);
}

#[tokio::test]
async fn events_without_a_store_is_an_error() {
    let project = process_case("process-unavailable", None);
    let reply = call(project.clone(), "events", json!({ "ticket": TICKET })).await;
    assert!(reply.is_error);
    assert_eq!(reply.content["status"], "unavailable");
    remove(&project);
}

#[tokio::test]
async fn step_set_with_an_unknown_step_is_an_error() {
    let project = process_case("process-unknown-step", Some(("alice", "human")));
    let reply = call(
        project.clone(),
        "step_set",
        json!({ "ticket": TICKET, "step": "domain", "status": "done" }),
    )
    .await;
    assert!(reply.is_error);
    assert_eq!(reply.content["status"], "invalid");
    remove(&project);
}

#[tokio::test]
async fn thread_open_without_anchor_is_invalid() {
    let project = process_case("process-no-anchor", Some(("bob", "human")));
    let message = invalid_arguments(
        project.clone(),
        "thread_open",
        json!({ "ticket": TICKET, "kind": "concern", "body": "Why?" }),
    )
    .await;
    assert!(message.contains("anchor"), "{message}");
    remove(&project);
}

#[tokio::test]
async fn sync_without_a_folder_is_an_error() {
    let project = process_case("sync-no-folder", None);
    let reply = call(project.clone(), "sync", json!({})).await;
    assert!(reply.is_error);
    assert_eq!(reply.content["status"], "invalid");
    assert!(
        reply.content["message"]
            .as_str()
            .is_some_and(|message| message.contains("--sync-folder")),
        "{:?}",
        reply.content
    );
    remove(&project);
}

#[tokio::test]
async fn sync_moves_events_through_the_fixed_folder() {
    let bob = process_case("sync-folder", Some(("bob", "human")));
    let folder = bob.root.parent().expect("temporary root").join("folder");
    std::fs::create_dir_all(&folder).expect("creatable");
    let bob = Project {
        sync_peer: Some(SyncPeer::Folder(folder.clone())),
        ..bob
    };
    let step = json!({ "ticket": TICKET, "step": "decision", "status": "done" });
    assert!(!call(bob.clone(), "step_set", step).await.is_error);

    let reply = call(bob.clone(), "sync", json!({})).await;
    assert!(!reply.is_error, "{:?}", reply.content);
    assert_eq!(reply.content["problems"], json!([]));
    let sent = reply.content["sent"].as_object().expect("sent");
    assert_eq!(sent.values().collect::<Vec<_>>(), [&json!(1)]);
    remove(&bob);
}

#[tokio::test]
async fn ticket_start_writes_as_the_servers_author_and_needs_one() {
    let mut project = resolve_case("defaults");
    let root = project.root.clone();
    let resolved = call(project.clone(), "resolve", json!({})).await;
    assert!(!resolved.is_error, "{}", resolved.content);
    project.data_directory = Some(root.join("data"));

    let without = call(project.clone(), "ticket_start", json!({})).await;
    assert!(without.is_error);
    assert_eq!(without.content["status"], "invalid");
    assert!(!root.join("tickets").exists(), "nothing is written");

    project.author = Some(Author {
        name: "triage-bot".into(),
        role: "human".into(),
    });
    let started = call(
        project,
        "ticket_start",
        json!({ "attributes": { "size": 3 }, "external": [{ "system": "github", "key": "8798" }] }),
    )
    .await;
    assert!(!started.is_error, "{}", started.content);
    assert_eq!(started.content["profile"], "only");
    assert_eq!(started.content["attributes"], json!({ "size": 3 }));
    assert_eq!(
        started.content["event"]["author"],
        json!({ "name": "triage-bot", "role": "human" })
    );
    let ticket = started.content["ticket"].as_str().expect("an ID");
    let record =
        std::fs::read_to_string(root.join(format!("tickets/{ticket}.yaml"))).expect("the record");
    assert!(
        record.ends_with(
            "attributes: { \"size\": 3 }\nexternal:\n  - { system: \"github\", key: \"8798\" }\n"
        ),
        "{record}"
    );
    let _ = std::fs::remove_dir_all(&root);
}
