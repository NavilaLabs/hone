//! A small agent of the Agent Client Protocol for the tests of the assistant. It answers every
//! prompt with one tool call of the MCP server it was handed: `thread_reply` to the thread the
//! prompt's context names, after asking for permission.
//!
//! - `HONE_TEST_AGENT_STATE`: a directory where it keeps each session's updates, which
//!   `session/load` replays.
//! - `HONE_TEST_AGENT_LOGIN`: when set, `session/new` asks for the login `test-login` first.

use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::process::{Command, Stdio};

use serde_json::{Value, json};

struct Agent {
    input: std::io::Lines<std::io::StdinLock<'static>>,
    state: PathBuf,
    logged_in: bool,
    capabilities: Value,
    servers: Vec<(String, Value)>,
    next_request: u64,
}

fn main() {
    let state =
        PathBuf::from(std::env::var_os("HONE_TEST_AGENT_STATE").expect("a state directory"));
    std::fs::create_dir_all(&state).expect("creatable");
    let mut agent = Agent {
        input: std::io::stdin().lock().lines(),
        state,
        logged_in: std::env::var_os("HONE_TEST_AGENT_LOGIN").is_none(),
        capabilities: Value::Null,
        servers: Vec::new(),
        next_request: 0,
    };
    while let Some(message) = agent.next_message() {
        let (Some(method), Some(id)) = (message["method"].as_str(), message.get("id")) else {
            continue;
        };
        let answer = agent.handle(method, &message["params"]);
        let reply = match answer {
            Ok(result) => json!({ "jsonrpc": "2.0", "id": id, "result": result }),
            Err((code, text)) => {
                json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": text } })
            }
        };
        send(&reply);
    }
}

fn send(message: &Value) {
    let mut out = std::io::stdout().lock();
    writeln!(out, "{message}").expect("writable");
    out.flush().expect("flushed");
}

impl Agent {
    fn next_message(&mut self) -> Option<Value> {
        loop {
            let line = self.input.next()?.ok()?;
            if let Ok(message) = serde_json::from_str(&line) {
                return Some(message);
            }
        }
    }

    fn handle(&mut self, method: &str, params: &Value) -> Result<Value, (i64, String)> {
        match method {
            "initialize" => {
                self.capabilities = params["clientCapabilities"].clone();
                Ok(json!({
                    "protocolVersion": 1,
                    "agentCapabilities": { "loadSession": true },
                    "authMethods": [{ "id": "test-login", "name": "Log in to the test agent" }],
                    "agentInfo": { "name": "hone-test-agent", "title": "Test agent", "version": "0" }
                }))
            }
            "authenticate" if params["methodId"] == "test-login" => {
                self.logged_in = true;
                Ok(json!({}))
            }
            "session/new" if !self.logged_in => Err((-32000, "Authentication required".to_owned())),
            "session/new" => {
                self.next_request += 1;
                let session = format!("sess-{}-{}", std::process::id(), self.next_request);
                std::fs::write(self.state.join(&session), "").expect("writable");
                self.servers
                    .push((session.clone(), params["mcpServers"][0].clone()));
                Ok(json!({ "sessionId": session }))
            }
            "session/load" => {
                let session = params["sessionId"].as_str().unwrap_or_default().to_owned();
                let saved = std::fs::read_to_string(self.state.join(&session))
                    .map_err(|_| (-32002, format!("no session {session}")))?;
                self.servers
                    .push((session.clone(), params["mcpServers"][0].clone()));
                for line in saved.lines() {
                    let update: Value = serde_json::from_str(line).expect("kept as JSON");
                    send(&notification(&session, update));
                }
                Ok(json!({}))
            }
            "session/prompt" => self.prompt(params),
            _ => Err((-32601, format!("{method} is not offered"))),
        }
    }

    fn update(&self, session: &str, update: Value) {
        let mut kept = std::fs::OpenOptions::new()
            .append(true)
            .open(self.state.join(session))
            .expect("openable");
        writeln!(kept, "{update}").expect("writable");
        send(&notification(session, update));
    }

