//! The assistant panel (concept Section 12, "Assistant"): an agent of the person's choice over
//! the Agent Client Protocol, handed `hone mcp` with the assistant's own author. One agent runs
//! per window; every ticket has a chat of its own, resumed from `assistant.json` where the agent
//! can load it.

mod chat;
mod context;
mod setup;
mod store;

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;

use dioxus::core::spawn_forever;
use dioxus::prelude::*;

use self::chat::{Answer, Chat, Entry, diff_lines};
pub(crate) use self::context::{Chip, ChipKind};
use self::store::{ChatKey, ChatStore};
use super::common::ProblemAlert;
use super::explorer::reached_modules;
use super::{Project, Session};
use crate::acp::{
    Agent, AgentCommand, AuthMethod, Incoming, McpServer, PermissionOption, ToolCall, ToolContent,
    Update,
};
use crate::markdown::preview;
use crate::settings::{AssistantSettings, Settings};
use crate::ui::{Button, ButtonVariant, Choice, Combobox};

#[derive(Debug, Clone, PartialEq)]
enum Phase {
    Off,
    Starting,
    Ready,
    Failed(String),
    /// The agent asks for a login before it opens a session.
    Login {
        methods: Vec<AuthMethod>,
        message: String,
    },
}

#[derive(Debug, Clone, Default, PartialEq)]
struct TicketChat {
    session: Option<String>,
    chat: Chat,
    busy: bool,
    /// While `session/load` replays the conversation; only then does the person's text come
    /// from the agent, which may otherwise echo the prompt the panel already shows.
    replaying: bool,
}

/// What the person is writing to one ticket's chat. The default context (the ticket and its
/// current step) is computed when shown, so it follows the ticket.
#[derive(Debug, Clone, Default, PartialEq)]
struct Draft {
    text: String,
    added: Vec<Chip>,
    /// Default chips the person removed.
    removed: Vec<Chip>,
}

impl Draft {
    fn chips(&self, defaults: Vec<Chip>) -> Vec<Chip> {
        let mut chips: Vec<Chip> = defaults
            .into_iter()
            .filter(|chip| !self.removed.contains(chip))
            .collect();
        for chip in &self.added {
            if !chips.contains(chip) {
                chips.push(chip.clone());
            }
        }
        chips
    }
}

/// Where a ticket's chat runs: its key in `assistant.json`, the MCP server, and the working
/// directory.
#[derive(Clone, PartialEq)]
struct Place {
    key: ChatKey,
    mcp: McpServer,
}

/// The assistant of the window: the agent, its state, and the chats of the tickets.
#[derive(Clone, Copy)]
pub(crate) struct Assistant {
    settings: Signal<Result<Option<AssistantSettings>, String>>,
    setup_open: Signal<bool>,
    agent: Signal<Option<Arc<Agent>>>,
    phase: Signal<Phase>,
    /// Bumped when the agent is replaced, so that a task of the old one stops.
    generation: Signal<u64>,
    chats: Signal<BTreeMap<String, TicketChat>>,
    composers: Signal<BTreeMap<String, Draft>>,
}

fn read_settings() -> Result<Option<AssistantSettings>, String> {
    Settings::locate()
        .ok_or_else(|| "no configuration directory: set HONE_CONFIG_DIR".to_owned())?
        .assistant()
}

impl Assistant {
    /// The assistant of the window, with its settings read from `gui.json`. Dropping the
    /// window's scope stops the agent and answers its open permission requests as cancelled.
    pub(crate) fn use_assistant() -> Self {
        let assistant = Self {
            settings: use_signal(read_settings),
            setup_open: use_signal(|| false),
            agent: use_signal(|| None),
            phase: use_signal(|| Phase::Off),
            generation: use_signal(|| 0),
            chats: use_signal(BTreeMap::new),
            composers: use_signal(BTreeMap::new),
        };
        use_drop(move || {
            let mut agent = assistant.agent;
            agent.take();
        });
        assistant
    }

