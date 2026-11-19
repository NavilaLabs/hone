use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Args, Subcommand, ValueEnum};
use hone_core::{
    Appended, Author, BoundContext, Change, Condensed, Context, ContextPeer, Contexts, Cursor,
    Peer, ProcessError, ProcessStore, Server, StepStatus, SyncReport, ThreadAnchor, ThreadStatus,
    TicketEvents,
};
use serde::Serialize;
use serde_json::Value;

#[derive(Args)]
pub(crate) struct Target {
    /// Project root, the directory that contains `.hone/`.
    #[arg(long, default_value = ".")]
    root: PathBuf,
    /// Print the result as JSON.
    #[arg(long)]
    json: bool,
}

#[derive(Args)]
pub(crate) struct Writer {
    /// The handle of the author. Without it and `HONE_AUTHOR`, the author of the project's
    /// context.
    #[arg(long, env = "HONE_AUTHOR", requires = "role")]
    author: Option<String>,
    /// The role of the author. It must exist in the ticket's configuration.
    #[arg(long, env = "HONE_ROLE", requires = "author")]
    role: Option<String>,
}

impl Writer {
    fn author(&self, store: &ProcessStore) -> Result<Author, ProcessError> {
        if let (Some(name), Some(role)) = (&self.author, &self.role) {
            return Ok(Author {
                name: name.clone(),
                role: role.clone(),
            });
        }
        Contexts::load()?
            .of_project(store.project())
            .and_then(|bound| bound.context.author)
            .ok_or_else(|| ProcessError::Invalid {
                message:
                    "no author: pass --author and --role, or set an author in the project's context"
                        .into(),
            })
    }
}

#[derive(Subcommand)]
pub(crate) enum Thread {
    /// Open a comment thread on a ticket.
    Open {
        #[command(flatten)]
        target: Target,
        #[command(flatten)]
        writer: Writer,
        /// The ticket the thread belongs to.
        #[arg(long)]
        ticket: String,
        /// The kind of the thread, for example `question` or `concern`.
        #[arg(long)]
        kind: String,
        /// The thread blocks `no-open-blocking-threads` while it is open.
        #[arg(long)]
        blocking: bool,
        #[command(flatten)]
        anchor: AnchorArguments,
        /// With `--anchor-artifact`: text to find the spot again.
        #[arg(long, requires = "anchor_artifact")]
        quote: Option<String>,
        /// With `--anchor-transcript`: a position in the transcript.
        #[arg(long, requires = "anchor_transcript")]
        position: Option<String>,
        /// The text of the thread.
        #[arg(long)]
        body: String,
    },
    /// Answer in a thread.
    Reply {
        #[command(flatten)]
        target: Target,
        #[command(flatten)]
        writer: Writer,
        /// The thread ID.
        thread: String,
        /// The text of the message.
        #[arg(long)]
        body: String,
    },
    /// Resolve a thread.
    Resolve {
        #[command(flatten)]
        target: Target,
        #[command(flatten)]
        writer: Writer,
        /// The thread ID.
        thread: String,
    },
    /// Reopen a resolved thread.
    Reopen {
        #[command(flatten)]
        target: Target,
        #[command(flatten)]
        writer: Writer,
        /// The thread ID.
        thread: String,
    },
}

#[derive(Args)]
#[group(required = true, multiple = false)]
pub(crate) struct AnchorArguments {
    /// Attach the thread to an artifact by ID. hone takes the content hash from its file.
    #[arg(long, value_name = "ID")]
    anchor_artifact: Option<String>,
    /// Attach the thread to code, `path#symbol`.
    #[arg(long, value_name = "REFERENCE")]
    anchor_code: Option<String>,
    /// Attach the thread to an external change, for example a pull request and a line range.
    #[arg(long, value_name = "REFERENCE")]
    anchor_change: Option<String>,
    /// Attach the thread to an agent transcript by URL.
    #[arg(long, value_name = "URL")]
    anchor_transcript: Option<String>,
}

#[derive(Subcommand)]
pub(crate) enum Step {
    /// Set the state of a step of a ticket.
    Set {
        #[command(flatten)]
        target: Target,
        #[command(flatten)]
        writer: Writer,
        /// The ticket.
        #[arg(long)]
        ticket: String,
        /// A step of the ticket's profile.
        step: String,
        /// The new state.
        #[arg(value_enum)]
        status: Status,
        /// Why, for example why the step is skipped.
        #[arg(long)]
        reason: Option<String>,
    },
}

