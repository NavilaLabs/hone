//! One module per feature. Each holds the view of its screen and the logic that turns the
//! command line's results into what the screen shows.

pub(crate) mod artifact;
mod artifact_form;
mod artifact_tab;
pub(crate) mod artifacts;
pub(crate) mod assistant;
mod attributes;
pub(crate) mod check;
pub(crate) mod code;
mod common;
pub(crate) mod configuration;
pub(crate) mod editor;
pub(crate) mod events;
pub(crate) mod explorer;
pub(crate) mod graph;
mod journal;
pub(crate) mod new_artifact;
pub(crate) mod new_ticket;
pub(crate) mod overview;
mod progress;
pub(crate) mod project_settings;
pub(crate) mod search;
pub(crate) mod server;
pub(crate) mod settings;
pub(crate) mod status;
pub(crate) mod sync;
pub(crate) mod tabs;
pub(crate) mod threads;
mod ticket_settings;
pub(crate) mod tickets;
pub(crate) mod tracker;

use std::path::PathBuf;

use dioxus::prelude::*;

use crate::hone::{
    Catalog, CheckRun, ContextAuthor, Failure, GateScope, Hone, Run, StoreLog, TicketEvents,
    TicketList, TicketOverview, TicketSummary, Writer,
};
use crate::ide::Ide;
use crate::settings::{ProjectSettings, Settings};
use crate::tracker::Tracker;
use crate::ui::Choice;

/// What the centre shows: a view outside a ticket, or the open ticket's workspace.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Screen {
    Tickets,
    Artifacts,
    Profiles,
    Plugins,
    Sync,
    Settings,
    /// The workspace of the open ticket: explorer, tabs, and assistant.
    Ticket,
}

/// A tab of a ticket's workspace.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum Tab {
    Overview,
    Journal,
    Artifact(String),
    /// A source file, read-only.
    Code(code::CodeTarget),
    /// The ticket's check run while the store holds none.
    Check,
    /// A stored check run, by its ID.
    Run(String),
    Graph,
    Events,
    Threads,
}

impl Tab {
    /// The label of the tab.
    pub(crate) fn title(&self) -> String {
        match self {
            Self::Overview => "Overview".into(),
            Self::Journal => "Journal".into(),
            Self::Artifact(id) => id.clone(),
            Self::Code(target) => {
                let file = target.path.rsplit('/').next().unwrap_or(&target.path);
                match &target.symbol {
                    Some(symbol) => format!("{file} # {symbol}"),
                    None => file.to_owned(),
                }
            }
            Self::Check => "Check run".into(),
            Self::Run(id) => format!("Check run {}", journal::clock(id)),
            Self::Graph => "Link graph".into(),
            Self::Events => "Event log".into(),
            Self::Threads => "Threads".into(),
        }
    }
}

/// The open tabs of one ticket. The Overview is always open and first.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TicketTabs {
    pub(crate) open: Vec<Tab>,
    pub(crate) active: Tab,
}

impl Default for TicketTabs {
    fn default() -> Self {
        Self {
            open: vec![Tab::Overview],
            active: Tab::Overview,
        }
    }
}

impl TicketTabs {
    /// Opens `tab`, or shows it when it is open already.
    pub(crate) fn show(&mut self, tab: Tab) {
        if !self.open.contains(&tab) {
            self.open.push(tab.clone());
        }
        self.active = tab;
    }

    /// Shows `new` in the place of `old`, or only shows `new` when `old` is not open.
    pub(crate) fn replace(&mut self, old: &Tab, new: Tab) {
        match self.open.iter().position(|open| open == old) {
            Some(index) if !self.open.contains(&new) => {
                self.open[index] = new.clone();
                self.active = new;
            }
            _ => self.show(new),
        }
    }

    /// Closes `tab`; the Overview stays. Closing the active tab shows its left neighbour.
    pub(crate) fn close(&mut self, tab: &Tab) {
        if *tab == Tab::Overview {
            return;
        }
        let Some(index) = self.open.iter().position(|open| open == tab) else {
            return;
        };
        self.open.remove(index);
        if self.active == *tab {
            self.active = self.open[index - 1].clone();
        }
    }
}

