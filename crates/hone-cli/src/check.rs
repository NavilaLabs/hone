use std::io::Read as _;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::{Args, Subcommand, ValueEnum};
use hone_core::{CheckError, CheckReport, CheckRequest, GateClass};
use serde_json::{Value, json};

const GATE_FAILED: u8 = 1;
const COULD_NOT_RUN: u8 = 2;
// Claude Code blocks a `PreToolUse` call only on exit code 2. Any other code lets it through.
const BLOCK_TOOL_CALL: u8 = 2;
const EDIT_TOOLS: [&str; 4] = ["Write", "Edit", "MultiEdit", "NotebookEdit"];

#[derive(Args)]
pub(crate) struct CheckArguments {
    /// Project root, the directory that contains `.hone/`.
    #[arg(long, default_value = ".")]
    root: PathBuf,
    /// The ticket whose gates run. Without one, only the directory gates run.
    #[arg(long)]
    ticket: Option<String>,
    /// Run only the gates of this step of the ticket's profile.
    #[arg(long, requires = "ticket")]
    step: Option<String>,
    /// A path the change touches, relative to the project root. Repeatable.
    #[arg(long = "changed", value_name = "PATH")]
    changed: Vec<String>,
    /// A file with one changed path per line, or `-` for stdin. An empty file means no changes.
    #[arg(long, value_name = "FILE")]
    changed_from: Option<PathBuf>,
    /// Run only gates of this class.
    #[arg(long, value_enum)]
    class: Option<Class>,
    /// Print the result as JSON.
    #[arg(long)]
    json: bool,
}

#[derive(Clone, Copy, ValueEnum)]
enum Class {
    Pure,
    Exec,
}

#[derive(Subcommand)]
pub(crate) enum Hook {
    /// A `PreToolUse` hook for Claude Code: blocks the edit of code that has no binding decision.
    ClaudeCode {
        /// Project root, the directory that contains `.hone/`. Defaults to `cwd` of the hook input.
        #[arg(long)]
        root: Option<PathBuf>,
        /// The ticket the agent works on. Defaults to the environment variable `HONE_TICKET`.
        #[arg(long)]
        ticket: Option<String>,
    },
}

pub(crate) fn run_check(arguments: &CheckArguments) -> ExitCode {
    let changed_paths = match changed_paths(arguments) {
        Ok(paths) => paths,
        Err(message) => return could_not_run(arguments.json, &message),
    };
    let request = CheckRequest {
        ticket: arguments.ticket.clone(),
        step: arguments.step.clone(),
        changed_paths,
        class: arguments.class.map(|class| match class {
            Class::Pure => GateClass::Pure,
            Class::Exec => GateClass::Exec,
        }),
        data_directory: None,
    };
    match hone_core::check(&arguments.root, &request) {
        Ok(report) => {
            if arguments.json {
                let status = if report.passed() { "passed" } else { "failed" };
                println!("{}", json!({ "status": status, "gates": report.gates }));
            } else {
                print_report(&report);
            }
            if report.passed() {
                ExitCode::SUCCESS
            } else {
                ExitCode::from(GATE_FAILED)
            }
        }
        Err(CheckError { message }) => could_not_run(arguments.json, &message),
    }
}

fn changed_paths(arguments: &CheckArguments) -> Result<Option<Vec<String>>, String> {
    let mut paths = arguments.changed.clone();
    if let Some(source) = &arguments.changed_from {
        let text = if source == Path::new("-") {
            let mut text = String::new();
            std::io::stdin()
                .read_to_string(&mut text)
                .map_err(|error| format!("stdin: {error}"))?;
            text
        } else {
            std::fs::read_to_string(source)
                .map_err(|error| format!("{}: {error}", source.display()))?
        };
        paths.extend(
            text.lines()
                .map(str::trim)
                .filter(|line| !line.is_empty())
                .map(str::to_owned),
        );
    }
    let supplied = !arguments.changed.is_empty() || arguments.changed_from.is_some();
    Ok(supplied.then_some(paths))
}

fn print_report(report: &CheckReport) {
    for gate in &report.gates {
        println!("{} {}", if gate.ok { "ok  " } else { "FAIL" }, gate.name);
        for finding in &gate.findings {
            println!("     {finding}");
        }
    }
    if report.gates.is_empty() {
        println!("no gate ran");
    }
}

fn could_not_run(as_json: bool, message: &str) -> ExitCode {
    if as_json {
        println!("{}", json!({ "status": "error", "message": message }));
    } else {
        eprintln!("{message}");
    }
    ExitCode::from(COULD_NOT_RUN)
}

pub(crate) fn run_hook(hook: &Hook) -> ExitCode {
    let Hook::ClaudeCode { root, ticket } = hook;
    let mut text = String::new();
    if let Err(error) = std::io::stdin().read_to_string(&mut text) {
        eprintln!("hone hook: stdin: {error}");
        return ExitCode::FAILURE;
    }
    let input: Value = match serde_json::from_str(&text) {
        Ok(input) => input,
        Err(error) => {
            eprintln!("hone hook: the hook input is not JSON: {error}");
            return ExitCode::FAILURE;
        }
    };
    if !EDIT_TOOLS.contains(&input["tool_name"].as_str().unwrap_or_default()) {
        return ExitCode::SUCCESS;
    }
    let Some(file) = input["tool_input"]["file_path"]
        .as_str()
        .or_else(|| input["tool_input"]["notebook_path"].as_str())
    else {
        return ExitCode::SUCCESS;
    };
    let Some(ticket) = ticket.clone().or_else(|| std::env::var("HONE_TICKET").ok()) else {
        eprintln!("hone hook: no ticket (--ticket or HONE_TICKET), the edit is not checked");
        return ExitCode::SUCCESS;
    };
    let root = root
        .clone()
        .or_else(|| input["cwd"].as_str().map(PathBuf::from))
        .unwrap_or_else(|| PathBuf::from("."));
    let Some(path) = relative_to(&root, Path::new(file)) else {
        return ExitCode::SUCCESS;
    };
    match hone_core::check_edit(&root, &ticket, &path) {
        Ok(report) if report.passed() => ExitCode::SUCCESS,
        Ok(report) => {
            eprintln!("hone: {path} must not be changed yet, the change has no binding decision:");
            for finding in report.gates.iter().flat_map(|gate| &gate.findings) {
                eprintln!("  {finding}");
            }
            ExitCode::from(BLOCK_TOOL_CALL)
        }
        Err(error) => {
            eprintln!("hone hook: {error}");
            ExitCode::FAILURE
        }
    }
}

// The path of `file` relative to `root` with `/` as separator, or `None` if it lies outside.
// The file and its directories may not exist yet, so only the nearest existing ancestor is
// resolved.
fn relative_to(root: &Path, file: &Path) -> Option<String> {
    let root = root.canonicalize().ok()?;
    let mut existing = if file.is_absolute() {
        file.to_path_buf()
    } else {
        root.join(file)
    };
    let mut missing = Vec::new();
    while !existing.exists() {
        missing.push(existing.file_name()?.to_owned());
        existing = existing.parent()?.to_path_buf();
    }
    let mut resolved = existing.canonicalize().ok()?;
    resolved.extend(missing.iter().rev());
    let segments: Option<Vec<&str>> = resolved
        .strip_prefix(&root)
        .ok()?
        .components()
        .map(|component| component.as_os_str().to_str())
        .collect();
    Some(segments?.join("/"))
}
