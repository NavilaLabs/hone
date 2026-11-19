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
    let root = scratch
        .root
        .join("case/project")
        .canonicalize()
        .expect("canonical")
        .display()
        .to_string();
    assert_eq!(
        json_of(&used),
        json!({ "name": "acme", "root": root, "context": { "peer": { "folder": folder }, "author": { "name": "alice", "role": "architect" } } })
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

#[test]
fn another_checkout_with_the_same_project_id_gets_no_context() {
    let scratch = Scratch::new("claimed");
    let folder = scratch.root.join("folder").display().to_string();
    scratch.hone(&[
        "context",
        "set",
        "acme",
        "--folder",
        &folder,
        "--author",
        "alice",
        "--role",
        "architect",
    ]);
    scratch.hone(&["context", "use", "acme"]);
    copy(
        &scratch.root.join("case/project"),
        &scratch.root.join("claimed"),
    );

    let output = Command::new(env!("CARGO_BIN_EXE_hone"))
        .args(["sync", "--json"])
        .current_dir(scratch.root.join("claimed"))
        .env("HONE_DATA_DIR", scratch.root.join("case/data"))
        .env("HONE_CONFIG_DIR", scratch.root.join("config"))
        .env_remove("HONE_SYNC_FOLDER")
        .env_remove("HONE_SYNC_SERVER")
        .env_remove("HONE_SYNC_GIT")
        .output()
        .expect("hone runs");
    assert_eq!(output.status.code(), Some(2));
    let message = json_of(&output)["message"]
        .as_str()
        .unwrap_or_default()
        .to_owned();
    assert!(
        message.starts_with("the project `prj-8m2k4d7q` is bound to the context `acme` in "),
        "{message}"
    );
    assert_eq!(
        std::fs::read_dir(scratch.root.join("folder"))
            .expect("readable")
            .count(),
        0,
        "nothing is synced into the context's folder"
    );
}

// One MCP session of `hone mcp` over stdio, answered line by line.
struct McpSession {
    child: std::process::Child,
    stdout: std::io::BufReader<std::process::ChildStdout>,
    next_id: u64,
}

impl McpSession {
    fn start(scratch: &Scratch, arguments: &[&str]) -> Self {
        let mut child = Command::new(env!("CARGO_BIN_EXE_hone"))
            .arg("mcp")
            .args(arguments)
            .current_dir(scratch.root.join("case/project"))
            .env("HONE_DATA_DIR", scratch.root.join("case/data"))
            .env("HONE_CONFIG_DIR", scratch.root.join("config"))
            .env_remove("HONE_AUTHOR")
            .env_remove("HONE_ROLE")
            .env_remove("HONE_SYNC_FOLDER")
            .env_remove("HONE_SYNC_SERVER")
            .env_remove("HONE_SYNC_GIT")
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .spawn()
            .expect("hone mcp starts");
        let stdout = std::io::BufReader::new(child.stdout.take().expect("piped"));
        let mut session = Self {
            child,
            stdout,
            next_id: 0,
        };
        session.request(
            "initialize",
            json!({ "protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": { "name": "test", "version": "0" } }),
        );
        session.send(&json!({ "jsonrpc": "2.0", "method": "notifications/initialized" }));
        session
    }

    fn send(&mut self, message: &Value) {
        use std::io::Write;
        let stdin = self.child.stdin.as_mut().expect("piped");
        writeln!(stdin, "{message}").expect("writable");
        stdin.flush().expect("flushed");
    }

    fn request(&mut self, method: &str, params: Value) -> Value {
        use std::io::BufRead;
        self.next_id += 1;
        let id = self.next_id;
        self.send(&json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params }));
        let mut line = String::new();
        self.stdout.read_line(&mut line).expect("readable");
        let answer: Value = serde_json::from_str(&line).expect("one JSON-RPC message");
        assert_eq!(answer["id"], id, "{line}");
        answer["result"].clone()
    }

    fn call(&mut self, tool: &str, arguments: Value) -> Value {
        let result = self.request(
            "tools/call",
            json!({ "name": tool, "arguments": arguments }),
        );
        assert_eq!(result["isError"], false, "{result}");
        result["structuredContent"].clone()
    }
}

impl Drop for McpSession {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn bind_folder_context(scratch: &Scratch) {
    let folder = scratch.root.join("folder").display().to_string();
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
    ]);
    assert_eq!(set.status.code(), Some(0));
    let used = scratch.hone(&["context", "use", "acme"]);
    assert_eq!(used.status.code(), Some(0));
}

