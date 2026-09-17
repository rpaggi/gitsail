//! Repository session (SAD §21, §22).
//!
//! A [`RepositorySession`] tracks the active application context for one
//! open repository — its canonical identity, the last known HEAD/status
//! snapshot, the current selection, a refresh generation counter, and any
//! operations in flight (SAD §21). Sessions must tolerate external Git
//! changes made by another terminal/editor, so [`RepositorySession::refresh`]
//! always re-reads from the port rather than trusting a cached value.
//!
//! Refreshed results are only ever applied through the generation-guarded
//! path in [`RepositorySession::apply_refresh`]: a result computed against a
//! stale generation — an older refresh, or one issued before the repository
//! was switched — is discarded rather than silently overwriting newer state
//! (SAD §22; US-009 criterion 3).

use std::collections::HashSet;
use std::sync::Arc;

use gitsail_domain::{
    BranchName, CommitHash, GitSailError, OperationId, Repository, RepositoryId, RepositoryStatus,
};

use crate::ports::RepositoryReadPort;

/// The branch or commit currently selected in the session, independent of
/// HEAD (SAD §21).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Selection {
    pub branch: Option<BranchName>,
    pub commit: Option<CommitHash>,
}

/// Why a refresh was triggered (SAD §22). Carried for callers that want to
/// log/observe it; it does not change refresh behavior — manual, on-focus
/// and after-mutation refreshes all invalidate the same way (US-009
/// criterion 2).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RefreshReason {
    Manual,
    Focus,
    AfterMutation,
}

/// A capability to apply exactly one refresh result, tied to the
/// generation active when it was issued (SAD §22).
///
/// Obtained from [`RepositorySession::begin_refresh`]. A caller doing
/// asynchronous work (fetch on a background thread, then apply) should hold
/// onto the ticket across that work and apply the result through
/// [`RepositorySession::apply_refresh`], which is a no-op if the session's
/// generation has since moved on — a newer refresh started, or the
/// repository was switched (US-009 criterion 3).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RefreshTicket {
    generation: u64,
}

/// Active application context for one open repository (SAD §21).
pub struct RepositorySession {
    port: Arc<dyn RepositoryReadPort>,
    repository: Repository,
    status: Option<RepositoryStatus>,
    selection: Selection,
    generation: u64,
    active_operations: HashSet<OperationId>,
}

impl RepositorySession {
    /// Opens `repository` in a new session with no status snapshot yet;
    /// call [`refresh`](Self::refresh) to populate one.
    pub fn new(port: Arc<dyn RepositoryReadPort>, repository: Repository) -> Self {
        Self {
            port,
            repository,
            status: None,
            selection: Selection::default(),
            generation: 0,
            active_operations: HashSet::new(),
        }
    }

    pub fn id(&self) -> &RepositoryId {
        &self.repository.id
    }

    pub fn repository(&self) -> &Repository {
        &self.repository
    }

    /// The last applied status snapshot, or `None` before the first
    /// successful refresh.
    pub fn status(&self) -> Option<&RepositoryStatus> {
        self.status.as_ref()
    }

    pub fn selection(&self) -> &Selection {
        &self.selection
    }

    /// The current refresh generation. Bumped by every
    /// [`begin_refresh`](Self::begin_refresh) and by
    /// [`switch_repository`](Self::switch_repository).
    pub fn generation(&self) -> u64 {
        self.generation
    }

    pub fn select_branch(&mut self, branch: BranchName) {
        self.selection = Selection {
            branch: Some(branch),
            commit: None,
        };
    }

    pub fn select_commit(&mut self, commit: CommitHash) {
        self.selection = Selection {
            branch: None,
            commit: Some(commit),
        };
    }

    pub fn clear_selection(&mut self) {
        self.selection = Selection::default();
    }

    /// Records that `id` is now in flight (e.g. a mutation submitted to a
    /// future write port). Session state is otherwise unaffected: the
    /// caller decides what, if anything, to refresh once it completes.
    pub fn begin_operation(&mut self, id: OperationId) {
        self.active_operations.insert(id);
    }

    pub fn end_operation(&mut self, id: &OperationId) {
        self.active_operations.remove(id);
    }

    pub fn has_active_operations(&self) -> bool {
        !self.active_operations.is_empty()
    }

    pub fn active_operations(&self) -> impl Iterator<Item = &OperationId> {
        self.active_operations.iter()
    }

    /// Issues a ticket for a new refresh, bumping the generation so any
    /// ticket issued before this call is now stale (SAD §22).
    pub fn begin_refresh(&mut self, _reason: RefreshReason) -> RefreshTicket {
        self.generation += 1;
        RefreshTicket {
            generation: self.generation,
        }
    }

    /// Applies a refresh result obtained for `ticket`. Returns `false`
    /// without changing state when `ticket` is stale (US-009 criterion 3).
    pub fn apply_refresh(&mut self, ticket: RefreshTicket, status: RepositoryStatus) -> bool {
        if ticket.generation != self.generation {
            return false;
        }
        self.status = Some(status);
        true
    }

