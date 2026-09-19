//! Mutation risk metadata and precondition revalidation (SAD §20, §21, §26;
//! EPIC-22/T-222/US-111).
//!
//! Two concerns live here, both explicitly Core (application-layer)
//! capabilities that presentations *consume* rather than reinvent (SAD §20:
//! "the application layer supplies operation intent and risk metadata"):
//!
//! 1. [`RiskLevel`]/[`MutationKind`] — a single, canonical Safe/Moderate/
//!    Destructive classification for every `RepositoryWritePort` mutation
//!    that exists today. Before this module, `gitsail-tui`'s
//!    `OperationKind::risk` and `apps/desktop`'s
//!    `stores/operation.ts::OperationRisk` each hand-rolled the same
//!    three-tier taxonomy independently (the Desktop store's own comment
//!    calls amend's Destructive classification "a Desktop-side design
//!    decision, no TUI precedent exists" — this module now makes that
//!    decision once, centrally, so neither frontend has to guess again).
//! 2. [`Precondition`] — generalizes the "capture state now, re-check it
//!    immediately before the mutation actually runs" discipline
//!    `RepositoryWritePort::amend_commit`'s `expected_head` parameter
//!    already implements inline, so any future mutation with the same
//!    preview/execute race window (an interactive rebase plan against
//!    current refs, a reset preview against current HEAD, ...) reuses one
//!    tested mechanism instead of hand-rolling its own comparison.
//!
//! Neither TUI nor Desktop is migrated to consume this module by this
//! change — see the EPIC-22 session report for why that is a deliberate,
//! separate, out-of-scope-here migration.

use std::fmt;
use std::path::PathBuf;

use gitsail_domain::{ConflictSide, ErrorCode, GitSailError};

/// Risk tier for a mutating operation, per SAD §20's exact three-tier
/// taxonomy ("Safe: fetch, stage, unstage"; "Moderate: commit, checkout,
/// merge"; "Destructive: reset --hard, discard changes, force push, stash
/// drop"). Presentation layers must surface exactly these three labels
/// (wiki "Destructive Operations & Confirmation Guardrails" rule 2) rather
/// than invent their own.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RiskLevel {
    Safe,
    Moderate,
    Destructive,
}

impl RiskLevel {
    /// Whether a presentation layer must require *reinforced* confirmation —
    /// naming the exact target and expected loss, never a generic "are you
    /// sure?" dialog (SAD §20; wiki rule 3). Only `Destructive` operations
    /// require this.
    pub const fn requires_reinforced_confirmation(self) -> bool {
        matches!(self, RiskLevel::Destructive)
    }

    /// Whether a presentation layer may skip an explicit confirmation step
    /// and run immediately (mirrors the "Safe operations skip Confirming"
    /// rule `gitsail-tui`'s `OperationState` and `apps/desktop`'s
    /// `operation` store both already implement independently).
    pub const fn skips_confirmation(self) -> bool {
        matches!(self, RiskLevel::Safe)
    }
}

impl fmt::Display for RiskLevel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            RiskLevel::Safe => "safe",
            RiskLevel::Moderate => "moderate",
            RiskLevel::Destructive => "destructive",
        })
    }
}

