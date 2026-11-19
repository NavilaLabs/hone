use std::collections::BTreeMap;

use dioxus::prelude::*;

use super::Session;
use super::common::{Empty, FailureAlert, Field, Mark, PageHeader, SectionLabel, StatusMark};
use crate::hone::{Failure, Peer, SyncReport};
use crate::ui::{
    Button, Card, CardContent, CardHeader, CardTitle, Input, Progress, Skeleton, Table, TableBody,
    TableCell, TableHead, TableHeader, TableRow, TableWrapper, Tabs, TabsList, TabsTrigger,
    TabsVariant,
};

/// A device of the store: the `seq` known locally, and what the last sync moved for it.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Device {
    device: String,
    known: u64,
    received: u64,
    sent: u64,
}

/// Every device of the cursor or the report, in the order of their IDs.
fn devices(cursor: &BTreeMap<String, u64>, report: Option<&SyncReport>) -> Vec<Device> {
    let mut names: Vec<&String> = cursor.keys().collect();
    if let Some(report) = report {
        names.extend(report.received.keys());
        names.extend(report.sent.keys());
    }
    names.sort();
    names.dedup();
    let count = |map: Option<&BTreeMap<String, u64>>, name: &String| {
        map.and_then(|map| map.get(name)).copied().unwrap_or(0)
    };
    names
        .into_iter()
        .map(|name| Device {
            device: name.clone(),
            known: cursor.get(name).copied().unwrap_or(0),
            received: count(report.map(|report| &report.received), name),
            sent: count(report.map(|report| &report.sent), name),
        })
        .collect()
}