/// The unsaved text of one artifact, kept while its tab is not shown.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Draft {
    pub(crate) path: String,
    pub(crate) text: String,
    /// The fields as the form set them; `None` after an edit of the source, which the GUI
    /// cannot read back into fields without parsing YAML.
    pub(crate) data: Option<serde_json::Map<String, serde_json::Value>>,
}

/// The state every screen shares: the project, the writer, the open screen and ticket, and a
/// counter that a write bumps, so that every screen reads the store again.
#[derive(Clone, Copy)]
pub(crate) struct Session {
    pub(crate) hone: Signal<Hone>,
    pub(crate) writer: Signal<Writer>,
    pub(crate) screen: Signal<Screen>,
    pub(crate) ticket: Signal<Option<String>>,
    pub(crate) thread: Signal<Option<String>>,
    pub(crate) revision: Signal<u64>,
    /// The artifact open on the Artifacts screen.
    pub(crate) artifact: Signal<Option<String>>,
    /// The node the link graph highlights.
    pub(crate) graph_focus: Signal<Option<String>>,
    pub(crate) search_open: Signal<bool>,
    /// The tabs of every ticket opened in this window.
    pub(crate) tabs: Signal<std::collections::BTreeMap<String, TicketTabs>>,
    pub(crate) explorer_open: Signal<bool>,
    pub(crate) assistant_open: Signal<bool>,
    /// When this window last synced, `HH:MM` UTC.
    pub(crate) last_sync: Signal<Option<String>>,
    /// The ticket the New ticket dialog starts, `None` while it is closed.
    pub(crate) new_ticket: Signal<Option<new_ticket::Seed>>,
    /// Whether the New artifact dialog is open.
    pub(crate) new_artifact: Signal<bool>,
    /// The unsaved artifacts of this window, by ID.
    pub(crate) drafts: Signal<std::collections::BTreeMap<String, Draft>>,
    /// The tab with unsaved changes that asks before it closes.
    pub(crate) closing: Signal<Option<Tab>>,
}

impl Session {
    pub(crate) fn new(root: PathBuf) -> Self {
        let writer = Writer {
            author: std::env::var("HONE_AUTHOR").unwrap_or_default(),
            role: std::env::var("HONE_ROLE").unwrap_or_default(),
        };
        Self {
            hone: Signal::new(Hone::new(root)),
            writer: Signal::new(writer),
            screen: Signal::new(Screen::Tickets),
            ticket: Signal::new(None),
            thread: Signal::new(None),
            revision: Signal::new(0),
            artifact: Signal::new(None),
            graph_focus: Signal::new(None),
            search_open: Signal::new(false),
            tabs: Signal::new(std::collections::BTreeMap::new()),
            explorer_open: Signal::new(true),
            assistant_open: Signal::new(true),
            last_sync: Signal::new(None),
            new_ticket: Signal::new(None),
            new_artifact: Signal::new(false),
            drafts: Signal::new(std::collections::BTreeMap::new()),
            closing: Signal::new(None),
        }
    }

    pub(crate) fn changed(mut self) {
        self.revision += 1;
    }

    /// Opens the workspace of `ticket` with the tabs it had.
    pub(crate) fn open_ticket(mut self, ticket: String) {
        if (self.ticket)().as_deref() != Some(ticket.as_str()) {
            self.thread.set(None);
        }
        self.tabs.write().entry(ticket.clone()).or_default();
        self.ticket.set(Some(ticket));
        self.screen.set(Screen::Ticket);
    }

    /// Shows `tab` in the open ticket's workspace.
    pub(crate) fn open_tab(mut self, tab: Tab) {
        let Some(ticket) = (self.ticket)() else {
            return;
        };
        self.tabs.write().entry(ticket).or_default().show(tab);
        self.screen.set(Screen::Ticket);
    }

