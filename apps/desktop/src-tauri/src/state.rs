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
use gitsail_domain::{ErrorCode, GitSailError, Repository};

pub struct AppState {
    port: Arc<dyn RepositoryReadPort>,
    session: Mutex<Option<RepositorySession>>,
}

impl AppState {
    pub fn new(port: Arc<dyn RepositoryReadPort>) -> Self {
        Self {
            port,
            session: Mutex::new(None),
        }
    }

    pub fn port(&self) -> Arc<dyn RepositoryReadPort> {
        self.port.clone()
    }

    /// Replaces the active session with a fresh one over `repository`,
    /// discarding any previously open repository's session state.
    pub fn open_session(&self, repository: Repository) {
        let session = RepositorySession::new(self.port.clone(), repository);
        *self.session.lock().expect("session mutex poisoned") = Some(session);
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
}
