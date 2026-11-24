//! A client of the Agent Client Protocol, version 1 (<https://agentclientprotocol.com>), over the
//! standard input and output of an agent's process (concept Section 12, "Assistant"). It offers
//! the agent neither files nor a terminal, so the agent reaches the project only through the MCP
//! server it is handed.

use std::collections::{BTreeMap, HashMap};
use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, PoisonError};

use serde_json::{Value, json};
use tokio::sync::{mpsc, oneshot};

const PROTOCOL_VERSION: u64 = 1;
const AUTH_REQUIRED: i64 = -32000;
const METHOD_NOT_FOUND: i64 = -32601;
const STDERR_LINES: usize = 20;

/// The program that starts an agent, with the environment variables it needs.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AgentCommand {
    pub argv: Vec<String>,
    pub env: BTreeMap<String, String>,
}

/// An MCP server the agent starts over standard input and output.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct McpServer {
    pub name: String,
    pub command: PathBuf,
    pub args: Vec<String>,
    pub env: Vec<(String, String)>,
}

/// What the agent announced in `initialize`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentInfo {
    /// The agent's title or name, if it gave one.
    pub name: Option<String>,
    /// Whether the agent can load an earlier session (`loadSession`).
    pub load_session: bool,
    /// The login methods the client may pass to [`Agent::authenticate`].
    pub auth_methods: Vec<AuthMethod>,
}

/// A login method of the agent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthMethod {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
}

/// Why a call to the agent failed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentError {
    /// The JSON-RPC error code, `None` when the agent could not be reached or answered
    /// something this client does not understand.
    pub code: Option<i64>,
    pub message: String,
}

impl AgentError {
    fn local(message: impl Into<String>) -> Self {
        Self {
            code: None,
            message: message.into(),
        }
    }

    /// Whether the agent asks for a login first (ACP `auth_required`).
    #[must_use]
    pub fn needs_login(&self) -> bool {
        self.code == Some(AUTH_REQUIRED)
    }
}

impl std::fmt::Display for AgentError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.message)
    }
}

/// What the agent sends without being asked.
#[derive(Debug, Clone, PartialEq)]
pub enum Incoming {
    /// A `session/update` of the session `session`.
    Update { session: String, update: Update },
    /// A `session/request_permission`, open until [`Agent::answer`] answers it by `key`.
    Permission(Permission),
    /// The agent's process ended; the message names its exit and its last error output.
    Exited(String),
}

/// A change to a session's conversation.
#[derive(Debug, Clone, PartialEq)]
pub enum Update {
    /// A piece of the person's message, as an agent replays it in `session/load`.
    UserText(String),
    /// A piece of the agent's answer, Markdown.
    AgentText(String),
    /// A piece of the agent's reasoning.
    Thought(String),
    /// A new tool call.
    ToolCall(ToolCall),
    /// A change to a tool call; only the fields it names change.
    ToolCallUpdate(ToolCall),
    /// Any other update, by its `sessionUpdate`.
    Other(String),
}

/// A tool call, or the fields a change to it names.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ToolCall {
    pub id: String,
    pub title: Option<String>,
    /// `read`, `edit`, `execute`, ...
    pub kind: Option<String>,
    /// `pending`, `in_progress`, `completed`, or `failed`.
    pub status: Option<String>,
    pub input: Option<Value>,
    pub output: Option<Value>,
    pub content: Option<Vec<ToolContent>>,
}

/// What a tool call produced.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ToolContent {
    Text(String),
    /// A change to a file; `old` is `None` for a new file.
    Diff {
        path: String,
        old: Option<String>,
        new: String,
    },
}

/// A permission the agent asks for.
#[derive(Debug, Clone, PartialEq)]
pub struct Permission {
    /// The client's key of the request.
    pub key: u64,
    pub session: String,
    pub tool: ToolCall,
    pub options: Vec<PermissionOption>,
}

/// One answer the agent offers to a permission request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PermissionOption {
    pub id: String,
    /// The text the agent wants shown, word for word.
    pub name: String,
    /// `allow_once`, `allow_always`, `reject_once`, or `reject_always`.
    pub kind: String,
}

type Pending = Arc<Mutex<HashMap<u64, oneshot::Sender<Result<Value, AgentError>>>>>;
// Open permission requests: the client's key, the session, and the agent's request ID.
type OpenPermissions = Arc<Mutex<BTreeMap<u64, (String, Value)>>>;

/// A running agent. Dropping it answers every open permission request as cancelled and stops
/// the process.
#[derive(Debug)]
pub struct Agent {
    info: AgentInfo,
    child: Mutex<Child>,
    connection: Connection,
}