#[derive(Clone, Copy, ValueEnum)]
pub(crate) enum Status {
    Open,
    Done,
    Skipped,
}

#[derive(Args)]
pub(crate) struct ApproveArguments {
    #[command(flatten)]
    target: Target,
    #[command(flatten)]
    writer: Writer,
    /// The ticket.
    #[arg(long)]
    ticket: String,
    /// The artifact ID.
    artifact: String,
    /// Request changes instead of approving.
    #[arg(long)]
    changes_requested: bool,
    /// A free proof that you approved, stored as `proof`.
    #[arg(long)]
    proof: Option<String>,
}

#[derive(Subcommand)]
pub(crate) enum Review {
    /// Write the `review` block into an artifact: the approvals of its current content hash,
    /// and a summary.
    Condense {
        #[command(flatten)]
        target: Target,
        /// The ticket.
        #[arg(long)]
        ticket: String,
        /// The artifact ID.
        artifact: String,
        /// A YAML file with a list of `{ q, a }` or `{ rejected }` entries.
        #[arg(long, value_name = "FILE")]
        summary: Option<PathBuf>,
    },
    /// Confirm the condensed `review` block of an artifact.
    Confirm {
        #[command(flatten)]
        target: Target,
        #[command(flatten)]
        writer: Writer,
        /// The ticket.
        #[arg(long)]
        ticket: String,
        /// The artifact ID.
        artifact: String,
    },
}

pub(crate) fn run_approve(arguments: ApproveArguments) -> ExitCode {
    let change = Change::Approve {
        ticket: arguments.ticket,
        artifact: arguments.artifact,
        changes_requested: arguments.changes_requested,
        proof: arguments.proof,
    };
    append(&arguments.target, &arguments.writer, change)
}

pub(crate) fn run_review(review: Review) -> ExitCode {
    match review {
        Review::Condense {
            target,
            ticket,
            artifact,
            summary,
        } => {
            let outcome = ProcessStore::locate(&target.root, None)
                .and_then(|store| store.condense(&ticket, &artifact, summary.as_deref()));
            finish(target.json, outcome, print_condensed)
        }
        Review::Confirm {
            target,
            writer,
            ticket,
            artifact,
        } => append(&target, &writer, Change::ConfirmReview { ticket, artifact }),
    }
}

fn print_condensed(condensed: &Condensed) {
    let approvals = condensed.review["approved_by"]
        .as_array()
        .map_or(0, Vec::len);
    let summary = if condensed.review.get("summary").is_some() {
        " and a summary"
    } else {
        ""
    };
    println!(
        "condensed {approvals} approval(s){summary} into {}",
        condensed.path
    );
}

#[derive(Args)]
pub(crate) struct EventsArguments {
    #[command(flatten)]
    target: Target,
    /// The ticket.
    #[arg(long)]
    ticket: String,
}

pub(crate) fn run_thread(thread: Thread) -> ExitCode {
    let (target, writer, change) = match thread {
        Thread::Open {
            target,
            writer,
            ticket,
            kind,
            blocking,
            anchor,
            quote,
            position,
            body,
        } => {
            let anchor = if let Some(artifact) = anchor.anchor_artifact {
                ThreadAnchor::Artifact { artifact, quote }
            } else if let Some(reference) = anchor.anchor_code {
                ThreadAnchor::Code { reference }
            } else if let Some(reference) = anchor.anchor_change {
                ThreadAnchor::Change { reference }
            } else {
                ThreadAnchor::Transcript {
                    url: anchor.anchor_transcript.unwrap_or_default(),
                    position,
                }
            };
            let change = Change::OpenThread {
                ticket,
                kind,
                blocking,
                anchor,
                body,
            };
            (target, writer, change)
        }
        Thread::Reply {
            target,
            writer,
            thread,
            body,
        } => (target, writer, Change::Reply { thread, body }),
        Thread::Resolve {
            target,
            writer,
            thread,
        } => (target, writer, Change::Resolve { thread }),
        Thread::Reopen {
            target,
            writer,
            thread,
        } => (target, writer, Change::Reopen { thread }),
    };
    append(&target, &writer, change)
}