#[test]
fn the_mcp_server_writes_as_and_syncs_with_the_bound_context() {
    let scratch = Scratch::new("mcp-bound");
    bind_folder_context(&scratch);

    let mut session = McpSession::start(&scratch, &[]);
    let written = session.call(
        "step_set",
        json!({ "ticket": "tk-9f2x4a8m", "step": "decision", "status": "done" }),
    );
    assert_eq!(
        written["event"]["author"],
        json!({ "name": "alice", "role": "architect" })
    );
    let synced = session.call("sync", json!({}));
    assert_eq!(
        synced["sent"]
            .as_object()
            .map(|map| map.values().cloned().collect::<Vec<_>>()),
        Some(vec![json!(1)])
    );
    let device_logs = std::fs::read_dir(scratch.root.join("folder"))
        .expect("readable")
        .count();
    assert_eq!(device_logs, 1, "the sync wrote into the context's folder");
}

#[test]
fn a_flag_of_the_mcp_server_wins_over_the_context() {
    let scratch = Scratch::new("mcp-flag");
    bind_folder_context(&scratch);
    let other = scratch.root.join("other");
    std::fs::create_dir_all(&other).expect("creatable");

    let mut session = McpSession::start(
        &scratch,
        &[
            "--author",
            "critic-bot",
            "--role",
            "human",
            "--sync-folder",
            &other.display().to_string(),
        ],
    );
    let written = session.call(
        "step_set",
        json!({ "ticket": "tk-9f2x4a8m", "step": "decision", "status": "done" }),
    );
    assert_eq!(
        written["event"]["author"],
        json!({ "name": "critic-bot", "role": "human" })
    );
    session.call("sync", json!({}));
    assert_eq!(
        std::fs::read_dir(&other).expect("readable").count(),
        1,
        "the flag's folder"
    );
    assert_eq!(
        std::fs::read_dir(scratch.root.join("folder"))
            .expect("readable")
            .count(),
        0,
        "not the context's folder"
    );
}

#[test]
fn the_mcp_server_does_not_start_with_a_context_it_cannot_use() {
    let scratch = Scratch::new("mcp-refused");
    bind_folder_context(&scratch);
    copy(
        &scratch.root.join("case/project"),
        &scratch.root.join("claimed"),
    );
    let claimed = Command::new(env!("CARGO_BIN_EXE_hone"))
        .args(["mcp", "--root"])
        .arg(scratch.root.join("claimed"))
        .env("HONE_DATA_DIR", scratch.root.join("case/data"))
        .env("HONE_CONFIG_DIR", scratch.root.join("config"))
        .env_remove("HONE_AUTHOR")
        .env_remove("HONE_ROLE")
        .env_remove("HONE_SYNC_FOLDER")
        .env_remove("HONE_SYNC_SERVER")
        .env_remove("HONE_SYNC_GIT")
        .stdin(std::process::Stdio::null())
        .output()
        .expect("hone runs");
    assert_eq!(claimed.status.code(), Some(1));
    let message = String::from_utf8_lossy(&claimed.stderr).into_owned();
    assert!(
        message
            .starts_with("hone mcp: the project `prj-8m2k4d7q` is bound to the context `acme` in "),
        "{message}"
    );

    let set = scratch.hone(&[
        "context",
        "set",
        "acme",
        "--server",
        "http://127.0.0.1:9",
        "--token-env",
        "HONE_TOKEN_ACME_TEST",
    ]);
    assert_eq!(set.status.code(), Some(0));
    let without_token = Command::new(env!("CARGO_BIN_EXE_hone"))
        .arg("mcp")
        .current_dir(scratch.root.join("case/project"))
        .env("HONE_DATA_DIR", scratch.root.join("case/data"))
        .env("HONE_CONFIG_DIR", scratch.root.join("config"))
        .env_remove("HONE_AUTHOR")
        .env_remove("HONE_ROLE")
        .env_remove("HONE_SYNC_FOLDER")
        .env_remove("HONE_SYNC_SERVER")
        .env_remove("HONE_SYNC_GIT")
        .env_remove("HONE_TOKEN_ACME_TEST")
        .stdin(std::process::Stdio::null())
        .output()
        .expect("hone runs");
    assert_eq!(without_token.status.code(), Some(1));
    // A machine with an OS keyring adds nothing here, because no token is stored for this
    // context and server; one whose keyring fails to answer appends the reason.
    let message = String::from_utf8_lossy(&without_token.stderr).into_owned();
    assert!(
        message.starts_with(
            "hone mcp: no token for the server: set HONE_TOKEN_ACME_TEST or store one with hone context login acme"
        ),
        "{message}"
    );
}