#[derive(Debug, Clone)]
struct Connection {
    stdin: Arc<Mutex<ChildStdin>>,
    pending: Pending,
    permissions: OpenPermissions,
    next: Arc<AtomicU64>,
}

impl Agent {
    /// Starts `command` in `cwd` and initializes it, offering neither `fs` nor `terminal`.
    /// The receiver yields what the agent sends without being asked.
    ///
    /// # Errors
    ///
    /// Returns an [`AgentError`] when the command is empty or cannot be started, when the
    /// agent fails `initialize`, or when it speaks another protocol version.
    pub async fn start(
        command: &AgentCommand,
        cwd: &Path,
    ) -> Result<(Self, mpsc::UnboundedReceiver<Incoming>), AgentError> {
        let (program, arguments) = command
            .argv
            .split_first()
            .ok_or_else(|| AgentError::local("the agent's command is empty"))?;
        let mut child = Command::new(program)
            .args(arguments)
            .envs(&command.env)
            .current_dir(cwd)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|error| {
                AgentError::local(format!("`{program}` cannot be started: {error}"))
            })?;
        let (Some(stdin), Some(stdout), Some(stderr)) =
            (child.stdin.take(), child.stdout.take(), child.stderr.take())
        else {
            let _ = child.kill();
            return Err(AgentError::local("the agent's pipes cannot be opened"));
        };
        let connection = Connection {
            stdin: Arc::new(Mutex::new(stdin)),
            pending: Arc::default(),
            permissions: Arc::default(),
            next: Arc::new(AtomicU64::new(0)),
        };
        let errors = Arc::new(Mutex::new(Vec::<String>::new()));
        std::thread::spawn({
            let errors = errors.clone();
            move || keep_last_lines(stderr, &errors)
        });
        let (sender, receiver) = mpsc::unbounded_channel();
        std::thread::spawn({
            let connection = connection.clone();
            move || connection.read(stdout, &sender, &errors)
        });
        let mut agent = Self {
            info: AgentInfo {
                name: None,
                load_session: false,
                auth_methods: Vec::new(),
            },
            child: Mutex::new(child),
            connection,
        };
        let answer = agent
            .connection
            .request(
                "initialize",
                json!({
                    "protocolVersion": PROTOCOL_VERSION,
                    "clientCapabilities": {
                        "fs": { "readTextFile": false, "writeTextFile": false },
                        "terminal": false
                    },
                    "clientInfo": {
                        "name": "hone-gui",
                        "title": "hone",
                        "version": env!("CARGO_PKG_VERSION")
                    }
                }),
            )
            .await?;
        agent.info = initialized(&answer)?;
        Ok((agent, receiver))
    }

    /// What the agent announced in `initialize`.
    #[must_use]
    pub fn info(&self) -> &AgentInfo {
        &self.info
    }

    /// `authenticate` with one of the agent's login methods.
    ///
    /// # Errors
    ///
    /// Returns the agent's [`AgentError`].
    pub async fn authenticate(&self, method: &str) -> Result<(), AgentError> {
        self.connection
            .request("authenticate", json!({ "methodId": method }))
            .await
            .map(drop)
    }

    /// `session/new` in `cwd` with the MCP server `mcp`; returns the session ID.
    ///
    /// # Errors
    ///
    /// Returns the agent's [`AgentError`]; [`AgentError::needs_login`] tells a missing login.
    pub async fn new_session(&self, cwd: &Path, mcp: &McpServer) -> Result<String, AgentError> {
        let answer = self
            .connection
            .request(
                "session/new",
                json!({ "cwd": cwd, "mcpServers": [mcp_server(mcp)] }),
            )
            .await?;
        answer["sessionId"]
            .as_str()
            .map(str::to_owned)
            .ok_or_else(|| AgentError::local("the agent's session/new names no sessionId"))
    }

    /// `session/load` of `session`. The agent replays the conversation as updates before this
    /// returns. Only for an agent that announced `loadSession`.
    ///
    /// # Errors
    ///
    /// Returns an [`AgentError`] without a call when the agent cannot load sessions, otherwise
    /// the agent's.
    pub async fn load_session(
        &self,
        session: &str,
        cwd: &Path,
        mcp: &McpServer,
    ) -> Result<(), AgentError> {
        if !self.info.load_session {
            return Err(AgentError::local(
                "the agent cannot load an earlier session",
            ));
        }
        self.connection
            .request(
                "session/load",
                json!({ "sessionId": session, "cwd": cwd, "mcpServers": [mcp_server(mcp)] }),
            )
            .await
            .map(drop)
    }

    /// `session/prompt` with `text`; returns the stop reason (`end_turn`, `cancelled`, ...)
    /// once the agent has finished the turn.
    ///
    /// # Errors
    ///
    /// Returns the agent's [`AgentError`].
    pub async fn prompt(&self, session: &str, text: &str) -> Result<String, AgentError> {
        let answer = self
            .connection
            .request(
                "session/prompt",
                json!({ "sessionId": session, "prompt": [{ "type": "text", "text": text }] }),
            )
            .await?;
        Ok(answer["stopReason"]
            .as_str()
            .unwrap_or("end_turn")
            .to_owned())
    }

    /// `session/cancel` of `session`; its open permission requests are answered as cancelled.
    pub fn cancel(&self, session: &str) {
        self.connection.cancel_permissions(Some(session));
        self.connection.send(&json!({
            "jsonrpc": "2.0",
            "method": "session/cancel",
            "params": { "sessionId": session }
        }));
    }

    /// Answers the permission request `key` with the option `option`, or as cancelled with
    /// `None`. A request that is no longer open is left alone.
    pub fn answer(&self, key: u64, option: Option<&str>) {
        let open = self
            .connection
            .permissions
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .remove(&key);
        if let Some((_, id)) = open {
            self.connection.respond_permission(&id, option);
        }
    }
}

