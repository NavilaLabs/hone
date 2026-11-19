//! MCP server of the hone reference implementation: one tool per public function of
//! `hone-core`, over stdio, for exactly one project (concept Section 12, "MCP server").

use std::path::PathBuf;

use hone_core::{
    Author, Change, CheckReport, CheckRequest, GateClass, NoPackages, PackageDirectory,
    PackageSource, Peer, ProcessError, ProcessStore, Resolved, StepStatus, ThreadAnchor,
};
use rmcp::handler::server::router::tool::ToolRouter;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{CallToolResult, Implementation, ServerCapabilities, ServerConfig};
use rmcp::schemars::JsonSchema;
use rmcp::{ServerHandler, ServiceExt, tool, tool_handler, tool_router};
use serde::Deserialize;
use serde_json::{Value, json};

/// The project an MCP server serves.
#[derive(Debug, Clone)]
pub struct Project {
    /// The project root, the directory that contains `.hone/`.
    pub root: PathBuf,
    /// The directory with one subdirectory per package name, or `None` without packages.
    pub packages: Option<PathBuf>,
    /// The base directory of the process store, or `None` for `HONE_DATA_DIR` or the data
    /// directory of the platform.
    pub data_directory: Option<PathBuf>,
    /// The author of every event the server writes, fixed for the session so that an agent
    /// cannot choose it per call. Without one, the writing tools fail.
    pub author: Option<Author>,
    /// The peer of the tool `sync`, fixed for the session so that an agent cannot sync into an
    /// arbitrary path or server. Without one, `sync` fails.
    pub sync_peer: Option<SyncPeer>,
}

/// The peer the tool `sync` syncs with.
#[derive(Debug, Clone)]
pub enum SyncPeer {
    /// A shared folder: its root.
    Folder(PathBuf),
    /// A replication server.
    Server(hone_core::Server),
}

/// Serves the MCP tools for `project` over stdin and stdout until the client disconnects.
///
/// # Errors
///
/// Returns an error if the async runtime cannot start or the MCP session fails.
pub fn serve_stdio(project: Project) -> Result<(), Box<dyn std::error::Error>> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_io()
        .enable_time()
        .build()?;
    runtime.block_on(async {
        Server::new(project)
            .serve(rmcp::transport::stdio())
            .await?
            .waiting()
            .await?;
        Ok(())
    })
}

#[derive(Clone)]
struct Server {
    project: Project,
    tool_router: ToolRouter<Self>,
}

#[derive(Deserialize, JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
struct ResolveInput {
    #[schemars(
        description = "Only compare `.hone/resolved.json` with its sources, write nothing."
    )]
    #[serde(default)]
    check: bool,
}

#[derive(Deserialize, JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
struct CheckInput {
    #[schemars(
        description = "The ticket whose gates run. Without one, only the directory gates run."
    )]
    ticket: Option<String>,
    #[schemars(
        description = "Run only the gates of this step of the ticket's profile. Requires `ticket`."
    )]
    step: Option<String>,
    #[schemars(
        description = "The paths the change touches, relative to the project root. Leave it out if they are unknown; an empty list means no changes."
    )]
    changed_paths: Option<Vec<String>>,
    #[schemars(description = "Run only gates of this class.")]
    class: Option<Class>,
    #[schemars(
        description = "The base state, a directory that plugins read under `/base`, relative to the project root or absolute."
    )]
    base: Option<PathBuf>,
    #[schemars(
        description = "The lockfile that approves plugins and `exec` commands, relative to the project root or absolute. Defaults to `.hone/lock.yaml`."
    )]
    lock: Option<PathBuf>,
}

#[derive(Clone, Copy, Deserialize, JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
#[serde(rename_all = "kebab-case")]
enum Class {
    Pure,
    Exec,
}

#[derive(Deserialize, JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
struct CheckEditInput {
    #[schemars(description = "The ticket the agent works on.")]
    ticket: String,
    #[schemars(description = "The file the agent wants to write, relative to the project root.")]
    path: String,
}

#[derive(Deserialize, JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
struct ArtifactInput {
    #[schemars(
        description = "The path of the artifact file relative to the project root. Its extension selects the file shape."
    )]
    path: String,
    #[schemars(description = "The full text of the artifact file.")]
    content: String,
}

#[derive(Deserialize, JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
struct ThreadOpenInput {
    #[schemars(description = "The ticket the thread belongs to.")]
    ticket: String,
    #[schemars(description = "The kind of the thread, a key such as `question` or `concern`.")]
    kind: String,
    #[schemars(
        description = "Whether the thread blocks the gate `no-open-blocking-threads` while it is open."
    )]
    #[serde(default)]
    blocking: bool,
    #[schemars(description = "What the thread is attached to.")]
    anchor: AnchorInput,
    #[schemars(description = "The text of the thread.")]
    body: String,
}

