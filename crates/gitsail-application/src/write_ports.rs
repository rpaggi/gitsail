//! Write-side port for repository mutation (SAD §9, §10, §20; ADR-009).
//!
//! Deliberately a separate trait from [`crate::ports::RepositoryReadPort`]:
//! a consumer that only holds a `RepositoryReadPort` must never gain
//! mutation capability just by virtue of depending on this crate (ADR-009).
//! `gitsail-git`'s `GitCliProvider` implements both traits, but application
//! code depends on whichever capability it actually needs.
//!
//! Per SAD §20, `stage`/`unstage` are Safe operations and `commit` is
//! Moderate; the full risk-description/revalidation framework (US-111) and
//! cross-operation mutation serialization (US-116) are separate, not-yet-
//! built epics (EPIC-22/EPIC-23) and are out of scope here. A single
//! `RepositoryWritePort` call already only ever runs one `git` process at a
//! time, so it cannot corrupt the index by itself; coordinating *across*
//! concurrent calls from multiple sessions is left to that future work.

use std::path::{Path, PathBuf};

use gitsail_domain::{
    BranchName, CommitHash, ErrorCode, FileDiff, GitSailError, Repository, Stash, Worktree,
};

/// Explicit scope for [`RepositoryWritePort::create_stash`] (US-092
/// criterion 1). Every flag mirrors a real `git stash push` flag 1:1 and
/// must be requested explicitly by the caller — there is no hidden
/// adapter-chosen default a person cannot see or control, and no flag
/// implies another (in particular, `all` is never implied by
/// `include_untracked`: US-092 criterion 2, "arquivos ignorados
/// (`.gitignore`) não entram silenciosamente").
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct StashScope {
    /// `git stash push --keep-index`: staged changes remain staged in the
    /// index/working tree after the stash is created, instead of being
    /// reset back to `HEAD` along with everything else `git stash` captures.
    pub keep_index: bool,
    /// `git stash push --include-untracked`: untracked (but not ignored)
    /// files are captured too.
    pub include_untracked: bool,
    /// `git stash push --all`: also captures files ignored via
    /// `.gitignore`.
    pub all: bool,
}

/// Outcome of [`RepositoryWritePort::apply_stash`]/
/// [`RepositoryWritePort::pop_stash`] (US-093 criterion 2).
///
/// A conflict is a legitimate, expected outcome here, never a
/// [`GitSailError`]: Git already reports it structurally — a non-zero exit
/// that still modifies the working tree/index, the same shape a failed
/// merge takes — so this type carries that distinction through rather than
/// collapsing it into either an opaque success or a generic process error
/// (mirrors [`gitsail_domain::FileContentKind`]'s "Missing/Binary are
/// legitimate outcomes" convention).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StashApplyOutcome {
    /// `true` when Git reported one or more merge conflicts while restoring
    /// the stash. The stash entry itself is always preserved in this case —
    /// for `apply` that is already Git's normal behavior; for `pop`, this
    /// port never lets a conflicted restoration go on to drop the stash
    /// (US-093 criterion 2: "nunca faça `pop` remover o stash se a
    /// aplicação gerou conflito").
    pub had_conflicts: bool,
}

/// Whether a new tag is lightweight or carries an annotation (US-094
/// criterion 1).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TagAnnotation {
    Lightweight,
    Annotated { message: String },
}

/// How a new worktree's `HEAD` is populated (US-095 criterion 2).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorktreeBranchSpec {
    /// Checks out an existing local branch. Git itself refuses when that
    /// branch is already checked out in another worktree (US-095 criterion
    /// 2: "Git recusa usar uma branch já checked-out em outro worktree —
    /// propague esse erro com clareza, não tente contornar"); this port
    /// propagates that refusal rather than working around it.
    ExistingBranch(BranchName),
    /// Creates a new branch named `name` (from `start_point`, or the
    /// current `HEAD` when `None`) and checks it out in the new worktree —
    /// `git worktree add -b`.
    NewBranch {
        name: BranchName,
        start_point: Option<CommitHash>,
    },
    /// Checks out `commit` directly, detached.
    Detached(CommitHash),
}

/// The error every new-in-EPIC-18 mutation method below defaults to when a
/// `RepositoryWritePort` implementer does not override it (see this
/// module's trait doc for why a default exists at all rather than a
/// required method).
fn unsupported(operation: &str) -> GitSailError {
    GitSailError::new(
        ErrorCode::Internal,
        format!("{operation} is not supported by this write port"),
    )
}

/// Mutation capability against a Git repository's index and history
/// (SAD §9's v0.2/v0.3 mutation use cases; ADR-009).
///
/// `Send + Sync` so an `Arc<dyn RepositoryWritePort>` can be handed to a
/// background thread, matching [`crate::ports::RepositoryReadPort`] — first
/// exercised by `gitsail-tui` (EPIC-10), which never runs a mutation on its
/// render/event loop (SAD §18, §26).
pub trait RepositoryWritePort: Send + Sync {
    /// Stages exactly `paths` into the index, including paths whose
    /// working-tree entry was deleted (US-011 criterion 1).
    fn stage_files(&self, repo: &Repository, paths: &[PathBuf]) -> Result<(), GitSailError>;

