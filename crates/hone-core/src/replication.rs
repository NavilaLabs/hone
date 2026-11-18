use std::collections::{BTreeMap, BTreeSet};
use std::fs::File;
use std::io::{ErrorKind, Write};
use std::path::{Path, PathBuf};

use serde::Serialize;
use serde_json::Value;

use crate::canonical;
use crate::log::{self, Log};

/// The kind of a problem a sync reports (concept Section 4, "Replication").
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum SyncCode {
    /// Another event at the same device and `seq`: two machines write as one device.
    Fork,
    /// The same event ID with other content.
    IdConflict,
    /// A line that breaks the reading rules, or a gap in a device's log.
    InvalidEvent,
    /// A path that leads outside the shared folder.
    UnsafePath,
}

/// A problem a sync found. Nothing after it is imported from or written for that device.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub struct SyncProblem {
    /// The kind of the problem.
    pub code: SyncCode,
    /// The file concerned, in the shared folder or in the local store.
    pub path: PathBuf,
    /// The line in the file, counted from 1, or `None` if the whole file is concerned.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub line: Option<usize>,
    /// What is wrong.
    pub message: String,
}

/// The result of a sync with a shared folder.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize)]
pub struct SyncReport {
    /// The number of events imported from the folder, per device that had any.
    pub received: BTreeMap<String, u64>,
    /// The number of events written to the folder, per device that had any. Only this
    /// device's own log is written.
    pub sent: BTreeMap<String, u64>,
    /// The problems, sorted.
    pub problems: Vec<SyncProblem>,
}

impl SyncReport {
    fn problem(&mut self, code: SyncCode, path: PathBuf, line: Option<usize>, message: String) {
        self.problems.push(SyncProblem {
            code,
            path,
            line,
            message,
        });
    }
}

pub(crate) fn sync_folder(
    local: &Path,
    workspace: &str,
    project: &str,
    own: Option<&str>,
    folder: &Path,
) -> std::io::Result<SyncReport> {
    let mut report = SyncReport::default();
    let remote = folder.join(workspace).join(project);
    let remote_exists = remote.symlink_metadata().is_ok();
    if remote_exists && !log::below(&remote, folder) {
        report.problem(
            SyncCode::UnsafePath,
            remote,
            None,
            "the project directory leads outside the shared folder".into(),
        );
        return Ok(report);
    }
    let local_log = log::read(local, project, local)?;
    let remote_log = if remote_exists {
        log::read(&remote, project, folder)?
    } else {
        Log::default()
    };
    for invalid in &remote_log.invalid {
        let code = if remote_log.unsafe_paths.contains(&invalid.path) {
            SyncCode::UnsafePath
        } else {
            SyncCode::InvalidEvent
        };
        report.problem(
            code,
            remote.join(&invalid.path),
            invalid.line,
            invalid.message.clone(),
        );
    }
    pull(local, &local_log, &remote, &remote_log, &mut report)?;
    if let Some(own) = own {
        push(
            local,
            own,
            folder,
            &[workspace, project, "log", own],
            &mut report,
        )?;
    }
    report.problems.sort();
    // Pull and push can find the same fork or link; it is one problem.
    report.problems.dedup_by(|later, earlier| {
        (later.code, &later.path, later.line) == (earlier.code, &earlier.path, earlier.line)
    });
    Ok(report)
}