/// Every mutation `RepositoryWritePort` exposes today, named with its target
/// so a confirmation prompt built from this can always be unambiguous
/// (never a generic "mutate" bucket) — mirrors `gitsail-tui::operation::
/// OperationKind`'s shape, extended to cover `stage_hunks`/`unstage_hunks`
/// (not modeled there yet) and `amend_commit` (not modeled in the TUI at
/// all, only in the Desktop store).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MutationKind {
    StageFiles,
    UnstageFiles,
    StageHunks,
    UnstageHunks,
    CreateCommit,
    SwitchBranch {
        target: String,
    },
    CreateBranch {
        name: String,
    },
    DeleteBranch {
        name: String,
        force: bool,
    },
    /// T-157/US-024: `RepositoryWritePort::rename_branch`.
    RenameBranch {
        old_name: String,
        new_name: String,
    },
    /// Rewrites `HEAD` in place (SAD §20's "history rewrite" category of
    /// destructive action, via `RepositoryWritePort::amend_commit`).
    AmendCommit,
    /// EPIC-18/US-092: `RepositoryWritePort::create_stash`.
    CreateStash,
    /// EPIC-18/US-093: `RepositoryWritePort::apply_stash`, identified by the
    /// stash's index at preview time (`stash@{index}`).
    ApplyStash {
        index: u32,
    },
    /// EPIC-18/US-093: `RepositoryWritePort::pop_stash`.
    PopStash {
        index: u32,
    },
    /// EPIC-18/US-093: `RepositoryWritePort::drop_stash`. SAD §20 names
    /// "stash drop" as its own canonical example of a `Destructive`
    /// mutation.
    DropStash {
        index: u32,
    },
    /// EPIC-18/US-094: `RepositoryWritePort::create_tag`.
    CreateTag {
        name: String,
    },
    /// EPIC-18/US-094: `RepositoryWritePort::delete_tag`.
    DeleteTag {
        name: String,
    },
    /// EPIC-18/US-095: `RepositoryWritePort::create_worktree`.
    CreateWorktree {
        path: PathBuf,
    },
    /// EPIC-18/US-095: `RepositoryWritePort::remove_worktree`.
    RemoveWorktree {
        path: PathBuf,
        force: bool,
    },
    /// EPIC-19/US-096: `RepositoryWritePort::fetch`. SAD §20 explicitly
    /// lists fetch among its own canonical `Safe` examples.
    Fetch {
        remote: String,
    },
    /// EPIC-19/US-097: `RepositoryWritePort::pull` — a fast-forward-only
    /// integration of `remote`'s tracked `branch` in this version (see that
    /// method's doc for why divergence is refused rather than merged/
    /// rebased automatically).
    Pull {
        remote: String,
        branch: String,
    },
    /// EPIC-19/US-098: `RepositoryWritePort::push` — a plain, non-force
    /// push that never rewrites history.
    Push {
        remote: String,
        branch: String,
    },
    /// EPIC-19/US-099: `RepositoryWritePort::force_push_with_lease`. Named
    /// distinctly from [`MutationKind::Push`] so a confirmation prompt is
    /// never generic (US-099 criterion 1): this is the one operation in
    /// this module that can discard *remote* history another collaborator
    /// already observed.
    ForcePushWithLease {
        remote: String,
        branch: String,
    },
    /// EPIC-06/T-163 (US-030): `RepositoryWritePort::apply_patch`. Carries
    /// the affected-file count from the already-computed
    /// [`crate::write_ports::PatchPreview`] so a confirmation prompt names
    /// concrete scope rather than a generic "apply a patch" (mirrors
    /// [`Self::target_label`]'s convention across every other variant here).
    ApplyPatch {
        affected_file_count: usize,
    },
    /// EPIC-16/T-231 (US-079): `RepositoryWritePort::merge`. Carries the
    /// target revision so a confirmation prompt names it explicitly, never a
    /// generic "merge" (US-079 criterion 1: origin, destination and policy
    /// are shown before executing).
    Merge {
        target: String,
    },
    /// EPIC-16/T-233 (US-081): `RepositoryWritePort::continue_operation`.
    /// Generic across merge/rebase/cherry-pick/revert (the port method
    /// itself dispatches on whatever `InProgressOperation` is actually
    /// detected), so this carries no per-kind data of its own.
    ContinueOperation,
    /// EPIC-16/T-233 (US-081): `RepositoryWritePort::abort_operation`.
    AbortOperation,
    /// EPIC-16/T-232 (US-080): `RepositoryWritePort::mark_conflict_resolved`.
    MarkConflictResolved {
        path: PathBuf,
    },
    /// EPIC-16/T-232 (US-080): `RepositoryWritePort::take_conflict_side`
    /// (the documented binary-conflict flow).
    TakeConflictSide {
        path: PathBuf,
        side: ConflictSide,
    },
    /// EPIC-17/T-235 (US-083): `RepositoryWritePort::rebase`. Carries the
    /// target base so a confirmation prompt names it explicitly, never a
    /// generic "rebase" (mirrors [`Self::Merge`]'s own rationale).
    Rebase {
        onto: String,
    },
    /// EPIC-17/T-235 (US-083): `RepositoryWritePort::skip_operation`.
    /// Generic across whichever sequencer operation is actually detected
    /// (the port method itself dispatches, mirroring
    /// [`Self::ContinueOperation`]), so this carries no per-kind data.
    SkipOperation,
    /// EPIC-17/T-236/T-237 (US-084/US-085):
    /// `RepositoryWritePort::execute_rebase_plan`. Carries the target base
    /// and the number of commits the plan reapplies so a confirmation
    /// prompt names concrete scope, never a generic "rebase" (mirrors
    /// [`Self::ApplyPatch`]'s own `affected_file_count` convention).
    ExecuteRebasePlan {
        onto: String,
        commit_count: usize,
    },
    /// EPIC-17/T-238 (US-086): `RepositoryWritePort::cherry_pick`. Carries
    /// the commit's short hash so a confirmation prompt names it explicitly,
    /// never a generic "cherry-pick a commit".
    CherryPick {
        commit: String,
    },
    /// EPIC-17/T-239 (US-087): `RepositoryWritePort::revert`. Carries the
    /// commit's short hash, mirroring [`Self::CherryPick`]'s own rationale.
    Revert {
        commit: String,
    },
    /// EPIC-17/T-240 (US-088): `RepositoryWritePort::reset`. Carries the
    /// target revision and the exact mode so a confirmation prompt always
    /// names both (US-088 criterion 1) — see
    /// [`crate::write_ports::ResetMode`] for what each mode does to
    /// `HEAD`/the index/the working tree.
    Reset {
        target: String,
        mode: crate::write_ports::ResetMode,
    },
}

