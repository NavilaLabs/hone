mod check;

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::{Parser, Subcommand};
use hone_core::{NoPackages, PackageDirectory, PackageSource, ResolveError, Resolved};
use serde_json::json;

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
            let output = run_resolve(&root, source.as_ref(), check);
            output.print(json);
            output.exit_code()
        }
        Command::Check(arguments) => check::run_check(&arguments),
        Command::Hook(hook) => check::run_hook(&hook),
    }
}

enum Output {
    Written { hash: String },
    UpToDate { hash: String },
    OutOfDate { hash: String },
    Failed(ResolveError),
    Io(String),
}

fn run_resolve(root: &Path, packages: &dyn PackageSource, check: bool) -> Output {
    let resolved: Resolved = match hone_core::resolve(root, packages, None) {
        Ok(resolved) => resolved,
        Err(error) => return Output::Failed(error),
    };
    let hash = resolved.hash();
    let target = root.join(".hone").join("resolved.json");
    let contents = resolved.file_contents();
    if check {
        return match std::fs::read(&target) {
            Ok(existing) if existing == contents => Output::UpToDate { hash },
            _ => Output::OutOfDate { hash },
        };
    }
    match std::fs::write(&target, contents) {
        Ok(()) => Output::Written { hash },
        Err(error) => Output::Io(format!("{}: {error}", target.display())),
    }
}

impl Output {
    fn print(&self, as_json: bool) {
        if as_json {
            let value = match self {
                Output::Written { hash } => json!({ "status": "written", "hash": hash }),
                Output::UpToDate { hash } => json!({ "status": "up-to-date", "hash": hash }),
                Output::OutOfDate { hash } => json!({ "status": "out-of-date", "hash": hash }),
                Output::Failed(error) => json!({ "status": "failed", "problems": error.problems }),
                Output::Io(message) => json!({ "status": "io-error", "message": message }),
            };
            println!("{value}");
            return;
        }
        match self {
            Output::Written { hash } => println!("wrote .hone/resolved.json ({hash})"),
            Output::UpToDate { hash } => println!(".hone/resolved.json is up to date ({hash})"),
            Output::OutOfDate { hash } => {
                eprintln!(".hone/resolved.json does not match its sources (expected {hash})");
            }
            Output::Failed(error) => eprintln!("{error}"),
            Output::Io(message) => eprintln!("{message}"),
        }
    }

    fn exit_code(&self) -> ExitCode {
        match self {
            Output::Written { .. } | Output::UpToDate { .. } => ExitCode::SUCCESS,
            Output::OutOfDate { .. } | Output::Failed(_) | Output::Io(_) => ExitCode::FAILURE,
        }
    }
}