#[derive(Deserialize, JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
#[serde(tag = "kind", rename_all = "kebab-case")]
enum AnchorInput {
    /// An artifact by ID. hone takes its content hash from the current file.
    Artifact {
        artifact: String,
        quote: Option<String>,
    },
    /// Code as `path#symbol`.
    Code {
        #[serde(rename = "ref")]
        reference: String,
    },
    /// An external change, for example a pull request and a line range.
    Change {
        #[serde(rename = "ref")]
        reference: String,
    },
    /// A position in an agent transcript.
    Transcript {
        url: String,
        position: Option<String>,
    },
}

#[derive(Deserialize, JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
struct ThreadReplyInput {
    #[schemars(description = "The thread ID, `th-` plus a UUID.")]
    thread: String,
    #[schemars(description = "The text of the message.")]
    body: String,
}

#[derive(Deserialize, JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
struct ThreadInput {
    #[schemars(description = "The thread ID, `th-` plus a UUID.")]
    thread: String,
}

#[derive(Deserialize, JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
struct StepSetInput {
    #[schemars(description = "The ticket.")]
    ticket: String,
    #[schemars(description = "A step of the ticket's profile.")]
    step: String,
    #[schemars(description = "The new state of the step.")]
    status: StepStatusInput,
    #[schemars(description = "Why, for example why the step is skipped.")]
    reason: Option<String>,
}

#[derive(Clone, Copy, Deserialize, JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
#[serde(rename_all = "kebab-case")]
enum StepStatusInput {
    Open,
    Done,
    Skipped,
}

#[derive(Deserialize, JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
struct EventsInput {
    #[schemars(description = "The ticket.")]
    ticket: String,
}

#[tool_router]
impl Server {
    fn new(project: Project) -> Self {
        Self {
            project,
            tool_router: Self::tool_router(),
        }
    }

    #[tool(
        description = "Resolve the configuration of the project and write `.hone/resolved.json`, or with `check` only compare it. Returns `status` (`written`, `up-to-date`, `out-of-date`) and `hash`."
    )]
    fn resolve(&self, Parameters(input): Parameters<ResolveInput>) -> CallToolResult {
        let packages: Box<dyn PackageSource> = match &self.project.packages {
            Some(directory) => Box::new(PackageDirectory::new(directory)),
            None => Box::new(NoPackages),
        };
        let root = &self.project.root;
        let outcome = if input.check {
            hone_core::compare_resolved(root, packages.as_ref())
        } else {
            hone_core::write_resolved(root, packages.as_ref())
        };
        match outcome {
            Ok(file) => CallToolResult::structured(to_value(&file)),
            Err(error) => CallToolResult::structured_error(to_value(&error)),
        }
    }

    #[tool(
        description = "Run the gates over the checked directory. Returns `status` (`passed`, `failed`) and one report per gate."
    )]
    fn check(&self, Parameters(input): Parameters<CheckInput>) -> CallToolResult {
        let request = CheckRequest {
            ticket: input.ticket,
            step: input.step,
            changed_paths: input.changed_paths,
            class: input.class.map(|class| match class {
                Class::Pure => GateClass::Pure,
                Class::Exec => GateClass::Exec,
            }),
            data_directory: self.project.data_directory.clone(),
            base: input.base.map(|base| self.project.root.join(base)),
            lock: input.lock.map(|lock| self.project.root.join(lock)),
        };
        report_result(hone_core::check(&self.project.root, &request))
    }

    #[tool(
        description = "Ask before writing a file whether the ticket allows it: runs the `change-needs-binding` gates of the ticket's required steps with `path` as the only changed path, like the agent hook. Artifact files always pass. Returns `status` (`passed`, `failed`) and one report per gate."
    )]
    fn check_edit(&self, Parameters(input): Parameters<CheckEditInput>) -> CallToolResult {
        report_result(hone_core::check_edit(
            &self.project.root,
            &input.ticket,
            &input.path,
        ))
    }

    #[tool(
        description = "Check the content of one artifact file, for example a draft before it is written: YAML profile, kind, prefix, status, schema, and unknown fields. Returns `status` (`passed`, `failed`) and `findings`."
    )]
    fn check_artifact(&self, Parameters(input): Parameters<ArtifactInput>) -> CallToolResult {
        match Resolved::load(&self.project.root) {
            Ok(config) => {
                let findings =
                    hone_core::check_artifact(&config, &input.path, input.content.as_bytes());
                let failed = findings
                    .iter()
                    .any(|finding| finding.level == hone_core::Level::Error);
                CallToolResult::structured(json!({
                    "status": if failed { "failed" } else { "passed" },
                    "findings": findings,
                }))
            }
            Err(error) => could_not_run(&error.message),
        }
    }

    #[tool(
        description = "Compute the content hash of one artifact file (concept Section 5). Returns `hash`."
    )]
    fn content_hash(&self, Parameters(input): Parameters<ArtifactInput>) -> CallToolResult {
        match hone_core::content_hash(&input.path, input.content.as_bytes()) {
            Ok(hash) => CallToolResult::structured(json!({ "hash": hash })),
            Err(error) => CallToolResult::structured_error(json!({
                "status": "error",
                "message": error.to_string(),
                "violations": error.violations,
            })),
        }
    }

    #[tool(
        description = "Open a comment thread on a ticket, written as the server's author. Returns the written `event` and the `thread` ID."
    )]
    fn thread_open(&self, Parameters(input): Parameters<ThreadOpenInput>) -> CallToolResult {
        let anchor = match input.anchor {
            AnchorInput::Artifact { artifact, quote } => ThreadAnchor::Artifact { artifact, quote },
            AnchorInput::Code { reference } => ThreadAnchor::Code { reference },
            AnchorInput::Change { reference } => ThreadAnchor::Change { reference },
            AnchorInput::Transcript { url, position } => ThreadAnchor::Transcript { url, position },
        };
        self.append(Change::OpenThread {
            ticket: input.ticket,
            kind: input.kind,
            blocking: input.blocking,
            anchor,
            body: input.body,
        })
    }

    #[tool(description = "Answer in a thread. Returns the written `event`.")]
    fn thread_reply(&self, Parameters(input): Parameters<ThreadReplyInput>) -> CallToolResult {
        self.append(Change::Reply {
            thread: input.thread,
            body: input.body,
        })
    }

    #[tool(description = "Resolve a thread. Returns the written `event`.")]
    fn thread_resolve(&self, Parameters(input): Parameters<ThreadInput>) -> CallToolResult {
        self.append(Change::Resolve {
            thread: input.thread,
        })
    }

    #[tool(description = "Reopen a resolved thread. Returns the written `event`.")]
    fn thread_reopen(&self, Parameters(input): Parameters<ThreadInput>) -> CallToolResult {
        self.append(Change::Reopen {
            thread: input.thread,
        })
    }

    #[tool(
        description = "Set the state of a step of a ticket: `open`, `done`, or `skipped` with an optional reason. Returns the written `event`."
    )]
    fn step_set(&self, Parameters(input): Parameters<StepSetInput>) -> CallToolResult {
        self.append(Change::SetStep {
            ticket: input.ticket,
            step: input.step,
            status: match input.status {
                StepStatusInput::Open => StepStatus::Open,
                StepStatusInput::Done => StepStatus::Done,
                StepStatusInput::Skipped => StepStatus::Skipped,
            },
            reason: input.reason,
        })
    }

    #[tool(
        description = "Read the process state of a ticket: its `events` in order, the computed `threads` and `steps`, and the `invalid` events."
    )]
    fn events(&self, Parameters(input): Parameters<EventsInput>) -> CallToolResult {
        match self.store().and_then(|store| store.events(&input.ticket)) {
            Ok(events) => CallToolResult::structured(to_value(&events)),
            Err(error) => CallToolResult::structured_error(to_value(&error)),
        }
    }

    #[tool(
        description = "Sync the process store with the shared folder or the replication server the MCP server was started with: the events `received` and `sent` per device, and the `problems` (fork, id-conflict, invalid-event, unsafe-path, permission-denied, scope-mismatch, identity-mismatch, role-mismatch, seq-gap)."
    )]
    fn sync(&self) -> CallToolResult {
        let peer = match &self.project.sync_peer {
            Some(SyncPeer::Folder(folder)) => Peer::Folder(folder),
            Some(SyncPeer::Server(server)) => Peer::Server(server),
            None => {
                return CallToolResult::structured_error(to_value(&ProcessError::Invalid {
                    message: "the server has no peer: start it with --sync-folder or --sync-server"
                        .into(),
                }));
            }
        };
        match self.store().and_then(|store| store.sync(peer)) {
            Ok(report) => CallToolResult::structured(to_value(&report)),
            Err(error) => CallToolResult::structured_error(to_value(&error)),
        }
    }
}

