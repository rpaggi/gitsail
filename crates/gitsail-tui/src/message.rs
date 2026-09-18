//! Typed results flowing back onto the main loop's single channel — either
//! a terminal input event or the outcome of a background [`crate::worker::Command`]
//! (SAD §18: "... return typed messages/events").

use gitsail_application::{Page, RefreshTicket};
use gitsail_domain::{
    Blame, Branch, Commit, CommitHash, Diff, GitSailError, Repository, RepositoryStatus,
};

#[derive(Debug)]
pub enum Message {
    /// A raw terminal event, forwarded by [`crate::event::spawn`].
    Term(crossterm::event::Event),
    /// A periodic wake-up with no event, so the render loop is never
    /// blocked indefinitely on `crossterm::event::read`.
    Tick,
    /// [`crate::worker::Command::OpenRepository`] completed.
    RepositoryOpened(Result<Repository, GitSailError>),
    /// [`crate::worker::Command::RefreshStatus`] completed. Tagged with the
    /// [`RefreshTicket`] it was issued for, so [`crate::app::App`] can
    /// discard a stale result exactly like
    /// [`gitsail_application::RepositorySession::apply_refresh`] already
    /// does for a successful one (US-041 criterion 3).
    StatusRefreshed(RefreshTicket, Result<RepositoryStatus, GitSailError>),
    /// [`crate::worker::Command::LoadBranches`] completed. Tagged with the
    /// session generation active when it was requested, for the same
    /// staleness check.
    BranchesLoaded(u64, Result<Vec<Branch>, GitSailError>),
    /// [`crate::worker::Command::LoadDiff`] completed (US-046). Tagged with
    /// the request id issued when it was dispatched, so a diff computed for
    /// a since-abandoned selection is discarded.
    DiffLoaded(u64, Result<Diff, GitSailError>),
    /// [`crate::worker::Command::LoadBlame`] completed (US-046), tagged like
    /// [`Self::DiffLoaded`].
    BlameLoaded(u64, Result<Blame, GitSailError>),
    /// [`crate::worker::Command::LoadCommitGraph`] completed (US-065,
    /// US-066), tagged like [`Self::DiffLoaded`].
    CommitGraphPageLoaded(u64, Result<Page<Commit>, GitSailError>),
    /// A `SwitchBranch`/`CreateBranch`/`DeleteBranch`/`StageFiles`/
    /// `UnstageFiles` [`crate::worker::Command`] completed (US-047, US-048).
    OperationFinished(Result<(), GitSailError>),
    /// [`crate::worker::Command::CreateCommit`] completed (US-047).
    CommitCreated(Result<CommitHash, GitSailError>),
}
