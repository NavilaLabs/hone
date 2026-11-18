use std::collections::BTreeSet;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use serde_json::{Value, json};

use crate::finding::{Code, Finding};
use crate::manifest::ApprovedCommand;

const DEFAULT_TIMEOUT_MS: u64 = 600_000;
const OUTPUT_LIMIT: usize = 1024 * 1024;
const MESSAGE_TAIL: usize = 4096;

// The result of a command that ended normally (concept Section 9, "What WASM cannot do").
pub(crate) struct ToolOutput {
    pub(crate) exit: i32,
    pub(crate) stdout: String,
    pub(crate) stderr: String,
    pub(crate) truncated: bool,
}

impl ToolOutput {
    pub(crate) fn to_json(&self) -> Value {
        json!({
            "exit": self.exit,
            "stdout": self.stdout,
            "stderr": self.stderr,
            "truncated": self.truncated,
        })
    }

    // The finding of a gate without `evaluate`: an exit code other than 0 fails.
    pub(crate) fn exit_finding(&self) -> Option<Finding> {
        (self.exit != 0).then(|| {
            Finding::error(
                Code::ExecFailed,
                format!("exited with {}{}", self.exit, tail(&self.stderr)),
            )
        })
    }
}

// Runs the command of an `exec` gate with `params` in `project_root`, if `approved` contains it.
// A command that is not approved, cannot start, times out, or ends through a signal is a finding.
pub(crate) fn run(
    project_root: &Path,
    params: &Value,
    approved: &[ApprovedCommand],
) -> Result<ToolOutput, Finding> {
    let wanted = requested(params);
    if !approved.contains(&wanted) {
        return Err(Finding::error(
            Code::ExecUnapproved,
            format!(
                "the lockfile does not approve {}",
                json!({ "command": wanted.command, "cwd": wanted.cwd, "env": wanted.env })
            ),
        ));
    }
    let failed = |message: String| Finding::error(Code::ExecFailed, message);
    let program = &wanted.command[0];
    let executable = locate(program).ok_or_else(|| failed(format!("`{program}` not found")))?;
    let mut command = Command::new(executable);
    command
        .args(&wanted.command[1..])
        .current_dir(project_root.join(&wanted.cwd))
        .env_clear()
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    for name in &wanted.env {
        if let Some(value) = std::env::var_os(name) {
            command.env(name, value);
        }
    }
    let mut child = command
        .spawn()
        .map_err(|error| failed(format!("`{program}` could not start: {error}")))?;
    let stdout = drain(child.stdout.take());
    let stderr = drain(child.stderr.take());
    let timeout = params["timeout_ms"].as_u64().unwrap_or(DEFAULT_TIMEOUT_MS);
    // After a timeout the readers are not joined: a process the command started may still hold
    // the pipes open.
    let status = wait(&mut child, Duration::from_millis(timeout))
        .map_err(|message| failed(format!("`{program}` {message}")))?;
    let (stdout, stdout_truncated) = stdout.join().unwrap_or_default();
    let (stderr, stderr_truncated) = stderr.join().unwrap_or_default();
    let Some(exit) = status.code() else {
        return Err(failed(format!(
            "`{program}` ended through a signal{}",
            tail(&stderr)
        )));
    };
    Ok(ToolOutput {
        exit,
        stdout,
        stderr,
        truncated: stdout_truncated || stderr_truncated,
    })
}

fn requested(params: &Value) -> ApprovedCommand {
    let strings = |key: &str| {
        params[key]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .map(str::to_owned)
    };
    ApprovedCommand {
        command: strings("command").collect(),
        cwd: params["cwd"].as_str().unwrap_or(".").to_owned(),
        env: strings("env").collect::<BTreeSet<_>>(),
    }
}

// A program without a path separator is looked up through the `PATH` of hone itself, because
// the command's own environment may not contain `PATH`.
fn locate(program: &str) -> Option<PathBuf> {
    if program.contains(['/', '\\']) {
        return Some(PathBuf::from(program));
    }
    let suffixes: &[&str] = if cfg!(windows) {
        &[".exe", ".cmd", ".bat", ""]
    } else {
        &[""]
    };
    std::env::split_paths(&std::env::var_os("PATH")?).find_map(|directory| {
        suffixes
            .iter()
            .map(|suffix| directory.join(format!("{program}{suffix}")))
            .find(|candidate| candidate.is_file())
    })
}

// Reads a pipe to its end on a thread of its own, so that a full pipe never blocks the command.
// Keeps the first `OUTPUT_LIMIT` bytes.
fn drain(pipe: Option<impl Read + Send + 'static>) -> std::thread::JoinHandle<(String, bool)> {
    std::thread::spawn(move || {
        let Some(mut pipe) = pipe else {
            return (String::new(), false);
        };
        let mut kept = Vec::new();
        let mut truncated = false;
        let mut buffer = [0u8; 8192];
        while let Ok(read) = pipe.read(&mut buffer) {
            if read == 0 {
                break;
            }
            let room = OUTPUT_LIMIT - kept.len();
            kept.extend_from_slice(&buffer[..read.min(room)]);
            truncated |= read > room;
        }
        (String::from_utf8_lossy(&kept).into_owned(), truncated)
    })
}

fn wait(child: &mut Child, timeout: Duration) -> Result<std::process::ExitStatus, String> {
    let deadline = Instant::now() + timeout;
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return Ok(status),
            Ok(None) if Instant::now() >= deadline => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!("timed out after {} ms", timeout.as_millis()));
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(5)),
            Err(error) => return Err(format!("could not be waited for: {error}")),
        }
    }
}

fn tail(stderr: &str) -> String {
    let text = stderr.trim_end();
    if text.is_empty() {
        return String::new();
    }
    let start = text
        .char_indices()
        .map(|(index, _)| index)
        .find(|&index| text.len() - index <= MESSAGE_TAIL)
        .unwrap_or(0);
    format!(": {}", &text[start..])
}