impl Server {
    fn store(&self) -> Result<ProcessStore, ProcessError> {
        ProcessStore::locate(&self.project.root, self.project.data_directory.as_deref())
    }

    fn append(&self, change: Change) -> CallToolResult {
        let Some(author) = &self.project.author else {
            return CallToolResult::structured_error(to_value(&ProcessError::Invalid {
                message: "the server has no author: start it with --author and --role".into(),
            }));
        };
        match self.store().and_then(|store| store.append(author, change)) {
            Ok(appended) => CallToolResult::structured(to_value(&appended)),
            Err(error) => CallToolResult::structured_error(to_value(&error)),
        }
    }
}

#[tool_handler(router = self.tool_router)]
impl ServerHandler for Server {
    fn get_info(&self) -> ServerConfig {
        ServerConfig::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(Implementation::new("hone", env!("CARGO_PKG_VERSION")))
            .with_instructions(
                "hone checks design artifacts and the gates from ticket to code for one project.",
            )
    }
}

fn report_result(result: Result<CheckReport, hone_core::CheckError>) -> CallToolResult {
    match result {
        Ok(report) => CallToolResult::structured(json!({
            "status": if report.passed() { "passed" } else { "failed" },
            "gates": report.gates,
        })),
        Err(error) => could_not_run(&error.message),
    }
}

fn could_not_run(message: &str) -> CallToolResult {
    CallToolResult::structured_error(json!({ "status": "error", "message": message }))
}

fn to_value(value: &impl serde::Serialize) -> Value {
    serde_json::to_value(value).expect("the results of hone-core are serializable")
}

#[cfg(test)]
mod tests;
