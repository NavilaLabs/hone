//! The assistant's client of the Agent Client Protocol, `hone_gui::acp`, against the test agent
//! in `tests/agent/` (concept Section 12, "Assistant"). The agent calls the real `hone mcp` that
//! the client hands it, so a write shows in the process store under the assistant's author.
//!
//! It needs the `hone` binary: `HONE_BIN`, otherwise `target/debug/hone` of the main workspace
//! (`cargo build -p hone-cli` at the repository root).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use hone_gui::acp::{Agent, AgentCommand, Incoming, ToolCall, ToolContent, Update};
use hone_gui::hone::Hone;
use tokio::sync::mpsc::UnboundedReceiver;

const THREAD: &str = "th-0193a8f0-9b2c-7e4d-a1f3-000000000001";

fn repository() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("..")
}

fn hone_binary() -> PathBuf {
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

// `cargo test` builds the examples into `examples/` of the profile's directory, which holds
// this test's executable somewhere below it.
fn agent_binary() -> PathBuf {
    let name = format!("acp-test-agent{}", std::env::consts::EXE_SUFFIX);
    let executable = std::env::current_exe().expect("the test's executable");
    executable
        .ancestors()
        .map(|directory| directory.join("examples").join(&name))
        .find(|binary| binary.is_file())
        .unwrap_or_else(|| panic!("{name} is missing next to {}", executable.display()))
}

// A copy of a gate test case's project and process store, with an open thread.
struct Scratch {
    root: PathBuf,
}

impl Scratch {
    fn new(name: &str) -> Self {
        let root =
            std::env::temp_dir().join(format!("hone-gui-assistant-{}-{name}", std::process::id()));
        if root.exists() {
            std::fs::remove_dir_all(&root).expect("removable");
        }
        let case = repository().join("schemas/tests/check/no-open-blocking-threads-open");
        copy(&case.join("project"), &root.join("project"));
        copy(&case.join("data"), &root.join("data"));
        Self { root }
    }

    fn project(&self) -> PathBuf {
        self.root.join("project")
    }

    fn hone(&self) -> Hone {
        Hone::with_binary(hone_binary(), self.project())
            .with_variable("HONE_DATA_DIR", self.root.join("data"))
    }

    fn command(&self, login: bool) -> AgentCommand {
        let mut env = BTreeMap::from([(
            "HONE_TEST_AGENT_STATE".to_owned(),
            self.root.join("agent").display().to_string(),
        )]);
        if login {
            env.insert("HONE_TEST_AGENT_LOGIN".into(), "1".into());
        }
        AgentCommand {
            argv: vec![agent_binary().display().to_string()],
            env,
        }
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

fn prompt_text() -> String {
    format!(
        "Answer the thread.\n\nContext, to read through the hone MCP server:\n- ticket tk-9f2x4a8m\n- thread {THREAD}"
    )
}

/// Runs one turn, answering the permission request with `answer` (an option ID, `None` to
/// cancel the turn); returns the stop reason and the session's updates.
async fn turn(
    agent: &Arc<Agent>,
    incoming: &mut UnboundedReceiver<Incoming>,
    session: &str,
    answer: Option<&str>,
) -> (String, Vec<Update>) {
    let prompt = tokio::spawn({
        let agent = agent.clone();
        let session = session.to_owned();
        async move { agent.prompt(&session, &prompt_text()).await }
    });
    let mut updates = Vec::new();
    loop {
        match incoming.recv().await.expect("the agent runs") {
            Incoming::Update {
                session: owner,
                update,
            } => {
                assert_eq!(owner, session);
                updates.push(update);
            }
            Incoming::Permission(permission) => {
                assert_eq!(permission.session, session);
                assert_eq!(permission.tool.title.as_deref(), Some("thread_reply"));
                let names: Vec<&str> = permission
                    .options
                    .iter()
                    .map(|option| option.name.as_str())
                    .collect();
                assert_eq!(names, ["Allow once", "Reject"]);
                match answer {
                    Some(option) => agent.answer(permission.key, Some(option)),
                    None => agent.cancel(session),
                }
                break;
            }
            Incoming::Exited(message) => panic!("{message}"),
        }
    }
    let reason = prompt
        .await
        .expect("the turn ends")
        .expect("the agent answers");
    while let Ok(message) = incoming.try_recv() {
        if let Incoming::Update { update, .. } = message {
            updates.push(update);
        }
    }
    (reason, updates)
}

fn tool_status(updates: &[Update]) -> Option<ToolCall> {
    updates.iter().rev().find_map(|update| match update {
        Update::ToolCallUpdate(call) => Some(call.clone()),
        _ => None,
    })
}

#[tokio::test(flavor = "multi_thread")]
async fn a_turn_writes_through_hone_mcp_as_the_assistant_and_resumes_after_a_restart() {
    let scratch = Scratch::new("turn");
    let hone = scratch.hone();
    // The test case's configuration knows the roles `architect` and `human`.
    let mcp = hone.mcp_server("assistant", "human").expect("a root");
    let root = scratch.project().canonicalize().expect("canonical");
    let (agent, mut incoming) = Agent::start(&scratch.command(false), &root)
        .await
        .expect("started");
    assert_eq!(agent.info().name.as_deref(), Some("Test agent"));
    assert!(agent.info().load_session);
    let agent = Arc::new(agent);
    let session = agent.new_session(&root, &mcp).await.expect("a session");

    let (reason, updates) = turn(&agent, &mut incoming, &session, Some("allow")).await;
    assert_eq!(reason, "end_turn");
    let call = tool_status(&updates).expect("a tool call update");
    assert_eq!(call.status.as_deref(), Some("completed"));
    assert!(matches!(
        call.content.as_deref(),
        Some([ToolContent::Text(_), ToolContent::Diff { path, old: Some(old), new }])
            if path == "notes.md" && old == "a\n" && new == "b\n"
    ));
    assert!(
        updates.contains(&Update::AgentText(
            " fs read false write false, terminal false.".into()
        )),
        "the client offers neither files nor a terminal: {updates:?}"
    );
    let log = hone.log().await.expect("the log");
    // The log orders events by device, so the new device's event need not come last.
    let written: Vec<_> = log
        .events
        .iter()
        .filter(|event| event.payload["body"] == "Answered by the test agent.")
        .collect();
    let [written] = written.as_slice() else {
        panic!("one reply: {written:?}");
    };
    assert_eq!(written.kind, "thread.message");
    let author = written.author.as_ref().expect("an author");
    assert_eq!(
        (author.name.as_str(), author.role.as_str()),
        ("assistant", "human")
    );
    assert_eq!(written.payload["thread"], THREAD);
    drop(agent);

    let (agent, mut incoming) = Agent::start(&scratch.command(false), &root)
        .await
        .expect("started again");
    agent
        .load_session(&session, &root, &mcp)
        .await
        .expect("loaded");
    let mut replayed = Vec::new();
    while let Ok(Incoming::Update { update, .. }) = incoming.try_recv() {
        replayed.push(update);
    }
    assert_eq!(replayed.first(), Some(&Update::UserText(prompt_text())));
    assert_eq!(
        replayed.get(1),
        Some(&Update::AgentText("Replying ".into()))
    );
    assert_eq!(
        tool_status(&replayed).and_then(|call| call.status),
        Some("completed".into())
    );
    let unknown = agent.load_session("sess-unknown", &root, &mcp).await;
    assert_eq!(
        unknown.map_err(|error| error.message),
        Err("no session sess-unknown".into())
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_role_the_configuration_lacks_writes_nothing() {
    let scratch = Scratch::new("role");
    let hone = scratch.hone();
    let mcp = hone.mcp_server("assistant", "agent").expect("a root");
    let root = scratch.project().canonicalize().expect("canonical");
    let before = hone.log().await.expect("the log").events.len();
    let (agent, mut incoming) = Agent::start(&scratch.command(false), &root)
        .await
        .expect("started");
    let agent = Arc::new(agent);
    let session = agent.new_session(&root, &mcp).await.expect("a session");
    let (reason, updates) = turn(&agent, &mut incoming, &session, Some("allow")).await;
    assert_eq!(reason, "end_turn");
    let call = tool_status(&updates).expect("a tool call update");
    assert_eq!(call.status.as_deref(), Some("failed"));
    assert_eq!(
        call.output,
        Some(serde_json::json!({
            "status": "invalid",
            "message": "the role `agent` does not exist in the ticket's configuration"
        }))
    );
    assert_eq!(hone.log().await.expect("the log").events.len(), before);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_cancelled_turn_answers_the_permission_as_cancelled_and_writes_nothing() {
    let scratch = Scratch::new("cancel");
    let hone = scratch.hone();
    let mcp = hone.mcp_server("assistant", "agent").expect("a root");
    let root = scratch.project().canonicalize().expect("canonical");
    let before = hone.log().await.expect("the log").events.len();
    let (agent, mut incoming) = Agent::start(&scratch.command(false), &root)
        .await
        .expect("started");
    let agent = Arc::new(agent);
    let session = agent.new_session(&root, &mcp).await.expect("a session");
    let (reason, updates) = turn(&agent, &mut incoming, &session, None).await;
    assert_eq!(reason, "cancelled");
    assert_eq!(
        tool_status(&updates).and_then(|call| call.status),
        Some("failed".into())
    );
    assert_eq!(hone.log().await.expect("the log").events.len(), before);
}

#[tokio::test(flavor = "multi_thread")]
async fn an_agent_that_asks_for_a_login_opens_a_session_after_it() {
    let scratch = Scratch::new("login");
    let mcp = scratch
        .hone()
        .mcp_server("assistant", "agent")
        .expect("a root");
    let root = scratch.project().canonicalize().expect("canonical");
    let (agent, _incoming) = Agent::start(&scratch.command(true), &root)
        .await
        .expect("started");
    let methods: Vec<(&str, &str)> = agent
        .info()
        .auth_methods
        .iter()
        .map(|method| (method.id.as_str(), method.name.as_str()))
        .collect();
    assert_eq!(methods, [("test-login", "Log in to the test agent")]);
    let refused = agent
        .new_session(&root, &mcp)
        .await
        .expect_err("a login first");
    assert!(refused.needs_login(), "{refused:?}");
    agent
        .authenticate("unknown")
        .await
        .expect_err("not a method");
    agent.authenticate("test-login").await.expect("logged in");
    agent.new_session(&root, &mcp).await.expect("a session");
}

#[tokio::test(flavor = "multi_thread")]
async fn an_agent_that_cannot_start_or_ends_at_once_is_reported() {
    let scratch = Scratch::new("start");
    let root = scratch.project();
    let missing = AgentCommand {
        argv: vec!["hone-no-such-agent".into()],
        env: BTreeMap::new(),
    };
    let error = Agent::start(&missing, &root)
        .await
        .expect_err("not started");
    assert!(
        error
            .message
            .starts_with("`hone-no-such-agent` cannot be started:"),
        "{error:?}"
    );
    // Without its state directory the test agent ends before it answers.
    let ending = AgentCommand {
        argv: vec![agent_binary().display().to_string()],
        env: BTreeMap::new(),
    };
    let error = Agent::start(&ending, &root).await.expect_err("ended");
    assert_eq!(error.message, "the agent has ended");
    let empty = Agent::start(&AgentCommand::default(), &root)
        .await
        .expect_err("empty");
    assert_eq!(empty.message, "the agent's command is empty");
}
