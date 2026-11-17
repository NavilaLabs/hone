use std::path::{Path, PathBuf};

use rmcp::ServiceExt;
use rmcp::model::CallToolRequestParams;
use rmcp::service::{RoleClient, RunningService, ServiceError};
use serde_json::{Value, json};

use crate::{Project, Server};

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
            "check",
            "check_artifact",
            "check_edit",
            "content_hash",
            "events",
            "resolve",
            "step_set",
            "thread_open",
            "thread_reopen",
            "thread_reply",
            "thread_resolve",
        ]
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
