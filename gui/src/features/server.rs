use std::path::PathBuf;

use dioxus::prelude::*;
use tokio::sync::mpsc::{UnboundedReceiver, unbounded_channel};

use super::Session;
use super::common::{Empty, FailureAlert, Field, Mark, PageHeader, SectionLabel, StatusMark};
use crate::hone::{Failure, Hone, ServerProcess};
use crate::ui::{Button, ButtonVariant, Input};

/// The most output lines the screen keeps.
const KEPT_LINES: usize = 200;

enum Control {
    Start(Hone, PathBuf),
    Stop,
}

#[derive(Clone, Copy)]
struct ServerState {
    running: Signal<bool>,
    output: Signal<Vec<String>>,
    failure: Signal<Option<Failure>>,
}

/// Starts and stops `hone serve --config <file>` and shows what it writes.
#[component]
pub(crate) fn Server() -> Element {
    let session = use_context::<Session>();
    let state = ServerState {
        running: use_signal(|| false),
        output: use_signal(Vec::new),
        failure: use_signal(|| None),
    };
    let control = use_hook(move || {
        let (sender, receiver) = unbounded_channel();
        spawn(supervise(receiver, state));
        sender
    });
    let mut config = use_signal(String::new);
    let start = {
        let control = control.clone();
        move |_| {
            let hone = (session.hone)();
            let _ = control.send(Control::Start(hone, PathBuf::from(config().trim())));
        }
    };
    let stop = move |_| {
        let _ = control.send(Control::Stop);
    };
    let running = (state.running)();
    rsx! {
        PageHeader {
            context: "reference server",
            title: "hone serve",
            actions: rsx! {
                if running {
                    Button { variant: ButtonVariant::Outline, onclick: stop, "Stop server" }
                } else {
                    Button { onclick: start, "Start server" }
                }
            },
        }
        Field { label: "Server configuration (YAML: listen, data, identities, projects)",
            Input { class: "font-mono max-w-xl", value: config(), placeholder: "/etc/hone/server.yaml", oninput: move |value| config.set(value) }
        }
        div {
            if running {
                StatusMark { mark: Mark::Done, word: "running" }
            } else {
                StatusMark { mark: Mark::Open, word: "stopped" }
            }
        }
        if let Some(failure) = (state.failure)() {
            FailureAlert { failure }
        }
        section {
            SectionLabel { label: "Output" }
            if (state.output)().is_empty() {
                Empty { text: "Nothing yet. The server's messages appear here once it is started." }
            } else {
                pre { class: "m-0 bg-surface border border-line p-4 font-mono text-[12.5px] leading-[1.7] whitespace-pre-wrap break-all max-h-[480px] overflow-auto",
                    {(state.output)().join("\n")}
                }
            }
        }
    }
}

enum Happened {
    Command(Option<Control>),
    Line(Option<String>),
}

// The server outlives renders of the screen, so one task owns it and takes commands.
async fn supervise(mut control: UnboundedReceiver<Control>, mut state: ServerState) {
    let mut server: Option<ServerProcess> = None;
    loop {
        let happened = match server.as_mut() {
            Some(process) => tokio::select! {
                command = control.recv() => Happened::Command(command),
                line = process.next_line() => Happened::Line(line),
            },
            None => Happened::Command(control.recv().await),
        };
        match happened {
            Happened::Command(None) => break,
            Happened::Command(Some(Control::Start(hone, config))) => {
                if let Some(mut old) = server.take() {
                    old.stop().await;
                }
                state.output.set(Vec::new());
                match hone.serve(&config) {
                    Ok(process) => {
                        server = Some(process);
                        state.failure.set(None);
                        state.running.set(true);
                    }
                    Err(failure) => state.failure.set(Some(failure)),
                }
            }
            Happened::Command(Some(Control::Stop)) => {
                if let Some(mut process) = server.take() {
                    process.stop().await;
                }
                state.running.set(false);
            }
            Happened::Line(Some(line)) => state.output.with_mut(|output| {
                output.push(line);
                let excess = output.len().saturating_sub(KEPT_LINES);
                output.drain(..excess);
            }),
            Happened::Line(None) => {
                server = None;
                state.running.set(false);
            }
        }
    }
}
