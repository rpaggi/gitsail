//! Desktop session state (SAD §17's "repository session state" category;
//! §21, §22).
//!
//! [`AppState`] holds exactly one open repository's [`RepositorySession`] at
//! a time behind a single [`Mutex`]. This is a deliberate simplification for
//! this story: every command that touches the session, including reads,
//! serializes behind that one lock. SAD §26 ("read operations may run
//! concurrently") is intentionally not satisfied yet — moving to
//! per-repository or lock-free concurrent reads is in scope for US-054
//! ("keep GUI responsive"), not this story, whose acceptance criteria only
//! require the shell/bridge layering to work end to end.

use std::sync::{Arc, Mutex};

use gitsail_application::{RepositoryReadPort, RepositorySession};
use gitsail_domain::{CommitGraph, ErrorCode, GitSailError, Repository};

pub struct AppState {
    port: Arc<dyn RepositoryReadPort>,
    session: Mutex<Option<RepositorySession>>,
    /// The commit graph accumulated for the active repository (US-067),
    /// separate from `session`: a session tracks HEAD/status/selection
    /// (SAD §21), while this is presentation-facing paginated layout state
    /// that only `get_commit_graph_page` touches. Reset whenever a new
    /// repository is opened, or explicitly when a filter change means the
    /// previously accumulated lanes no longer apply (US-067 criterion 3).
    commit_graph: Mutex<CommitGraph>,
}

impl AppState {
    pub fn new(port: Arc<dyn RepositoryReadPort>) -> Self {
        Self {
            port,
            session: Mutex::new(None),
            commit_graph: Mutex::new(CommitGraph::new()),
        }
    }

    pub fn port(&self) -> Arc<dyn RepositoryReadPort> {
        self.port.clone()
    }

    /// Replaces the active session with a fresh one over `repository`,
    /// discarding any previously open repository's session state, and
    /// starts a brand new commit graph (a graph accumulated for the
    /// previous repository must never be appended to as if it were the new
    /// one's history).
    pub fn open_session(&self, repository: Repository) {
        let session = RepositorySession::new(self.port.clone(), repository);
        *self.session.lock().expect("session mutex poisoned") = Some(session);
        self.reset_commit_graph();
    }

    /// Runs `f` against the active session, or fails with
    /// [`ErrorCode::InvalidRepositoryState`] when no repository is open yet
    /// — the same error taxonomy every other GitSail surface uses, rather
    /// than a Desktop-specific error shape (US-051 criterion 3).
    pub fn with_session_mut<T>(
        &self,
        f: impl FnOnce(&mut RepositorySession) -> Result<T, GitSailError>,
    ) -> Result<T, GitSailError> {
        let mut guard = self.session.lock().expect("session mutex poisoned");
        match guard.as_mut() {
            Some(session) => f(session),
            None => Err(GitSailError::new(
                ErrorCode::InvalidRepositoryState,
                "no repository is open; call open_repository first",
            )),
        }
    }

    /// The active repository, or the same [`ErrorCode::InvalidRepositoryState`]
    /// failure [`Self::with_session_mut`] uses. A read-only counterpart to
    /// it for commands (like the commit graph) that need the repository but
    /// never mutate the session itself.
    pub fn repository(&self) -> Result<Repository, GitSailError> {
        let guard = self.session.lock().expect("session mutex poisoned");
        guard
            .as_ref()
            .map(|session| session.repository().clone())
            .ok_or_else(|| {
                GitSailError::new(
                    ErrorCode::InvalidRepositoryState,
                    "no repository is open; call open_repository first",
                )
            })
    }

    /// Discards the accumulated commit graph, starting the next
    /// `get_commit_graph_page` call from an empty graph (US-067 criterion
    /// 3: a filter change gets a fresh layout rather than one mixing rows
    /// from two different queries).
    pub fn reset_commit_graph(&self) {
        *self.commit_graph.lock().expect("commit graph mutex poisoned") = CommitGraph::new();
    }

    /// Runs `f` against the accumulated commit graph.
    pub fn with_commit_graph_mut<T>(&self, f: impl FnOnce(&mut CommitGraph) -> T) -> T {
        let mut guard = self.commit_graph.lock().expect("commit graph mutex poisoned");
        f(&mut guard)
    }
}