impl Drop for Agent {
    fn drop(&mut self) {
        self.connection.cancel_permissions(None);
        let mut child = self.child.lock().unwrap_or_else(PoisonError::into_inner);
        let _ = child.kill();
        let _ = child.wait();
    }
}

impl Connection {
    async fn request(&self, method: &str, params: Value) -> Result<Value, AgentError> {
        let id = self.next.fetch_add(1, Ordering::Relaxed);
        let (sender, receiver) = oneshot::channel();
        self.pending
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(id, sender);
        self.send(&json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params }));
        receiver
            .await
            .unwrap_or_else(|_| Err(AgentError::local("the agent has ended")))
    }

    // A failed write surfaces when the reader sees the process end.
    fn send(&self, message: &Value) {
        let mut line = message.to_string();
        line.push('\n');
        let mut stdin = self.stdin.lock().unwrap_or_else(PoisonError::into_inner);
        let _ = stdin
            .write_all(line.as_bytes())
            .and_then(|()| stdin.flush());
    }

    fn respond_permission(&self, id: &Value, option: Option<&str>) {
        let outcome = match option {
            Some(option) => json!({ "outcome": "selected", "optionId": option }),
            None => json!({ "outcome": "cancelled" }),
        };
        self.send(&json!({ "jsonrpc": "2.0", "id": id, "result": { "outcome": outcome } }));
    }

    fn cancel_permissions(&self, session: Option<&str>) {
        let cancelled: Vec<Value> = {
            let mut open = self
                .permissions
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
            let keys: Vec<u64> = open
                .iter()
                .filter(|(_, (owner, _))| session.is_none_or(|session| owner == session))
                .map(|(key, _)| *key)
                .collect();
            keys.iter()
                .filter_map(|key| open.remove(key).map(|(_, id)| id))
                .collect()
        };
        for id in cancelled {
            self.respond_permission(&id, None);
        }
    }

    fn read(
        &self,
        stdout: impl Read,
        incoming: &mpsc::UnboundedSender<Incoming>,
        errors: &Mutex<Vec<String>>,
    ) {
        let mut keys = 0u64;
        for line in BufReader::new(stdout).lines() {
            let Ok(line) = line else { break };
            let Ok(message) = serde_json::from_str::<Value>(&line) else {
                continue;
            };
            let method = message["method"].as_str();
            let id = message.get("id").filter(|id| !id.is_null());
            match (method, id) {
                (None, Some(id)) => self.settle(id, &message),
                (Some("session/request_permission"), Some(id)) => {
                    keys += 1;
                    let params = &message["params"];
                    let session = params["sessionId"].as_str().unwrap_or_default().to_owned();
                    self.permissions
                        .lock()
                        .unwrap_or_else(PoisonError::into_inner)
                        .insert(keys, (session.clone(), id.clone()));
                    let _ = incoming.send(Incoming::Permission(Permission {
                        key: keys,
                        session,
                        tool: tool_call(&params["toolCall"]),
                        options: permission_options(&params["options"]),
                    }));
                }
                (Some(method), Some(id)) => self.send(&json!({
                    "jsonrpc": "2.0",
                    "id": id,
                    "error": { "code": METHOD_NOT_FOUND, "message": format!("{method} is not offered") }
                })),
                (Some("session/update"), None) => {
                    let params = &message["params"];
                    let _ = incoming.send(Incoming::Update {
                        session: params["sessionId"].as_str().unwrap_or_default().to_owned(),
                        update: update(&params["update"]),
                    });
                }
                (_, None) => {}
            }
        }
        let ended = AgentError::local("the agent has ended");
        for (_, waiting) in self
            .pending
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .drain()
        {
            let _ = waiting.send(Err(ended.clone()));
        }
        self.permissions
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clear();
        let output = errors
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .join("\n");
        let message = if output.is_empty() {
            "The agent has ended.".to_owned()
        } else {
            format!("The agent has ended. Its last output:\n{output}")
        };
        let _ = incoming.send(Incoming::Exited(message));
    }

    fn settle(&self, id: &Value, message: &Value) {
        let Some(waiting) = id.as_u64().and_then(|id| {
            self.pending
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .remove(&id)
        }) else {
            return;
        };
        let result = match message.get("error") {
            Some(error) => Err(AgentError {
                code: error["code"].as_i64(),
                message: error["message"]
                    .as_str()
                    .unwrap_or("the agent reported an error")
                    .to_owned(),
            }),
            None => Ok(message.get("result").cloned().unwrap_or(Value::Null)),
        };
        let _ = waiting.send(result);
    }
}

