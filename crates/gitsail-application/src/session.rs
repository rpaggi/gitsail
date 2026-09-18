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
use std::sync::{Arc, Mutex};

use gitsail_domain::{
    BranchName, CommitHash, GitSailError, OperationId, Repository, RepositoryId, RepositoryStatus,
};

use crate::concurrency::{global_lock_registry, Invalidatable};
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
    /// Resolved lazily (on first mutation) via
    /// [`RepositoryReadPort::lock_key`], then cached — resolving it eagerly
    /// in [`Self::new`] would make every session construction do adapter
    /// I/O even for a session that never mutates anything.
    mutation_lock: Mutex<Option<Arc<Mutex<()>>>>,
    /// Caches this session invalidates whenever the repository is observed
    /// to have actually changed (T-227/US-116 criterion 3; T-228/US-117
    /// criterion 2) — e.g. a [`crate::blame_cache::BlameCache`] or
    /// [`crate::graph_cache::GraphCache`] the same frontend also holds.
    /// Invalidated unconditionally after a successful [`Self::run_mutation`]
    /// (a mutation is always assumed to have changed something), and
    /// conditionally by [`Self::apply_refresh`] when a refresh (manual,
    /// on-focus, or after a mutation) reveals a status different from what
    /// was previously applied — covering "a ref changed underneath us"
    /// (another terminal/editor committing, switching branches, ...)
    /// without invalidating on every no-op refresh. Registered after
    /// construction via [`Self::register_cache`] rather than threaded
    /// through [`Self::new`], so existing callers are unaffected until they
    /// opt in.
    registered_caches: Vec<Arc<dyn Invalidatable>>,
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
            mutation_lock: Mutex::new(None),
            registered_caches: Vec::new(),
        }
    }

    /// Registers `cache` to be invalidated whenever this session observes
    /// the repository has changed (see [`Self::registered_caches`]'s doc for
    /// exactly when). Idempotent registration is the caller's
    /// responsibility — this simply appends, so registering the same cache
    /// twice invalidates it twice (harmless, but redundant).
    pub fn register_cache(&mut self, cache: Arc<dyn Invalidatable>) {
        self.registered_caches.push(cache);
    }

    fn invalidate_registered_caches(&self) {
        for cache in &self.registered_caches {
            cache.invalidate_all();
        }
    }

    /// Resolves (and caches) this session's mutation-serialization lock via
    /// [`RepositoryReadPort::lock_key`] (ADR-019). Distinct sessions whose
    /// port resolves the same key — e.g. two linked worktrees of the same
    /// physical repository — share the same underlying
    /// [`std::sync::Mutex`], obtained from
    /// [`crate::concurrency::global_lock_registry`].
    fn resolve_mutation_lock(&self) -> Result<Arc<Mutex<()>>, GitSailError> {
        let mut slot = self
            .mutation_lock
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        if let Some(lock) = slot.as_ref() {
            return Ok(Arc::clone(lock));
        }
        let key = self.port.lock_key(&self.repository)?;
        let lock = global_lock_registry().lock_for(&key);
        *slot = Some(Arc::clone(&lock));
        Ok(lock)
    }

    /// Runs `mutation` serialized against every other session (in this
    /// process) targeting the same physical repository (SAD §26; T-227/
    /// US-116 criterion 1), then — only on success — refreshes this
    /// session's status snapshot (`RefreshReason::AfterMutation`, bumping
    /// the generation so any in-flight read ticket becomes stale, criterion
    /// 2) and invalidates every cache registered via [`Self::register_cache`]
    /// (criterion 3).
    ///
    /// `mutation` is exactly the caller's own mutation call (e.g. a
    /// `write_use_cases::CreateCommit::execute(...)`) — this method knows
    /// nothing about `RepositoryWritePort` itself, keeping the read/write
    /// port separation ADR-009 established intact. A failed mutation is
    /// returned as-is: the lock is still released, but no refresh or
    /// invalidation runs, since nothing about the repository actually
    /// changed.
    ///
    /// The refresh runs after the lock is released: a refresh is a read,
    /// and SAD §26 allows reads to run concurrently with other reads —
    /// holding the mutation lock during it would serialize more than
    /// necessary. A refresh failure is deliberately swallowed here (not
    /// propagated) so a successful mutation is never reported as failed
    /// just because the follow-up read hit a transient error; a caller that
    /// needs to know can always call [`Self::refresh`] itself afterward.
    pub fn run_mutation<T>(
        &mut self,
        mutation: impl FnOnce() -> Result<T, GitSailError>,
    ) -> Result<T, GitSailError> {
        let lock = self.resolve_mutation_lock()?;
        let result = {
            let _guard = lock.lock().unwrap_or_else(|poison| poison.into_inner());
            mutation()
        };
        let value = result?;
        let _ = self.refresh(RefreshReason::AfterMutation);
        // Unconditional: a successful mutation is always assumed to have
        // changed something, regardless of whether the follow-up refresh
        // above (best-effort; its own failure is already swallowed) managed
        // to observe that change in the new status snapshot.
        self.invalidate_registered_caches();
        Ok(value)
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

    /// Whether `ticket` was issued for the session's current generation.
    ///
    /// [`apply_refresh`](Self::apply_refresh) already performs this check
    /// internally when there is a [`RepositoryStatus`] to apply; this
    /// method lets a caller make the same staleness decision for a
    /// *failed* refresh, where there is no status to hand to
    /// `apply_refresh` (e.g. US-041 criterion 3: a stale error must be
    /// discarded exactly like a stale success).
    pub fn is_current(&self, ticket: RefreshTicket) -> bool {
        ticket.generation == self.generation
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
    ///
    /// When accepted, also invalidates every cache registered via
    /// [`Self::register_cache`] if the new status differs from whatever was
    /// previously applied (T-228/US-117 criterion 2: "mudança de refs ...
    /// invalidam as entradas relevantes") — but not otherwise, so a routine
    /// on-focus refresh that finds nothing new does not needlessly discard
    /// a still-valid cache. The very first applied status (`self.status`
    /// was `None`) always counts as "changed": harmless, since nothing has
    /// been cached yet at that point anyway.
    pub fn apply_refresh(&mut self, ticket: RefreshTicket, status: RepositoryStatus) -> bool {
        if ticket.generation != self.generation {
            return false;
        }
        let changed = self.status.as_ref() != Some(&status);
        self.status = Some(status);
        if changed {
            self.invalidate_registered_caches();
        }
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
        *self
            .mutation_lock
            .lock()
            .unwrap_or_else(|poison| poison.into_inner()) = None;
        self.invalidate_registered_caches();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gitsail_domain::{
        Blame, Branch, ChangeType, Commit, Diff, ErrorCode, FileChange, FileContentAtRevision,
        FileStatusCode, HeadState,
    };
    use std::path::{Path, PathBuf};
    use std::sync::Mutex;

    use crate::ports::{BlameRequest, CommitQuery, DiffRequest, Page};

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

        fn diff(
            &self,
            _repo: &Repository,
            _request: &DiffRequest,
            _cancel: &gitsail_domain::CancellationToken,
        ) -> Result<Diff, GitSailError> {
            unimplemented!("not exercised by session tests")
        }

        fn resolve_revision(
            &self,
            _repo: &Repository,
            _revision: &str,
        ) -> Result<CommitHash, GitSailError> {
            unimplemented!("not exercised by session tests")
        }

        fn blame(
            &self,
            _repo: &Repository,
            _request: &BlameRequest,
            _cancel: &gitsail_domain::CancellationToken,
        ) -> Result<Blame, GitSailError> {
            unimplemented!("not exercised by session tests")
        }

        fn line_history(
            &self,
            _repo: &Repository,
            _request: &crate::ports::LineHistoryRequest,
            _cancel: &gitsail_domain::CancellationToken,
        ) -> Result<gitsail_domain::LineHistory, GitSailError> {
            unimplemented!("not exercised by session tests")
        }

        fn file_content(
            &self,
            _repo: &Repository,
            _revision: &CommitHash,
            _path: &Path,
        ) -> Result<FileContentAtRevision, GitSailError> {
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
    fn is_current_reflects_generation_independently_of_apply_refresh() {
        let port = Arc::new(FakePort::new(clean_status()));
        let mut session = RepositorySession::new(port, sample_repository("/repo"));

        let ticket = session.begin_refresh(RefreshReason::Manual);
        assert!(session.is_current(ticket));

        // A newer refresh starts before the first one's caller checks back in.
        let _newer = session.begin_refresh(RefreshReason::Manual);
        assert!(
            !session.is_current(ticket),
            "a ticket issued before a newer refresh must be reported stale"
        );
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
            fn commit(
                &self,
                _repo: &Repository,
                _hash: &CommitHash,
            ) -> Result<Commit, GitSailError> {
                unimplemented!()
            }
            fn branches(&self, _repo: &Repository) -> Result<Vec<Branch>, GitSailError> {
                unimplemented!()
            }
            fn diff(
                &self,
                _repo: &Repository,
                _request: &DiffRequest,
                _cancel: &gitsail_domain::CancellationToken,
            ) -> Result<Diff, GitSailError> {
                unimplemented!()
            }
            fn resolve_revision(
                &self,
                _repo: &Repository,
                _revision: &str,
            ) -> Result<CommitHash, GitSailError> {
                unimplemented!()
            }
            fn blame(
                &self,
                _repo: &Repository,
                _request: &BlameRequest,
                _cancel: &gitsail_domain::CancellationToken,
            ) -> Result<Blame, GitSailError> {
                unimplemented!()
            }
            fn line_history(
                &self,
                _repo: &Repository,
                _request: &crate::ports::LineHistoryRequest,
                _cancel: &gitsail_domain::CancellationToken,
            ) -> Result<gitsail_domain::LineHistory, GitSailError> {
                unimplemented!()
            }
            fn file_content(
                &self,
                _repo: &Repository,
                _revision: &CommitHash,
                _path: &Path,
            ) -> Result<FileContentAtRevision, GitSailError> {
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

    // -----------------------------------------------------------------
    // T-227/US-116: mutation serialization, lock-conflict propagation,
    // cache invalidation, and stale-read discard around `run_mutation`.
    // -----------------------------------------------------------------

    /// Real OS threads (not sequential simulation), each owning its *own*
    /// `RepositorySession` opened against the same repository path — the
    /// same shape as two independent frontends/processes both opening the
    /// same repository. `RepositoryReadPort::lock_key`'s default
    /// implementation resolves the same key (the shared root path) for
    /// both, so both sessions must serialize against each other through
    /// [`crate::concurrency::global_lock_registry`] even though neither
    /// session object is ever shared between the threads.
    #[test]
    fn run_mutation_serializes_across_independent_sessions_targeting_the_same_repository() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        use std::sync::Barrier;

        const THREADS: usize = 8;
        let concurrent = Arc::new(AtomicUsize::new(0));
        let max_concurrent = Arc::new(AtomicUsize::new(0));
        let barrier = Arc::new(Barrier::new(THREADS));

        // A repository path unique to this test, so this test's lock never
        // collides with another test's entry in the process-wide registry.
        let repo_path = "/shared-repo-for-run-mutation-serialization-test";

        let handles: Vec<_> = (0..THREADS)
            .map(|_| {
                let concurrent = Arc::clone(&concurrent);
                let max_concurrent = Arc::clone(&max_concurrent);
                let barrier = Arc::clone(&barrier);
                std::thread::spawn(move || {
                    let port = Arc::new(FakePort::new(clean_status()));
                    let mut session = RepositorySession::new(port, sample_repository(repo_path));

                    barrier.wait();
                    for _ in 0..15 {
                        session
                            .run_mutation(|| {
                                let now = concurrent.fetch_add(1, Ordering::SeqCst) + 1;
                                max_concurrent.fetch_max(now, Ordering::SeqCst);
                                std::thread::sleep(std::time::Duration::from_micros(200));
                                concurrent.fetch_sub(1, Ordering::SeqCst);
                                Ok::<(), GitSailError>(())
                            })
                            .unwrap();
                    }
                })
            })
            .collect();

        for handle in handles {
            handle.join().unwrap();
        }

        assert_eq!(
            max_concurrent.load(Ordering::SeqCst),
            1,
            "the shared per-repository lock must serialize mutations across independent sessions, \
             not only within one session instance"
        );
    }

    /// Multiple real threads driving mutations and reads through one
    /// literal shared `RepositorySession` (wrapped the same way
    /// `apps/desktop`'s `AppState` already wraps its session in a
    /// `Mutex`) — deterministic in the sense that every mutation fully
    /// completes (its log entry is recorded) with no lost or duplicated
    /// entries, under real contention.
    #[test]
    fn concurrent_mutations_and_reads_on_one_shared_session_never_lose_or_duplicate_work() {
        const THREADS: usize = 6;
        const OPS_PER_THREAD: usize = 10;

        let port = Arc::new(FakePort::new(clean_status()));
        let session = Arc::new(Mutex::new(RepositorySession::new(
            port,
            sample_repository("/repo-shared-session"),
        )));
        let log = Arc::new(Mutex::new(Vec::new()));

        let handles: Vec<_> = (0..THREADS)
            .map(|thread_id| {
                let session = Arc::clone(&session);
                let log = Arc::clone(&log);
                std::thread::spawn(move || {
                    for _ in 0..OPS_PER_THREAD {
                        let mut guard = session.lock().unwrap();
                        guard
                            .run_mutation(|| {
                                log.lock().unwrap().push(thread_id);
                                Ok::<(), GitSailError>(())
                            })
                            .unwrap();
                        // A concurrent "read" through the same session.
                        let _ = guard.refresh(RefreshReason::Manual);
                    }
                })
            })
            .collect();

        for handle in handles {
            handle.join().unwrap();
        }

        assert_eq!(log.lock().unwrap().len(), THREADS * OPS_PER_THREAD);
    }

    /// A spy [`Invalidatable`] that only records whether it was invalidated,
    /// standing in for a real cache ([`crate::blame_cache::BlameCache`],
    /// [`crate::graph_cache::GraphCache`]) so this test can assert on
    /// exactly when invalidation happens without constructing real cache
    /// entries.
    struct SpyCache {
        invalidated: std::sync::atomic::AtomicBool,
    }

    impl Invalidatable for SpyCache {
        fn invalidate_all(&self) {
            self.invalidated
                .store(true, std::sync::atomic::Ordering::SeqCst);
        }
    }

    /// T-227/US-116 criterion 3: a mutation invalidates relevant caches —
    /// and, symmetrically, a *failed* mutation must not: nothing about the
    /// repository actually changed, so a registered cache's existing
    /// entries are still valid, and refreshing status would only waste a
    /// read.
    #[test]
    fn run_mutation_only_refreshes_and_invalidates_caches_on_success() {
        let port = Arc::new(FakePort::new(clean_status()));
        let mut session = RepositorySession::new(port, sample_repository("/repo"));
        let spy = Arc::new(SpyCache {
            invalidated: std::sync::atomic::AtomicBool::new(false),
        });
        session.register_cache(spy.clone());

        let err = session
            .run_mutation(|| {
                Err::<(), _>(GitSailError::new(
                    ErrorCode::RepositoryLocked,
                    "locked by another process",
                ))
            })
            .unwrap_err();

        assert_eq!(err.code(), ErrorCode::RepositoryLocked);
        assert!(
            !spy.invalidated.load(std::sync::atomic::Ordering::SeqCst),
            "a failed mutation must not invalidate registered caches"
        );
        assert!(
            session.status().is_none(),
            "a failed mutation must not trigger a refresh"
        );

        session.run_mutation(|| Ok::<(), GitSailError>(())).unwrap();

        assert!(
            spy.invalidated.load(std::sync::atomic::Ordering::SeqCst),
            "a successful mutation must invalidate every registered cache"
        );
        assert!(
            session.status().is_some(),
            "a successful mutation must trigger an AfterMutation refresh"
        );
    }

    /// T-228/US-117 criterion 2: a plain refresh (manual or on-focus, not
    /// only a mutation's own `AfterMutation` refresh) that reveals a ref
    /// change underneath the session — another terminal committing,
    /// switching branches, ... — must invalidate registered caches too, not
    /// only mutations GitSail itself performed.
    #[test]
    fn a_manual_refresh_invalidates_caches_when_it_observes_an_actual_change() {
        let port = Arc::new(FakePort::new(clean_status()));
        let mut session = RepositorySession::new(port.clone(), sample_repository("/repo"));
        let spy = Arc::new(SpyCache {
            invalidated: std::sync::atomic::AtomicBool::new(false),
        });
        session.register_cache(spy.clone());
        session.refresh(RefreshReason::Manual).unwrap();

        // The very first refresh always "changes" status from `None`, so
        // reset the spy before the interesting part of this test.
        spy.invalidated
            .store(false, std::sync::atomic::Ordering::SeqCst);

        // Nothing external happened: a repeated manual refresh sees the
        // same status again and must not needlessly invalidate.
        session.refresh(RefreshReason::Manual).unwrap();
        assert!(
            !spy.invalidated.load(std::sync::atomic::Ordering::SeqCst),
            "a refresh that observes no actual change must not invalidate caches"
        );

        // Another terminal/editor changes the repository between refreshes.
        port.set_status(dirty_status());
        session.refresh(RefreshReason::Focus).unwrap();

        assert!(
            spy.invalidated.load(std::sync::atomic::Ordering::SeqCst),
            "a refresh that observes an actual change must invalidate registered caches"
        );
    }

    /// T-227/US-116 DoD: "descarte de uma leitura obsoleta que 'chega'
    /// depois de uma mutação mais recente" — a read that began before a
    /// mutation landed must never overwrite what the mutation's own refresh
    /// already applied, once it finally completes.
    #[test]
    fn a_read_started_before_a_mutation_is_discarded_once_the_mutation_completes() {
        let port = Arc::new(FakePort::new(clean_status()));
        let mut session = RepositorySession::new(port.clone(), sample_repository("/repo"));

        // A read starts (e.g. a background status poll) and is still
        // in-flight when a mutation lands.
        let stale_ticket = session.begin_refresh(RefreshReason::Manual);

        port.set_status(dirty_status());
        session.run_mutation(|| Ok::<(), GitSailError>(())).unwrap();
        assert!(!session.status().unwrap().is_clean());

        // The slow read that started before the mutation finally "arrives"
        // — it must never overwrite the mutation's newer result.
        let applied = session.apply_refresh(stale_ticket, clean_status());

        assert!(
            !applied,
            "a read that started before a mutation must be discarded, not overwrite it"
        );
        assert!(!session.status().unwrap().is_clean());
    }
}