    /// The tabs of the open ticket.
    pub(crate) fn ticket_tabs(&self) -> TicketTabs {
        (self.ticket)()
            .and_then(|ticket| self.tabs.read().get(&ticket).cloned())
            .unwrap_or_default()
    }

    /// Opens an artifact: as a tab while a ticket is open, otherwise on the Artifacts view.
    pub(crate) fn open_artifact(mut self, id: String) {
        if (self.ticket)().is_some() {
            self.open_tab(Tab::Artifact(id));
        } else {
            self.artifact.set(Some(id));
            self.screen.set(Screen::Artifacts);
        }
    }

    pub(crate) fn open_thread(mut self, ticket: String, thread: String) {
        self.open_ticket(ticket);
        self.thread.set(Some(thread));
        self.open_tab(Tab::Threads);
    }
}

/// What several screens read from the project, read once per change: the artifacts, the event
/// log, the context's author, and the tracker setting.
#[derive(Clone, Copy)]
pub(crate) struct Project {
    pub(crate) catalog: Resource<Result<Catalog, Failure>>,
    pub(crate) log: Resource<Result<StoreLog, Failure>>,
    pub(crate) context: Resource<Result<ContextAuthor, Failure>>,
    /// The GUI's settings of the project in this checkout.
    pub(crate) settings: Resource<Result<ProjectSettings, String>>,
    /// The open ticket with the steps of its profile, `None` without an open ticket.
    pub(crate) overview: Resource<Option<Result<TicketOverview, Failure>>>,
    /// The process state of the open ticket, `None` without an open ticket.
    pub(crate) state: Resource<Option<Result<TicketEvents, Failure>>>,
    /// Every ticket with its progress, read again after the open ticket's check.
    pub(crate) tickets: Resource<Result<TicketList, Failure>>,
    /// The pure gates of the open ticket, run when it opens and after every write; the run is
    /// stored with the source `gui`.
    pub(crate) check: Resource<Option<Result<CheckRun, Failure>>>,
    /// The stored runs of the open ticket, newest first.
    pub(crate) runs: Resource<Option<Result<Vec<Run>, Failure>>>,
    /// Bumped when a setting changes.
    pub(crate) settings_revision: Signal<u64>,
}

impl Project {
    /// Reads the project of `session` and reads it again after every change.
    pub(crate) fn use_project(session: Session) -> Self {
        let catalog = use_resource(move || async move {
            let _ = (session.revision)();
            (session.hone)().artifacts().await
        });
        let log = use_resource(move || async move {
            let _ = (session.revision)();
            (session.hone)().log().await
        });
        let context = use_resource(move || async move {
            let _ = (session.revision)();
            (session.hone)().context().await
        });
        let overview = use_resource(move || async move {
            let _ = (session.revision)();
            let ticket = (session.ticket)()?;
            Some((session.hone)().ticket(&ticket).await)
        });
        let state = use_resource(move || async move {
            let _ = (session.revision)();
            let ticket = (session.ticket)()?;
            Some((session.hone)().events(&ticket).await)
        });
        let check = use_resource(move || async move {
            let _ = (session.revision)();
            let ticket = (session.ticket)()?;
            Some((session.hone)().check(Some(&ticket), GateScope::Pure).await)
        });
        let tickets = use_resource(move || async move {
            let _ = (session.revision)();
            // After the check, so that its stored run shows.
            let _ = check.read().is_some();
            (session.hone)().ticket_list().await
        });
        let runs = use_resource(move || async move {
            let _ = (session.revision)();
            let _ = check.read().is_some();
            let ticket = (session.ticket)()?;
            Some((session.hone)().runs(&ticket).await)
        });
        let settings_revision = use_signal(|| 0u64);
        let settings = use_resource(move || async move {
            let _ = settings_revision();
            let root = (session.hone)().root().to_path_buf();
            let project = match &*catalog.read() {
                Some(Ok(catalog)) => catalog.project.clone(),
                _ => return Ok(ProjectSettings::default()),
            };
            let settings = Settings::locate()
                .ok_or_else(|| "no configuration directory: set HONE_CONFIG_DIR".to_owned())?;
            settings.project(&project, &root)
        });
        Self {
            catalog,
            log,
            context,
            settings,
            overview,
            state,
            tickets,
            check,
            runs,
            settings_revision,
        }
    }