fn keep_last_lines(stderr: impl Read, lines: &Mutex<Vec<String>>) {
    for line in BufReader::new(stderr).lines() {
        let Ok(line) = line else { break };
        let mut lines = lines.lock().unwrap_or_else(PoisonError::into_inner);
        if lines.len() == STDERR_LINES {
            lines.remove(0);
        }
        lines.push(line);
    }
}

fn initialized(answer: &Value) -> Result<AgentInfo, AgentError> {
    let version = answer["protocolVersion"].as_u64();
    if version != Some(PROTOCOL_VERSION) {
        return Err(AgentError::local(format!(
            "the agent speaks ACP version {}, the GUI speaks version {PROTOCOL_VERSION}",
            version.map_or_else(|| "?".to_owned(), |version| version.to_string())
        )));
    }
    let agent = &answer["agentInfo"];
    let auth_methods = answer["authMethods"]
        .as_array()
        .into_iter()
        .flatten()
        // A terminal method is offered only to a client with terminal login, which this is not.
        .filter(|method| method["type"].as_str().is_none_or(|kind| kind == "agent"))
        .filter_map(|method| {
            Some(AuthMethod {
                id: method["id"].as_str()?.to_owned(),
                name: method["name"].as_str()?.to_owned(),
                description: method["description"].as_str().map(str::to_owned),
            })
        })
        .collect();
    Ok(AgentInfo {
        name: agent["title"]
            .as_str()
            .or_else(|| agent["name"].as_str())
            .map(str::to_owned),
        load_session: answer["agentCapabilities"]["loadSession"]
            .as_bool()
            .unwrap_or(false),
        auth_methods,
    })
}

fn mcp_server(mcp: &McpServer) -> Value {
    json!({
        "name": mcp.name,
        "command": mcp.command,
        "args": mcp.args,
        "env": mcp.env.iter().map(|(name, value)| json!({ "name": name, "value": value })).collect::<Vec<_>>()
    })
}

fn update(update: &Value) -> Update {
    let kind = update["sessionUpdate"].as_str().unwrap_or_default();
    match kind {
        "user_message_chunk" => Update::UserText(content_text(&update["content"])),
        "agent_message_chunk" => Update::AgentText(content_text(&update["content"])),
        "agent_thought_chunk" => Update::Thought(content_text(&update["content"])),
        "tool_call" => Update::ToolCall(tool_call(update)),
        "tool_call_update" => Update::ToolCallUpdate(tool_call(update)),
        other => Update::Other(other.to_owned()),
    }
}

fn tool_call(call: &Value) -> ToolCall {
    let text = |name: &str| call[name].as_str().map(str::to_owned);
    let value = |name: &str| call.get(name).filter(|value| !value.is_null()).cloned();
    ToolCall {
        id: text("toolCallId").unwrap_or_default(),
        title: text("title"),
        kind: text("kind"),
        status: text("status"),
        input: value("rawInput"),
        output: value("rawOutput"),
        content: call["content"].as_array().map(|content| {
            content
                .iter()
                .filter_map(|item| match item["type"].as_str()? {
                    "content" => Some(ToolContent::Text(content_text(&item["content"]))),
                    "diff" => Some(ToolContent::Diff {
                        path: item["path"].as_str()?.to_owned(),
                        old: item["oldText"].as_str().map(str::to_owned),
                        new: item["newText"].as_str()?.to_owned(),
                    }),
                    _ => None,
                })
                .collect()
        }),
    }
}