    /// Adds `chip` to the context of `ticket`'s next message.
    pub(crate) fn add(mut self, ticket: &str, chip: Chip) {
        let mut composers = self.composers.write();
        let composer = composers.entry(ticket.to_owned()).or_default();
        composer.removed.retain(|removed| *removed != chip);
        if !composer.added.contains(&chip) {
            composer.added.push(chip);
        }
    }

    /// Puts `text` into `ticket`'s message field without sending it.
    pub(crate) fn draft(mut self, ticket: &str, text: String) {
        self.composers
            .write()
            .entry(ticket.to_owned())
            .or_default()
            .text = text;
    }

    fn command(&self) -> Option<(AgentCommand, AssistantSettings)> {
        match &*self.settings.read() {
            Ok(Some(settings)) if !settings.argv.is_empty() => Some((
                AgentCommand {
                    argv: settings.argv.clone(),
                    env: settings.env.clone(),
                },
                settings.clone(),
            )),
            _ => None,
        }
    }

    fn configured(&self) -> bool {
        self.command().is_some()
    }

    fn close_setup(mut self) {
        self.setup_open.set(false);
    }

    /// Takes the saved settings: stops the agent, so that the next one starts with them.
    fn saved(mut self) {
        self.settings.set(read_settings());
        self.setup_open.set(false);
        self.restart();
    }

    fn restart(mut self) {
        self.agent.take();
        self.generation += 1;
        self.chats.write().clear();
        self.phase.set(Phase::Off);
    }

    fn start(mut self, cwd: PathBuf) {
        let Some((command, _)) = self.command() else {
            return;
        };
        self.phase.set(Phase::Starting);
        let generation = *self.generation.peek();
        spawn_forever(async move {
            let started = Agent::start(&command, &cwd).await;
            if *self.generation.peek() != generation {
                return;
            }
            match started {
                Ok((agent, mut incoming)) => {
                    self.agent.set(Some(Arc::new(agent)));
                    self.phase.set(Phase::Ready);
                    while let Some(message) = incoming.recv().await {
                        if *self.generation.peek() != generation {
                            return;
                        }
                        self.receive(message);
                    }
                }
                Err(error) => self.phase.set(Phase::Failed(error.message)),
            }
        });
    }

    fn receive(mut self, message: Incoming) {
        match message {
            Incoming::Update { session, update } => {
                self.with_chat(&session, |chat| {
                    if chat.replaying || !matches!(update, Update::UserText(_)) {
                        chat.chat.apply(update);
                    }
                });
            }
            Incoming::Permission(permission) => {
                let session = permission.session.clone();
                let key = permission.key;
                let known = self.with_chat(&session, |chat| chat.chat.ask(permission));
                if !known && let Some(agent) = self.agent.peek().clone() {
                    agent.answer(key, None);
                }
            }
            Incoming::Exited(message) => {
                self.agent.take();
                for chat in self.chats.write().values_mut() {
                    chat.busy = false;
                    chat.chat.cancel_open();
                }
                self.phase.set(Phase::Failed(message));
            }
        }
    }

    /// Changes the chat of `session`; `false` when no ticket's chat has it.
    fn with_chat(&mut self, session: &str, change: impl FnOnce(&mut TicketChat)) -> bool {
        let mut chats = self.chats.write();
        match chats
            .values_mut()
            .find(|chat| chat.session.as_deref() == Some(session))
        {
            Some(chat) => {
                change(chat);
                true
            }
            None => false,
        }
    }