impl MutationKind {
    /// The canonical SAD §20 classification for this operation.
    ///
    /// `AmendCommit` is classified `Destructive` here: it replaces `HEAD`'s
    /// commit object in place and can rewrite history another clone or
    /// collaborator may already have observed, the same "hard to reverse,
    /// visible consequence" character SAD §20 lists `reset --hard` and force
    /// push under. This adopts `apps/desktop`'s existing decision as the
    /// canonical one rather than leaving it Desktop-only.
    pub fn risk(&self) -> RiskLevel {
        match self {
            MutationKind::StageFiles
            | MutationKind::UnstageFiles
            | MutationKind::StageHunks
            | MutationKind::UnstageHunks => RiskLevel::Safe,
            MutationKind::CreateCommit
            | MutationKind::SwitchBranch { .. }
            | MutationKind::CreateBranch { .. } => RiskLevel::Moderate,
            MutationKind::DeleteBranch { force, .. } => {
                if *force {
                    RiskLevel::Destructive
                } else {
                    RiskLevel::Moderate
                }
            }
            // Renaming moves a ref/checkout-shaped identity, not content: no
            // commit becomes unreachable, no working-tree/index state
            // changes, and Git itself refuses a colliding name outright
            // (never overwritten) — the same "confirmed, deliberate,
            // non-destructive ref change" character `CreateBranch` already
            // has, hence `Moderate` rather than `Destructive` (T-157/US-024
            // task scope note).
            MutationKind::RenameBranch { .. } => RiskLevel::Moderate,
            MutationKind::AmendCommit => RiskLevel::Destructive,
            // Creating a stash rewrites the working tree/index back to
            // `HEAD` (recoverably — the removed content is captured in the
            // stash itself), the same "mutates the working tree, but not
            // irreversibly" character `checkout`/`commit` already have —
            // hence `Moderate`, mirroring `CreateCommit`/`SwitchBranch`
            // rather than `Destructive`.
            MutationKind::CreateStash => RiskLevel::Moderate,
            // Applying/popping a stash mutates the working tree/index and
            // can produce merge conflicts to resolve, the same character
            // `SwitchBranch` already has — `Moderate`, not `Destructive`:
            // Git itself refuses (or, on conflict, preserves the stash
            // rather than losing it — see `RepositoryWritePort::pop_stash`'s
            // doc) rather than ever silently discarding work.
            MutationKind::ApplyStash { .. } => RiskLevel::Moderate,
            MutationKind::PopStash { .. } => RiskLevel::Moderate,
            // SAD §20 explicitly lists "stash drop" among its own canonical
            // `Destructive` examples (see this module's own doc comment):
            // unlike apply/pop, a drop has no built-in Git recovery path.
            MutationKind::DropStash { .. } => RiskLevel::Destructive,
            // A tag is a ref creation/removal with no working-tree effect
            // and no history rewrite — the same tier `CreateBranch` already
            // occupies. Unlike `DeleteBranch`, Git has no "unmerged commits"
            // protection for tags to mirror into a force/no-force split, so
            // `DeleteTag` gets one fixed tier rather than a conditional one:
            // `Moderate` (a confirmed, deliberate ref deletion, not the
            // "hard to reverse, visible consequence" character SAD §20's
            // `Destructive` examples share — recreating a deleted tag from
            // its recorded target commit, once known, is straightforward).
            MutationKind::CreateTag { .. } => RiskLevel::Moderate,
            MutationKind::DeleteTag { .. } => RiskLevel::Moderate,
            // Creating a worktree is a ref/checkout-shaped operation with no
            // destructive potential of its own — mirrors `CreateBranch`.
            MutationKind::CreateWorktree { .. } => RiskLevel::Moderate,
            // Mirrors `DeleteBranch { force }` exactly: a plain removal only
            // succeeds when the worktree is clean (Moderate — a confirmed,
            // reversible-in-effect cleanup), while `force: true` discards
            // whatever uncommitted changes are sitting in that worktree,
            // the same "discard changes" character SAD §20 lists under
            // `Destructive`.
            MutationKind::RemoveWorktree { force, .. } => {
                if *force {
                    RiskLevel::Destructive
                } else {
                    RiskLevel::Moderate
                }
            }
            // SAD §20's own named `Safe` example: fetch only updates
            // remote-tracking refs, never the working tree, index, or any
            // ref a person is actually standing on.
            MutationKind::Fetch { .. } => RiskLevel::Safe,
            // A fast-forward-only integration mutates the current branch
            // and working tree, but — by construction — only ever moves
            // them forward along history everyone already agrees on (this
            // version refuses anything else rather than merging/rebasing
            // automatically): the same "mutates, but not irreversibly or
            // surprisingly" character `SwitchBranch`/`CreateCommit` already
            // have.
            MutationKind::Pull { .. } => RiskLevel::Moderate,
            // A plain push never rewrites history (it can only fast-forward
            // the remote, and is refused otherwise — see
            // `RepositoryWritePort::push`'s doc), so it is no riskier than
            // any other ref-advancing mutation already classified
            // `Moderate` here.
            MutationKind::Push { .. } => RiskLevel::Moderate,
            // The one remote mutation that can discard commits a
            // collaborator already published (that is exactly what
            // `--force-with-lease` protects against racing, not what it
            // makes safe to do) — SAD §20's own "force push" example of a
            // `Destructive` operation.
            MutationKind::ForcePushWithLease { .. } => RiskLevel::Destructive,
            // Applying a patch mutates the working tree — the same
            // character `CreateCommit`/`SwitchBranch` already have — but is
            // not irreversible the way SAD §20's `Destructive` examples
            // are: the patch text itself remains available to reapply, and
            // `RepositoryWritePort::apply_patch` never touches the index or
            // HEAD, only working-tree file content. Hence `Moderate`, per
            // this task's own scope note (T-163/US-030).
            MutationKind::ApplyPatch { .. } => RiskLevel::Moderate,
            // SAD §20's own named `Moderate` example ("Moderate: commit,
            // checkout, merge"). A merge mutates the working tree/index and
            // history, but confirmed/deliberate merges (fast-forward or a
            // clean merge commit) are the ordinary, expected case; a
            // conflict is never silently lost work either — it lands in
            // `MergeResult::Conflict`, fully recoverable via continue/abort
            // (T-232/T-233), not a "hard to reverse" `Destructive` outcome.
            MutationKind::Merge { .. } => RiskLevel::Moderate,
            // Resuming a merge/rebase/cherry-pick/revert once conflicts are
            // resolved is the deliberate conclusion of an already-confirmed
            // operation — the same character `CreateCommit`/`Merge` already
            // have.
            MutationKind::ContinueOperation => RiskLevel::Moderate,
            // Aborting discards the in-progress operation's own changes
            // (e.g. a merge's conflict resolutions in progress) — not
            // "hard to reverse" the way a force push or `reset --hard` on
            // arbitrary history is (Git restores the pre-operation state),
            // but real, confirmed work in the index/working tree is thrown
            // away, so this task's own scope note classifies it
            // `Destructive` rather than `Moderate`, requiring reinforced
            // confirmation.
            MutationKind::AbortOperation => RiskLevel::Destructive,
            // `git add`, the same tier `StageFiles` already occupies (SAD
            // §20's own named `Safe` example) — this only records that a
            // conflict's current working-tree content is the resolution, it
            // never itself discards or overwrites anything.
            MutationKind::MarkConflictResolved { .. } => RiskLevel::Safe,
            // Overwrites the file's working-tree content with one whole
            // side, discarding whatever it held before — the same "mutates,
            // but not irreversibly" character `ApplyPatch`/`CreateStash`
            // already have (the conflict's other side remains inspectable
            // via `conflict_sides` until continue/abort concludes the
            // operation), hence `Moderate` rather than `Destructive`.
            MutationKind::TakeConflictSide { .. } => RiskLevel::Moderate,
            // SAD §20's own named `Moderate` example includes merge, and
            // this task's own scope note classifies a plain rebase the same
            // way: it mutates the current branch's history (every replayed
            // commit gets a new hash), but a confirmed, non-conflicting
            // rebase is the ordinary, expected case, and a conflict is never
            // silently lost work either — it lands in `RebaseResult::Conflict`,
            // fully recoverable via continue/skip/abort, not a "hard to
            // reverse" `Destructive` outcome (Git's own `ORIG_HEAD`/reflog
            // also keep the pre-rebase tip reachable for a real mistake).
            MutationKind::Rebase { .. } => RiskLevel::Moderate,
            // Mirrors `ContinueOperation`: deliberately advancing past the
            // current step of an already-confirmed, in-progress operation.
            MutationKind::SkipOperation => RiskLevel::Moderate,
            // An interactive rebase plan (reorder/reword/squash/fixup/drop)
            // is a richer rebase, not a different character of operation —
            // see `MutationKind::Rebase`'s own rationale, which applies
            // identically here (same recoverability via continue/skip/abort
            // mid-flight, and via `ORIG_HEAD`/reflog once finished).
            MutationKind::ExecuteRebasePlan { .. } => RiskLevel::Moderate,
            // A cherry-pick applies one already-reviewed commit's change as
            // a brand-new commit on the current branch — the same
            // "mutates, but a confirmed, non-conflicting result is the
            // ordinary, expected case" character `Merge`/`Rebase` already
            // have (T-238/US-086 task scope note). A conflict or an "already
            // applied" result is never silently lost work either — both
            // land in a distinct `CherryPickResult` variant, fully
            // recoverable via continue/skip/abort.
            MutationKind::CherryPick { .. } => RiskLevel::Moderate,
            // Mirrors `CherryPick` exactly (T-239/US-087 task scope note): a
            // revert only ever adds a new commit undoing another one's
            // change, never rewrites or moves an existing reference
            // (History Editing Rules #8), so it carries the same
            // "confirmed, deliberate, recoverable" character.
            MutationKind::Revert { .. } => RiskLevel::Moderate,
            // T-240/US-088's own explicit classification: `Soft`/`Mixed`
            // only ever move `HEAD` (and, for `Mixed`, the index) — the
            // working tree is always preserved, and whatever moved is still
            // fully recoverable as staged/unstaged changes, the same
            // character `SwitchBranch`/`CreateCommit` already have. `Hard`
            // additionally overwrites the working tree to match the target
            // exactly, discarding any uncommitted change outright — SAD
            // §20's own named `Destructive` example ("reset --hard").
            MutationKind::Reset { mode, .. } => match mode {
                crate::write_ports::ResetMode::Soft | crate::write_ports::ResetMode::Mixed => {
                    RiskLevel::Moderate
                }
                crate::write_ports::ResetMode::Hard => RiskLevel::Destructive,
            },
        }
    }

