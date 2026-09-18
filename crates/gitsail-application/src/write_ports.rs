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
    BranchName, CancellationToken, CommitHash, ConflictSide, ConflictedFile, ErrorCode, FileDiff,
    GitSailError, Repository, Stash, Worktree,
};

use crate::mutation::Precondition;

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

/// Preview of what applying a candidate patch would do, built from a
/// non-mutating `git apply --check` (US-030 criterion 1): a presentation
/// layer shows this — affected files and whether the destination is
/// supported — *before* asking for confirmation. Building this never
/// touches the working tree, the index, or any ref.
///
/// [`Self::affected_files`] is derived directly from the patch's own
/// `---`/`+++` unified-diff headers, independent of whether `git apply`
/// would accept it — a rejected patch still shows what it *claimed* to
/// touch (e.g. a path traversal attempt shows the offending path itself,
/// which is exactly what makes [`Self::rejection_reason`] concrete rather
/// than generic).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PatchPreview {
    /// Every path the patch's headers declare touching, in header order,
    /// deduplicated. Populated even when [`Self::supported`] is `false`, to
    /// the extent the patch text has recognizable file headers at all (a
    /// patch with none yields an empty list here rather than a fabricated
    /// guess).
    pub affected_files: Vec<PathBuf>,
    /// Whether this exact patch, against the repository's current state,
    /// can actually be applied. `false` for a malformed patch, a patch
    /// referencing a path outside the repository, or a patch whose context
    /// no longer matches the current file content (US-030 criterion 2).
    pub supported: bool,
    /// A clear, human-readable reason `supported` is `false`. Always `None`
    /// when `supported` is `true`.
    pub rejection_reason: Option<String>,
}

/// Result of a successful [`RepositoryWritePort::apply_patch`] (US-030
/// criterion 3): exactly which files were modified, so a caller never has
/// to guess or re-diff to find out.
///
/// There is deliberately no "applied some files, failed on others" variant
/// here: a single `git apply` invocation validates every hunk of every file
/// *before* writing anything, so it either applies the whole patch or
/// writes nothing at all — verified empirically against a real multi-file
/// patch where one file's hunk was made stale on purpose (see
/// `gitsail-git`'s `apply_patch_is_all_or_nothing_across_files` test): the
/// working tree came back completely untouched, not partially patched.
/// [`RepositoryWritePort::apply_patch`] additionally never passes
/// `--unsafe-paths`, so it can never write outside the repository either
/// (verified against a real path-traversal and a real symlink-escape
/// patch in that same test module) — a failure from it is always a plain
/// `Err` with the repository left exactly as it was found, never described
/// as "rolled back" (Git provides no such guarantee, and this port makes
/// none either).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ApplyPatchResult {
    pub applied_files: Vec<PathBuf>,
}

/// Outcome of [`RepositoryWritePort::pull`] (US-097 criterion 2's "fast-
/// forward only" policy). Mirrors [`StashApplyOutcome`]'s "legitimate,
/// expected outcome, not an error" convention: a pull with nothing new to
/// integrate is a success, not a special-cased failure.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PullOutcome {
    /// The current branch was already even with (or ahead of) `remote`'s
    /// tracked branch; nothing changed.
    AlreadyUpToDate,
    /// The current branch fast-forwarded to `new_head`.
    FastForwarded { new_head: CommitHash },
}