    /// Opens `ticket`'s chat once per agent: resumed with `session/load` when `assistant.json`
    /// keeps one, the agent can load it, and its command is unchanged.
    fn open_chat(mut self, ticket: &str, place: Place) {
        if self.chats.peek().contains_key(ticket) {
            return;
        }
        let (Some(agent), Some((command, _))) = (self.agent.peek().clone(), self.command()) else {
            return;
        };
        let mut chat = TicketChat::default();
        let saved = ChatStore::locate()
            .map(|store| store.find(&place.key))
            .unwrap_or(Ok(None));
        let resumable = match saved {
            Ok(Some((session, used))) if used == command && agent.info().load_session => {
                Some(session)
            }
            Ok(Some(_)) => {
                chat.chat.entries.push(Entry::Note(
                    "The agent cannot resume the earlier chat of this ticket, so this is a new chat."
                        .into(),
                ));
                None
            }
            Ok(None) => None,
            Err(message) => {
                chat.chat.entries.push(Entry::Note(message));
                None
            }
        };
        chat.session.clone_from(&resumable);
        chat.busy = resumable.is_some();
        chat.replaying = resumable.is_some();
        self.chats.write().insert(ticket.to_owned(), chat);
        let Some(session) = resumable else {
            return;
        };
        let ticket = ticket.to_owned();
        let generation = *self.generation.peek();
        spawn_forever(async move {
            let loaded = agent
                .load_session(&session, &place.key.root, &place.mcp)
                .await;
            if *self.generation.peek() != generation {
                return;
            }
            if let Some(chat) = self.chats.write().get_mut(&ticket) {
                chat.busy = false;
                chat.replaying = false;
                if let Err(error) = loaded {
                    chat.session = None;
                    chat.chat.entries.push(Entry::Note(format!(
                        "The earlier chat cannot be resumed ({error}), so this is a new chat."
                    )));
                }
            }
        });
    }

    fn send(mut self, ticket: String, place: Place, chips: Vec<Chip>) {
        let (Some(agent), Some((command, _))) = (self.agent.peek().clone(), self.command()) else {
            return;
        };
        let text = self
            .composers
            .peek()
            .get(&ticket)
            .map(|composer| composer.text.trim().to_owned())
            .unwrap_or_default();
        if text.is_empty() {
            return;
        }
        let prompt = context::prompt(&text, &chips);
        let session = {
            let mut chats = self.chats.write();
            let chat = chats.entry(ticket.clone()).or_default();
            if chat.busy {
                return;
            }
            chat.busy = true;
            chat.chat.entries.push(Entry::User {
                text: text.clone(),
                chips: chips.iter().map(Chip::label).collect(),
            });
            chat.session.clone()
        };
        self.composers.write().remove(&ticket);
        let generation = *self.generation.peek();
        spawn_forever(async move {
            let session = match session {
                Some(session) => Ok(session),
                None => agent.new_session(&place.key.root, &place.mcp).await,
            };
            if *self.generation.peek() != generation {
                return;
            }
            let session =
                match session {
                    Ok(session) => session,
                    Err(error) => {
                        if error.needs_login() {
                            self.phase.set(Phase::Login {
                                methods: agent.info().auth_methods.clone(),
                                message: error.message.clone(),
                            });
                        }
                        self.draft(&ticket, text);
                        self.finish(&ticket, Some(if error.needs_login() {
                        "The agent asks for a login. Log in above, then send the message again."
                            .to_owned()
                    } else {
                        format!("No chat could be opened: {error}")
                    }));
                        return;
                    }
                };
            if let Some(chat) = self.chats.write().get_mut(&ticket) {
                chat.session = Some(session.clone());
            }
            if let Some(Err(message)) =
                ChatStore::locate().map(|store| store.save(&place.key, &session, &command))
            {
                self.note(
                    &ticket,
                    format!("The chat is not kept for later: {message}"),
                );
            }
            let stopped = agent.prompt(&session, &prompt).await;
            if *self.generation.peek() != generation {
                return;
            }
            self.finish(
                &ticket,
                match stopped {
                    Ok(reason) => stop_note(&reason),
                    Err(error) => Some(format!("The agent failed: {error}")),
                },
            );
        });
    }

    fn finish(mut self, ticket: &str, note: Option<String>) {
        if let Some(chat) = self.chats.write().get_mut(ticket) {
            chat.busy = false;
            if let Some(note) = note {
                chat.chat.entries.push(Entry::Note(note));
            }
        }
    }

    fn note(mut self, ticket: &str, note: String) {
        if let Some(chat) = self.chats.write().get_mut(ticket) {
            chat.chat.entries.push(Entry::Note(note));
        }
    }

    /// Stops the turn of `ticket`'s chat; its open permission requests end as cancelled.
    fn stop(mut self, ticket: &str) {
        let session = self
            .chats
            .peek()
            .get(ticket)
            .and_then(|chat| chat.session.clone());
        if let (Some(agent), Some(session)) = (self.agent.peek().clone(), session) {
            agent.cancel(&session);
        }
        if let Some(chat) = self.chats.write().get_mut(ticket) {
            chat.chat.cancel_open();
        }
    }

