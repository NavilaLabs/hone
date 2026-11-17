use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::sync::LazyLock;

use regex::Regex;
use serde::Serialize;
use serde_json::Value;

use crate::canonical;
use crate::event;

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

#[derive(Debug, Clone, Default)]
pub(crate) struct Log {
    pub(crate) events: Vec<Value>,
    pub(crate) invalid: Vec<InvalidEvent>,
    pub(crate) open_segments: BTreeMap<String, OpenSegment>,
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
}

struct Candidate {
    line: String,
    event: Value,
    path: String,
    line_number: usize,
}

pub(crate) fn read(store: &Path, project: &str) -> std::io::Result<Log> {
    let mut log = Log::default();
    let mut candidates = Vec::new();
    let directory = store.join("log");
    if !directory.exists() {
        return Ok(log);
    }
    for device in sorted_entries(&directory)? {
        if directory.join(&device).is_dir() {
            read_device(&directory, &device, project, &mut log, &mut candidates)?;
        }
    }
    log.events = order(remove_conflicts(candidates, &mut log.invalid));
    log.invalid.sort();
    Ok(log)
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
    log: &mut Log,
    candidates: &mut Vec<Candidate>,
) -> std::io::Result<()> {
    let directory = log_directory.join(device);
    let mut numbers = Vec::new();
    for name in sorted_entries(&directory)? {
        if let Some(captures) = SEGMENT.captures(&name) {
            if let Ok(number) = captures[1].parse::<u64>() {
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
        let bytes = std::fs::read(directory.join(format!("{number}.ndjson")))?;
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

fn remove_conflicts(candidates: Vec<Candidate>, invalid: &mut Vec<InvalidEvent>) -> Vec<Value> {
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
            events.push(copies.swap_remove(0).event);
            continue;
        }
        for copy in copies {
            invalid.push(InvalidEvent {
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
