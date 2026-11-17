//! MCP server of the hone reference implementation: one tool per public function of
//! `hone-core`, over stdio, for exactly one project (concept Section 12, "MCP server").

use std::path::PathBuf;

use hone_core::{
    CheckReport, CheckRequest, GateClass, NoPackages, PackageDirectory, PackageSource, Resolved,
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