    fn prompt(&mut self, params: &Value) -> Result<Value, (i64, String)> {
        let session = params["sessionId"].as_str().unwrap_or_default().to_owned();
        let server = self
            .servers
            .iter()
            .find(|(known, _)| *known == session)
            .map(|(_, server)| server.clone())
            .ok_or((-32002, format!("no session {session}")))?;
        let text: String = params["prompt"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|block| block["text"].as_str())
            .collect();
        self.update(
            &session,
            json!({ "sessionUpdate": "user_message_chunk", "content": { "type": "text", "text": text } }),
        );
        let thread = text
            .lines()
            .find_map(|line| line.strip_prefix("- thread "))
            .unwrap_or_default()
            .to_owned();
        let input = json!({ "thread": thread, "body": "Answered by the test agent." });
        self.update(
            &session,
            json!({ "sessionUpdate": "agent_message_chunk", "content": { "type": "text", "text": "Replying " } }),
        );
        self.update(
            &session,
            json!({ "sessionUpdate": "agent_message_chunk", "content": { "type": "text", "text": "in the thread." } }),
        );
        self.update(
            &session,
            json!({ "sessionUpdate": "tool_call", "toolCallId": "call-1", "title": "thread_reply", "kind": "edit", "status": "pending", "rawInput": input }),
        );
        self.next_request += 1;
        let request = self.next_request;
        send(&json!({
            "jsonrpc": "2.0",
            "id": request,
            "method": "session/request_permission",
            "params": {
                "sessionId": session,
                "toolCall": { "toolCallId": "call-1", "title": "thread_reply", "rawInput": input },
                "options": [
                    { "optionId": "allow", "name": "Allow once", "kind": "allow_once" },
                    { "optionId": "reject", "name": "Reject", "kind": "reject_once" }
                ]
            }
        }));
        let mut cancelled = false;
        let outcome = loop {
            let Some(message) = self.next_message() else {
                return Err((-32603, "the client has gone".to_owned()));
            };
            if message["method"] == "session/cancel" {
                cancelled = true;
            } else if message["id"] == request {
                break message["result"]["outcome"].clone();
            }
        };
        if outcome["outcome"] == "selected" && outcome["optionId"] == "allow" {
            let result = call_tool(&server, "thread_reply", &input);
            let output = result["content"][0]["text"]
                .as_str()
                .unwrap_or_default()
                .to_owned();
            self.update(
                &session,
                json!({
                    "sessionUpdate": "tool_call_update",
                    "toolCallId": "call-1",
                    "status": if result["isError"] == true { "failed" } else { "completed" },
                    "rawOutput": result["structuredContent"],
                    "content": [
                        { "type": "content", "content": { "type": "text", "text": output } },
                        { "type": "diff", "path": "notes.md", "oldText": "a\n", "newText": "b\n" }
                    ]
                }),
            );
        } else {
            self.update(
                &session,
                json!({ "sessionUpdate": "tool_call_update", "toolCallId": "call-1", "status": "failed" }),
            );
        }
        let fs = &self.capabilities["fs"];
        self.update(
            &session,
            json!({ "sessionUpdate": "agent_message_chunk", "content": { "type": "text", "text": format!(
                " fs read {} write {}, terminal {}.",
                fs["readTextFile"], fs["writeTextFile"], self.capabilities["terminal"]
            ) } }),
        );
        let reason = if cancelled || outcome["outcome"] == "cancelled" {
            "cancelled"
        } else {
            "end_turn"
        };
        Ok(json!({ "stopReason": reason }))
    }
}

fn notification(session: &str, update: Value) -> Value {
    json!({
        "jsonrpc": "2.0",
        "method": "session/update",
        "params": { "sessionId": session, "update": update }
    })
}

/// Starts the MCP server `server` and calls its tool `name` with `arguments`.
fn call_tool(server: &Value, name: &str, arguments: &Value) -> Value {
    let mut command = Command::new(server["command"].as_str().expect("a command"));
    command
        .args(
            server["args"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(Value::as_str),
        )
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit());
    for variable in server["env"].as_array().into_iter().flatten() {
        command.env(
            variable["name"].as_str().expect("a name"),
            variable["value"].as_str().expect("a value"),
        );
    }
    let mut child = command.spawn().expect("the MCP server starts");
    let mut stdin = child.stdin.take().expect("piped");
    let mut lines = BufReader::new(child.stdout.take().expect("piped")).lines();
    let mut call = |message: Value| {
        writeln!(stdin, "{message}").expect("writable");
        stdin.flush().expect("flushed");
    };
    let mut answer = |id: u64| loop {
        let line = lines.next().expect("an answer").expect("readable");
        let message: Value = serde_json::from_str(&line).expect("JSON");
        if message["id"] == id {
            return message;
        }
    };
    call(json!({
        "jsonrpc": "2.0", "id": 1, "method": "initialize",
        "params": { "protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": { "name": "hone-test-agent", "version": "0" } }
    }));
    answer(1);
    call(json!({ "jsonrpc": "2.0", "method": "notifications/initialized" }));
    call(json!({
        "jsonrpc": "2.0", "id": 2, "method": "tools/call",
        "params": { "name": name, "arguments": arguments }
    }));
    let result = answer(2)["result"].clone();
    drop(call);
    let _ = child.kill();
    let _ = child.wait();
    result
}