    /// Starts a new chat for `ticket`, cancelling the turn of the old one.
    fn new_chat(mut self, ticket: &str, key: Option<ChatKey>) {
        let busy = self
            .chats
            .peek()
            .get(ticket)
            .is_some_and(|chat| chat.busy || !chat.chat.open_permissions().is_empty());
        if busy {
            self.stop(ticket);
        }
        self.chats
            .write()
            .insert(ticket.to_owned(), TicketChat::default());
        if let (Some(store), Some(key)) = (ChatStore::locate(), key)
            && let Err(message) = store.forget(&key)
        {
            self.note(ticket, message);
        }
    }

    fn answer(mut self, ticket: &str, key: u64, option: &str) {
        if let Some(agent) = self.agent.peek().clone() {
            agent.answer(key, Some(option));
        }
        if let Some(chat) = self.chats.write().get_mut(ticket) {
            chat.chat.answered(key, Some(option));
        }
    }

    fn login(mut self, method: String) {
        let Some(agent) = self.agent.peek().clone() else {
            return;
        };
        let methods = agent.info().auth_methods.clone();
        let generation = *self.generation.peek();
        spawn_forever(async move {
            let result = agent.authenticate(&method).await;
            if *self.generation.peek() != generation {
                return;
            }
            self.phase.set(match result {
                Ok(()) => Phase::Ready,
                Err(error) => Phase::Login {
                    methods,
                    message: error.message,
                },
            });
        });
    }
}

fn stop_note(reason: &str) -> Option<String> {
    match reason {
        "end_turn" => None,
        "cancelled" => Some("Stopped.".into()),
        "max_tokens" => Some("The agent stopped at its token limit.".into()),
        "max_turn_requests" => Some("The agent stopped at its limit of requests in a turn.".into()),
        "refusal" => Some("The agent refused to go on.".into()),
        other => Some(format!("The agent stopped: {other}.")),
    }
}

/// The place of the open ticket's chat, once the project is read.
fn place(
    session: Session,
    project: &Project,
    settings: &AssistantSettings,
) -> Result<Place, String> {
    let ticket = (session.ticket)().ok_or("no ticket is open")?;
    let id = project
        .project_id()
        .ok_or("the project cannot be read yet")?;
    let hone = project.resolving((session.hone)());
    let key = ChatKey::new(id, hone.root(), ticket)?;
    let mcp = hone
        .mcp_server(&settings.author, &settings.role)
        .map_err(|failure| failure.message)?;
    Ok(Place { key, mcp })
}

