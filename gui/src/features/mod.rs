//! One module per feature. Each holds the view of its screen and the logic that turns the
//! command line's results into what the screen shows.

pub(crate) mod check;
mod common;
pub(crate) mod events;
pub(crate) mod server;
pub(crate) mod settings;
pub(crate) mod sync;
pub(crate) mod threads;
pub(crate) mod tickets;

use std::path::PathBuf;

use dioxus::prelude::*;

use crate::hone::{Hone, Writer};

/// The screens of the navigation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Screen {
    Tickets,
    Threads,
    Events,
    Check,
    Sync,
    Server,
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
        }
    }

    pub(crate) fn changed(mut self) {
        self.revision += 1;
    }

    pub(crate) fn open_ticket(mut self, ticket: String) {
        self.ticket.set(Some(ticket));
        self.screen.set(Screen::Tickets);
    }

    pub(crate) fn open_thread(mut self, ticket: String, thread: String) {
        self.ticket.set(Some(ticket));
        self.thread.set(Some(thread));
        self.screen.set(Screen::Threads);
    }
}