    /// The open ticket as `hone ticket list` gives it.
    /// The tab of the open ticket's newest stored run, or the Check run tab while there is
    /// none.
    pub(crate) fn newest_run(&self) -> Tab {
        match &*self.runs.read() {
            Some(Some(Ok(runs))) if !runs.is_empty() => Tab::Run(runs[0].id.clone()),
            _ => Tab::Check,
        }
    }

    pub(crate) fn summary(&self, ticket: &str) -> Option<TicketSummary> {
        match &*self.tickets.read() {
            Some(Ok(list)) => list
                .tickets
                .iter()
                .find(|summary| summary.ticket == ticket)
                .cloned(),
            _ => None,
        }
    }

    /// Who the window writes as: the chosen author, otherwise the context's.
    pub(crate) fn me(&self, writer: &Writer) -> Option<(String, String)> {
        if !writer.author.is_empty() {
            return Some((writer.author.clone(), writer.role.clone()));
        }
        match &*self.context.read() {
            Some(Ok(context)) => context
                .author
                .as_ref()
                .map(|author| (author.name.clone(), author.role.clone())),
            _ => None,
        }
    }

    /// The project ID, once the artifacts are read.
    pub(crate) fn project_id(&self) -> Option<String> {
        match &*self.catalog.read() {
            Some(Ok(catalog)) => Some(catalog.project.clone()),
            _ => None,
        }
    }

    /// The tracker of the project, if one is set.
    pub(crate) fn tracker(&self) -> Option<Tracker> {
        self.setting(|settings| settings.tracker.clone())
    }

    /// The editor of the project, if one is set.
    pub(crate) fn ide(&self) -> Option<Ide> {
        self.setting(|settings| settings.ide.clone())
    }

    /// The command line with the project's packages directory, for the commands that resolve.
    pub(crate) fn resolving(&self, hone: Hone) -> Hone {
        hone.with_packages(self.setting(|settings| settings.packages.clone()))
    }

    fn setting<T>(&self, pick: impl FnOnce(&ProjectSettings) -> Option<T>) -> Option<T> {
        match &*self.settings.read() {
            Some(Ok(settings)) => pick(settings),
            _ => None,
        }
    }

    /// The tickets to choose from: the ticket records, then tickets the store knows without a
    /// record, each with its external keys.
    pub(crate) fn ticket_choices(&self) -> Vec<Choice> {
        let mut choices: Vec<Choice> = Vec::new();
        if let Some(Ok(catalog)) = &*self.catalog.read() {
            for record in catalog.tickets() {
                let Some(id) = record.id.clone() else {
                    continue;
                };
                let external: Vec<String> = record
                    .external
                    .iter()
                    .map(|external| format!("{} {}", external.system, external.key))
                    .collect();
                choices.push(
                    Choice::plain(id)
                        .label(external.join(", "))
                        .note(record.profile.clone().unwrap_or_default()),
                );
            }
        }
        if let Some(Ok(log)) = &*self.log.read() {
            for ticket in log.events.iter().filter_map(|event| event.ticket.clone()) {
                if !choices.iter().any(|choice| choice.value == ticket) {
                    choices.push(Choice::plain(ticket).note("no record"));
                }
            }
        }
        choices
    }

    /// The roles of the open ticket's configuration.
    pub(crate) fn role_choices(&self) -> Vec<Choice> {
        match &*self.overview.read() {
            Some(Some(Ok(overview))) => overview.roles.iter().map(Choice::plain).collect(),
            _ => Vec::new(),
        }
    }

