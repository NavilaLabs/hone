use std::collections::{BTreeMap, BTreeSet};
use std::fs::{File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use regex::Regex;
use serde::Serialize;
use serde_json::Value;

use crate::canonical;
use crate::event;

const SEGMENT_MAX_EVENTS: usize = 1000;
const SEGMENT_MAX_BYTES: usize = 1024 * 1024;

static SEGMENT: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^([1-9][0-9]*)\.ndjson$").expect("valid regex"));

/// An event of the process store that is not read (concept Section 4, "Reading").
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub struct InvalidEvent {
    /// The segment, relative to the store directory, for example `log/dev-3k9a5h2t/1.ndjson`.
    pub path: String,
    /// The line in the segment, counted from 1, or `None` if the whole file is concerned.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub line: Option<usize>,
    /// The field `ticket` of the event, if it can be read.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ticket: Option<String>,
    /// Why the event is invalid.
    pub message: String,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct OpenSegment {
    pub(crate) number: u64,
    pub(crate) lines: usize,
    pub(crate) bytes: usize,
    pub(crate) complete_bytes: usize,
    pub(crate) next_seq: Option<u64>,
}

/// The highest contiguous `seq` per device (concept Section 4, "Replication").
pub type Cursor = BTreeMap<String, u64>;

#[derive(Debug, Clone, Default)]
pub(crate) struct Log {
    pub(crate) events: Vec<Value>,
    pub(crate) invalid: Vec<InvalidEvent>,
    pub(crate) open_segments: BTreeMap<String, OpenSegment>,
    pub(crate) locations: BTreeMap<String, (String, usize)>,
    pub(crate) unsafe_paths: BTreeSet<String>,
}

impl Log {
    pub(crate) fn heads(&self, device: &str) -> Vec<String> {
        let mut newest: BTreeMap<&str, (u64, &str)> = BTreeMap::new();
        for event in &self.events {
            let (Some(owner), Some(seq), Some(id)) = (
                event["device"].as_str(),
                event["seq"].as_u64(),
                event["id"].as_str(),
            ) else {
                continue;
            };
            if owner != device && newest.get(owner).is_none_or(|(known, _)| seq > *known) {
                newest.insert(owner, (seq, id));
            }
        }
        let mut heads: Vec<String> = newest.values().map(|(_, id)| (*id).to_owned()).collect();
        heads.sort();
        heads
    }

    pub(crate) fn chains(&self) -> BTreeMap<&str, Vec<&Value>> {
        let mut chains: BTreeMap<&str, Vec<&Value>> = BTreeMap::new();
        for event in &self.events {
            chains
                .entry(event["device"].as_str().unwrap_or_default())
                .or_default()
                .push(event);
        }
        for chain in chains.values_mut() {
            chain.sort_by_key(|event| event["seq"].as_u64());
        }
        chains
    }

    pub(crate) fn cursor(&self) -> Cursor {
        self.chains()
            .into_iter()
            .map(|(device, chain)| (device.to_owned(), contiguous_end(&chain)))
            .collect()
    }

    pub(crate) fn after(&self, cursor: &Cursor) -> Vec<Value> {
        let mut events = Vec::new();
        for (device, chain) in self.chains() {
            let from = cursor.get(device).copied().unwrap_or(0);
            let to = contiguous_end(&chain);
            events.extend(
                chain
                    .into_iter()
                    .filter(|event| {
                        event["seq"]
                            .as_u64()
                            .is_some_and(|seq| seq > from && seq <= to)
                    })
                    .cloned(),
            );
        }
        events
    }
}

fn contiguous_end(chain: &[&Value]) -> u64 {
    let mut end = None;
    for seq in chain.iter().filter_map(|event| event["seq"].as_u64()) {
        match end {
            None => end = Some(seq),
            Some(last) if seq == last + 1 => end = Some(seq),
            Some(_) => break,
        }
    }
    end.unwrap_or(0)
}

struct Candidate {
    line: String,
    event: Value,
    path: String,
    line_number: usize,
}

pub(crate) fn read(store: &Path, project: &str, root: &Path) -> std::io::Result<Log> {
    // Every directory and segment must stay below `root`: in an untrusted store, a symbolic
    // link can point anywhere.
    let mut log = Log::default();
    let mut candidates = Vec::new();
    let directory = store.join("log");
    if !directory.exists() {
        return Ok(log);
    }
    if !log.permitted(&directory, "log", root) {
        return Ok(log);
    }
    for device in sorted_entries(&directory)? {
        let path = directory.join(&device);
        if path.is_dir() && log.permitted(&path, &format!("log/{device}"), root) {
            read_device(
                &directory,
                &device,
                project,
                root,
                &mut log,
                &mut candidates,
            )?;
        }
    }
    log.events = order(remove_conflicts(candidates, &mut log));
    log.invalid.sort();
    Ok(log)
}

impl Log {
    fn permitted(&mut self, path: &Path, relative: &str, root: &Path) -> bool {
        if below(path, root) {
            return true;
        }
        self.report_unsafe(relative);
        false
    }

    fn report_unsafe(&mut self, relative: &str) {
        self.unsafe_paths.insert(relative.to_owned());
        self.invalid.push(InvalidEvent {
            path: relative.to_owned(),
            line: None,
            ticket: None,
            message: "the path leads outside the store".into(),
        });
    }
}

pub(crate) fn open_below(
    path: &Path,
    root: &Path,
    options: &OpenOptions,
) -> std::io::Result<Option<File>> {
    // Checking the path before opening it leaves a window in which a writer of the folder can
    // swap in a link, so the opened file itself must be the one found below `root`.
    let file = options.open(path)?;
    let Ok(canonical) = path.canonicalize() else {
        return Ok(None);
    };
    if !below(&canonical, root) {
        return Ok(None);
    }
    Ok(same_file(&file, &canonical)?.then_some(file))
}

#[cfg(unix)]
fn same_file(file: &File, canonical: &Path) -> std::io::Result<bool> {
    use std::os::unix::fs::MetadataExt;
    let opened = file.metadata()?;
    let named = std::fs::metadata(canonical)?;
    // A hard link inside the folder can name a file outside it.
    Ok(opened.is_file()
        && opened.nlink() == 1
        && opened.dev() == named.dev()
        && opened.ino() == named.ino())
}

#[cfg(not(unix))]
fn same_file(file: &File, _canonical: &Path) -> std::io::Result<bool> {
    Ok(file.metadata()?.is_file())
}

pub(crate) fn below(path: &Path, root: &Path) -> bool {
    match (path.canonicalize(), root.canonicalize()) {
        (Ok(path), Ok(root)) => path.starts_with(root),
        _ => false,
    }
}

pub(crate) fn segment_numbers(directory: &Path) -> std::io::Result<Vec<u64>> {
    let mut numbers: Vec<u64> = sorted_entries(directory)?
        .iter()
        .filter_map(|name| SEGMENT.captures(name))
        .filter_map(|captures| captures[1].parse().ok())
        .collect();
    numbers.sort_unstable();
    Ok(numbers)
}

pub(crate) fn append_lines(
    log_directory: &Path,
    device: &str,
    open: &mut OpenSegment,
    lines: &[String],
) -> std::io::Result<()> {
    if lines.is_empty() {
        return Ok(());
    }
    let device_directory = log_directory.join(device);
    std::fs::create_dir_all(&device_directory)?;
    let segment = |number: u64| device_directory.join(format!("{number}.ndjson"));
    if open.number == 0 {
        open.number = 1;
    } else if open.bytes > open.complete_bytes {
        File::options()
            .write(true)
            .open(segment(open.number))?
            .set_len(open.complete_bytes as u64)?;
        open.bytes = open.complete_bytes;
    }
    let mut file: Option<(u64, File)> = None;
    for line in lines {
        if open.lines >= SEGMENT_MAX_EVENTS || open.complete_bytes >= SEGMENT_MAX_BYTES {
            open.number += 1;
            open.lines = 0;
            open.bytes = 0;
            open.complete_bytes = 0;
        }
        if file
            .as_ref()
            .is_none_or(|(number, _)| *number != open.number)
        {
            if let Some((_, previous)) = file.take() {
                previous.sync_data()?;
            }
            let opened = File::options()
                .create(true)
                .append(true)
                .open(segment(open.number))?;
            file = Some((open.number, opened));
        }
        let mut bytes = line.clone().into_bytes();
        bytes.push(b'\n');
        if let Some((_, handle)) = file.as_mut() {
            handle.write_all(&bytes)?;
        }
        open.lines += 1;
        open.complete_bytes += bytes.len();
        open.bytes = open.complete_bytes;
        open.next_seq = open.next_seq.map(|seq| seq + 1);
    }
    if let Some((_, handle)) = file {
        handle.sync_data()?;
    }
    Ok(())
}

pub(crate) fn segment_path(store: &Path, device: &str, number: u64) -> PathBuf {
    store
        .join("log")
        .join(device)
        .join(format!("{number}.ndjson"))
}

fn sorted_entries(directory: &Path) -> std::io::Result<Vec<String>> {
    let mut names = Vec::new();
    for entry in std::fs::read_dir(directory)? {
        if let Ok(name) = entry?.file_name().into_string() {
            names.push(name);
        }
    }
    names.sort();
    Ok(names)
}

fn read_device(
    log_directory: &Path,
    device: &str,
    project: &str,
    root: &Path,
    log: &mut Log,
    candidates: &mut Vec<Candidate>,
) -> std::io::Result<()> {
    let directory = log_directory.join(device);
    let mut numbers = Vec::new();
    for name in sorted_entries(&directory)? {
        if let Some(captures) = SEGMENT.captures(&name) {
            if let Ok(number) = captures[1].parse::<u64>()
                && log.permitted(
                    &directory.join(&name),
                    &format!("log/{device}/{name}"),
                    root,
                )
            {
                numbers.push(number);
            }
        } else if name.ends_with(".ndjson.zst") {
            log.invalid.push(InvalidEvent {
                path: format!("log/{device}/{name}"),
                line: None,
                ticket: None,
                message: "compressed segments are not supported yet".into(),
            });
        }
    }
    numbers.sort_unstable();
    let open = numbers.last().copied();
    let mut previous: Option<(u64, u64)> = None;
    for number in numbers {
        let path = format!("log/{device}/{number}.ndjson");
        let Some(mut file) = open_below(
            &directory.join(format!("{number}.ndjson")),
            root,
            OpenOptions::new().read(true),
        )?
        else {
            log.report_unsafe(&path);
            continue;
        };
        let mut bytes = Vec::new();
        file.read_to_end(&mut bytes)?;
        let is_open = Some(number) == open;
        let complete = bytes
            .iter()
            .rposition(|byte| *byte == b'\n')
            .map_or(0, |last| last + 1);
        let mut start = match previous {
            _ if number == 1 => Some(1),
            Some((before, next)) if before + 1 == number => Some(next),
            _ => None,
        };
        let mut lines: Vec<&[u8]> = bytes[..complete].split(|byte| *byte == b'\n').collect();
        lines.pop();
        if complete < bytes.len() && !is_open {
            lines.push(&bytes[complete..]);
        }
        let mut invalid = |line_number: usize, ticket: Option<String>, message: String| {
            log.invalid.push(InvalidEvent {
                path: path.clone(),
                line: Some(line_number),
                ticket,
                message,
            });
        };
        for (index, raw) in lines.iter().enumerate() {
            let line_number = index + 1;
            let unterminated = complete < bytes.len() && index + 1 == lines.len() && !is_open;
            let Some(event) = std::str::from_utf8(raw)
                .ok()
                .and_then(|text| serde_json::from_str::<Value>(text).ok())
            else {
                invalid(line_number, None, "the line is not JSON".into());
                continue;
            };
            let ticket = event["ticket"].as_str().map(str::to_owned);
            let line = String::from_utf8_lossy(raw).into_owned();
            let seq = event["seq"].as_u64();
            if start.is_none()
                && let Some(seq) = seq
                && seq > index as u64
            {
                start = Some(seq - index as u64);
            }
            let problem = if unterminated {
                Some("the last line of a closed segment does not end with a newline".to_owned())
            } else if canonical::to_canonical(&event) != line {
                Some("the line is not in the canonical form of RFC 8785".to_owned())
            } else if let Some(violation) = event::schema_violations(&event).into_iter().next() {
                Some(format!(
                    "the event does not match the event schema: {violation}"
                ))
            } else if event["device"] != device {
                Some(format!(
                    "the event of `{}` lies in the log of `{device}`",
                    event["device"].as_str().unwrap_or_default()
                ))
            } else if event["project"] != project {
                Some(format!(
                    "the event belongs to the project `{}`, not `{project}`",
                    event["project"].as_str().unwrap_or_default()
                ))
            } else {
                let expected = start.map(|start| start + index as u64);
                (seq != expected).then(|| {
                    format!(
                        "`seq` is {}, expected {}",
                        seq.unwrap_or_default(),
                        expected.map_or("a continuation".to_owned(), |value| value.to_string())
                    )
                })
            };
            match problem {
                Some(message) => invalid(line_number, ticket, message),
                None => candidates.push(Candidate {
                    line,
                    event,
                    path: path.clone(),
                    line_number,
                }),
            }
        }
        let next = start.map(|start| start + lines.len() as u64);
        if is_open {
            log.open_segments.insert(
                device.to_owned(),
                OpenSegment {
                    number,
                    lines: lines.len(),
                    bytes: bytes.len(),
                    complete_bytes: complete,
                    next_seq: next,
                },
            );
        }
        previous = next.map(|next| (number, next));
    }
    Ok(())
}

fn remove_conflicts(candidates: Vec<Candidate>, log: &mut Log) -> Vec<Value> {
    let mut by_id: BTreeMap<String, Vec<Candidate>> = BTreeMap::new();
    for candidate in candidates {
        let id = candidate.event["id"]
            .as_str()
            .unwrap_or_default()
            .to_owned();
        by_id.entry(id).or_default().push(candidate);
    }
    let mut events = Vec::new();
    for (id, mut copies) in by_id {
        if copies.iter().all(|copy| copy.line == copies[0].line) {
            let kept = copies.swap_remove(0);
            log.locations.insert(id, (kept.path, kept.line_number));
            events.push(kept.event);
            continue;
        }
        for copy in copies {
            log.invalid.push(InvalidEvent {
                path: copy.path,
                line: Some(copy.line_number),
                ticket: copy.event["ticket"].as_str().map(str::to_owned),
                message: format!("another event carries the ID `{id}` with different content"),
            });
        }
    }
    events
}

fn order(events: Vec<Value>) -> Vec<Value> {
    let id = |index: usize| events[index]["id"].as_str().unwrap_or_default();
    let index_of: BTreeMap<&str, usize> =
        (0..events.len()).map(|index| (id(index), index)).collect();
    let mut predecessors: Vec<BTreeSet<usize>> = vec![BTreeSet::new(); events.len()];
    for (index, event) in events.iter().enumerate() {
        for reference in event["after"].as_array().into_iter().flatten() {
            if let Some(&before) = reference.as_str().and_then(|r| index_of.get(r))
                && before != index
            {
                predecessors[index].insert(before);
            }
        }
    }
    let mut by_device: BTreeMap<&str, Vec<(u64, usize)>> = BTreeMap::new();
    for (index, event) in events.iter().enumerate() {
        by_device
            .entry(event["device"].as_str().unwrap_or_default())
            .or_default()
            .push((event["seq"].as_u64().unwrap_or_default(), index));
    }
    for chain in by_device.values_mut() {
        chain.sort_unstable();
        for pair in chain.windows(2) {
            predecessors[pair[1].1].insert(pair[0].1);
        }
    }
    let mut successors: Vec<Vec<usize>> = vec![Vec::new(); events.len()];
    for (index, before) in predecessors.iter().enumerate() {
        for &predecessor in before {
            successors[predecessor].push(index);
        }
    }
    let mut waiting: Vec<usize> = predecessors.iter().map(BTreeSet::len).collect();
    let mut ready: BTreeSet<(&str, usize)> = (0..events.len())
        .filter(|&index| waiting[index] == 0)
        .map(|index| (id(index), index))
        .collect();
    let mut remaining: BTreeSet<(&str, usize)> =
        (0..events.len()).map(|index| (id(index), index)).collect();
    let mut sequence = Vec::with_capacity(events.len());
    while let Some(&next) = ready.first().or_else(|| remaining.first()) {
        ready.remove(&next);
        remaining.remove(&next);
        sequence.push(next.1);
        for &successor in &successors[next.1] {
            if waiting[successor] > 0 {
                waiting[successor] -= 1;
                if waiting[successor] == 0 && remaining.contains(&(id(successor), successor)) {
                    ready.insert((id(successor), successor));
                }
            }
        }
    }
    let mut slots: Vec<Option<Value>> = events.into_iter().map(Some).collect();
    sequence
        .into_iter()
        .filter_map(|index| slots[index].take())
        .collect()
}