/// Outcome of [`RepositoryWritePort::merge`] (T-231/US-079 criterion 2):
/// fast-forward, a new merge commit, and a conflict are always reported as
/// three distinct, explicit results — never collapsed into one another, and
/// a conflict is never reported as a completed success (History Editing
/// Rules #1).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MergeResult {
    /// The current branch had no divergent work of its own: HEAD simply
    /// moved forward to the target's commit, with no new commit object
    /// created. Also reported when the target was already the current
    /// branch's ancestor (Git's own "Already up to date") — HEAD ends at
    /// `new_head` either way, just without having moved in that
    /// degenerate case.
    FastForwarded { new_head: CommitHash },
    /// Both sides had diverged, non-conflicting work: Git created a new
    /// two-parent merge commit, `hash`.
    MergeCommitCreated { hash: CommitHash },
    /// The merge could not be completed automatically: `files` are left
    /// unmerged in the index, exactly as
    /// [`crate::ports::RepositoryReadPort::detect_in_progress_operation`]
    /// would also report them, and the repository is left with a pending
    /// merge (`InProgressOperation::Merge`) for T-232/T-233's continue/abort
    /// flow to pick up.
    Conflict { files: Vec<ConflictedFile> },
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

    /// Renames the local branch `old_name` to `new_name` via `git branch
    /// -m` (T-157/US-024). Never overwrites a colliding reference — Git
    /// itself refuses `-m` when `new_name` already names another branch,
    /// and this port never passes `-M`/force to work around that refusal
    /// (US-024 criterion 2: "colisão de nome não sobrescreve a
    /// referência"). Works whether or not `old_name` is the currently
    /// checked-out branch: Git's own `-m` renames the checked-out branch in
    /// place (still current, under its new name) exactly as it renames any
    /// other local branch, so this port needs no special-casing for either
    /// case, unlike [`Self::delete_branch`]'s current-branch protection.
    /// Any configured upstream on `old_name` survives the rename (Git
    /// itself carries `branch.<name>.remote`/`.merge` over to the new
    /// name) — presenting that, and the branch's new current-ness, to the
    /// person afterward is a presentation-layer concern (US-024 criterion
    /// 2's "contexto resultante ... fica visível"), not this port's.
    fn rename_branch(
        &self,
        repo: &Repository,
        old_name: &BranchName,
        new_name: &BranchName,
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

    // -------------------------------------------------------------------
    // EPIC-19/T-211..T-214 (US-096..099): remote operations (fetch, pull,
    // push, force-push-with-lease). Defaulted the same way as EPIC-18's own
    // batch above, for the same reason (existing `RepositoryWritePort`
    // implementers predating this epic — `gitsail-tui`'s and
    // `apps/desktop`'s own fakes — keep compiling unchanged). Every method
    // here takes an explicit `remote`/`branch` (never an implicit "the"
    // remote/upstream — US-096 criterion 1, US-098 criterion 1) and a
    // [`CancellationToken`] (a network-bound call can run long; a caller
    // running it off its own render/event loop can still stop it, matching
    // [`crate::ports::RepositoryReadPort::diff`]/`blame`'s own convention).
    // [`gitsail_git::GitCliProvider`] overrides every one of these with a
    // real `git` implementation.

    /// Fetches `remote`'s refs into this repository's own remote-tracking
    /// refs (`refs/remotes/<remote>/...`) via `git fetch` (US-096). `Safe`
    /// per SAD §20's own named example: this never touches the working
    /// tree, the index, or any ref a person has actually checked out — only
    /// `refs/remotes/*` moves. A network/authentication failure is reported
    /// with a distinct, classified [`gitsail_domain::ErrorCode`]
    /// (`NetworkFailure`/`AuthenticationRequired`) rather than a bare
    /// [`gitsail_domain::ErrorCode::ProcessFailure`] (US-096 criterion 2),
    /// and cancellation via `cancel` is likewise distinct
    /// (`ErrorCode::Cancelled`) from either.
    fn fetch(
        &self,
        repo: &Repository,
        remote: &str,
        cancel: &CancellationToken,
    ) -> Result<(), GitSailError> {
        let _ = (repo, remote, cancel);
        Err(unsupported("fetch"))
    }

    /// Integrates `remote`'s tracked `branch` into the current branch
    /// (US-097). This version's policy is fixed and fast-forward-only
    /// (US-097 criterion 2, documented here rather than left to each
    /// caller to rediscover): it fetches `remote` first — so the
    /// remote-tracking ref this compares against is current, not whatever a
    /// caller last happened to observe — then integrates only when the
    /// current branch can fast-forward to it. When the two have diverged,
    /// this refuses outright (never merges, rebases, or resets — US-097
    /// criterion 3) with [`gitsail_domain::ErrorCode::OperationConflict`];
    /// automatic merge/rebase recovery is out of scope for this version (see
    /// [`RepositoryWritePort`]'s module doc / the EPIC-19 session report).
    fn pull(
        &self,
        repo: &Repository,
        remote: &str,
        branch: &BranchName,
        cancel: &CancellationToken,
    ) -> Result<PullOutcome, GitSailError> {
        let _ = (repo, remote, branch, cancel);
        Err(unsupported("pull"))
    }

    /// Publishes the current local `branch` to `remote` via a plain `git
    /// push` (US-098). Never passes `--force`: when the remote already has
    /// commits this branch does not (a non-fast-forward rejection), this
    /// simply fails with [`gitsail_domain::ErrorCode::OperationConflict`]
    /// (US-098 criterion 2) rather than ever escalating to force
    /// automatically. A network/authentication failure never reports a
    /// false success (US-098 criterion 3); the remote's real state can
    /// always be reinspected afterward via [`Self::fetch`] regardless of how
    /// this call resolved.
    fn push(
        &self,
        repo: &Repository,
        remote: &str,
        branch: &BranchName,
        cancel: &CancellationToken,
    ) -> Result<(), GitSailError> {
        let _ = (repo, remote, branch, cancel);
        Err(unsupported("push"))
    }

    /// Force-publishes rewritten history for `branch` to `remote` via `git
    /// push --force-with-lease` (US-099) — a name and
    /// [`crate::mutation::MutationKind`] deliberately distinct from
    /// [`Self::push`] (US-099 criterion 1: never conflated with a common
    /// push). `expected_remote_head` is the commit hash a caller last
    /// observed as `remote`'s tip for `branch` (typically from a prior
    /// [`Self::fetch`] plus a read of the resulting remote-tracking ref);
    /// this is a [`Precondition`] carried the same way
    /// [`Self::amend_commit`]'s `expected_head` is, but checked by the
    /// remote itself at push time (via `--force-with-lease=<branch>:
    /// <expected>`) rather than compared locally first — a local-only
    /// comparison could never be race-free against a server another
    /// process/machine can also push to in between (US-099 criterion 2:
    /// "compare and swap"). When the remote has moved since — another
    /// push landed in the meantime — this is refused, and never silently
    /// retried as an unconditional `--force` (US-099 criterion 3).
    fn force_push_with_lease(
        &self,
        repo: &Repository,
        remote: &str,
        branch: &BranchName,
        expected_remote_head: &Precondition<CommitHash>,
        cancel: &CancellationToken,
    ) -> Result<(), GitSailError> {
        let _ = (repo, remote, branch, expected_remote_head, cancel);
        Err(unsupported("force_push_with_lease"))
    }

    // -------------------------------------------------------------------
    // EPIC-06/T-163 (US-030): applying a selected patch. Defaulted the same
    // way as the EPIC-18/19 batches above, for the same reason (existing
    // `RepositoryWritePort` implementers predating this task keep compiling
    // unchanged). [`gitsail_git::GitCliProvider`] overrides both with a real
    // `git apply` implementation.

    /// Validates `patch_text` against the repository's current state via a
    /// non-mutating `git apply --check` and reports which files it would
    /// touch (US-030 criterion 1). Never passes `--unsafe-paths` — Git
    /// itself then refuses a patch referencing an absolute path, a path
    /// with a `..` component, or a path reached through a symbolic link,
    /// all verified empirically (see [`ApplyPatchResult`]'s doc) — so this
    /// preview, and [`Self::apply_patch`] which shares the same invocation
    /// shape, can never write outside the repository.
    fn preview_patch_application(
        &self,
        repo: &Repository,
        patch_text: &str,
    ) -> Result<PatchPreview, GitSailError> {
        let _ = (repo, patch_text);
        Err(unsupported("preview_patch_application"))
    }

    /// Applies `patch_text` to the working tree via a plain `git apply`
    /// (never `--cached`/`--index`: this reuses changes into the working
    /// tree for a person to review/stage/commit normally, a distinct
    /// capability from [`Self::stage_hunks`]'s index-only hunk staging).
    ///
    /// Revalidates with the same `--check` [`Self::preview_patch_application`]
    /// performs, immediately before writing anything (Destructive
    /// Operations & Confirmation Guardrails rule 4: a confirmation given
    /// against an older state must never authorize execution against a
    /// newer one) — a caller that already previewed moments ago still gets
    /// this re-check for free, rather than having to remember to ask for it
    /// again itself. Rejects a malformed patch, a patch referencing a path
    /// outside the repository, or a patch whose context no longer matches
    /// the current file content (US-030 criterion 2), and never applies
    /// part of the patch while rejecting the rest of it (US-030 criterion
    /// 3; see [`ApplyPatchResult`]'s doc for why that "partial apply" case
    /// does not exist for a single `git apply` invocation).
    fn apply_patch(
        &self,
        repo: &Repository,
        patch_text: &str,
    ) -> Result<ApplyPatchResult, GitSailError> {
        let _ = (repo, patch_text);
        Err(unsupported("apply_patch"))
    }

    // -------------------------------------------------------------------
    // EPIC-16/T-231..T-233 (US-079..081): merge, conflict resolution, and
    // continue/abort of a pending operation. Defaulted the same way as the
    // EPIC-18/19 batches above, for the same reason (existing
    // `RepositoryWritePort` implementers predating this epic keep compiling
    // unchanged). [`gitsail_git::GitCliProvider`] overrides every one of
    // these with a real `git` implementation.

    /// Integrates `target_revision` (a branch, tag, or other Git revision
    /// expression — mirrors
    /// [`crate::ports::RepositoryReadPort::resolve_revision`]'s own
    /// `revision: &str`) into the current branch via `git merge` (T-231/
    /// US-079). Refuses up front when another [`gitsail_domain::InProgressOperation`]
    /// is already pending (T-230/US-078 criterion 3) rather than starting a
    /// second one on top of it. Never opens an interactive editor for the
    /// merge commit message (a headless process has no terminal to satisfy
    /// one). See [`MergeResult`] for why fast-forward/merge-commit/conflict
    /// are always reported as three distinct outcomes (US-079 criterion 2),
    /// never a generic error for the conflict case.
    fn merge(&self, repo: &Repository, target_revision: &str) -> Result<MergeResult, GitSailError> {
        let _ = (repo, target_revision);
        Err(unsupported("merge"))
    }

    /// Stages `path`'s currently-conflicted content into the index via
    /// `git add -- <path>` (T-232/US-080 criterion 3), marking it resolved.
    /// Only ever called on the caller's own explicit action — this port
    /// itself never infers that a conflict "looks resolved" and calls this
    /// automatically; that judgment call belongs entirely to the
    /// presentation layer (US-080 criterion 3: "nunca automaticamente").
    /// Works identically whether the file's current content was produced by
    /// editing inside GitSail or resolved externally (another editor) and
    /// then merely observed on the next refresh (US-080 criterion 2) — `git
    /// add` only ever looks at the working tree's current content, not at
    /// how it got there.
    fn mark_conflict_resolved(&self, repo: &Repository, path: &Path) -> Result<(), GitSailError> {
        let _ = (repo, path);
        Err(unsupported("mark_conflict_resolved"))
    }

    /// Resolves `path`'s conflict by taking `side`'s content wholesale
    /// (`git checkout --ours`/`--theirs -- <path>`), then stages it exactly
    /// like [`Self::mark_conflict_resolved`] (T-232/US-080 criterion 3's
    /// documented binary-conflict flow — a binary file has no meaningful
    /// textual merge, so choosing one side in full *is* the resolution).
    /// Equally usable for a text file a person simply wants to resolve by
    /// taking one side outright, without editing it.
    fn take_conflict_side(
        &self,
        repo: &Repository,
        path: &Path,
        side: ConflictSide,
    ) -> Result<(), GitSailError> {
        let _ = (repo, path, side);
        Err(unsupported("take_conflict_side"))
    }

    /// Resumes whichever multi-step operation
    /// [`crate::ports::RepositoryReadPort::detect_in_progress_operation`]
    /// currently detects (`git <op> --continue`) — merge, rebase,
    /// cherry-pick, or revert (T-233/US-081; generic across all four so
    /// T-235+/T-238/T-239 reuse this unchanged, even though only merge calls
    /// it today). Refuses, rather than guessing, when: no operation is
    /// pending; the detected operation's own
    /// [`gitsail_domain::OperationCapability`] set does not offer `Continue`
    /// (US-081 criterion 1 — e.g. a bisect run, which has no `Continue` of
    /// this shape); or conflicted files still remain (US-081 criterion 2).
    /// Never presumes success: a caller re-inspects
    /// `detect_in_progress_operation` afterward to see the real resulting
    /// state (US-081 criterion 3), this method's own `Ok(())` only means
    /// "the command exited successfully", not "the operation is now fully
    /// concluded and no other issue turned up".
    fn continue_operation(&self, repo: &Repository) -> Result<(), GitSailError> {
        let _ = repo;
        Err(unsupported("continue_operation"))
    }

    /// Abandons whichever multi-step operation
    /// [`crate::ports::RepositoryReadPort::detect_in_progress_operation`]
    /// currently detects (`git <op> --abort`), restoring the pre-operation
    /// state as far as Git itself guarantees — e.g. for a merge, HEAD before
    /// the merge started, discarding the merge's own in-progress changes,
    /// but never touching unrelated work (T-233/US-081 criterion 3). Generic
    /// across merge/rebase/cherry-pick/revert/bisect, matching
    /// [`Self::continue_operation`]'s own reuse rationale. Refuses when no
    /// operation is pending or the detected operation's
    /// [`gitsail_domain::OperationCapability`] set does not offer `Abort`.
    /// Never presumes success: a caller re-inspects
    /// `detect_in_progress_operation` afterward (US-081 criterion 3).
    fn abort_operation(&self, repo: &Repository) -> Result<(), GitSailError> {
        let _ = repo;
        Err(unsupported("abort_operation"))
    }
}
