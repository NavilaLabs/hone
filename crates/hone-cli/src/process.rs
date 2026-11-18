use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Args, Subcommand, ValueEnum};
use hone_core::{
    Appended, Author, Change, ProcessError, ProcessStore, StepStatus, SyncReport, ThreadAnchor,
    ThreadStatus, TicketEvents,
};
use serde::Serialize;

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
    /// The handle of the author.
    #[arg(long, env = "HONE_AUTHOR")]
    author: String,
    /// The role of the author. It must exist in the ticket's configuration.
    #[arg(long, env = "HONE_ROLE")]
    role: String,
}

impl Writer {
    pub(crate) fn author(&self) -> Author {
        Author {
            name: self.author.clone(),
            role: self.role.clone(),
        }
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

#[derive(Args)]
pub(crate) struct SyncArguments {
    #[command(flatten)]
    target: Target,
    /// The shared folder: below it, `<workspace>/<project>/log/`.
    #[arg(long, env = "HONE_SYNC_FOLDER")]
    folder: PathBuf,
}

pub(crate) fn run_sync(arguments: &SyncArguments) -> ExitCode {
    let target = &arguments.target;
    let outcome =
        ProcessStore::locate(&target.root, None).and_then(|store| store.sync(&arguments.folder));
    match (&outcome, target.json) {
        (Ok(report), true) => println!("{}", to_json(report)),
        (Ok(report), false) => print_sync(report),
        (Err(error), true) => println!("{}", to_json(error)),
        (Err(error), false) => eprintln!("{error}"),
    }
    match outcome {
        Ok(report) if report.problems.is_empty() => ExitCode::SUCCESS,
        Ok(_) => ExitCode::FAILURE,
        Err(_) => ExitCode::from(2),
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
            problem.path.display(),
            problem.message
        );
    }
}

fn append(target: &Target, writer: &Writer, change: Change) -> ExitCode {
    let outcome = ProcessStore::locate(&target.root, None)
        .and_then(|store| store.append(&writer.author(), change));
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
        println!("invalid {}{line}: {}", invalid.path, invalid.message);
    }
}