pub(crate) fn run_step(step: Step) -> ExitCode {
    let Step::Set {
        target,
        writer,
        ticket,
        step,
        status,
        reason,
    } = step;
    let status = match status {
        Status::Open => StepStatus::Open,
        Status::Done => StepStatus::Done,
        Status::Skipped => StepStatus::Skipped,
    };
    let change = Change::SetStep {
        ticket,
        step,
        status,
        reason,
    };
    append(&target, &writer, change)
}

pub(crate) fn run_events(arguments: &EventsArguments) -> ExitCode {
    let target = &arguments.target;
    let outcome =
        ProcessStore::locate(&target.root, None).and_then(|store| store.events(&arguments.ticket));
    finish(target.json, outcome, print_events)
}

/// The valid events of a process store and its cursor.
#[derive(Serialize)]
struct StoreLog {
    cursor: Cursor,
    events: Vec<Value>,
}

pub(crate) fn run_log(target: &Target) -> ExitCode {
    let outcome = ProcessStore::locate(&target.root, None).and_then(|store| {
        Ok(StoreLog {
            cursor: store.cursor()?,
            events: store.read(&Cursor::new())?,
        })
    });
    finish(target.json, outcome, print_log)
}

#[derive(Args)]
pub(crate) struct SyncArguments {
    #[command(flatten)]
    target: Target,
    /// The shared folder: below it, `<workspace>/<project>/log/`.
    #[arg(long, env = "HONE_SYNC_FOLDER", conflicts_with_all = ["server", "git"])]
    folder: Option<PathBuf>,
    /// A branch of the project's Git repository used as the shared folder, for example
    /// `hone/process`.
    #[arg(long, env = "HONE_SYNC_GIT", conflicts_with = "server")]
    git: Option<String>,
    /// The remote of `--git`.
    #[arg(long, default_value = "origin", requires = "git")]
    remote: String,
    /// The base URL of a replication server, reached with the token in `HONE_TOKEN`.
    #[arg(long, env = "HONE_SYNC_SERVER")]
    server: Option<String>,
    /// Stay connected to the server: import events as they arrive and send new own events.
    /// Ends with Ctrl-C.
    #[arg(long, requires = "server")]
    watch: bool,
}

pub(crate) fn token() -> Result<String, String> {
    token_from("HONE_TOKEN")
}

fn token_from(variable: &str) -> Result<String, String> {
    std::env::var(variable)
        .ok()
        .filter(|token| !token.is_empty())
        .ok_or_else(|| format!("no token for the server: set {variable}"))
}

fn sync_with_context(store: &ProcessStore) -> Result<SyncReport, ProcessError> {
    let bound = Contexts::load()?
        .of_project(store.project())
        .ok_or_else(|| ProcessError::Invalid {
            message: "name a peer: --folder, --server, or --git, or bind the project to a context with hone context use".into(),
        })?;
    match &bound.context.peer {
        ContextPeer::Folder { folder } => store.sync(Peer::Folder(folder)),
        ContextPeer::Git { git, remote } => store.sync(Peer::Git {
            remote,
            branch: git,
        }),
        ContextPeer::Server { server } => {
            let variable = bound.context.token_env.as_deref().unwrap_or("HONE_TOKEN");
            let token =
                token_from(variable).map_err(|message| ProcessError::Invalid { message })?;
            store.sync(Peer::Server(&Server::new(server, &token)))
        }
    }
}

#[derive(Subcommand)]
pub(crate) enum ContextCommand {
    /// Create or replace a context: its peer, author, and the variable of its token.
    Set {
        /// The context's name: lowercase letters, digits, and `-`.
        name: String,
        /// A replication server by base URL.
        #[arg(long, group = "peer")]
        server: Option<String>,
        /// A shared folder.
        #[arg(long, group = "peer")]
        folder: Option<PathBuf>,
        /// A branch of the project's Git repository.
        #[arg(long, group = "peer")]
        git: Option<String>,
        /// The remote of `--git`.
        #[arg(long, default_value = "origin", requires = "git")]
        remote: String,
        /// The author of events written in this context.
        #[arg(long, requires = "role")]
        author: Option<String>,
        /// The role of that author.
        #[arg(long, requires = "author")]
        role: Option<String>,
        /// The environment variable that holds the server's token (default `HONE_TOKEN`).
        #[arg(long)]
        token_env: Option<String>,
        /// Print the result as JSON.
        #[arg(long)]
        json: bool,
    },
    /// Bind the project of the root to a context.
    Use {
        #[command(flatten)]
        target: Target,
        /// The context's name.
        name: String,
    },
    /// Show the context the project of the root is bound to.
    Show {
        #[command(flatten)]
        target: Target,
    },
}