fn pull(
    local: &Path,
    local_log: &Log,
    remote: &Path,
    remote_log: &Log,
    report: &mut SyncReport,
) -> std::io::Result<()> {
    let cursor = local_log.cursor();
    let mut slots: BTreeMap<(&str, u64), &Value> = BTreeMap::new();
    let mut ids: BTreeSet<&str> = BTreeSet::new();
    for event in &local_log.events {
        let device = event["device"].as_str().unwrap_or_default();
        slots.insert((device, event["seq"].as_u64().unwrap_or_default()), event);
        ids.insert(event["id"].as_str().unwrap_or_default());
    }
    // A gap behind an invalid line is already reported with that line.
    let devices_with_invalid_lines: BTreeSet<&str> = remote_log
        .invalid
        .iter()
        .filter_map(|invalid| invalid.path.split('/').nth(1))
        .collect();
    for (device, chain) in remote_log.chains() {
        let local_end = cursor.get(device).copied().unwrap_or(0);
        let mut open = local_log
            .open_segments
            .get(device)
            .cloned()
            .unwrap_or_default();
        let ends_at_cursor = open.number == 0
            || open.next_seq == Some(local_end + 1)
            || (open.lines == 0 && local_end == 0);
        if !ends_at_cursor {
            // Appending behind a hole in the local log would break its `seq`.
            if chain
                .iter()
                .any(|event| event["seq"].as_u64() > Some(local_end))
            {
                report.problem(
                    SyncCode::InvalidEvent,
                    log::segment_path(local, device, open.number),
                    None,
                    format!(
                        "the local log of `{device}` does not end at `seq` {local_end}, nothing is imported"
                    ),
                );
            }
            continue;
        }
        let mut next = local_end + 1;
        let mut lines = Vec::new();
        for event in chain {
            let id = event["id"].as_str().unwrap_or_default();
            let seq = event["seq"].as_u64().unwrap_or_default();
            let line = canonical::to_canonical(event);
            let (path, line_number) = remote_log
                .locations
                .get(id)
                .map_or((remote.to_path_buf(), None), |(path, number)| {
                    (remote.join(path), Some(*number))
                });
            let conflict = match slots.get(&(device, seq)) {
                Some(held) if canonical::to_canonical(held) == line => continue,
                Some(held) if held["id"] == id => Some((
                    SyncCode::IdConflict,
                    format!("the local event `{id}` has other content"),
                )),
                Some(held) => Some((
                    SyncCode::Fork,
                    format!(
                        "`seq` {seq} of `{device}` is `{}` locally and `{id}` in the folder",
                        held["id"].as_str().unwrap_or_default()
                    ),
                )),
                None if ids.contains(id) => Some((
                    SyncCode::IdConflict,
                    format!("the local store holds `{id}` with other content"),
                )),
                None => None,
            };
            if let Some((code, message)) = conflict {
                report.problem(code, path, line_number, message);
                break;
            }
            if seq != next {
                if !devices_with_invalid_lines.contains(device) {
                    report.problem(
                        SyncCode::InvalidEvent,
                        path,
                        line_number,
                        format!("`seq` is {seq}, expected {next}: events are missing"),
                    );
                }
                break;
            }
            lines.push(line);
            next += 1;
        }
        if lines.is_empty() {
            continue;
        }
        log::append_lines(&local.join("log"), device, &mut open, &lines)?;
        report
            .received
            .insert(device.to_owned(), lines.len() as u64);
    }
    Ok(())
}

fn push(
    local: &Path,
    own: &str,
    folder: &Path,
    components: &[&str],
    report: &mut SyncReport,
) -> std::io::Result<()> {
    let local_directory = local.join("log").join(own);
    if !local_directory.is_dir() {
        return Ok(());
    }
    let numbers = log::segment_numbers(&local_directory)?;
    if numbers.is_empty() {
        return Ok(());
    }
    let Some(target_directory) = directory_below(folder, components, report)? else {
        return Ok(());
    };
    let mut sent = 0;
    for number in numbers {
        let name = format!("{number}.ndjson");
        let bytes = std::fs::read(local_directory.join(&name))?;
        let complete = &bytes[..bytes
            .iter()
            .rposition(|byte| *byte == b'\n')
            .map_or(0, |last| last + 1)];
        let target = target_directory.join(&name);
        let held = match target.symlink_metadata() {
            Ok(_) if !log::below(&target, folder) => {
                report.problem(
                    SyncCode::UnsafePath,
                    target,
                    None,
                    "the segment leads outside the shared folder".into(),
                );
                break;
            }
            Ok(_) => std::fs::read(&target)?,
            Err(error) if error.kind() == ErrorKind::NotFound => Vec::new(),
            Err(error) => return Err(error),
        };
        if !complete.starts_with(&held) {
            let common = held
                .iter()
                .zip(complete)
                .take_while(|(left, right)| left == right)
                .count();
            let line = held[..common].iter().filter(|byte| **byte == b'\n').count() + 1;
            report.problem(
                SyncCode::Fork,
                target,
                Some(line),
                format!("the folder copy of the log of `{own}` differs from the local one"),
            );
            break;
        }
        let rest = &complete[held.len()..];
        if rest.is_empty() {
            continue;
        }
        let mut file = File::options().create(true).append(true).open(&target)?;
        file.write_all(rest)?;
        file.sync_data()?;
        sent += rest.iter().filter(|byte| **byte == b'\n').count() as u64;
    }
    if sent > 0 {
        report.sent.insert(own.to_owned(), sent);
    }
    Ok(())
}

fn directory_below(
    root: &Path,
    components: &[&str],
    report: &mut SyncReport,
) -> std::io::Result<Option<PathBuf>> {
    // One component at a time, so that a symbolic link in the folder never makes hone create
    // anything outside it.
    let mut path = root.to_path_buf();
    for component in components {
        path.push(component);
        match std::fs::create_dir(&path) {
            Err(error) if error.kind() != ErrorKind::AlreadyExists => return Err(error),
            _ => {}
        }
        if !log::below(&path, root) {
            report.problem(
                SyncCode::UnsafePath,
                path,
                None,
                "the directory leads outside the shared folder".into(),
            );
            return Ok(None);
        }
    }
    Ok(Some(path))
}
