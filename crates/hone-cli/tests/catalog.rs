//! `hone artifacts --json` and `hone ticket show --json`, the lists a GUI chooses from (concept
//! Section 12, "Desktop GUI").

use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::{Value, json};

const CASE: &str = "schemas/tests/check/no-open-blocking-threads-open/project";

fn project() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(CASE)
}

fn hone(arguments: &[&str]) -> (Option<i32>, Value) {
    let output = Command::new(env!("CARGO_BIN_EXE_hone"))
        .args(arguments)
        .arg("--root")
        .arg(project())
        .arg("--json")
        .output()
        .expect("hone runs");
    let value = serde_json::from_slice(&output.stdout).expect("one JSON document");
    (output.status.code(), value)
}

#[test]
fn artifacts_lists_every_artifact_of_the_checked_directory() {
    let (code, value) = hone(&["artifacts"]);
    assert_eq!(code, Some(0));
    assert_eq!(
        value,
        json!({
            "project": "prj-8m2k4d7q",
            "artifacts": [
                {
                    "id": "dec-7k3m2q8p",
                    "type": "decision",
                    "path": "design/decisions/dec-7k3m2q8p-payment-async.md",
                    "ticket": "tk-9f2x4a8m",
                    "status": "accepted",
                    "effective": "accepted",
                    "links": { "affects": ["mod-4t8w2n6c"] }
                },
                {
                    "id": "mod-4t8w2n6c",
                    "type": "module",
                    "path": "design/modules/mod-4t8w2n6c-fee.yaml"
                },
                {
                    "id": "spec-3b8n1c5w",
                    "type": "spec",
                    "path": "design/tickets/tk-9f2x4a8m/spec-3b8n1c5w-async.md",
                    "ticket": "tk-9f2x4a8m",
                    "status": "accepted",
                    "effective": "accepted"
                },
                {
                    "id": "tk-9f2x4a8m",
                    "type": "ticket",
                    "path": "design/tickets/tk-9f2x4a8m/ticket.yaml",
                    "profile": "standard"
                }
            ],
            "unreadable": []
        })
    );
}

#[test]
fn ticket_show_prints_the_profile_steps_and_roles() {
    let (code, value) = hone(&["ticket", "show", "--ticket", "tk-9f2x4a8m"]);
    assert_eq!(code, Some(0));
    assert_eq!(
        value,
        json!({
            "ticket": "tk-9f2x4a8m",
            "path": "design/tickets/tk-9f2x4a8m/ticket.yaml",
            "profile": "standard",
            "selected": "standard",
            "config": "sha256:4c076d82c93f26478168d828d016650bde32bdc36819505c13e6f9d103671b15",
            "attributes": {},
            "external": [],
            "roles": ["architect", "human"],
            "steps": [
                { "step": "completion", "requirement": "optional", "after": [] },
                { "step": "decision", "requirement": "required", "after": [] },
                { "step": "implementation", "requirement": "required", "after": [] },
                { "step": "interfaces", "requirement": "optional", "after": [] },
                { "step": "settled", "requirement": "optional", "after": [] },
                { "step": "specify", "requirement": "required", "after": [] }
            ]
        })
    );
}

#[test]
fn review_show_without_a_store_has_the_hash_and_the_file_review() {
    let missing = std::env::temp_dir().join(format!("hone-review-show-{}", std::process::id()));
    let output = Command::new(env!("CARGO_BIN_EXE_hone"))
        .args([
            "review",
            "show",
            "--ticket",
            "tk-9f2x4a8m",
            "dec-7k3m2q8p",
            "--json",
            "--root",
        ])
        .arg(project())
        .env("HONE_DATA_DIR", &missing)
        .output()
        .expect("hone runs");
    assert_eq!(output.status.code(), Some(0));
    let value: Value = serde_json::from_slice(&output.stdout).expect("one JSON document");
    // The fixture's review block approves the current content.
    let hash = "sha256:678e7bc1cacb3c07fe77369e22fb499f58067c31ad2eeb66b929a32ce8ce45f6";
    assert_eq!(
        value,
        json!({
            "artifact": "dec-7k3m2q8p",
            "path": "design/decisions/dec-7k3m2q8p-payment-async.md",
            "hash": hash,
            "decisions": [],
            "confirmations": [],
            "review": { "approved_by": [{ "name": "alice", "role": "architect", "hash": hash, "date": "2026-11-16" }] }
        })
    );
    assert!(!missing.exists(), "showing creates no store");
}

#[test]
fn ticket_show_of_a_malformed_id_fails_with_exit_code_1() {
    let (code, value) = hone(&["ticket", "show", "--ticket", "PAY-123"]);
    assert_eq!(code, Some(1));
    assert_eq!(
        value,
        json!({ "status": "error", "message": "`PAY-123` is not a ticket ID" })
    );
}

#[test]
fn ticket_select_reads_attributes_like_ticket_start() {
    let (code, value) = hone(&[
        "ticket",
        "select",
        "--attribute",
        "risk=high",
        "--attribute",
        "size=3",
    ]);
    assert_eq!(code, Some(0));
    assert_eq!(
        value,
        json!({ "selected": "standard", "rule": null, "rules": [], "default": "standard", "profiles": ["standard"] })
    );
    let (code, value) = hone(&["ticket", "select", "--attribute", "risk"]);
    assert_eq!(code, Some(1));
    assert_eq!(value["status"], "error");
    assert!(
        value["message"]
            .as_str()
            .is_some_and(|message| message.ends_with("`risk` is not `<key>=<value>`")),
        "{value}"
    );
}
