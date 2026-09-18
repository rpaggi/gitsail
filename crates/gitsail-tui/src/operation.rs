//! Generic progress/confirmation state machine for mutating operations
//! (SAD §20; US-044).
//!
//! [`OperationState`] delivers the TUI Foundation *framework* — confirm,
//! then run in the background, then land on a distinct success or failure
//! state, with cancellation always available before anything mutates
//! (criterion 2). It is deliberately not wired to any real keystroke here:
//! no story in EPIC-09 authorizes a specific mutating shortcut (staging a
//! file, committing, switching branches, ...), and every such shortcut
//! belongs to a later epic that already lists US-044 as its dependency
//! (e.g. US-046 "Compose and create a commit"). Wiring one in without a
//! real caller would be exactly the kind of half-built, unreachable
//! capability DOD-G rules out.
//!
//! This mirrors the precedent already set for `gitsail-application`'s
//! `RepositoryWritePort` operations (`stage_files`, `switch_branch`, ...):
//! T-144/T-154/T-155/T-156 built the real Core capability while explicitly
//! deferring presentation (confirmation UI) to "TUI/Desktop, EPIC-09/
//! EPIC-11" — this module is that deferred piece landing, still without a
//! bound trigger.
//!
//! Risk metadata (criterion 2, "confirmação usa metadados do Core") comes
//! from the static classification SAD §20 already defines
//! (Safe/Moderate/Destructive), not from the fuller revalidation framework
//! US-111 will eventually add (EPIC-22, not yet built — see
//! `gitsail_application::write_ports`' own doc comment for the same
//! conscious scope cut).

use gitsail_domain::GitSailError;

/// Risk tier for a mutating operation (SAD §20).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OperationRisk {
    Safe,
    Moderate,
    Destructive,
}

/// A mutating operation the TUI knows how to confirm and track, one
/// variant per `RepositoryWritePort` call. Naming the specific call (with
/// its target) rather than a generic "mutate" bucket is what keeps a
/// confirmation prompt unambiguous (US-044 DoD: "sem confirmação genérica
/// ambígua").
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OperationKind {
    StageFiles,
    UnstageFiles,
    CreateCommit,
    SwitchBranch { target: String },
    CreateBranch { name: String },
    DeleteBranch { name: String, force: bool },
    /// T-157/US-024: `RepositoryWritePort::rename_branch`. Carries both
    /// names so the confirmation prompt (and [`Self::target_label`]) always
    /// names the exact source and destination, never a generic "rename a
    /// branch".
    RenameBranch { old_name: String, new_name: String },
    /// T-182/US-049: `RepositoryWritePort::fetch`. `Safe` per SAD §20's own
    /// named example — see [`Self::risk`].
    Fetch { remote: String },
    /// T-182/US-049: `RepositoryWritePort::pull` — this version's
    /// fast-forward-only policy (see that method's doc); divergence comes
    /// back as an ordinary [`OperationState::Failed`], never an automatic
    /// merge/rebase.
    Pull { remote: String, branch: String },
    /// T-182/US-049: `RepositoryWritePort::push` — a plain, non-force push.
    /// `RepositoryWritePort::force_push_with_lease` (US-099) is deliberately
    /// out of scope for this operation set; see this crate's module docs
    /// for why.
    Push { remote: String, branch: String },
    /// T-163/US-030: `RepositoryWritePort::apply_patch`. Carries the
    /// affected-file count from the [`gitsail_application::PatchPreview`]
    /// [`crate::app::App`] already computed via
    /// `RepositoryWritePort::preview_patch_application` before ever
    /// reaching this `Confirming` state (US-030 criterion 1: the prompt
    /// always names concrete scope, never a generic "apply a patch").
    ApplyPatch { affected_file_count: usize },
    /// T-231/US-079: `RepositoryWritePort::merge`. Carries the target
    /// revision so origin, destination and policy are all shown before
    /// executing (US-079 criterion 1) — never a generic "merge" prompt.
    Merge { target: String },
    /// T-233/US-081: `RepositoryWritePort::continue_operation`. Generic
    /// across merge/rebase/cherry-pick/revert (the port itself dispatches on
    /// whatever `InProgressOperation` is actually detected), so this carries
    /// no per-kind data of its own — mirrors
    /// `gitsail_application::MutationKind::ContinueOperation`.
    ContinueOperation,
    /// T-233/US-081: `RepositoryWritePort::abort_operation`. Mirrors
    /// `gitsail_application::MutationKind::AbortOperation`.
    AbortOperation,
    /// T-235/US-083: `RepositoryWritePort::rebase`. Carries the target base
    /// so origin, destination and policy are all shown before executing,
    /// mirroring [`Self::Merge`]'s own rationale.
    Rebase { onto: String },
    /// T-235/US-083: `RepositoryWritePort::skip_operation`. Generic across
    /// whichever sequencer operation is actually detected, mirroring
    /// [`Self::ContinueOperation`]'s own reuse rationale.
    SkipOperation,
}

