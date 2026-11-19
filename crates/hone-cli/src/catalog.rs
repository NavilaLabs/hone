use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Args, Subcommand};
use hone_core::{Catalog, CheckError, TicketOverview};
use serde::Serialize;
use serde_json::json;

#[derive(Args)]
pub(crate) struct CatalogArguments {
    /// Project root, the directory that contains `.hone/`.
    #[arg(long, default_value = ".")]
    root: PathBuf,
    /// Print the result as JSON.
    #[arg(long)]
    json: bool,
}

#[derive(Subcommand)]
pub(crate) enum Ticket {
    /// Show a ticket record with the steps of its profile and the roles of its snapshot.
    Show {
        /// Project root, the directory that contains `.hone/`.
        #[arg(long, default_value = ".")]
        root: PathBuf,
        /// The ticket.
        #[arg(long)]
        ticket: String,
        /// Print the result as JSON.
        #[arg(long)]
        json: bool,
    },
}

pub(crate) fn run_artifacts(arguments: &CatalogArguments) -> ExitCode {
    finish(
        arguments.json,
        hone_core::catalog(&arguments.root),
        print_catalog,
    )
}

pub(crate) fn run_ticket(command: Ticket) -> ExitCode {
    match command {
        Ticket::Show { root, ticket, json } => finish(
            json,
            hone_core::ticket_overview(&root, &ticket),
            print_overview,
        ),
    }
}

fn finish<T: Serialize>(as_json: bool, outcome: Result<T, CheckError>, print: fn(&T)) -> ExitCode {
    match (outcome, as_json) {
        (Ok(value), true) => {
            println!(
                "{}",
                serde_json::to_string(&value).expect("the results of hone-core are serializable")
            );
            ExitCode::SUCCESS
        }
        (Ok(value), false) => {
            print(&value);
            ExitCode::SUCCESS
        }
        (Err(CheckError { message }), true) => {
            println!("{}", json!({ "status": "error", "message": message }));
            ExitCode::FAILURE
        }
        (Err(CheckError { message }), false) => {
            eprintln!("{message}");
            ExitCode::FAILURE
        }
    }
}

fn print_catalog(catalog: &Catalog) {
    for artifact in &catalog.artifacts {
        let id = artifact.id.as_deref().unwrap_or("-");
        let status = artifact.effective.as_deref().unwrap_or("-");
        println!("{id} {} {status} {}", artifact.kind, artifact.path);
    }
    for file in &catalog.unreadable {
        eprintln!("{}: {}", file.path, file.message);
    }
}

fn print_overview(overview: &TicketOverview) {
    println!(
        "{} profile {} ({})",
        overview.ticket, overview.profile, overview.config
    );
    for step in &overview.steps {
        if step.after.is_empty() {
            println!("  {} {}", step.step, step.requirement);
        } else {
            println!(
                "  {} {} after {}",
                step.step,
                step.requirement,
                step.after.join(", ")
            );
        }
    }
    println!("roles {}", overview.roles.join(", "));
}
