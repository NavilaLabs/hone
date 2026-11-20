mod catalog;
mod check;
mod credentials;
mod editing;
mod process;

use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand};
use hone_core::{NoPackages, PackageDirectory, PackageSource, ResolvedFile, ResolvedFileError};

/// hone: design artifacts, their links and lifecycle, and the steps from ticket to code.
#[derive(Parser)]
#[command(version)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Resolve the configuration of the project and write `.hone/resolved.json`.
    Resolve {
        /// Project root, the directory that contains `.hone/`.
        #[arg(long, default_value = ".")]
        root: PathBuf,
        /// Directory with one subdirectory per package name, for example `acme/base/`.
        #[arg(long)]
        packages: Option<PathBuf>,
        /// Only check that `.hone/resolved.json` matches its sources, write nothing.
        #[arg(long)]
        check: bool,
        /// Print the result as JSON.
        #[arg(long)]
        json: bool,
    },
    /// Run the gates over the checked directory. Exits with 0 when every gate passed, 1 when a
    /// gate failed, and 2 when the check could not run.
    Check(check::CheckArguments),
    /// Write comment threads of a ticket to the process store.
    #[command(subcommand)]
    Thread(process::Thread),
    /// Write the state of a step of a ticket to the process store.
    #[command(subcommand)]
    Step(process::Step),
    /// Approve an artifact of a ticket, or request changes, for its current content hash.
    Approve(process::ApproveArguments),
    /// Condense the approvals into an artifact's `review` block, or confirm the block.
    #[command(subcommand)]
    Review(process::Review),
    /// Show the process state of a ticket: events, threads, steps, and invalid events.
    Events(process::EventsArguments),
    /// Show every valid event of the project's process store and its cursor.
    Log(process::Target),
    /// List the artifacts of the checked directory, ticket records included, with their
    /// declared and effective status.
    Artifacts(catalog::CatalogArguments),
    /// Show, save, and create artifact files.
    #[command(subcommand)]
    Artifact(editing::Artifact),
    /// Show source files of the project.
    #[command(subcommand)]
    File(editing::File),
    /// Show the tickets of the project.
    #[command(subcommand)]
    Ticket(catalog::Ticket),
    /// Write the project's process store, the logs of every device, into a new or empty
    /// directory in the layout of a shared folder, from which `hone sync --folder` imports it.
    Export(process::ExportArguments),
    /// Remove the project's local process store and its context binding. Copies in a shared
    /// folder or on a server stay.
    Forget(process::ForgetArguments),
    /// Sync the process store with a shared folder or a server. Exits with 0 when the sync found
    /// no problem, 1 when it reported problems, and 2 when it could not run.
    Sync(process::SyncArguments),
    /// Manage the contexts of this device: peers, authors, and which project uses which.
    #[command(subcommand)]
    Context(process::ContextCommand),
    /// Run the reference replication server until Ctrl-C or SIGTERM. Exits with 2 when it
    /// cannot start.
    Serve {
        /// The server configuration, a YAML file.
        #[arg(long)]
        config: PathBuf,
    },
    /// Serve the MCP tools of the project over stdio. Without the flags of the author or the
    /// peer, the server takes them from the project's context when it starts.
    Mcp(McpArguments),
    /// Run hone as a hook of an agent.
    #[command(subcommand)]
    Hook(check::Hook),
}

fn main() -> ExitCode {
    match Cli::parse().command {
        Command::Resolve {
            root,
            packages,
            check,
            json,
        } => {
            let source: Box<dyn PackageSource> = match packages {
                Some(directory) => Box::new(PackageDirectory::new(directory)),
                None => Box::new(NoPackages),
            };
            let outcome = if check {
                hone_core::compare_resolved(&root, source.as_ref())
            } else {
                hone_core::write_resolved(&root, source.as_ref())
            };
            print_resolve(&outcome, json);
            match outcome {
                Ok(ResolvedFile::Written { .. } | ResolvedFile::UpToDate { .. }) => {
                    ExitCode::SUCCESS
                }
                Ok(ResolvedFile::OutOfDate { .. }) | Err(_) => ExitCode::FAILURE,
            }
        }
        Command::Check(arguments) => check::run_check(&arguments),
        Command::Thread(thread) => process::run_thread(thread),
        Command::Step(step) => process::run_step(step),
        Command::Approve(arguments) => process::run_approve(arguments),
        Command::Review(review) => process::run_review(review),
        Command::Events(arguments) => process::run_events(&arguments),
        Command::Log(target) => process::run_log(&target),
        Command::Artifacts(arguments) => catalog::run_artifacts(&arguments),
        Command::Ticket(command) => catalog::run_ticket(command),
        Command::Artifact(command) => editing::run_artifact(command),
        Command::File(command) => editing::run_file(command),
        Command::Export(arguments) => process::run_export(&arguments),
        Command::Forget(arguments) => process::run_forget(&arguments),
        Command::Sync(arguments) => process::run_sync(&arguments),
        Command::Context(command) => process::run_context(command),
        Command::Serve { config } => match hone_server::Config::load(&config) {
            Ok(config) => match hone_server::serve(config) {
                Ok(()) => ExitCode::SUCCESS,
                Err(error) => {
                    eprintln!("hone serve: {error}");
                    ExitCode::from(2)
                }
            },
            Err(message) => {
                eprintln!("hone serve: {message}");
                ExitCode::from(2)
            }
        },
        Command::Mcp(arguments) => match mcp_project(arguments) {
            Ok(project) => match hone_mcp::serve_stdio(project) {
                Ok(()) => ExitCode::SUCCESS,
                Err(error) => {
                    eprintln!("hone mcp: {error}");
                    ExitCode::FAILURE
                }
            },
            Err(message) => {
                eprintln!("hone mcp: {message}");
                ExitCode::FAILURE
            }
        },
        Command::Hook(hook) => check::run_hook(&hook),
    }
}