    /// Fetches a fresh status snapshot from the port synchronously and
    /// applies it, for the given `reason` (SAD §22: manual, on focus, and
    /// after a GitSail mutation all invalidate the same way — US-009
    /// criterion 2). Always re-reads from the port, so external changes
    /// (another terminal/editor) are picked up (SAD §21).
    pub fn refresh(&mut self, reason: RefreshReason) -> Result<(), GitSailError> {
        let ticket = self.begin_refresh(reason);
        let status = self.port.status(&self.repository)?;
        self.apply_refresh(ticket, status);
        Ok(())
    }

    /// Replaces the open repository, discarding the previous snapshot,
    /// selection and in-flight refresh tickets (SAD §22; US-009 criterion 3:
    /// switching repositories cancels/discards stale responses).
    pub fn switch_repository(&mut self, repository: Repository) {
        self.repository = repository;
        self.status = None;
        self.selection = Selection::default();
        self.generation += 1;
        self.active_operations.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gitsail_domain::{
        Blame, Branch, ChangeType, Commit, Diff, ErrorCode, FileChange, FileStatusCode, HeadState,
    };
    use std::path::{Path, PathBuf};
    use std::sync::Mutex;

    use crate::ports::{CommitQuery, DiffRequest, Page};

    /// A double whose reported status can be swapped mid-test, so tests can
    /// simulate an external Git change between two refreshes.
    struct FakePort {
        status: Mutex<RepositoryStatus>,
    }

    impl FakePort {
        fn new(status: RepositoryStatus) -> Self {
            Self {
                status: Mutex::new(status),
            }
        }

        fn set_status(&self, status: RepositoryStatus) {
            *self.status.lock().unwrap() = status;
        }
    }

    impl RepositoryReadPort for FakePort {
        fn discover(&self, _path: &Path) -> Result<Repository, GitSailError> {
            unimplemented!("not exercised by session tests")
        }

        fn status(&self, _repo: &Repository) -> Result<RepositoryStatus, GitSailError> {
            Ok(self.status.lock().unwrap().clone())
        }

        fn commits(
            &self,
            _repo: &Repository,
            _query: &CommitQuery,
        ) -> Result<Page<Commit>, GitSailError> {
            unimplemented!("not exercised by session tests")
        }

        fn commit(&self, _repo: &Repository, _hash: &CommitHash) -> Result<Commit, GitSailError> {
            unimplemented!("not exercised by session tests")
        }

        fn branches(&self, _repo: &Repository) -> Result<Vec<Branch>, GitSailError> {
            unimplemented!("not exercised by session tests")
        }

        fn diff(&self, _repo: &Repository, _request: &DiffRequest) -> Result<Diff, GitSailError> {
            unimplemented!("not exercised by session tests")
        }

        fn blame(
            &self,
            _repo: &Repository,
            _file: &Path,
            _revision: Option<&CommitHash>,
        ) -> Result<Blame, GitSailError> {
            unimplemented!("not exercised by session tests")
        }
    }

    fn sample_repository(root: &str) -> Repository {
        Repository {
            id: RepositoryId::from_canonical_root(Path::new(root)),
            root_path: PathBuf::from(root),
            worktree_path: Some(PathBuf::from(root)),
            is_bare: false,
            head_state: HeadState::Attached {
                branch: BranchName::new("main").unwrap(),
            },
            current_branch: Some(BranchName::new("main").unwrap()),
        }
    }

    fn clean_status() -> RepositoryStatus {
        RepositoryStatus {
            branch: Some(BranchName::new("main").unwrap()),
            head_state: HeadState::Attached {
                branch: BranchName::new("main").unwrap(),
            },
            files: vec![],
        }
    }

    fn dirty_status() -> RepositoryStatus {
        RepositoryStatus {
            branch: Some(BranchName::new("main").unwrap()),
            head_state: HeadState::Attached {
                branch: BranchName::new("main").unwrap(),
            },
            files: vec![FileChange {
                path: PathBuf::from("a.txt"),
                previous_path: None,
                change_type: ChangeType::Modified,
                index_status: FileStatusCode::Unmodified,
                worktree_status: FileStatusCode::Modified,
            }],
        }
    }

    #[test]
    fn new_session_tracks_identity_but_has_no_status_until_refreshed() {
        let port = Arc::new(FakePort::new(clean_status()));
        let repo = sample_repository("/repo");
        let session = RepositorySession::new(port, repo.clone());

        assert_eq!(session.id(), &repo.id);
        assert_eq!(session.repository(), &repo);
        assert!(session.status().is_none());
        assert_eq!(session.generation(), 0);
        assert!(!session.has_active_operations());
    }

    #[test]
    fn refresh_reads_through_to_the_port_and_tolerates_external_changes() {
        let port = Arc::new(FakePort::new(clean_status()));
        let mut session = RepositorySession::new(port.clone(), sample_repository("/repo"));

        session.refresh(RefreshReason::Manual).unwrap();
        assert!(session.status().unwrap().is_clean());

        // Another terminal/editor mutates the repository between refreshes.
        port.set_status(dirty_status());
        session.refresh(RefreshReason::Focus).unwrap();
        assert!(!session.status().unwrap().is_clean());
    }

    #[test]
    fn manual_focus_and_after_mutation_refreshes_all_invalidate_the_snapshot() {
        let port = Arc::new(FakePort::new(clean_status()));
        let mut session = RepositorySession::new(port.clone(), sample_repository("/repo"));

        for reason in [
            RefreshReason::Manual,
            RefreshReason::Focus,
            RefreshReason::AfterMutation,
        ] {
            port.set_status(dirty_status());
            session.refresh(reason).unwrap();
            assert!(!session.status().unwrap().is_clean());

            port.set_status(clean_status());
            session.refresh(reason).unwrap();
            assert!(session.status().unwrap().is_clean());
        }
    }

    #[test]
    fn a_stale_refresh_result_never_overwrites_newer_state() {
        let port = Arc::new(FakePort::new(clean_status()));
        let mut session = RepositorySession::new(port, sample_repository("/repo"));

        // Simulates two refreshes racing: the first ticket is issued, then a
        // second refresh completes and applies first, then the first
        // (now-stale) result arrives late.
        let stale_ticket = session.begin_refresh(RefreshReason::Manual);
        session.refresh(RefreshReason::Manual).unwrap();
        assert!(session.status().unwrap().is_clean());

        let applied = session.apply_refresh(stale_ticket, dirty_status());

        assert!(!applied, "a stale ticket must not apply");
        assert!(
            session.status().unwrap().is_clean(),
            "the newer refresh's result must survive a late, stale response"
        );
    }

    #[test]
    fn switching_repository_discards_stale_tickets_selection_and_snapshot() {
        let port = Arc::new(FakePort::new(clean_status()));
        let mut session = RepositorySession::new(port, sample_repository("/repo-a"));
        session.refresh(RefreshReason::Manual).unwrap();
        session.select_branch(BranchName::new("main").unwrap());
        let ticket = session.begin_refresh(RefreshReason::Manual);

        let repo_b = sample_repository("/repo-b");
        session.switch_repository(repo_b.clone());

        assert_eq!(session.repository(), &repo_b);
        assert!(session.status().is_none());
        assert_eq!(session.selection(), &Selection::default());
        assert!(!session.apply_refresh(ticket, dirty_status()));
    }

    #[test]
    fn active_operations_can_be_tracked_and_cleared() {
        let port = Arc::new(FakePort::new(clean_status()));
        let mut session = RepositorySession::new(port, sample_repository("/repo"));
        let op = OperationId::new("op-1");

        session.begin_operation(op.clone());
        assert!(session.has_active_operations());
        assert_eq!(session.active_operations().count(), 1);

        session.end_operation(&op);
        assert!(!session.has_active_operations());
    }

    #[test]
    fn two_sessions_over_the_same_repository_do_not_share_state() {
        let port_a = Arc::new(FakePort::new(clean_status()));
        let port_b = Arc::new(FakePort::new(clean_status()));
        let mut session_a = RepositorySession::new(port_a.clone(), sample_repository("/repo"));
        let session_b = RepositorySession::new(port_b, sample_repository("/repo"));

        port_a.set_status(dirty_status());
        session_a.refresh(RefreshReason::Manual).unwrap();

        assert!(!session_a.status().unwrap().is_clean());
        assert!(
            session_b.status().is_none(),
            "session_b must be unaffected by session_a's refresh"
        );
    }

    #[test]
    fn refresh_propagates_port_errors_without_bumping_the_applied_snapshot() {
        struct FailingPort;
        impl RepositoryReadPort for FailingPort {
            fn discover(&self, _path: &Path) -> Result<Repository, GitSailError> {
                unimplemented!()
            }
            fn status(&self, _repo: &Repository) -> Result<RepositoryStatus, GitSailError> {
                Err(GitSailError::new(ErrorCode::ProcessFailure, "boom"))
            }
            fn commits(
                &self,
                _repo: &Repository,
                _query: &CommitQuery,
            ) -> Result<Page<Commit>, GitSailError> {
                unimplemented!()
            }
            fn commit(&self, _repo: &Repository, _hash: &CommitHash) -> Result<Commit, GitSailError> {
                unimplemented!()
            }
            fn branches(&self, _repo: &Repository) -> Result<Vec<Branch>, GitSailError> {
                unimplemented!()
            }
            fn diff(&self, _repo: &Repository, _request: &DiffRequest) -> Result<Diff, GitSailError> {
                unimplemented!()
            }
            fn blame(
                &self,
                _repo: &Repository,
                _file: &Path,
                _revision: Option<&CommitHash>,
            ) -> Result<Blame, GitSailError> {
                unimplemented!()
            }
        }

        let mut session = RepositorySession::new(Arc::new(FailingPort), sample_repository("/repo"));
        let generation_before = session.generation();

        let err = session.refresh(RefreshReason::Manual).unwrap_err();

        assert_eq!(err.code(), ErrorCode::ProcessFailure);
        assert!(session.status().is_none());
        assert_eq!(session.generation(), generation_before + 1);
    }
}
