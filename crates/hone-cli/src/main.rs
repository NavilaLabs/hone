mod check;
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
    /// Show the process state of a ticket: events, threads, steps, and invalid events.
    Events(process::EventsArguments),
    /// Sync the process store with a shared folder. Exits with 0 when the sync found no problem,
    /// 1 when it reported problems, and 2 when it could not run.
    Sync(process::SyncArguments),
    /// Serve the MCP tools of the project over stdio.
    Mcp {
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
        #[arg(long, env = "HONE_SYNC_FOLDER")]
        sync_folder: Option<PathBuf>,
    },
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
        Command::Events(arguments) => process::run_events(&arguments),
        Command::Sync(arguments) => process::run_sync(&arguments),
        Command::Mcp {
            root,
            packages,
            author,
            role,
            sync_folder,
        } => {
            let author = author
                .zip(role)
                .map(|(name, role)| hone_core::Author { name, role });
            let project = hone_mcp::Project {
                root,
                packages,
                data_directory: None,
                author,
                sync_folder,
            };
            match hone_mcp::serve_stdio(project) {
                Ok(()) => ExitCode::SUCCESS,
                Err(error) => {
                    eprintln!("hone mcp: {error}");
                    ExitCode::FAILURE
                }
            }
        }
        Command::Hook(hook) => check::run_hook(&hook),
    }
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