    /// A short, human-readable description of what would be affected, for a
    /// confirmation prompt (never a generic "are you sure?").
    ///
    /// **A surface must not show this as the prompt on its own.** Naming the
    /// target alone is not enough to make a prompt answerable: creating,
    /// checking out and deleting the same branch all reduce to "branch
    /// 'feature'" here, and [`Self::risk`] does not separate them either
    /// (`Moderate` covers both the checkout and the plain delete). Both
    /// surfaces that actually ask the question own a label that names the
    /// action *and* its target — `gitsail_tui::operation::OperationKind::
    /// prompt_label` and the Desktop's `OperationDescriptor.promptLabel` —
    /// and no caller reads this method today (T-267).
    pub fn target_label(&self) -> String {
        match self {
            MutationKind::StageFiles => "staged files".to_string(),
            MutationKind::UnstageFiles => "unstaged files".to_string(),
            MutationKind::StageHunks => "staged hunks".to_string(),
            MutationKind::UnstageHunks => "unstaged hunks".to_string(),
            MutationKind::CreateCommit => "a new commit".to_string(),
            MutationKind::SwitchBranch { target } => format!("branch '{target}'"),
            MutationKind::CreateBranch { name } => format!("branch '{name}'"),
            MutationKind::DeleteBranch { name, .. } => format!("branch '{name}'"),
            MutationKind::RenameBranch { old_name, new_name } => {
                format!("branch '{old_name}' to '{new_name}'")
            }
            MutationKind::AmendCommit => "the current HEAD commit".to_string(),
            MutationKind::CreateStash => "a new stash entry".to_string(),
            MutationKind::ApplyStash { index } => format!("stash@{{{index}}}"),
            MutationKind::PopStash { index } => format!("stash@{{{index}}}"),
            MutationKind::DropStash { index } => format!("stash@{{{index}}}"),
            MutationKind::CreateTag { name } => format!("tag '{name}'"),
            MutationKind::DeleteTag { name } => format!("tag '{name}'"),
            MutationKind::CreateWorktree { path } => format!("worktree at '{}'", path.display()),
            MutationKind::RemoveWorktree { path, .. } => {
                format!("worktree at '{}'", path.display())
            }
            MutationKind::Fetch { remote } => format!("remote '{remote}'"),
            MutationKind::Pull { remote, branch } => {
                format!("branch '{branch}' from remote '{remote}'")
            }
            MutationKind::Push { remote, branch } => {
                format!("branch '{branch}' to remote '{remote}'")
            }
            MutationKind::ForcePushWithLease { remote, branch } => {
                format!("branch '{branch}' on remote '{remote}' (force push with lease)")
            }
            MutationKind::ApplyPatch {
                affected_file_count,
            } => {
                format!(
                    "{affected_file_count} file{} affected by the patch",
                    if *affected_file_count == 1 { "" } else { "s" }
                )
            }
            MutationKind::Merge { target } => format!("merging '{target}' into the current branch"),
            MutationKind::ContinueOperation => "the in-progress operation".to_string(),
            MutationKind::AbortOperation => "the in-progress operation".to_string(),
            MutationKind::MarkConflictResolved { path } => {
                format!("'{}' as resolved", path.display())
            }
            MutationKind::TakeConflictSide { path, side } => {
                let side_label = match side {
                    ConflictSide::Ours => "ours",
                    ConflictSide::Theirs => "theirs",
                };
                format!("'{}' (take {side_label})", path.display())
            }
            MutationKind::Rebase { onto } => format!("rebasing the current branch onto '{onto}'"),
            MutationKind::SkipOperation => {
                "the current step of the in-progress operation".to_string()
            }
            MutationKind::ExecuteRebasePlan { onto, commit_count } => format!(
                "rebasing {commit_count} commit{} onto '{onto}'",
                if *commit_count == 1 { "" } else { "s" }
            ),
            MutationKind::CherryPick { commit } => {
                format!("cherry-picking commit '{commit}' onto the current branch")
            }
            MutationKind::Revert { commit } => format!("reverting commit '{commit}'"),
            MutationKind::Reset { target, mode } => {
                format!("resetting to '{target}' ({mode} reset)")
            }
        }
    }
}

