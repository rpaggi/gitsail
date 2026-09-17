//! Executes [`Command`]s issued by [`crate::app::App::update`] on
//! background threads, so Git process execution never runs on the render
//! loop (SAD §18, §26; US-041 criterion 2).
//!
//! Each `Command` gets its own thread. A `RepositoryReadPort` call already
//! runs at most one `git` process at a time and TUI Foundation issues at
//! most a couple of these per refresh, so a persistent worker pool would be
//! premature machinery for what this epic needs; a future epic issuing
//! many concurrent reads can revisit this without changing the `Command`/
//! `Message` contract.

use std::path::PathBuf;
use std::sync::mpsc::Sender;
use std::sync::Arc;
use std::thread;

use gitsail_application::{
    GetRepositoryStatus, ListBranches, OpenRepository, RefreshTicket, RepositoryReadPort,
};
use gitsail_domain::Repository;

use crate::message::Message;

/// A side effect [`crate::app::App::update`] wants run outside itself.
/// `App::update` only ever returns these; it never touches `port` or a
/// thread directly, which is what keeps it testable without any I/O.
#[derive(Debug, Clone)]
pub enum Command {
    OpenRepository(PathBuf),
    RefreshStatus(RefreshTicket, Repository),
    LoadBranches(u64, Repository),
}

/// Spawns one background thread per command in `commands`, each reporting
/// its result back on `tx` as a [`Message`].
pub fn dispatch(commands: Vec<Command>, port: &Arc<dyn RepositoryReadPort>, tx: &Sender<Message>) {
    for command in commands {
        spawn_one(command, Arc::clone(port), tx.clone());
    }
}

fn spawn_one(command: Command, port: Arc<dyn RepositoryReadPort>, tx: Sender<Message>) {
    thread::spawn(move || {
        let message = match command {
            Command::OpenRepository(path) => {
                let result = OpenRepository::new(port).execute(&path);
                Message::RepositoryOpened(result)
            }
            Command::RefreshStatus(ticket, repo) => {
                let result = GetRepositoryStatus::new(port).execute(&repo);
                Message::StatusRefreshed(ticket, result)
            }
            Command::LoadBranches(generation, repo) => {
                let result = ListBranches::new(port).execute(&repo);
                Message::BranchesLoaded(generation, result)
            }
        };
        // The receiving end only disappears once the app is shutting down
        // (the main loop dropped its `Receiver`); a job finishing after
        // that has nothing useful left to report, so a send failure here
        // is expected and not an error to surface anywhere.
        let _ = tx.send(message);
    });
}