pub(crate) fn run_context(command: ContextCommand) -> ExitCode {
    match command {
        ContextCommand::Set {
            name,
            server,
            folder,
            git,
            remote,
            author,
            role,
            token_env,
            json,
        } => {
            let peer = match (server, folder, git) {
                (Some(server), _, _) => ContextPeer::Server { server },
                (_, Some(folder), _) => ContextPeer::Folder { folder },
                (_, _, Some(git)) => ContextPeer::Git { git, remote },
                (None, None, None) => {
                    let error = ProcessError::Invalid {
                        message: "name the context's peer: --server, --folder, or --git".into(),
                    };
                    return finish(json, Err::<BoundContext, _>(error), print_context);
                }
            };
            let context = Context {
                peer,
                author: author.zip(role).map(|(name, role)| Author { name, role }),
                token_env,
            };
            let outcome = Contexts::load().and_then(|mut contexts| {
                contexts.set(&name, context.clone())?;
                contexts.save()?;
                Ok(BoundContext { name, context })
            });
            finish(json, outcome, print_context)
        }
        ContextCommand::Use { target, name } => {
            let outcome = ProcessStore::locate(&target.root, None).and_then(|store| {
                let mut contexts = Contexts::load()?;
                contexts.bind(store.project(), &name)?;
                contexts.save()?;
                contexts
                    .of_project(store.project())
                    .ok_or_else(|| ProcessError::Invalid {
                        message: format!("no context `{name}`"),
                    })
            });
            finish(target.json, outcome, print_context)
        }
        ContextCommand::Show { target } => {
            let outcome = ProcessStore::locate(&target.root, None).and_then(|store| {
                Contexts::load()?
                    .of_project(store.project())
                    .ok_or_else(|| ProcessError::Invalid {
                        message: format!(
                            "the project `{}` is bound to no context",
                            store.project()
                        ),
                    })
            });
            finish(target.json, outcome, print_context)
        }
    }
}

fn print_context(bound: &BoundContext) {
    let peer = match &bound.context.peer {
        ContextPeer::Server { server } => format!("server {server}"),
        ContextPeer::Folder { folder } => format!("folder {}", folder.display()),
        ContextPeer::Git { git, remote } => format!("git {git} via {remote}"),
    };
    println!("{}: {peer}", bound.name);
    if let Some(author) = &bound.context.author {
        println!("author {} · {}", author.name, author.role);
    }
    if let Some(variable) = &bound.context.token_env {
        println!("token from {variable}");
    }
}

pub(crate) fn run_sync(arguments: &SyncArguments) -> ExitCode {
    let target = &arguments.target;
    let outcome = ProcessStore::locate(&target.root, None).and_then(|store| {
        if let Some(branch) = &arguments.git {
            return store.sync(Peer::Git {
                remote: &arguments.remote,
                branch,
            });
        }
        if arguments.folder.is_none() && arguments.server.is_none() {
            return sync_with_context(&store);
        }
        match (&arguments.folder, &arguments.server) {
            (Some(folder), _) => store.sync(Peer::Folder(folder)),
            (None, Some(url)) => {
                let server = Server::new(
                    url,
                    &token().map_err(|message| ProcessError::Invalid { message })?,
                );
                if arguments.watch {
                    let json = target.json;
                    let ctrl_c = async {
                        let _ = tokio::signal::ctrl_c().await;
                    };
                    return store
                        .watch(&server, ctrl_c, |report| print_report(report, json))
                        .map(|()| SyncReport::default());
                }
                store.sync(Peer::Server(&server))
            }
            (None, None) => Err(ProcessError::Invalid {
                message: "name a peer: --folder or --server".into(),
            }),
        }
    });
    match &outcome {
        Ok(report) if !arguments.watch => print_report(report, target.json),
        Ok(_) => {}
        Err(error) if target.json => println!("{}", to_json(error)),
        Err(error) => eprintln!("{}", printable(&error.to_string())),
    }
    match outcome {
        Ok(report) if report.problems.is_empty() => ExitCode::SUCCESS,
        Ok(_) => ExitCode::FAILURE,
        Err(_) => ExitCode::from(2),
    }
}