#[derive(clap::Args)]
struct McpArguments {
    /// Project root, the directory that contains `.hone/`.
    #[arg(long, default_value = ".")]
    root: PathBuf,
    /// Directory with one subdirectory per package name, for example `acme/base/`.
    #[arg(long)]
    packages: Option<PathBuf>,
    /// The handle of the author of every event the server writes.
    #[arg(long, env = "HONE_AUTHOR", requires = "role")]
    author: Option<String>,
    /// The role of that author.
    #[arg(long, env = "HONE_ROLE", requires = "author")]
    role: Option<String>,
    /// The shared folder of the tool `sync`.
    #[arg(long, env = "HONE_SYNC_FOLDER", conflicts_with = "sync_server")]
    sync_folder: Option<PathBuf>,
    /// The replication server of the tool `sync`, reached with the token in `HONE_TOKEN`.
    #[arg(long, env = "HONE_SYNC_SERVER", conflicts_with = "sync_git")]
    sync_server: Option<String>,
    /// The branch of the project's Git repository that the tool `sync` uses as a shared
    /// folder.
    #[arg(long, env = "HONE_SYNC_GIT", conflicts_with = "sync_folder")]
    sync_git: Option<String>,
    /// The remote of `--sync-git`.
    #[arg(long, default_value = "origin", requires = "sync_git")]
    sync_remote: String,
}

// A flag or its environment variable wins; the context fills only what they leave out, once at
// the start, so that the author and the peer stay fixed for the session.
fn mcp_project(arguments: McpArguments) -> Result<hone_mcp::Project, String> {
    let McpArguments {
        root,
        packages,
        author,
        role,
        sync_folder,
        sync_server,
        sync_git,
        sync_remote,
    } = arguments;
    let mut author = author
        .zip(role)
        .map(|(name, role)| hone_core::Author { name, role });
    let mut sync_peer = match (sync_folder, sync_server, sync_git) {
        (Some(folder), _, _) => Some(hone_mcp::SyncPeer::Folder(folder)),
        (None, None, Some(branch)) => Some(hone_mcp::SyncPeer::Git {
            branch,
            remote: sync_remote,
        }),
        (None, Some(url), _) => Some(hone_mcp::SyncPeer::Server(hone_core::Server::new(
            &url,
            &process::token()?,
        ))),
        (None, None, None) => None,
    };
    if author.is_none() || sync_peer.is_none() {
        let context = process::context_of(&root).map_err(|error| error.to_string())?;
        if let Some(bound) = context {
            if sync_peer.is_none() {
                sync_peer = Some(match &bound.context.peer {
                    hone_core::ContextPeer::Folder { folder } => {
                        hone_mcp::SyncPeer::Folder(folder.clone())
                    }
                    hone_core::ContextPeer::Git { git, remote } => hone_mcp::SyncPeer::Git {
                        branch: git.clone(),
                        remote: remote.clone(),
                    },
                    hone_core::ContextPeer::Server { server } => hone_mcp::SyncPeer::Server(
                        hone_core::Server::new(server, &process::context_token(&bound, server)?),
                    ),
                });
            }
            author = author.or(bound.context.author);
        }
    }
    Ok(hone_mcp::Project {
        root,
        packages,
        data_directory: None,
        author,
        sync_peer,
    })
}

fn print_resolve(outcome: &Result<ResolvedFile, ResolvedFileError>, as_json: bool) {
    if as_json {
        let value = match outcome {
            Ok(file) => serde_json::to_value(file),
            Err(error) => serde_json::to_value(error),
        };
        println!("{}", value.expect("serializable"));
        return;
    }
    match outcome {
        Ok(ResolvedFile::Written { hash }) => println!("wrote .hone/resolved.json ({hash})"),
        Ok(ResolvedFile::UpToDate { hash }) => {
            println!(".hone/resolved.json is up to date ({hash})");
        }
        Ok(ResolvedFile::OutOfDate { hash }) => {
            eprintln!(".hone/resolved.json does not match its sources (expected {hash})");
        }
        Err(error) => eprintln!("{error}"),
    }
}