/// The panel at the right edge of a ticket's workspace.
#[component]
pub(crate) fn AssistantPanel() -> Element {
    let session = use_context::<Session>();
    let project = use_context::<Project>();
    let assistant = use_context::<Assistant>();
    let mut open = session.assistant_open;
    let mut setup_open = assistant.setup_open;
    let ticket = (session.ticket)().unwrap_or_default();
    let phase = (assistant.phase)();
    let settings = assistant.settings.read().clone();
    let command = assistant.command();
    let place = command
        .as_ref()
        .map(|(_, settings)| place(session, &project, settings));
    let cwd = (session.hone)().root().to_path_buf();
    use_effect(move || {
        if (assistant.phase)() == Phase::Off && assistant.configured() && !(assistant.setup_open)()
        {
            assistant.start(cwd.clone());
        }
    });
    let effect_place = place.clone();
    let effect_ticket = ticket.clone();
    use_effect(use_reactive!(|(effect_ticket, effect_place)| {
        if (assistant.phase)() == Phase::Ready
            && let Some(Ok(place)) = effect_place
        {
            assistant.open_chat(&effect_ticket, place);
        }
    }));
    // The newest entry stays in view while the agent writes.
    use_effect(move || {
        let _ = assistant.chats.read();
        document::eval(
            "const log = document.getElementById('assistant-log'); if (log) { log.scrollTop = log.scrollHeight; }",
        );
    });
    let showing_setup = settings.is_ok() && (command.is_none() || setup_open());
    let chat = assistant
        .chats
        .read()
        .get(&ticket)
        .cloned()
        .unwrap_or_default();
    let key = place.clone().and_then(Result::ok).map(|place| place.key);
    let read_only = command
        .as_ref()
        .and_then(|(_, settings)| setup::read_only_note(&settings.role, &project.role_choices()));
    rsx! {
        aside { class: "w-[360px] shrink-0 border-l border-line flex flex-col min-h-0", "aria-label": "Assistant",
            div { class: "h-12 shrink-0 pl-4 pr-2 border-b border-line flex items-center gap-2",
                span { class: "font-mono text-[11px] font-semibold tracking-[0.08em] uppercase", "Assistant" }
                span { class: "font-mono text-[11px] text-ink-muted whitespace-nowrap", "via hone mcp" }
                span { class: "ml-auto" }
                if command.is_some() && !showing_setup {
                    Button { variant: ButtonVariant::Ghost, class: "h-8 px-2 text-[12px]",
                        title: "Choose the agent",
                        onclick: move |_| setup_open.set(true),
                        "Agent"
                    }
                    Button { variant: ButtonVariant::Outline, class: "h-8 px-2 text-[12px]",
                        onclick: {
                            let ticket = ticket.clone();
                            let key = key.clone();
                            move |_| assistant.new_chat(&ticket, key.clone())
                        },
                        "New chat"
                    }
                }
                Button { variant: ButtonVariant::Outline, class: "h-8 w-8 px-0",
                    title: "Close the assistant",
                    onclick: move |_| open.set(false),
                    "×"
                }
            }
            div { id: "assistant-log", class: "flex-1 min-h-0 overflow-y-auto p-4 flex flex-col gap-4 text-[13.5px]",
                if let Err(message) = settings.clone() {
                    ProblemAlert { title: "The assistant's settings cannot be read", message }
                } else if showing_setup {
                    setup::Setup { current: settings.clone().ok().flatten() }
                } else {
                    if let Some(note) = read_only {
                        p { class: "m-0 text-[13px] text-warn", "{note}" }
                    }
                    if chat.chat.entries.is_empty() && phase == Phase::Ready {
                        p { class: "m-0 text-ink-muted",
                            "Ask about this ticket. The agent reads it through hone mcp and writes only through the same commands and gates as you."
                        }
                    }
                    for (index, entry) in chat.chat.entries.iter().cloned().enumerate() {
                        EntryView { key: "{index}", ticket: ticket.clone(), entry }
                    }
                    if chat.busy {
                        p { class: "m-0 font-mono text-[11px] text-ink-muted", role: "status", "working …" }
                    }
                    match phase.clone() {
                        Phase::Off | Phase::Starting => rsx! {
                            p { class: "m-0 font-mono text-[12px] text-ink-muted", role: "status",
                                "Starting {command.as_ref().map(|(command, _)| command.argv.join(\" \")).unwrap_or_default()} …"
                            }
                        },
                        Phase::Ready => rsx! {},
                        Phase::Failed(message) => rsx! {
                            ProblemAlert { title: "The agent is not running", message }
                            Button { variant: ButtonVariant::Outline, class: "self-start",
                                onclick: move |_| assistant.restart(),
                                "Start again"
                            }
                        },
                        Phase::Login { methods, message } => rsx! {
                            div { class: "border-2 border-warn p-3 flex flex-col gap-2",
                                span { class: "font-semibold", "The agent asks for a login" }
                                span { class: "text-[13px] text-ink-muted", "{message}" }
                                if methods.is_empty() {
                                    span { class: "text-[13px]", "The agent names no way to log in here; log in with the agent's own tool." }
                                }
                                for method in methods {
                                    Button { variant: ButtonVariant::Outline, class: "self-start",
                                        title: method.description.clone().unwrap_or_default(),
                                        onclick: {
                                            let id = method.id.clone();
                                            move |_| assistant.login(id.clone())
                                        },
                                        "{method.name}"
                                    }
                                }
                            }
                        },
                    }
                    if let Some(Err(message)) = place.clone() {
                        p { class: "m-0 text-[13px] text-ink-muted", "{message}" }
                    }
                }
            }
            if command.is_some() && !showing_setup {
                Composer { ticket: ticket.clone(), busy: chat.busy, ready: phase == Phase::Ready && matches!(place, Some(Ok(_))) }
            }
        }
    }
}