    /// Unstages exactly `paths`, leaving the working tree untouched. Works
    /// before the first commit exists (US-011 criteria 2, 3).
    fn unstage_files(&self, repo: &Repository, paths: &[PathBuf]) -> Result<(), GitSailError>;

    /// Commits exactly the current index content with `message`. Never
    /// creates an empty commit implicitly; returns the new commit's hash on
    /// success (US-012).
    fn create_commit(&self, repo: &Repository, message: &str) -> Result<CommitHash, GitSailError>;

    /// Stages only the hunks carried by `selection`, each element being a
    /// [`FileDiff`] trimmed down to the hunks to stage (typically a subset
    /// of what [`crate::ports::RepositoryReadPort::diff`] returned for the
    /// unstaged diff). The working tree is never touched. Fails, rather
    /// than guessing, when a hunk no longer applies cleanly to the current
    /// index (US-013 criteria 1-3).
    fn stage_hunks(&self, repo: &Repository, selection: &[FileDiff]) -> Result<(), GitSailError>;

    /// Unstages only the hunks carried by `selection` (typically a subset
    /// of the staged diff), leaving the working tree untouched and the rest
    /// of the index intact.
    fn unstage_hunks(&self, repo: &Repository, selection: &[FileDiff]) -> Result<(), GitSailError>;

    /// Switches HEAD (and the working tree/index) to `target`. Refuses,
    /// rather than discarding, when the switch would overwrite local
    /// changes incompatible with `target` (US-021 criteria 2, 3). Whether
    /// the destination and any at-risk local changes are surfaced to the
    /// person *before* this call is a presentation-layer concern (US-021
    /// criterion 1) outside this port's contract; this call only guarantees
    /// that an incompatible switch never silently discards work.
    fn switch_branch(&self, repo: &Repository, target: &BranchName) -> Result<(), GitSailError>;

    /// Creates a local branch named `name`, pointing at `start_point` (or
    /// the current `HEAD` when `None`). Never switches to it and never
    /// overwrites an existing branch of the same name (US-022 criteria 2,
    /// 3).
    fn create_branch(
        &self,
        repo: &Repository,
        name: &BranchName,
        start_point: Option<&CommitHash>,
    ) -> Result<(), GitSailError>;

    /// Deletes the local branch `name`. With `force: false` (the default a
    /// caller should offer), refuses to delete a branch with unmerged
    /// commits rather than discarding them implicitly (US-023 criterion 3);
    /// `force: true` deletes regardless. Always refuses to delete the
    /// current branch or one checked out in another worktree (US-023
    /// criterion 2) — Git enforces this itself, so this port only surfaces
    /// it as a clear, classified error rather than an opaque process
    /// failure. Confirming the branch and scope with the person before
    /// calling this (US-023 criterion 1) is a presentation-layer concern.
    fn delete_branch(
        &self,
        repo: &Repository,
        name: &BranchName,
        force: bool,
    ) -> Result<(), GitSailError>;

    /// Replaces `HEAD`'s commit with a new one carrying `message` and
    /// whatever is currently staged (US-059). `expected_head` is the commit
    /// hash the caller last observed as `HEAD` (typically from a prior
    /// preview read) — this call revalidates it is still `HEAD` immediately
    /// before amending and refuses with [`gitsail_domain::ErrorCode::OperationConflict`]
    /// otherwise (US-059 criterion 3), the same "revalidate right before
    /// mutating" discipline `AppState`'s session epoch applies on the
    /// Desktop side. This is what keeps amend from silently rewriting a
    /// *different* commit than the one the person previewed, e.g. because
    /// another process committed in between. Returns the new commit's hash
    /// on success.
    fn amend_commit(
        &self,
        repo: &Repository,
        message: &str,
        expected_head: &CommitHash,
    ) -> Result<CommitHash, GitSailError>;

    // -------------------------------------------------------------------
    // EPIC-18/T-217..T-220 (US-092..095): stash, tag and worktree mutation.
    // -------------------------------------------------------------------
    //
    // Each method below has a default implementation returning
    // `Err(ErrorCode::Internal)` rather than being required: this is a large
    // batch of new mutation capability landing on a trait that already had
    // several independent implementers/test doubles predating this epic
    // (`gitsail-tui`'s and `apps/desktop`'s own `RepositoryWritePort` fakes),
    // none of which this epic's scope touches (see the EPIC-18 session
    // report). A default that fails loudly if ever actually called —
    // instead of a mechanical `unimplemented!()` stub every existing
    // implementer would otherwise need added just to keep compiling — keeps
    // those call sites unaffected until they deliberately opt in.
    // [`gitsail_git::GitCliProvider`] overrides every one of these with a
    // real `git` implementation; [`crate::write_use_cases`]'s own test
    // double overrides them too, to exercise the new use cases.