impl OperationKind {
    /// SAD §20's example classification (`Safe: fetch, stage, unstage`;
    /// `Moderate: commit, checkout, merge`; `Destructive: reset --hard,
    /// discard changes, force push, stash drop`), extended to cover every
    /// `RepositoryWritePort` call that exists today. A non-force branch
    /// delete is Moderate (Git itself refuses an unmerged branch, per
    /// US-023); forcing past that refusal is what makes it Destructive.
    pub fn risk(&self) -> OperationRisk {
        match self {
            OperationKind::StageFiles | OperationKind::UnstageFiles => OperationRisk::Safe,
            OperationKind::CreateCommit
            | OperationKind::SwitchBranch { .. }
            | OperationKind::CreateBranch { .. } => OperationRisk::Moderate,
            OperationKind::DeleteBranch { force, .. } => {
                if *force {
                    OperationRisk::Destructive
                } else {
                    OperationRisk::Moderate
                }
            }
            // Mirrors `gitsail_application::MutationKind::RenameBranch`: a
            // confirmed, non-destructive ref rename Git itself refuses to
            // let collide with an existing branch (never overwritten,
            // never forced) — the same tier `CreateBranch` already has.
            OperationKind::RenameBranch { .. } => OperationRisk::Moderate,
            // Mirrors `gitsail_application::MutationKind`'s canonical
            // classification (SAD §20's own named examples: fetch is
            // explicitly `Safe`; a fast-forward-only pull and a plain,
            // non-force push both only ever move refs forward along
            // history everyone already agrees on, the same character
            // `SwitchBranch`/`CreateCommit` already have).
            OperationKind::Fetch { .. } => OperationRisk::Safe,
            OperationKind::Pull { .. } | OperationKind::Push { .. } => OperationRisk::Moderate,
            // Mirrors `gitsail_application::MutationKind::ApplyPatch`:
            // mutates the working tree, but never HEAD/the index, and is
            // not irreversible the way a `Destructive` operation is.
            OperationKind::ApplyPatch { .. } => OperationRisk::Moderate,
            // Mirrors `gitsail_application::MutationKind::Merge`: SAD §20's
            // own named `Moderate` example includes merge.
            OperationKind::Merge { .. } => OperationRisk::Moderate,
            // Mirrors `gitsail_application::MutationKind::ContinueOperation`:
            // concluding an already-confirmed operation once conflicts are
            // resolved, the same character `CreateCommit`/`Merge` have.
            OperationKind::ContinueOperation => OperationRisk::Moderate,
            // Mirrors `gitsail_application::MutationKind::AbortOperation`:
            // discards the in-progress operation's own changes (e.g. a
            // merge's conflict resolutions in progress), so this requires
            // reinforced confirmation even though Git restores the
            // pre-operation state rather than losing history outright.
            OperationKind::AbortOperation => OperationRisk::Destructive,
            // Mirrors `gitsail_application::MutationKind::Rebase`: mutates
            // the current branch's history, but a confirmed,
            // non-conflicting rebase is the ordinary, expected case, and a
            // conflict is never silently lost work (it lands in a distinct
            // `RebaseResult::Conflict`, recoverable via continue/skip/abort).
            OperationKind::Rebase { .. } => OperationRisk::Moderate,
            // Mirrors `gitsail_application::MutationKind::SkipOperation`:
            // deliberately advancing past the current step of an
            // already-confirmed, in-progress operation, the same character
            // `ContinueOperation` already has.
            OperationKind::SkipOperation => OperationRisk::Moderate,
        }
    }

