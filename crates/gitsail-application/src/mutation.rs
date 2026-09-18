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

use gitsail_domain::{ErrorCode, GitSailError};

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
        }
    }

    /// A short, human-readable description of what would be affected, for a
    /// confirmation prompt (never a generic "are you sure?").
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
