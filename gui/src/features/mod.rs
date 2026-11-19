//! One module per feature. Each holds the view of its screen and the logic that turns the
//! command line's results into what the screen shows.

pub(crate) mod artifacts;
pub(crate) mod check;
mod common;
pub(crate) mod events;
mod progress;
pub(crate) mod server;
pub(crate) mod settings;
pub(crate) mod sync;
pub(crate) mod threads;
pub(crate) mod tickets;
pub(crate) mod tracker;

use std::path::PathBuf;

use dioxus::prelude::*;

use crate::hone::{
    Catalog, CheckRun, ContextAuthor, Failure, Hone, StoreLog, TicketEvents, TicketOverview, Writer,
};
use crate::tracker::{Settings, Tracker};
use crate::ui::Choice;

/// The screens of the navigation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Screen {
    Tickets,
    Artifacts,
    Threads,
    Events,
    Check,
    Sync,
    Server,
    Tracker,
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
    /// The last full `hone check` of the Checks screen, kept across screens.
    pub(crate) last_check: Signal<Option<LastCheck>>,
}

/// A full `hone check` and the ticket it ran for.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct LastCheck {
    pub(crate) ticket: Option<String>,
    pub(crate) outcome: Result<CheckRun, Failure>,
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
            last_check: Signal::new(None),
        }
    }

    pub(crate) fn changed(mut self) {
        self.revision += 1;
    }

    pub(crate) fn open_ticket(mut self, ticket: String) {
        if (self.ticket)().as_deref() != Some(ticket.as_str()) {
            self.thread.set(None);
        }
        self.ticket.set(Some(ticket));
        self.screen.set(Screen::Tickets);
    }

    pub(crate) fn open_thread(mut self, ticket: String, thread: String) {
        self.ticket.set(Some(ticket));
        self.thread.set(Some(thread));
        self.screen.set(Screen::Threads);
    }
}

/// What several screens read from the project, read once per change: the artifacts, the event
/// log, the context's author, and the tracker setting.
#[derive(Clone, Copy)]
pub(crate) struct Project {
    pub(crate) catalog: Resource<Result<Catalog, Failure>>,
    pub(crate) log: Resource<Result<StoreLog, Failure>>,
    pub(crate) context: Resource<Result<ContextAuthor, Failure>>,
    pub(crate) tracker: Resource<Result<Option<Tracker>, String>>,
    /// The open ticket with the steps of its profile, `None` without an open ticket.
    pub(crate) overview: Resource<Option<Result<TicketOverview, Failure>>>,
    /// The process state of the open ticket, `None` without an open ticket.
    pub(crate) state: Resource<Option<Result<TicketEvents, Failure>>>,
    /// Bumped when the tracker setting changes.
    pub(crate) tracker_revision: Signal<u64>,
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
        let tracker_revision = use_signal(|| 0u64);
        let tracker = use_resource(move || async move {
            let _ = tracker_revision();
            let root = (session.hone)().root().to_path_buf();
            let project = match &*catalog.read() {
                Some(Ok(catalog)) => catalog.project.clone(),
                _ => return Ok(None),
            };
            let settings = Settings::locate()
                .ok_or_else(|| "no configuration directory: set HONE_CONFIG_DIR".to_owned())?;
            settings.tracker(&project, &root)
        });
        Self {
            catalog,
            log,
            context,
            tracker,
            overview,
            state,
            tracker_revision,
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
        match &*self.tracker.read() {
            Some(Ok(tracker)) => tracker.clone(),
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
    fn authors_keep_their_first_appearance_and_collect_their_roles() {
        let log = StoreLog {
            cursor: BTreeMap::new(),
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