/// A state snapshot captured when a mutation was previewed/confirmed,
/// revalidated immediately before the mutation actually executes: a
/// confirmation given against an older repository state must never
/// authorize execution against a newer one (SAD §20 "pré-condições/HEAD são
/// revalidados antes de executar"; wiki "Destructive Operations &
/// Confirmation Guardrails" rule 4).
///
/// Generic over `T` so it can wrap whatever precondition a given mutation
/// actually depends on — a `CommitHash` (HEAD, as `amend_commit` already
/// checks), a generation/epoch counter (as `apps/desktop`'s session state
/// already tracks independently), or any other comparable snapshot a future
/// mutation needs revalidated (an interactive rebase plan against current
/// refs, for instance).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Precondition<T> {
    expected: T,
}

impl<T: fmt::Debug + PartialEq> Precondition<T> {
    /// Captures `expected` as the state a caller observed when the mutation
    /// was previewed/confirmed.
    pub fn new(expected: T) -> Self {
        Self { expected }
    }

    pub fn expected(&self) -> &T {
        &self.expected
    }

    /// Confirms `current` still matches what this precondition captured.
    /// Returns `Err` with [`ErrorCode::OperationConflict`] — never silently
    /// proceeds — when it does not, so a stale confirmation can never
    /// authorize a mutation against a state the person never actually saw.
    pub fn revalidate(&self, current: &T) -> Result<(), GitSailError> {
        if &self.expected == current {
            Ok(())
        } else {
            Err(GitSailError::new(
                ErrorCode::OperationConflict,
                format!(
                    "repository state changed since this operation was confirmed (expected {:?}, found {:?})",
                    self.expected, current
                ),
            )
            .with_remediation(
                "refresh and retry the operation against the current repository state",
            ))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn risk_classification_matches_sad_section_20() {
        assert!(MutationKind::StageFiles.risk().skips_confirmation());
        assert!(MutationKind::UnstageHunks.risk().skips_confirmation());
        assert_eq!(MutationKind::CreateCommit.risk(), RiskLevel::Moderate);
        assert_eq!(
            MutationKind::SwitchBranch {
                target: "main".into()
            }
            .risk(),
            RiskLevel::Moderate
        );
        assert_eq!(
            MutationKind::DeleteBranch {
                name: "x".into(),
                force: false
            }
            .risk(),
            RiskLevel::Moderate
        );
        assert!(MutationKind::DeleteBranch {
            name: "x".into(),
            force: true
        }
        .risk()
        .requires_reinforced_confirmation());
        assert!(MutationKind::AmendCommit
            .risk()
            .requires_reinforced_confirmation());
    }

    /// EPIC-18: the new stash/tag/worktree mutations classify per this
    /// module's doc rationale — in particular, `DropStash` is `Destructive`
    /// (SAD §20's own named example), while `ApplyStash`/`PopStash`/
    /// `CreateStash`/tag/worktree creation are `Moderate`, and
    /// `RemoveWorktree` only escalates to `Destructive` when `force: true`,
    /// mirroring `DeleteBranch` exactly.
    #[test]
    fn epic_18_stash_tag_worktree_mutations_classify_per_sad_section_20() {
        assert_eq!(MutationKind::CreateStash.risk(), RiskLevel::Moderate);
        assert_eq!(
            MutationKind::ApplyStash { index: 0 }.risk(),
            RiskLevel::Moderate
        );
        assert_eq!(
            MutationKind::PopStash { index: 0 }.risk(),
            RiskLevel::Moderate
        );
        assert!(MutationKind::DropStash { index: 0 }
            .risk()
            .requires_reinforced_confirmation());
        assert_eq!(
            MutationKind::CreateTag {
                name: "v1.0".into()
            }
            .risk(),
            RiskLevel::Moderate
        );
        assert_eq!(
            MutationKind::DeleteTag {
                name: "v1.0".into()
            }
            .risk(),
            RiskLevel::Moderate
        );
        assert_eq!(
            MutationKind::CreateWorktree {
                path: "/tmp/wt".into()
            }
            .risk(),
            RiskLevel::Moderate
        );
        assert_eq!(
            MutationKind::RemoveWorktree {
                path: "/tmp/wt".into(),
                force: false
            }
            .risk(),
            RiskLevel::Moderate
        );
        assert!(MutationKind::RemoveWorktree {
            path: "/tmp/wt".into(),
            force: true
        }
        .risk()
        .requires_reinforced_confirmation());
    }

    #[test]
    fn epic_18_target_labels_name_the_exact_target() {
        assert_eq!(
            MutationKind::ApplyStash { index: 2 }.target_label(),
            "stash@{2}"
        );
        assert_eq!(
            MutationKind::CreateTag {
                name: "v2.0".into()
            }
            .target_label(),
            "tag 'v2.0'"
        );
        assert!(MutationKind::CreateWorktree {
            path: "/repos/feature".into()
        }
        .target_label()
        .contains("/repos/feature"));
    }

    /// EPIC-19: the remote-operation mutations classify per this module's
    /// doc rationale — `Fetch` is `Safe` (SAD §20's own named example),
    /// `Pull`/`Push` are `Moderate` (a fast-forward-only integration and a
    /// non-force push both only ever move refs forward along agreed
    /// history), and `ForcePushWithLease` is the one `Destructive` case
    /// (SAD §20's own "force push" example) — distinct from `Push` so a
    /// confirmation prompt is never generic.
    #[test]
    fn epic_19_remote_operation_mutations_classify_per_sad_section_20() {
        assert!(MutationKind::Fetch {
            remote: "origin".into()
        }
        .risk()
        .skips_confirmation());
        assert_eq!(
            MutationKind::Pull {
                remote: "origin".into(),
                branch: "main".into()
            }
            .risk(),
            RiskLevel::Moderate
        );
        assert_eq!(
            MutationKind::Push {
                remote: "origin".into(),
                branch: "main".into()
            }
            .risk(),
            RiskLevel::Moderate
        );
        assert!(MutationKind::ForcePushWithLease {
            remote: "origin".into(),
            branch: "main".into()
        }
        .risk()
        .requires_reinforced_confirmation());
        assert_ne!(
            MutationKind::Push {
                remote: "origin".into(),
                branch: "main".into()
            },
            MutationKind::ForcePushWithLease {
                remote: "origin".into(),
                branch: "main".into()
            },
            "push and force-push-with-lease must remain distinct MutationKind values"
        );
    }

    #[test]
    fn epic_19_target_labels_name_the_exact_remote_and_branch() {
        assert_eq!(
            MutationKind::Fetch {
                remote: "upstream".into()
            }
            .target_label(),
            "remote 'upstream'"
        );
        let push_label = MutationKind::Push {
            remote: "origin".into(),
            branch: "feature/x".into(),
        }
        .target_label();
        assert!(push_label.contains("feature/x"));
        assert!(push_label.contains("origin"));
        let force_label = MutationKind::ForcePushWithLease {
            remote: "origin".into(),
            branch: "feature/x".into(),
        }
        .target_label();
        assert!(force_label.contains("feature/x"));
        assert!(force_label.contains("force"));
    }

    /// T-163/US-030: `ApplyPatch` classifies `Moderate` (mutates the working
    /// tree but never HEAD/index, and is not irreversible) and its label
    /// names the concrete affected-file count rather than a generic
    /// "apply a patch".
    #[test]
    fn apply_patch_classifies_moderate_with_a_concrete_target_label() {
        let one_file = MutationKind::ApplyPatch {
            affected_file_count: 1,
        };
        assert_eq!(one_file.risk(), RiskLevel::Moderate);
        assert!(!one_file.risk().requires_reinforced_confirmation());
        assert_eq!(one_file.target_label(), "1 file affected by the patch");

        let three_files = MutationKind::ApplyPatch {
            affected_file_count: 3,
        };
        assert_eq!(three_files.target_label(), "3 files affected by the patch");
    }

    /// T-157/US-024: `RenameBranch` classifies `Moderate` (a confirmed ref
    /// rename Git itself refuses to let collide, not a destructive/
    /// irreversible action) and its label names both the old and new name,
    /// never a generic "rename a branch".
    #[test]
    fn rename_branch_classifies_moderate_with_both_names_in_the_label() {
        let kind = MutationKind::RenameBranch {
            old_name: "old-name".into(),
            new_name: "new-name".into(),
        };
        assert_eq!(kind.risk(), RiskLevel::Moderate);
        assert!(!kind.risk().requires_reinforced_confirmation());
        let label = kind.target_label();
        assert!(label.contains("old-name"));
        assert!(label.contains("new-name"));
    }

    /// EPIC-16/T-231/T-233: `Merge`/`ContinueOperation` classify `Moderate`
    /// (SAD §20's own named `Moderate` example includes merge) while
    /// `AbortOperation` classifies `Destructive` (it discards the
    /// in-progress operation's own changes) per this task's scope note, and
    /// `Merge`'s label always names the concrete target revision.
    #[test]
    fn merge_and_continue_classify_moderate_while_abort_classifies_destructive() {
        let merge = MutationKind::Merge {
            target: "feature/x".into(),
        };
        assert_eq!(merge.risk(), RiskLevel::Moderate);
        assert!(!merge.risk().requires_reinforced_confirmation());
        assert!(merge.target_label().contains("feature/x"));

        assert_eq!(MutationKind::ContinueOperation.risk(), RiskLevel::Moderate);
        assert!(MutationKind::AbortOperation
            .risk()
            .requires_reinforced_confirmation());
    }

    /// EPIC-16/T-232: `MarkConflictResolved` classifies `Safe` (it is `git
    /// add`, mirroring `StageFiles`), while `TakeConflictSide` classifies
    /// `Moderate` (it overwrites working-tree content, but the discarded
    /// side remains recoverable via `conflict_sides` until the operation
    /// concludes), and both labels name the exact conflicted path.
    #[test]
    fn mark_conflict_resolved_classifies_safe_and_take_conflict_side_classifies_moderate() {
        let mark = MutationKind::MarkConflictResolved {
            path: PathBuf::from("a.txt"),
        };
        assert!(mark.risk().skips_confirmation());
        assert!(mark.target_label().contains("a.txt"));

        let take_ours = MutationKind::TakeConflictSide {
            path: PathBuf::from("image.png"),
            side: gitsail_domain::ConflictSide::Ours,
        };
        assert_eq!(take_ours.risk(), RiskLevel::Moderate);
        assert!(take_ours.target_label().contains("image.png"));
        assert!(take_ours.target_label().contains("ours"));
    }

    /// EPIC-17/T-235..T-237: `Rebase`/`SkipOperation`/`ExecuteRebasePlan` all
    /// classify `Moderate` (mutates history/advances a sequencer step, but a
    /// conflict is never silently lost work — see this module's own
    /// rationale comments), and their labels always name the concrete base/
    /// commit count rather than a generic "rebase".
    #[test]
    fn rebase_operations_classify_moderate_with_concrete_target_labels() {
        let rebase = MutationKind::Rebase {
            onto: "main".into(),
        };
        assert_eq!(rebase.risk(), RiskLevel::Moderate);
        assert!(!rebase.risk().requires_reinforced_confirmation());
        assert!(rebase.target_label().contains("main"));

        assert_eq!(MutationKind::SkipOperation.risk(), RiskLevel::Moderate);

        let plan = MutationKind::ExecuteRebasePlan {
            onto: "develop".into(),
            commit_count: 3,
        };
        assert_eq!(plan.risk(), RiskLevel::Moderate);
        let label = plan.target_label();
        assert!(label.contains("develop"));
        assert!(label.contains('3'));
    }

    #[test]
    fn target_label_is_never_a_generic_placeholder() {
        let kind = MutationKind::DeleteBranch {
            name: "feature/x".into(),
            force: true,
        };
        assert!(kind.target_label().contains("feature/x"));
        assert_ne!(kind.target_label(), "are you sure?");
    }

    /// DoD: "testes de intenção tipada e uma corrida simulada entre
    /// prévia/execução comprovam bloqueio de ação obsoleta" — this exercises
    /// the generic mechanism directly (not tied to any Git call), simulating
    /// the exact preview -> external-change -> stale-execute-attempt race
    /// `amend_commit`'s own "stale HEAD" test exercises for one concrete
    /// case.
    #[test]
    fn precondition_blocks_a_stale_action_after_a_simulated_external_change() {
        // A generic "generation" precondition, not specific to Git at all —
        // demonstrating this is a reusable mechanism, not amend-specific.
        let preview_generation = 1u64;
        let precondition = Precondition::new(preview_generation);

        // Nothing else has touched the repository yet: revalidating against
        // the same generation succeeds.
        assert!(precondition.revalidate(&1u64).is_ok());

        // Something external mutates the repository between preview and
        // confirm (e.g. another terminal/editor, or a concurrent GitSail
        // session) — the generation advances.
        let current_generation_after_external_change = 2u64;

        let result = precondition.revalidate(&current_generation_after_external_change);

        let err = result.expect_err("a stale precondition must never authorize execution");
        assert_eq!(err.code(), ErrorCode::OperationConflict);
    }

    /// T-238/T-239/T-240: `CherryPick`/`Revert` classify `Moderate`,
    /// `Reset`'s soft/mixed classify `Moderate` while hard classifies
    /// `Destructive` (SAD §20's own named "reset --hard" example), and every
    /// label names the exact commit/target/mode rather than a generic
    /// "cherry-pick"/"revert"/"reset".
    #[test]
    fn cherry_pick_revert_and_reset_classify_per_sad_section_20() {
        let cherry_pick = MutationKind::CherryPick {
            commit: "abc1234".into(),
        };
        assert_eq!(cherry_pick.risk(), RiskLevel::Moderate);
        assert!(!cherry_pick.risk().requires_reinforced_confirmation());
        assert!(cherry_pick.target_label().contains("abc1234"));

        let revert = MutationKind::Revert {
            commit: "def5678".into(),
        };
        assert_eq!(revert.risk(), RiskLevel::Moderate);
        assert!(revert.target_label().contains("def5678"));

        let soft = MutationKind::Reset {
            target: "HEAD~1".into(),
            mode: crate::write_ports::ResetMode::Soft,
        };
        assert_eq!(soft.risk(), RiskLevel::Moderate);
        assert!(!soft.risk().requires_reinforced_confirmation());
        assert!(soft.target_label().contains("HEAD~1"));
        assert!(soft.target_label().contains("soft"));

        let mixed = MutationKind::Reset {
            target: "HEAD~1".into(),
            mode: crate::write_ports::ResetMode::Mixed,
        };
        assert_eq!(mixed.risk(), RiskLevel::Moderate);

        let hard = MutationKind::Reset {
            target: "HEAD~1".into(),
            mode: crate::write_ports::ResetMode::Hard,
        };
        assert_eq!(hard.risk(), RiskLevel::Destructive);
        assert!(hard.risk().requires_reinforced_confirmation());
        assert!(hard.target_label().contains("hard"));
    }

    #[test]
    fn precondition_over_a_commit_hash_mirrors_amend_commits_own_check() {
        use gitsail_domain::CommitHash;

        let original_head = CommitHash::new("deadbeefdeadbeefdeadbeefdeadbeefdeadbeef").unwrap();
        let advanced_head = CommitHash::new("cafef00dcafef00dcafef00dcafef00dcafef00").unwrap();

        let precondition = Precondition::new(original_head.clone());

        assert!(precondition.revalidate(&original_head).is_ok());
        assert_eq!(
            precondition.revalidate(&advanced_head).unwrap_err().code(),
            ErrorCode::OperationConflict
        );
    }
}
