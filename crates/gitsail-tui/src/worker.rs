//! Executes [`Command`]s issued by [`crate::app::App::update`] on
//! background threads, so Git process execution never runs on the render
//! loop (SAD §18, §26; US-041 criterion 2).
//!
//! Each `Command` gets its own thread. A `RepositoryReadPort`/
//! `RepositoryWritePort` call already runs at most one `git` process at a
//! time and one frame issues only a couple of these, so a persistent worker
//! pool would be premature machinery for what this epic needs; a future
//! epic issuing many concurrent reads can revisit this without changing the
//! `Command`/`Message` contract.
//!
//! Cancellation (SAD §26: "operações de leitura caras devem suportar
//! cancelamento") is out of scope for the commands added by US-046: no
//! acceptance criterion here asks for a diff/blame in flight to be
//! cancelable, so each read is issued with a fresh, never-cancelled
//! [`CancellationToken`] — a deliberate scope cut, not an oversight.

use std::path::PathBuf;
use std::sync::mpsc::Sender;
use std::sync::Arc;
use std::thread;

use gitsail_application::{
    BlameRequest, CommitQuery, CreateBranch, CreateCommit, DeleteBranch, DiffRequest,
    GetCommitHistory, GetDiff, GetFileBlame, GetRepositoryStatus, ListBranches, OpenRepository,
    RefreshTicket, RepositoryReadPort, RepositoryWritePort, StageFiles, SwitchBranch,
    UnstageFiles,
};
use gitsail_domain::{BranchName, CancellationToken, CommitHash, Repository};

use crate::message::Message;

/// A side effect [`crate::app::App::update`] wants run outside itself.
/// `App::update` only ever returns these; it never touches a port or a
/// thread directly, which is what keeps it testable without any I/O.
#[derive(Debug, Clone)]
pub enum Command {
    OpenRepository(PathBuf),
    RefreshStatus(RefreshTicket, Repository),
    LoadBranches(u64, Repository),
    /// Loads a diff for the given request, tagged with a request id so a
    /// result computed for a since-abandoned selection can be discarded
    /// (US-046).
    LoadDiff(u64, Repository, DiffRequest),
    /// Loads blame for the given request, tagged like [`Self::LoadDiff`].
    /// The trailing `u64` is the `content_version` [`GetFileBlame`] uses to
    /// key its cache — the session's refresh generation, so any refresh
    /// invalidates it.
    LoadBlame(u64, Repository, BlameRequest, u64),
    /// Loads one page of commit-graph history (US-065, US-066), tagged
    /// with a request id like [`Self::LoadDiff`]. The `CommitQuery`'s
    /// `cursor` is what makes this "the next page" rather than a restart —
    /// [`crate::app::App`] carries it forward from the previous page's
    /// result.
    LoadCommitGraph(u64, Repository, CommitQuery),
    StageFiles(Repository, Vec<PathBuf>),
    UnstageFiles(Repository, Vec<PathBuf>),
    CreateCommit(Repository, String),
    SwitchBranch(Repository, BranchName),
    CreateBranch(Repository, BranchName, Option<CommitHash>),
    DeleteBranch(Repository, BranchName, bool),
}

/// Spawns one background thread per command in `commands`, each reporting
/// its result back on `tx` as a [`Message`].
pub fn dispatch(
    commands: Vec<Command>,
    read_port: &Arc<dyn RepositoryReadPort>,
    write_port: &Arc<dyn RepositoryWritePort>,
    tx: &Sender<Message>,
) {
    for command in commands {
        spawn_one(
            command,
            Arc::clone(read_port),
            Arc::clone(write_port),
            tx.clone(),
        );
    }
}

fn spawn_one(
    command: Command,
    read_port: Arc<dyn RepositoryReadPort>,
    write_port: Arc<dyn RepositoryWritePort>,
    tx: Sender<Message>,
) {
    thread::spawn(move || {
        let message = match command {
            Command::OpenRepository(path) => {
                let result = OpenRepository::new(read_port).execute(&path);
                Message::RepositoryOpened(result)
            }
            Command::RefreshStatus(ticket, repo) => {
                let result = GetRepositoryStatus::new(read_port).execute(&repo);
                Message::StatusRefreshed(ticket, result)
            }
            Command::LoadBranches(generation, repo) => {
                let result = ListBranches::new(read_port).execute(&repo);
                Message::BranchesLoaded(generation, result)
            }
            Command::LoadDiff(request_id, repo, request) => {
                let result =
                    GetDiff::new(read_port).execute(&repo, &request, &CancellationToken::new());
                Message::DiffLoaded(request_id, result)
            }
            Command::LoadBlame(request_id, repo, request, content_version) => {
                let result = GetFileBlame::new(read_port).execute(
                    &repo,
                    &request,
                    content_version,
                    &CancellationToken::new(),
                );
                Message::BlameLoaded(request_id, result)
            }
            Command::LoadCommitGraph(request_id, repo, query) => {
                let result = GetCommitHistory::new(read_port).execute(&repo, &query);
                Message::CommitGraphPageLoaded(request_id, result)
            }
            Command::StageFiles(repo, paths) => {
                let result = StageFiles::new(write_port).execute(&repo, &paths);
                Message::OperationFinished(result)
            }
            Command::UnstageFiles(repo, paths) => {
                let result = UnstageFiles::new(write_port).execute(&repo, &paths);
                Message::OperationFinished(result)
            }
            Command::CreateCommit(repo, message_text) => {
                let result = CreateCommit::new(write_port).execute(&repo, &message_text);
                Message::CommitCreated(result)
            }
            Command::SwitchBranch(repo, target) => {
                let result = SwitchBranch::new(write_port).execute(&repo, &target);
                Message::OperationFinished(result)
            }
            Command::CreateBranch(repo, name, start_point) => {
                let result =
                    CreateBranch::new(write_port).execute(&repo, &name, start_point.as_ref());
                Message::OperationFinished(result)
            }
            Command::DeleteBranch(repo, name, force) => {
                let result = DeleteBranch::new(write_port).execute(&repo, &name, force);
                Message::OperationFinished(result)
            }
        };
        // The receiving end only disappears once the app is shutting down
        // (the main loop dropped its `Receiver`); a job finishing after
        // that has nothing useful left to report, so a send failure here
        // is expected and not an error to surface anywhere.
        let _ = tx.send(message);
    });
}