#[component]
fn Composer(ticket: String, busy: bool, ready: bool) -> Element {
    let session = use_context::<Session>();
    let project = use_context::<Project>();
    let mut assistant = use_context::<Assistant>();
    let composer = assistant
        .composers
        .read()
        .get(&ticket)
        .cloned()
        .unwrap_or_default();
    let summary = project.summary(&ticket);
    let mut defaults = vec![Chip::new(ChipKind::Ticket, ticket.clone())];
    if let Some(step) = summary.as_ref().and_then(|summary| summary.current.clone()) {
        defaults.push(Chip::new(ChipKind::Step, step));
    }
    let chips = composer.chips(defaults.clone());
    let choices: Vec<Choice> = context_choices(&project, &ticket)
        .into_iter()
        .filter(|chip| !chips.contains(chip))
        .map(|chip| Choice::plain(chip.text()))
        .collect();
    let author = assistant
        .command()
        .map(|(_, settings)| format!("{} · {}", settings.author, settings.role))
        .unwrap_or_default();
    let send = {
        let ticket = ticket.clone();
        let chips = chips.clone();
        move || {
            let Some(Ok(place)) = assistant
                .command()
                .map(|(_, settings)| place(session, &project, &settings))
            else {
                return;
            };
            assistant.send(ticket.clone(), place, chips.clone());
        }
    };
    let send_key = send.clone();
    let send_click = send;
    let text = composer.text.clone();
    rsx! {
        div { class: "shrink-0 border-t border-line p-4 flex flex-col gap-3",
            div { class: "flex flex-wrap items-center gap-2 font-mono text-[12px]",
                for chip in chips.clone() {
                    span { key: "{chip.text()}", class: "h-6 pl-2 inline-flex items-center border border-line",
                        "{chip.label()}"
                        button {
                            class: "h-6 w-6 text-ink-muted hover:text-ink",
                            title: "Remove from the context",
                            onclick: {
                                let ticket = ticket.clone();
                                let defaults = defaults.clone();
                                move |_| {
                                    let mut composers = assistant.composers.write();
                                    let composer = composers.entry(ticket.clone()).or_default();
                                    composer.added.retain(|added| *added != chip);
                                    if defaults.contains(&chip) && !composer.removed.contains(&chip) {
                                        composer.removed.push(chip.clone());
                                    }
                                }
                            },
                            "×"
                        }
                    }
                }
                Combobox {
                    value: "",
                    choices,
                    class: "w-40 h-6 text-[12px]",
                    placeholder: "+ context",
                    label: "Add context",
                    empty: "Nothing more to add",
                    onselect: {
                        let ticket = ticket.clone();
                        move |text: String| {
                            if let Some(chip) = Chip::parse(&text) {
                                assistant.add(&ticket, chip);
                            }
                        }
                    },
                }
            }
            textarea {
                class: "h-24 p-3 border border-line bg-surface text-ink text-[13.5px] resize-y",
                placeholder: "Ask about this ticket",
                "aria-label": "Message to the assistant",
                value: "{text}",
                oninput: {
                    let ticket = ticket.clone();
                    move |event: FormEvent| assistant.draft(&ticket, event.value())
                },
                onkeydown: move |event: KeyboardEvent| {
                    if event.key() == Key::Enter && event.modifiers().ctrl() && ready && !busy {
                        event.prevent_default();
                        send_key();
                    }
                },
            }
            div { class: "flex items-center gap-3",
                span { class: "font-mono text-[11px] text-ink-muted flex-1", title: "The author and role the agent writes as",
                    "as {author} · writes only via gates"
                }
                if busy {
                    Button { variant: ButtonVariant::Outline,
                        onclick: {
                            let ticket = ticket.clone();
                            move |_| assistant.stop(&ticket)
                        },
                        "Stop"
                    }
                } else {
                    Button { variant: ButtonVariant::Outline,
                        disabled: !ready || text.trim().is_empty(),
                        title: "Send (Ctrl+Enter)",
                        onclick: move |_| send_click(),
                        "Send"
                    }
                }
            }
        }
    }
}