    /// A short, human-readable description of what would be affected, for
    /// the confirmation prompt (criterion 1: "operação mostra alvo").
    pub fn target_label(&self) -> String {
        match self {
            OperationKind::StageFiles => "staged files".to_string(),
            OperationKind::UnstageFiles => "unstaged files".to_string(),
            OperationKind::CreateCommit => "a new commit".to_string(),
            OperationKind::SwitchBranch { target } => format!("branch '{target}'"),
            OperationKind::CreateBranch { name } => format!("branch '{name}'"),
            OperationKind::DeleteBranch { name, .. } => format!("branch '{name}'"),
            OperationKind::RenameBranch { old_name, new_name } => {
                format!("branch '{old_name}' to '{new_name}'")
            }
            OperationKind::Fetch { remote } => format!("remote '{remote}'"),
            OperationKind::Pull { remote, branch } => {
                format!("branch '{branch}' from remote '{remote}'")
            }
            OperationKind::Push { remote, branch } => {
                format!("branch '{branch}' to remote '{remote}'")
            }
            OperationKind::ApplyPatch { affected_file_count } => {
                format!(
                    "{affected_file_count} file{} affected by the patch",
                    if *affected_file_count == 1 { "" } else { "s" }
                )
            }
            OperationKind::Merge { target } => format!("merging '{target}' into the current branch"),
            OperationKind::ContinueOperation => "the in-progress operation".to_string(),
            OperationKind::AbortOperation => "the in-progress operation".to_string(),
            OperationKind::Rebase { onto } => format!("rebasing the current branch onto '{onto}'"),
            OperationKind::SkipOperation => {
                "the current step of the in-progress operation".to_string()
            }
        }
    }
}

/// Lifecycle of one mutating operation (US-044).
#[derive(Debug, Default)]
pub enum OperationState {
    /// No operation pending; the default state.
    #[default]
    Idle,
    /// Awaiting explicit confirmation. Nothing has mutated yet — cancelling
    /// from here is always safe (criterion 2).
    Confirming(OperationKind),
    /// Confirmed and running in the background (criterion 1: "estado em
    /// andamento").
    InProgress(OperationKind),
    /// Completed successfully. A caller observing this transition is
    /// expected to trigger `RefreshReason::AfterMutation` (criterion 3:
    /// "sucesso provoca refresh").
    Succeeded(OperationKind),
    /// Failed with a user-safe error (criterion 3: "erro oferece mensagem
    /// segura/remediação").
    Failed(OperationKind, GitSailError),
}

impl OperationState {
    pub fn is_idle(&self) -> bool {
        matches!(self, Self::Idle)
    }

    /// Starts confirmation for `kind`, replacing whatever was there before
    /// — a new operation always starts from a clean prompt rather than
    /// inheriting a stale one.
    pub fn begin(&mut self, kind: OperationKind) {
        *self = Self::Confirming(kind);
    }

    /// Moves from `Confirming` to `InProgress`. A no-op from any other
    /// state, so a stray confirm keypress can never start work that was
    /// not actually pending confirmation.
    pub fn confirm(&mut self) {
        if let Self::Confirming(kind) = self {
            *self = Self::InProgress(kind.clone());
        }
    }

    /// Cancels a pending confirmation without mutating anything (criterion
    /// 2), and doubles as "dismiss" for a terminal (`Succeeded`/`Failed`)
    /// state. A no-op while `InProgress`: work already started cannot be
    /// un-started from here.
    pub fn cancel(&mut self) {
        match self {
            Self::Confirming(_) | Self::Succeeded(_) | Self::Failed(_, _) => *self = Self::Idle,
            Self::Idle | Self::InProgress(_) => {}
        }
    }

    /// Records success. A no-op unless the operation was actually in
    /// progress, so a duplicate or late completion message can never
    /// overwrite a state the caller has already moved past.
    pub fn succeed(&mut self) {
        if let Self::InProgress(kind) = self {
            *self = Self::Succeeded(kind.clone());
        }
    }

