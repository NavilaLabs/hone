use std::io::Read;
use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Args, Subcommand};
use hone_core::{
    ArtifactNew, ArtifactSource, ArtifactWritten, CheckError, Contexts, KindInfo, ProcessStore,
    SourceFile,
};
use serde::Serialize;
use serde_json::json;

const NOT_SAVED: u8 = 1;
const COULD_NOT_RUN: u8 = 2;

#[derive(Args)]
pub(crate) struct Root {
    /// Project root, the directory that contains `.hone/`.
    #[arg(long, default_value = ".")]
    root: PathBuf,
    /// Print the result as JSON.
    #[arg(long)]
    json: bool,
}

#[derive(Subcommand)]
pub(crate) enum Artifact {
    /// List the kinds of the configuration with what a new artifact needs.
    Kinds {
        #[command(flatten)]
        target: Root,
    },
    /// Show an artifact file with what a form needs: its text, data, body, and the kind's
    /// statuses and schema.
    Show {
        #[command(flatten)]
        target: Root,
        /// The artifact ID.
        #[arg(long)]
        id: String,
    },
    /// Check the text on standard input as the artifact file at a path and write it only if no
    /// finding is an error. Exits with 0 when written, 1 when an error stopped it, and 2 when
    /// it could not run.
    Save {
        #[command(flatten)]
        target: Root,
        /// The path relative to the project root.
        #[arg(long)]
        path: String,
    },
    /// Create an artifact of a kind with an unused ID at the kind's path, with the required
    /// fields of its lifecycle. Exits like `save`.
    New {
        #[command(flatten)]
        target: Root,
        /// The kind.
        #[arg(long)]
        kind: String,
        /// The title: the slug of the path and the first heading of a Markdown body.
        #[arg(long)]
        title: String,
        /// The ticket, needed for a kind with lifecycle `decided`.
        #[arg(long)]
        ticket: Option<String>,
        /// An anchor as `<kind>:<ref>`, for example `path:src/payments`.
        #[arg(long = "anchor", value_name = "KIND:REF")]
        anchors: Vec<String>,
        /// The author's handle for `authors`. Without it and `HONE_AUTHOR`, the author of the
        /// project's context.
        #[arg(long, env = "HONE_AUTHOR")]
        author: Option<String>,
    },
}

#[derive(Subcommand)]
pub(crate) enum File {
    /// Show a source file below the project root, and with a symbol its line from the language
    /// plugin that handles the file.
    Show {
        #[command(flatten)]
        target: Root,
        /// The path relative to the project root.
        #[arg(long)]
        path: String,
        /// A symbol in the file, in the syntax of its language plugin, for example `Fee::add`.
        #[arg(long)]
        symbol: Option<String>,
    },
}

pub(crate) fn run_artifact(command: Artifact) -> ExitCode {
    match command {
        Artifact::Kinds { target } => {
            shown(target.json, hone_core::kinds(&target.root), print_kinds)
        }
        Artifact::Show { target, id } => shown(
            target.json,
            hone_core::show_artifact(&target.root, &id),
            print_source,
        ),
        Artifact::Save { target, path } => {
            let mut bytes = Vec::new();
            let outcome = std::io::stdin()
                .read_to_end(&mut bytes)
                .map_err(|io| CheckError {
                    message: format!("standard input: {io}"),
                })
                .and_then(|_| hone_core::save_artifact(&target.root, &path, &bytes));
            written(target.json, outcome)
        }
        Artifact::New {
            target,
            kind,
            title,
            ticket,
            anchors,
            author,
        } => {
            let outcome = anchors
                .iter()
                .map(|text| {
                    text.split_once(':')
                        .filter(|(kind, reference)| !kind.is_empty() && !reference.is_empty())
                        .map(|(kind, reference)| (kind.to_owned(), reference.to_owned()))
                        .ok_or_else(|| CheckError {
                            message: format!("`{text}` is not `<kind>:<ref>`"),
                        })
                })
                .collect::<Result<Vec<_>, _>>()
                .and_then(|anchors| {
                    let new = ArtifactNew {
                        kind,
                        ticket,
                        title,
                        author: author.or_else(|| context_author(&target.root)),
                        anchors,
                    };
                    hone_core::new_artifact(&target.root, &new)
                });
            written(target.json, outcome)
        }
    }
}

pub(crate) fn run_file(command: File) -> ExitCode {
    match command {
        File::Show {
            target,
            path,
            symbol,
        } => shown(
            target.json,
            hone_core::show_file(&target.root, &path, symbol.as_deref()),
            print_file,
        ),
    }
}

fn context_author(root: &std::path::Path) -> Option<String> {
    let store = ProcessStore::locate(root, None).ok()?;
    Contexts::load()
        .ok()?
        .of_project(store.project(), store.root())
        .ok()
        .flatten()?
        .context
        .author
        .map(|author| author.name)
}

fn shown<T: Serialize>(as_json: bool, outcome: Result<T, CheckError>, print: fn(&T)) -> ExitCode {
    match outcome {
        Ok(value) => {
            if as_json {
                println!("{}", to_json(&value));
            } else {
                print(&value);
            }
            ExitCode::SUCCESS
        }
        Err(error) => could_not_run(as_json, &error, ExitCode::FAILURE),
    }
}

fn written(as_json: bool, outcome: Result<ArtifactWritten, CheckError>) -> ExitCode {
    match outcome {
        Ok(written) => {
            if as_json {
                println!("{}", to_json(&written));
            } else {
                let verb = if written.saved {
                    "wrote"
                } else {
                    "did not write"
                };
                println!("{verb} {}", written.path);
                for finding in &written.findings {
                    println!("  {finding}");
                }
            }
            if written.saved {
                ExitCode::SUCCESS
            } else {
                ExitCode::from(NOT_SAVED)
            }
        }
        Err(error) => could_not_run(as_json, &error, ExitCode::from(COULD_NOT_RUN)),
    }
}

fn could_not_run(as_json: bool, error: &CheckError, code: ExitCode) -> ExitCode {
    if as_json {
        println!("{}", json!({ "status": "error", "message": error.message }));
    } else {
        eprintln!("{}", error.message);
    }
    code
}

fn to_json(value: &impl Serialize) -> String {
    serde_json::to_string(value).expect("the results of hone-core are serializable")
}

fn print_kinds(kinds: &Vec<KindInfo>) {
    for kind in kinds {
        println!("{} ({}, {})", kind.kind, kind.lifecycle, kind.path);
    }
}

fn print_source(source: &ArtifactSource) {
    print!("{}", source.text);
}

fn print_file(file: &SourceFile) {
    match (file.line, &file.message) {
        (Some(line), _) => println!("{}:{line}", file.path),
        (None, Some(message)) => {
            eprintln!("{message}");
            print!("{}", file.text);
        }
        (None, None) => print!("{}", file.text),
    }
}