fn print_report(report: &SyncReport, json: bool) {
    if json {
        println!("{}", to_json(report));
    } else {
        print_sync(report);
    }
}

fn print_sync(report: &SyncReport) {
    for (device, count) in &report.received {
        println!("received {count} from {device}");
    }
    for (device, count) in &report.sent {
        println!("sent {count} of {device}");
    }
    for problem in &report.problems {
        let code = serde_json::to_value(problem.code).expect("serializable");
        let line = problem
            .line
            .map_or_else(String::new, |line| format!(":{line}"));
        eprintln!(
            "{} {}{line}: {}",
            code.as_str().unwrap_or_default(),
            printable(&problem.path),
            printable(&problem.message)
        );
    }
}

// Paths and messages can carry text from an untrusted folder, which must not reach the terminal
// as control sequences.
fn printable(text: &str) -> String {
    text.chars()
        .map(|character| {
            if character.is_control() {
                character.escape_unicode().to_string()
            } else {
                character.to_string()
            }
        })
        .collect()
}

fn append(target: &Target, writer: &Writer, change: Change) -> ExitCode {
    let outcome = ProcessStore::locate(&target.root, None)
        .and_then(|store| store.append(&writer.author(&store)?, change));
    finish(target.json, outcome, print_appended)
}

fn finish<T: Serialize>(
    as_json: bool,
    outcome: Result<T, ProcessError>,
    print: fn(&T),
) -> ExitCode {
    match (outcome, as_json) {
        (Ok(value), true) => {
            println!("{}", to_json(&value));
            ExitCode::SUCCESS
        }
        (Ok(value), false) => {
            print(&value);
            ExitCode::SUCCESS
        }
        (Err(error), true) => {
            println!("{}", to_json(&error));
            ExitCode::FAILURE
        }
        (Err(error), false) => {
            eprintln!("{error}");
            ExitCode::FAILURE
        }
    }
}

fn to_json(value: &impl Serialize) -> String {
    serde_json::to_string(value).expect("the results of hone-core are serializable")
}

fn print_appended(appended: &Appended) {
    let event = &appended.event;
    match &appended.thread {
        Some(thread) if event["type"] == "thread.opened" => println!("{thread}"),
        _ => println!(
            "{} {}",
            event["type"].as_str().unwrap_or_default(),
            event["id"].as_str().unwrap_or_default()
        ),
    }
}

fn print_log(log: &StoreLog) {
    for event in &log.events {
        println!(
            "{} {} {} {}",
            printable(event["device"].as_str().unwrap_or_default()),
            event["seq"],
            printable(event["type"].as_str().unwrap_or_default()),
            printable(event["id"].as_str().unwrap_or_default())
        );
    }
}

fn print_events(events: &TicketEvents) {
    for thread in &events.threads {
        let status = match thread.status {
            ThreadStatus::Open => "open",
            ThreadStatus::Resolved => "resolved",
        };
        let blocking = if thread.blocking { " blocking" } else { "" };
        let answered = if thread.answered { "" } else { " unanswered" };
        println!(
            "thread {} {status}{blocking}{answered} ({}, by {})",
            thread.thread, thread.kind, thread.opened_by.name
        );
    }
    for (step, state) in &events.steps {
        let status = serde_json::to_value(state.status).expect("serializable");
        let reason = state
            .reason
            .as_deref()
            .map_or(String::new(), |reason| format!(": {reason}"));
        println!(
            "step {step} {}{reason}",
            status.as_str().unwrap_or_default()
        );
    }
    for invalid in &events.invalid {
        let line = invalid
            .line
            .map_or(String::new(), |line| format!(" line {line}"));
        println!(
            "invalid {}{line}: {}",
            printable(&invalid.path),
            printable(&invalid.message)
        );
    }
}

#[cfg(test)]
mod tests {
    use super::printable;

    #[test]
    fn control_characters_are_escaped_for_the_terminal() {
        assert_eq!(
            printable("dev-\u{1b}[31mred\u{7}é"),
            "dev-\\u{1b}[31mred\\u{7}é"
        );
    }
}