    /// The author handles to choose from: the context's author, then every author of the log.
    pub(crate) fn author_choices(&self) -> Vec<Choice> {
        let mut choices: Vec<Choice> = Vec::new();
        if let Some(Ok(context)) = &*self.context.read()
            && let Some(author) = &context.author
        {
            choices.push(
                Choice::plain(author.name.clone())
                    .label(author.role.clone())
                    .note(format!("context {}", context.context)),
            );
        }
        if let Some(Ok(log)) = &*self.log.read() {
            for (name, roles) in authors_of(log) {
                if !choices.iter().any(|choice| choice.value == name) {
                    choices.push(
                        Choice::plain(name)
                            .label(roles.join(", "))
                            .note("in the log"),
                    );
                }
            }
        }
        choices
    }

    /// The role `author` last wrote with: the context's role for the context's author,
    /// otherwise the role of the author's last event in the log.
    pub(crate) fn last_role(&self, author: &str) -> Option<String> {
        if let Some(Ok(context)) = &*self.context.read()
            && let Some(own) = &context.author
            && own.name == author
        {
            return Some(own.role.clone());
        }
        match &*self.log.read() {
            Some(Ok(log)) => log
                .events
                .iter()
                .rev()
                .filter_map(|event| event.author.as_ref())
                .find(|known| known.name == author)
                .map(|known| known.role.clone()),
            _ => None,
        }
    }
}

/// The authors of `log` in the order they first appear, each with the roles they wrote with.
fn authors_of(log: &StoreLog) -> Vec<(String, Vec<String>)> {
    let mut authors: Vec<(String, Vec<String>)> = Vec::new();
    for author in log.events.iter().filter_map(|event| event.author.as_ref()) {
        match authors.iter_mut().find(|(name, _)| name == &author.name) {
            Some((_, roles)) if !roles.contains(&author.role) => roles.push(author.role.clone()),
            Some(_) => {}
            None => authors.push((author.name.clone(), vec![author.role.clone()])),
        }
    }
    authors
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use serde_json::json;

    use super::*;
    use crate::hone::{Author, Event};

    fn event(name: &str, role: &str) -> Event {
        Event {
            id: "ev-1".into(),
            device: "dev-3k9a5h2t".into(),
            seq: 1,
            kind: "step.changed".into(),
            ticket: None,
            author: Some(Author {
                name: name.into(),
                role: role.into(),
            }),
            payload: json!({}),
            raw: json!({}),
        }
    }

    #[test]
    fn tabs_keep_the_overview_and_show_the_left_neighbour_after_a_close() {
        let mut tabs = TicketTabs::default();
        tabs.show(Tab::Artifact("dec-1".into()));
        tabs.show(Tab::Check);
        tabs.show(Tab::Artifact("dec-1".into()));
        assert_eq!(
            tabs.open,
            [Tab::Overview, Tab::Artifact("dec-1".into()), Tab::Check]
        );
        tabs.close(&Tab::Artifact("dec-1".into()));
        assert_eq!(tabs.active, Tab::Overview);
        tabs.close(&Tab::Overview);
        assert_eq!(tabs.open, [Tab::Overview, Tab::Check]);
        tabs.replace(&Tab::Check, Tab::Run("run-2".into()));
        assert_eq!(tabs.open, [Tab::Overview, Tab::Run("run-2".into())]);
        assert_eq!(tabs.active, Tab::Run("run-2".into()));
        tabs.show(Tab::Run("run-1".into()));
        tabs.replace(&Tab::Run("run-1".into()), Tab::Run("run-2".into()));
        assert_eq!(
            tabs.open,
            [
                Tab::Overview,
                Tab::Run("run-2".into()),
                Tab::Run("run-1".into())
            ]
        );
        assert_eq!(tabs.active, Tab::Run("run-2".into()));
    }

    #[test]
    fn authors_keep_their_first_appearance_and_collect_their_roles() {
        let log = StoreLog {
            cursor: BTreeMap::new(),
            device: None,
            events: vec![
                event("bob", "human"),
                event("alice", "architect"),
                event("bob", "human"),
                event("alice", "human"),
            ],
        };
        assert_eq!(
            authors_of(&log),
            [
                ("bob".to_owned(), vec!["human".to_owned()]),
                (
                    "alice".to_owned(),
                    vec!["architect".to_owned(), "human".to_owned()]
                ),
            ]
        );
    }
}