/// The text of a content block; a block without text is named by its type.
fn content_text(block: &Value) -> String {
    match block["type"].as_str() {
        Some("text") => block["text"].as_str().unwrap_or_default().to_owned(),
        Some("resource_link") => format!("[{}]", block["name"].as_str().unwrap_or("link")),
        Some("resource") => block["resource"]["text"]
            .as_str()
            .map_or_else(|| "[resource]".to_owned(), str::to_owned),
        Some(kind) => format!("[{kind}]"),
        None => String::new(),
    }
}

fn permission_options(options: &Value) -> Vec<PermissionOption> {
    options
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|option| {
            Some(PermissionOption {
                id: option["optionId"].as_str()?.to_owned(),
                name: option["name"].as_str()?.to_owned(),
                kind: option["kind"].as_str().unwrap_or_default().to_owned(),
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_initialize_answer_names_the_agent_its_resumption_and_its_logins() {
        let info = initialized(&json!({
            "protocolVersion": 1,
            "agentCapabilities": { "loadSession": true },
            "authMethods": [
                { "id": "google", "name": "Log in with Google", "description": "Opens a browser" },
                { "id": "key", "name": "Use an API key", "type": "agent" },
                { "id": "tui", "name": "Log in in a terminal", "type": "terminal" }
            ],
            "agentInfo": { "name": "gemini-cli", "title": "Gemini CLI", "version": "0.71.1" }
        }))
        .expect("version 1");
        assert_eq!(
            info,
            AgentInfo {
                name: Some("Gemini CLI".into()),
                load_session: true,
                auth_methods: vec![
                    AuthMethod {
                        id: "google".into(),
                        name: "Log in with Google".into(),
                        description: Some("Opens a browser".into())
                    },
                    AuthMethod {
                        id: "key".into(),
                        name: "Use an API key".into(),
                        description: None
                    },
                ],
            }
        );
        assert_eq!(
            initialized(&json!({ "protocolVersion": 2 })),
            Err(AgentError::local(
                "the agent speaks ACP version 2, the GUI speaks version 1"
            ))
        );
        assert!(
            !initialized(&json!({ "protocolVersion": 1 }))
                .expect("version 1")
                .load_session
        );
    }

    #[test]
    fn updates_keep_text_tool_calls_and_diffs() {
        assert_eq!(
            update(
                &json!({ "sessionUpdate": "agent_message_chunk", "content": { "type": "text", "text": "**Done**" } })
            ),
            Update::AgentText("**Done**".into())
        );
        assert_eq!(
            update(
                &json!({ "sessionUpdate": "user_message_chunk", "content": { "type": "image", "data": "", "mimeType": "image/png" } })
            ),
            Update::UserText("[image]".into())
        );
        assert_eq!(
            update(&json!({
                "sessionUpdate": "tool_call_update",
                "toolCallId": "call-1",
                "status": "completed",
                "rawOutput": { "status": "written" },
                "content": [
                    { "type": "content", "content": { "type": "text", "text": "ok" } },
                    { "type": "diff", "path": "/r/a.md", "oldText": null, "newText": "b\n" },
                    { "type": "terminal", "terminalId": "t-1" }
                ]
            })),
            Update::ToolCallUpdate(ToolCall {
                id: "call-1".into(),
                status: Some("completed".into()),
                output: Some(json!({ "status": "written" })),
                content: Some(vec![
                    ToolContent::Text("ok".into()),
                    ToolContent::Diff {
                        path: "/r/a.md".into(),
                        old: None,
                        new: "b\n".into()
                    },
                ]),
                ..ToolCall::default()
            })
        );
        assert_eq!(
            update(&json!({ "sessionUpdate": "plan", "entries": [] })),
            Update::Other("plan".into())
        );
    }

    #[test]
    fn an_mcp_server_lists_its_environment_by_name_and_value() {
        let server = McpServer {
            name: "hone".into(),
            command: PathBuf::from("/usr/bin/hone"),
            args: vec!["mcp".into()],
            env: vec![("HONE_DATA_DIR".into(), "/d".into())],
        };
        assert_eq!(
            mcp_server(&server),
            json!({
                "name": "hone",
                "command": "/usr/bin/hone",
                "args": ["mcp"],
                "env": [{ "name": "HONE_DATA_DIR", "value": "/d" }]
            })
        );
    }
}