/// What the explorer would offer as context for `ticket`: its steps, artifacts, the modules
/// they reach, open threads, and stored runs.
fn context_choices(project: &Project, ticket: &str) -> Vec<Chip> {
    let mut chips = Vec::new();
    if let Some(summary) = project.summary(ticket) {
        chips.extend(
            summary
                .steps
                .iter()
                .map(|step| Chip::new(ChipKind::Step, step.step.clone())),
        );
    }
    if let Some(Ok(catalog)) = &*project.catalog.read() {
        let own: Vec<_> = catalog
            .artifacts
            .iter()
            .filter(|entry| entry.kind != "ticket" && entry.ticket.as_deref() == Some(ticket))
            .collect();
        let modules = reached_modules(&own, &catalog.artifacts);
        for entry in own.into_iter().chain(modules) {
            if let Some(id) = &entry.id {
                chips.push(Chip::new(ChipKind::Artifact, id.clone()));
            }
        }
    }
    if let Some(Some(Ok(state))) = &*project.state.read() {
        chips.extend(
            state
                .threads
                .iter()
                .filter(|thread| thread.status == "open")
                .map(|thread| Chip::new(ChipKind::Thread, thread.thread.clone())),
        );
    }
    if let Some(Some(Ok(runs))) = &*project.runs.read() {
        chips.extend(
            runs.iter()
                .map(|run| Chip::new(ChipKind::Run, run.id.clone())),
        );
    }
    chips
}

#[component]
fn EntryView(ticket: String, entry: Entry) -> Element {
    let session = use_context::<Session>();
    let project = use_context::<Project>();
    let assistant = use_context::<Assistant>();
    match entry {
        Entry::User { text, chips } => {
            let me = project
                .me(&(session.writer)())
                .map_or_else(|| "you".to_owned(), |(name, _)| name);
            rsx! {
                div { class: "p-3 bg-surface-2 flex flex-col gap-1",
                    span { class: "font-mono text-[11px] text-ink-muted", "{me}" }
                    span { class: "whitespace-pre-wrap break-words", "{text}" }
                    if !chips.is_empty() {
                        span { class: "font-mono text-[11px] text-ink-muted break-all", {chips.join(" · ")} }
                    }
                }
            }
        }
        Entry::Agent(markdown) => rsx! {
            div { class: "hone-prose break-words", dangerous_inner_html: preview(&markdown) }
        },
        Entry::Thought(text) => rsx! {
            details { class: "text-[12.5px] text-ink-muted",
                summary { class: "font-mono text-[11px] cursor-pointer", "thinking" }
                p { class: "m-0 mt-1 whitespace-pre-wrap break-words", "{text}" }
            }
        },
        Entry::Tool(call) => rsx! { ToolView { call } },
        Entry::Permission {
            key,
            tool,
            options,
            answer,
        } => {
            let open = answer.is_none();
            rsx! {
            div { class: if open { "border-2 border-signal p-3 flex flex-col gap-2" } else { "border border-line p-3 flex flex-col gap-2" },
                span { class: "font-mono text-[11px] text-ink-muted", "permission · {tool.title.clone().unwrap_or_default()}" }
                if let Some(input) = &tool.input {
                    span { class: "font-mono text-[11.5px] break-all", {input.to_string()} }
                }
                match answer {
                    None => rsx! {
                        div { class: "flex flex-wrap gap-2",
                            for option in options {
                                PermissionButton { ticket: ticket.clone(), permission: key, option }
                            }
                        }
                    },
                    Some(Answer::Chosen(name)) => rsx! { span { class: "font-mono text-[12px]", "→ {name}" } },
                    Some(Answer::Cancelled) => rsx! { span { class: "font-mono text-[12px] text-ink-muted", "→ cancelled" } },
                }
            }
            }
        }
        Entry::Note(text) => {
            let _ = assistant;
            rsx! {
                p { class: "m-0 font-mono text-[11.5px] text-ink-muted whitespace-pre-wrap break-words", "{text}" }
            }
        }
    }
}

