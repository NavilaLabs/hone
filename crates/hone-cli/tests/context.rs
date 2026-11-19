//! Contexts (concept Section 12, "Contexts"): `hone context`, and the peer and author a bound
//! project falls back to.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use serde_json::{Value, json};

const CASE: &str = "schemas/tests/check/no-open-blocking-threads-open";

fn repository() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

// A copy of a gate test case, a configuration directory, and a shared folder, removed on drop.
struct Scratch {
    root: PathBuf,
}

impl Scratch {
    fn new(name: &str) -> Self {
        let root =
            std::env::temp_dir().join(format!("hone-cli-context-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        copy(&repository().join(CASE), &root.join("case"));
        std::fs::create_dir_all(root.join("folder")).expect("creatable");
        Self { root }
    }

    fn hone(&self, arguments: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_hone"))
            .args(arguments)
            .current_dir(self.root.join("case/project"))
            .env("HONE_DATA_DIR", self.root.join("case/data"))
            .env("HONE_CONFIG_DIR", self.root.join("config"))
            .env_remove("HONE_AUTHOR")
            .env_remove("HONE_ROLE")
            .env_remove("HONE_SYNC_FOLDER")
            .env_remove("HONE_SYNC_SERVER")
            .env_remove("HONE_SYNC_GIT")
            .output()
            .expect("hone runs")
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

fn json_of(output: &Output) -> Value {
    serde_json::from_slice(&output.stdout).expect("one JSON document")
}

#[test]
fn a_bound_project_syncs_with_and_writes_as_its_context() {
    let scratch = Scratch::new("bound");
    let folder = scratch.root.join("folder").display().to_string();

    let unbound = scratch.hone(&["sync", "--json"]);
    assert_eq!(unbound.status.code(), Some(2));
    assert_eq!(json_of(&unbound)["status"], "invalid");

    let set = scratch.hone(&[
        "context",
        "set",
        "acme",
        "--folder",
        &folder,
        "--author",
        "alice",
        "--role",
        "architect",
        "--json",
    ]);
    assert_eq!(set.status.code(), Some(0));
    let used = scratch.hone(&["context", "use", "acme", "--json"]);
    assert_eq!(
        json_of(&used),
        json!({ "name": "acme", "context": { "peer": { "folder": folder }, "author": { "name": "alice", "role": "architect" } } })
    );

    let written = scratch.hone(&[
        "step",
        "set",
        "--ticket",
        "tk-9f2x4a8m",
        "decision",
        "done",
        "--json",
    ]);
    assert_eq!(
        written.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&written.stdout)
    );
    assert_eq!(
        json_of(&written)["event"]["author"],
        json!({ "name": "alice", "role": "architect" })
    );

    let explicit = scratch.hone(&[
        "step",
        "set",
        "--ticket",
        "tk-9f2x4a8m",
        "decision",
        "open",
        "--author",
        "bob",
        "--role",
        "human",
        "--json",
    ]);
    assert_eq!(
        json_of(&explicit)["event"]["author"],
        json!({ "name": "bob", "role": "human" }),
        "a flag wins over the context"
    );

    let synced = scratch.hone(&["sync", "--json"]);
    assert_eq!(synced.status.code(), Some(0));
    let sent = json_of(&synced)["sent"].clone();
    assert_eq!(
        sent.as_object()
            .map(|map| map.values().cloned().collect::<Vec<_>>()),
        Some(vec![json!(2)])
    );
}

#[test]
fn a_context_needs_a_known_name_and_a_peer() {
    let scratch = Scratch::new("errors");
    let unknown = scratch.hone(&["context", "use", "ghost", "--json"]);
    assert_eq!(unknown.status.code(), Some(1));
    assert_eq!(
        json_of(&unknown),
        json!({ "status": "invalid", "message": "no context `ghost`: create it with hone context set" })
    );
    let without_peer = scratch.hone(&["context", "set", "acme", "--json"]);
    assert_eq!(
        json_of(&without_peer)["message"],
        "name the context's peer: --server, --folder, or --git"
    );
    assert!(!scratch.root.join("config").exists(), "nothing is written");
}