    /// Creates a new stash from the current working tree/index, scoped by
    /// `scope` (US-092 criterion 1), with an optional `message` (US-092
    /// criterion 2: message and scope are always explicit, never inferred).
    /// Returns the created entry (always the new `stash@{0}`) so a caller
    /// can show what was captured without a second read (US-092 criterion
    /// 3).
    fn create_stash(
        &self,
        repo: &Repository,
        message: Option<&str>,
        scope: StashScope,
    ) -> Result<Stash, GitSailError> {
        let _ = (repo, message, scope);
        Err(unsupported("create_stash"))
    }

    /// Applies `expected` (a stash entry a caller previously observed via
    /// [`crate::ports::RepositoryReadPort::list_stash_entries`]) without
    /// removing it. Revalidates `expected` is still the same entry at the
    /// same position immediately before applying (US-093 criterion 1: "usam
    /// a identidade revalidada do stash"), refusing with
    /// [`gitsail_domain::ErrorCode::OperationConflict`] when the stash list
    /// has changed underneath the caller (e.g. another entry was pushed or
    /// dropped, shifting indices) — the same [`crate::Precondition`]
    /// discipline [`Self::amend_commit`] already applies to `HEAD`.
    fn apply_stash(
        &self,
        repo: &Repository,
        expected: &Stash,
    ) -> Result<StashApplyOutcome, GitSailError> {
        let _ = (repo, expected);
        Err(unsupported("apply_stash"))
    }

    /// Applies `expected`, then removes it from the stash list — but only
    /// when the apply succeeded without conflicts (US-093 criterion 2: a
    /// conflicted restoration must never also lose the stash). Revalidation
    /// matches [`Self::apply_stash`].
    fn pop_stash(
        &self,
        repo: &Repository,
        expected: &Stash,
    ) -> Result<StashApplyOutcome, GitSailError> {
        let _ = (repo, expected);
        Err(unsupported("pop_stash"))
    }

    /// Deletes `expected` without applying it. Revalidation matches
    /// [`Self::apply_stash`]. This is
    /// [`gitsail_application::MutationKind::DropStash`]'s
    /// `Destructive`-classified operation (SAD §20's own canonical example
    /// of a destructive mutation) — reinforced confirmation (naming the
    /// exact entry, e.g. requiring its index/hash typed back) is a
    /// presentation-layer concern (US-093 criterion 3), not this port's.
    fn drop_stash(&self, repo: &Repository, expected: &Stash) -> Result<(), GitSailError> {
        let _ = (repo, expected);
        Err(unsupported("drop_stash"))
    }

    /// Creates a local tag named `name` at `target` (or the current `HEAD`
    /// when `None`), lightweight or annotated per `annotation` (US-094
    /// criterion 1). Never overwrites an existing tag of the same name —
    /// Git itself refuses that without `-f`, and this port never passes
    /// `-f` (US-094 criterion 1: "colisão de nome não sobrescreve
    /// silenciosamente"). Always local: never contacts a remote (US-094
    /// criterion 3).
    fn create_tag(
        &self,
        repo: &Repository,
        name: &str,
        target: Option<&CommitHash>,
        annotation: TagAnnotation,
    ) -> Result<(), GitSailError> {
        let _ = (repo, name, target, annotation);
        Err(unsupported("create_tag"))
    }

    /// Deletes the local tag `name` (US-094 criterion 2: the caller is
    /// expected to have confirmed this exact name before calling). Always
    /// local: never contacts a remote (US-094 criterion 3).
    fn delete_tag(&self, repo: &Repository, name: &str) -> Result<(), GitSailError> {
        let _ = (repo, name);
        Err(unsupported("delete_tag"))
    }

    /// Creates a new worktree at `path`, populating its `HEAD` per `branch`
    /// (US-095 criterion 2). Fails — rather than working around it — when
    /// `path` already exists as a non-empty, unexpected directory, or when
    /// `branch` names a branch already checked out elsewhere; both are Git's
    /// own refusals, surfaced clearly rather than masked. Returns the
    /// created worktree so a caller can show it without a second read.
    fn create_worktree(
        &self,
        repo: &Repository,
        path: &Path,
        branch: WorktreeBranchSpec,
    ) -> Result<Worktree, GitSailError> {
        let _ = (repo, path, branch);
        Err(unsupported("create_worktree"))
    }

    /// Removes the worktree at `path`. With `force: false` (the default a
    /// caller should offer), refuses to remove a worktree with uncommitted
    /// changes rather than discarding them implicitly (US-095 criterion 3);
    /// `force: true` removes it regardless — reinforced confirmation for
    /// that case is a presentation-layer concern, matching
    /// [`Self::delete_branch`]'s own `force` contract.
    fn remove_worktree(
        &self,
        repo: &Repository,
        path: &Path,
        force: bool,
    ) -> Result<(), GitSailError> {
        let _ = (repo, path, force);
        Err(unsupported("remove_worktree"))
    }
}