#[component]
fn PermissionButton(ticket: String, permission: u64, option: PermissionOption) -> Element {
    let assistant = use_context::<Assistant>();
    let variant = if option.kind.starts_with("allow") {
        ButtonVariant::Outline
    } else {
        ButtonVariant::Ghost
    };
    let id = option.id.clone();
    rsx! {
        Button { variant, class: "h-8 px-3 text-[12px]",
            onclick: move |_| assistant.answer(&ticket, permission, &id),
            "{option.name}"
        }
    }
}

#[component]
fn ToolView(call: ToolCall) -> Element {
    let status = call.status.clone().unwrap_or_else(|| "pending".to_owned());
    let (glyph, class) = match status.as_str() {
        "completed" => ("■", "text-ok"),
        "failed" => ("✕", "text-bad"),
        _ => ("…", "text-ink-muted"),
    };
    let content = call.content.clone().unwrap_or_default();
    rsx! {
        div { class: "border border-line p-2 flex flex-col gap-1 font-mono text-[11.5px] min-w-0",
            span { class: "text-ink-muted break-all",
                "tool · {call.title.clone().unwrap_or_default()}"
                if let Some(kind) = &call.kind {
                    " · {kind}"
                }
            }
            if let Some(input) = &call.input {
                span { class: "break-all", {input.to_string()} }
            }
            span { class, "{glyph} {status}" }
            for (index, item) in content.into_iter().enumerate() {
                match item {
                    ToolContent::Text(text) => rsx! {
                        pre { key: "{index}", class: "m-0 p-2 bg-surface whitespace-pre-wrap break-words max-h-60 overflow-auto", "{text}" }
                    },
                    ToolContent::Diff { path, old, new } => rsx! {
                        DiffView { key: "{index}", path, old, new }
                    },
                }
            }
            if let Some(output) = &call.output {
                details {
                    summary { class: "cursor-pointer text-ink-muted", "result" }
                    pre { class: "m-0 p-2 bg-surface whitespace-pre-wrap break-all max-h-60 overflow-auto",
                        {serde_json::to_string_pretty(output).unwrap_or_default()}
                    }
                }
            }
        }
    }
}

#[component]
fn DiffView(path: String, old: Option<String>, new: String) -> Element {
    let lines: Vec<(char, String)> = diff_lines(old.as_deref(), &new)
        .into_iter()
        .map(|(mark, line)| (mark, line.to_owned()))
        .collect();
    rsx! {
        div { class: "flex flex-col",
            span { class: "text-ink-muted break-all", "{path}" }
            pre { class: "m-0 p-2 bg-surface border border-line whitespace-pre-wrap break-words max-h-80 overflow-auto",
                for (index, (mark, line)) in lines.into_iter().enumerate() {
                    span {
                        key: "{index}",
                        class: match mark {
                            '-' => "block text-bad",
                            '+' => "block text-ok",
                            _ => "block text-ink-muted",
                        },
                        "{mark} {line}"
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_context_follows_the_defaults_without_the_removed_ones() {
        let ticket = Chip::new(ChipKind::Ticket, "tk-9f2x4a8m");
        let step = Chip::new(ChipKind::Step, "interfaces");
        let run = Chip::new(ChipKind::Run, "run-1");
        let composer = Draft {
            text: String::new(),
            added: vec![run.clone(), ticket.clone()],
            removed: vec![step.clone()],
        };
        assert_eq!(
            composer.chips(vec![ticket.clone(), step]),
            [ticket.clone(), run.clone()]
        );
        let next = Chip::new(ChipKind::Step, "tests");
        assert_eq!(
            composer.chips(vec![ticket.clone(), next.clone()]),
            [ticket, next, run]
        );
    }

    #[test]
    fn a_stop_reason_other_than_the_end_of_the_turn_is_noted() {
        assert_eq!(stop_note("end_turn"), None);
        assert_eq!(stop_note("cancelled").as_deref(), Some("Stopped."));
        assert_eq!(
            stop_note("x-paused").as_deref(),
            Some("The agent stopped: x-paused.")
        );
    }
}