    /// Records failure. Same in-progress guard as [`Self::succeed`].
    pub fn fail(&mut self, error: GitSailError) {
        if let Self::InProgress(kind) = self {
            *self = Self::Failed(kind.clone(), error);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gitsail_domain::ErrorCode;

    fn sample() -> OperationKind {
        OperationKind::DeleteBranch {
            name: "feature/x".to_string(),
            force: false,
        }
    }

    #[test]
    fn risk_classification_matches_sad_section_20() {
        assert_eq!(OperationKind::StageFiles.risk(), OperationRisk::Safe);
        assert_eq!(OperationKind::CreateCommit.risk(), OperationRisk::Moderate);
        assert_eq!(
            OperationKind::DeleteBranch {
                name: "x".into(),
                force: true
            }
            .risk(),
            OperationRisk::Destructive
        );
        assert_eq!(
            OperationKind::DeleteBranch {
                name: "x".into(),
                force: false
            }
            .risk(),
            OperationRisk::Moderate
        );
    }

    /// T-157/US-024: `RenameBranch` classifies `Moderate` and its label
    /// names both the previous and new branch names, never a generic
    /// "rename a branch".
    #[test]
    fn rename_branch_classifies_moderate_with_both_names_in_the_label() {
        let kind = OperationKind::RenameBranch {
            old_name: "old-name".into(),
            new_name: "new-name".into(),
        };
        assert_eq!(kind.risk(), OperationRisk::Moderate);
        let label = kind.target_label();
        assert!(label.contains("old-name"));
        assert!(label.contains("new-name"));
    }

    /// T-163/US-030: `ApplyPatch` classifies `Moderate` and its label names
    /// the concrete affected-file count from the already-computed preview.
    #[test]
    fn apply_patch_classifies_moderate_with_a_concrete_target_label() {
        let kind = OperationKind::ApplyPatch {
            affected_file_count: 2,
        };
        assert_eq!(kind.risk(), OperationRisk::Moderate);
        assert_eq!(kind.target_label(), "2 files affected by the patch");
    }

    /// T-182: mirrors `gitsail_application::mutation`'s canonical
    /// classification for the same three operations.
    #[test]
    fn remote_sync_operations_classify_per_sad_section_20() {
        assert_eq!(
            OperationKind::Fetch {
                remote: "origin".into()
            }
            .risk(),
            OperationRisk::Safe
        );
        assert_eq!(
            OperationKind::Pull {
                remote: "origin".into(),
                branch: "main".into()
            }
            .risk(),
            OperationRisk::Moderate
        );
        assert_eq!(
            OperationKind::Push {
                remote: "origin".into(),
                branch: "main".into()
            }
            .risk(),
            OperationRisk::Moderate
        );
    }

    /// T-231/T-233: `Merge`/`ContinueOperation` classify `Moderate` while
    /// `AbortOperation` classifies `Destructive`, mirroring
    /// `gitsail_application::MutationKind`'s own classification, and
    /// `Merge`'s label always names the concrete target revision.
    #[test]
    fn merge_and_continue_classify_moderate_while_abort_classifies_destructive() {
        let merge = OperationKind::Merge {
            target: "feature/x".into(),
        };
        assert_eq!(merge.risk(), OperationRisk::Moderate);
        assert!(merge.target_label().contains("feature/x"));

        assert_eq!(OperationKind::ContinueOperation.risk(), OperationRisk::Moderate);
        assert_eq!(OperationKind::AbortOperation.risk(), OperationRisk::Destructive);
    }

    /// T-235/US-083: `Rebase`/`SkipOperation` both classify `Moderate`
    /// (mutates history / advances a sequencer step, but a conflict is
    /// never silently lost work), and `Rebase`'s label always names the
    /// concrete target base.
    #[test]
    fn rebase_and_skip_operation_classify_moderate_with_a_concrete_target_label() {
        let rebase = OperationKind::Rebase {
            onto: "main".to_string(),
        };
        assert_eq!(rebase.risk(), OperationRisk::Moderate);
        assert!(rebase.target_label().contains("main"));

        assert_eq!(OperationKind::SkipOperation.risk(), OperationRisk::Moderate);
    }

    #[test]
    fn confirm_then_succeed_reaches_a_distinct_terminal_state() {
        let mut state = OperationState::default();
        state.begin(sample());
        assert!(matches!(state, OperationState::Confirming(_)));

        state.confirm();
        assert!(matches!(state, OperationState::InProgress(_)));

        state.succeed();
        assert!(matches!(state, OperationState::Succeeded(_)));
    }

    #[test]
    fn cancel_before_confirm_returns_to_idle_without_starting_work() {
        let mut state = OperationState::default();
        state.begin(sample());

        state.cancel();

        assert!(
            state.is_idle(),
            "cancel before confirm must never leave a pending operation behind"
        );
    }

    #[test]
    fn confirm_is_ignored_without_a_pending_confirmation() {
        let mut state = OperationState::default();
        state.confirm();
        assert!(
            state.is_idle(),
            "confirm with nothing pending must not start work out of thin air"
        );
    }

    #[test]
    fn failure_carries_a_distinct_error_from_success() {
        let mut state = OperationState::default();
        state.begin(sample());
        state.confirm();

        state.fail(GitSailError::new(
            ErrorCode::OperationConflict,
            "branch not fully merged",
        ));

        match state {
            OperationState::Failed(_, err) => assert_eq!(err.code(), ErrorCode::OperationConflict),
            other => panic!("expected Failed, got {other:?}"),
        }
    }

    #[test]
    fn cancel_on_a_terminal_state_dismisses_it_back_to_idle() {
        let mut state = OperationState::default();
        state.begin(sample());
        state.confirm();
        state.succeed();

        state.cancel();

        assert!(
            state.is_idle(),
            "acknowledging a completed operation must clear it"
        );
    }
}