/// `hone sync` with a shared folder or a server, and the devices of the store.
#[component]
pub(crate) fn Sync() -> Element {
    let session = use_context::<Session>();
    let kind = use_signal(|| {
        if std::env::var_os("HONE_SYNC_SERVER").is_some() {
            "server"
        } else {
            "folder"
        }
        .to_owned()
    });
    let mut folder = use_signal(|| std::env::var("HONE_SYNC_FOLDER").unwrap_or_default());
    let mut server = use_signal(|| std::env::var("HONE_SYNC_SERVER").unwrap_or_default());
    let mut outcome = use_signal(|| None::<Result<SyncReport, Failure>>);
    let mut running = use_signal(|| false);
    let log = use_resource(move || async move {
        let _ = (session.revision)();
        let hone = (session.hone)();
        hone.log().await
    });
    let peer = move || {
        if kind() == "server" {
            Peer::Server(server().trim().to_owned())
        } else {
            Peer::Folder(folder().trim().to_owned())
        }
    };
    let sync = move |_| {
        running.set(true);
        spawn(async move {
            let hone = (session.hone)();
            let result = hone.sync(&peer()).await;
            if result.is_ok() {
                session.changed();
            }
            outcome.set(Some(result));
            running.set(false);
        });
    };
    let cursor = match &*log.read() {
        Some(Ok(log)) => log.cursor.clone(),
        _ => BTreeMap::new(),
    };
    let report = match outcome() {
        Some(Ok(report)) => Some(report),
        _ => None,
    };
    let rows = devices(&cursor, report.as_ref());
    let highest = rows.iter().map(|device| device.known).max().unwrap_or(0);
    rsx! {
        PageHeader {
            context: match peer() { Peer::Folder(folder) => format!("shared folder {folder}"), Peer::Server(url) => format!("server {url}") },
            title: "Sync",
            actions: rsx! {
                Button { disabled: running(), onclick: sync, "Sync now" }
            },
        }
        Tabs { active: kind,
            TabsList { variant: TabsVariant::Line,
                TabsTrigger { value: "folder", "Shared folder" }
                TabsTrigger { value: "server", "Server" }
            }
        }
        if kind() == "server" {
            Field { label: "Server URL (the token comes from HONE_TOKEN)",
                Input { class: "font-mono max-w-xl", value: server(), placeholder: "https://hone.example.org", oninput: move |value| server.set(value) }
            }
        } else {
            Field { label: "Shared folder",
                Input { class: "font-mono max-w-xl", value: folder(), placeholder: "/mnt/share/hone", oninput: move |value| folder.set(value) }
            }
        }
        if running() {
            Skeleton { class: "h-24 w-full" }
        } else if let Some(Err(failure)) = outcome() {
            FailureAlert { failure }
        } else if let Some(report) = report.clone() {
            div { class: "grid grid-cols-3 border border-line",
                Stat { label: "Received", value: report.received.values().sum::<u64>().to_string() }
                Stat { label: "Sent", value: report.sent.values().sum::<u64>().to_string() }
                Stat { label: "Problems", value: report.problems.len().to_string() }
            }
        }
        section {
            SectionLabel { label: "Devices", aside: format!("{} known", rows.len()) }
            if rows.is_empty() {
                Empty { text: "No device has events yet. Devices appear here after the first write or sync." }
            } else {
                TableWrapper {
                    Table {
                        TableHeader { class: "bg-surface",
                            TableRow {
                                TableHead { "Device" }
                                TableHead { "Known seq" }
                                TableHead { class: "text-right", "Received" }
                                TableHead { class: "text-right", "Sent" }
                            }
                        }
                        TableBody {
                            for device in rows {
                                TableRow {
                                    TableCell { class: "font-mono", "{device.device}" }
                                    TableCell {
                                        div { class: "flex items-center gap-3 min-w-[160px]",
                                            Progress { class: "h-2 bg-surface-2", value: device.known as f64, max: highest as f64 }
                                            span { class: "font-mono text-[12.5px]", "{device.known}" }
                                        }
                                    }
                                    TableCell { class: "font-mono text-right", "{device.received}" }
                                    TableCell { class: "font-mono text-right", "{device.sent}" }
                                }
                            }
                        }
                    }
                }
            }
        }
        if let Some(report) = report {
            if !report.problems.is_empty() {
                section {
                    SectionLabel { label: "Problems", aside: "nothing after a problem is imported or written for its device" }
                    Card { class: "py-0 gap-0 border-bad",
                        CardHeader { class: "bg-bad text-bg py-2",
                            CardTitle { class: "font-mono text-[11px] tracking-[0.08em] uppercase font-medium", "✕ problems" }
                        }
                        CardContent { class: "p-0",
                            for problem in report.problems {
                                div { class: "px-5 py-3 border-b border-line flex flex-col gap-1",
                                    StatusMark { mark: Mark::Fail, word: problem.code.clone() }
                                    span { class: "font-mono text-[13px] break-all",
                                        "{problem.path}"
                                        if let Some(line) = problem.line {
                                            ":{line}"
                                        }
                                    }
                                    span { class: "text-[13px]", "{problem.message}" }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

#[component]
fn Stat(label: String, value: String) -> Element {
    rsx! {
        div { class: "p-5 flex flex-col gap-2 border-r border-line",
            span { class: "font-mono text-[11px] tracking-[0.08em] uppercase text-ink-muted", "{label}" }
            span { class: "font-mono text-[40px] leading-10 font-bold", "{value}" }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn devices_join_the_cursor_and_the_report() {
        let cursor = BTreeMap::from([("dev-b".to_owned(), 4), ("dev-c".to_owned(), 1)]);
        let report = SyncReport {
            received: BTreeMap::from([("dev-a".to_owned(), 2)]),
            sent: BTreeMap::from([("dev-b".to_owned(), 3)]),
            problems: Vec::new(),
        };
        let device = |device: &str, known, received, sent| Device {
            device: device.into(),
            known,
            received,
            sent,
        };
        assert_eq!(
            devices(&cursor, Some(&report)),
            [
                device("dev-a", 0, 2, 0),
                device("dev-b", 4, 0, 3),
                device("dev-c", 1, 0, 0)
            ]
        );
        assert_eq!(
            devices(&cursor, None),
            [device("dev-b", 4, 0, 0), device("dev-c", 1, 0, 0)]
        );
    }
}
