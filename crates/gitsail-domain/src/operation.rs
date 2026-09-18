//! In-progress multi-step Git operations (T-230/US-078).
//!
//! Git tracks a merge, rebase, cherry-pick, revert, or bisect run purely
//! through marker files/directories under `.git/` (`MERGE_HEAD`,
//! `rebase-merge`/`rebase-apply`, `CHERRY_PICK_HEAD`, `REVERT_HEAD`,
//! `BISECT_LOG`) — never in any in-memory state GitSail itself keeps.
//! [`InProgressOperation`] is the shared, extensible shape every future
//! EPIC-16/EPIC-17 story (merge, rebase, cherry-pick, revert, reset) builds
//! on to answer "is some multi-step operation already running, and what can
//! I validly do about it right now" — this story (US-078) is explicitly the
//! shared foundation those stories depend on, not a merge-specific type.
//!
//! Detecting this is always a fresh read of real `.git/` state (never a
//! cached/in-memory flag): an operation started in another terminal (`git
//! merge`, `git rebase`, ...) must be recognized on the very next call
//! (US-078 criterion 2). See `RepositoryReadPort::detect_in_progress_operation`
//! (`gitsail-application`) and `GitCliProvider`'s implementation
//! (`gitsail-git`) for that I/O; this module only holds the pure result
//! shape.
//!
//! [`OperationCapability`] models exactly what the project wiki's
//! "Destructive Operations & Confirmation Guardrails" rule 7 requires:
//! "abort/continue is offered whenever Git supports it" — never a fixed
//! continue/abort pair assumed uniformly for every operation kind:
//! - `Merge` supports `Continue` (`git merge --continue`, Git ≥2.24 —
//!   completes the merge commit once conflicts are resolved) and `Abort`,
//!   but never `Skip`: a merge has no sequence of further steps to skip
//!   past.
//! - `Rebase`/`CherryPick`/`Revert` are Git's own "sequencer" operations and
//!   support `Continue`, `Skip`, and `Abort`.
//! - `BisectRun` supports `Skip` (skip a commit that cannot be tested) and
//!   `Abort` (`git bisect reset`), but not `Continue` in this generic
//!   sense — a bisect run instead advances via `git bisect good`/`git
//!   bisect bad`, a domain-specific action this type does not attempt to
//!   model yet (see this module's own doc on [`BisectOperation`]).
//!
//! Implementing the actions themselves (continue/skip/abort as real
//! `RepositoryWritePort` mutations) is explicitly out of scope for this
//! story (T-231/T-235/T-238/T-239 and friends) — [`OperationCapability`]
//! only states which ones are valid to *offer* right now.

use std::path::PathBuf;

use crate::ids::CommitHash;

/// A capability that makes sense to offer for a given
/// [`InProgressOperation`] right now (see this module's doc for exactly
/// which operations expose which capabilities, and why).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum OperationCapability {
    /// Resume after conflicts are resolved (`git <op> --continue`).
    Continue,
    /// Skip the current step and move to the next one (`git <op> --skip`)
    /// — only meaningful for a sequencer operation with further steps.
    Skip,
    /// Abandon the operation and restore the pre-operation state as far as
    /// Git guarantees (`git <op> --abort`, or `git bisect reset`).
    Abort,
}

/// The shape of a merge conflict for one path, derived from its index
/// stage combination. Mirrors `git status --porcelain=v2`'s own seven
/// unmerged `XY` codes exactly (`DD`, `AU`, `UD`, `UA`, `DU`, `AA`, `UU`),
/// so a caller can distinguish "both sides changed it" from "one side
/// deleted it" without re-parsing Git's raw status codes itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ConflictStage {
    /// `UU`: both sides modified the file.
    BothModified,
    /// `AA`: both sides added the file (independently).
    BothAdded,
    /// `DD`: both sides deleted the file.
    BothDeleted,
    /// `AU`: added on our side, absent on theirs.
    AddedByUs,
    /// `UA`: added on their side, absent on ours.
    AddedByThem,
    /// `DU`: deleted on our side, modified on theirs.
    DeletedByUs,
    /// `UD`: deleted on their side, modified on ours.
    DeletedByThem,
}

/// One path Git currently reports as unmerged (conflicted).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ConflictedFile {
    pub path: PathBuf,
    pub stage: ConflictStage,
}

/// A merge in progress (`.git/MERGE_HEAD` present).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MergeOperation {
    /// The commit(s) being merged in. A plain two-way merge has exactly
    /// one; an octopus merge (`git merge a b c`) records more than one
    /// line in `MERGE_HEAD`.
    pub heads: Vec<CommitHash>,
    pub conflicted_files: Vec<ConflictedFile>,
    pub capabilities: Vec<OperationCapability>,
}

/// A rebase in progress (`.git/rebase-merge` or `.git/rebase-apply`
/// present).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RebaseOperation {
    /// Whether this is an interactive rebase (`.git/rebase-merge/
    /// interactive` present). Git only exposes this distinction for the
    /// `rebase-merge` backend; a `rebase-apply` (`am`-based) rebase is
    /// always `false` here — Git itself gives that backend no interactive
    /// mode.
    pub interactive: bool,
    /// The commit the branch is being rebased onto, when Git recorded a
    /// parseable one (`.git/rebase-merge/onto` or `.git/rebase-apply/
    /// onto`). Best-effort: `None` if the file is missing or its content
    /// does not parse as a commit hash — never a fatal detection error.
    pub onto: Option<CommitHash>,
    pub conflicted_files: Vec<ConflictedFile>,
    pub capabilities: Vec<OperationCapability>,
}

/// A cherry-pick or revert in progress (`.git/CHERRY_PICK_HEAD` or
/// `.git/REVERT_HEAD` present, respectively) — both are Git's "sequencer"
/// operations with an identical shape, disambiguated by
/// [`InProgressOperation`]'s own variant rather than a field here.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SequencerOperation {
    /// The commit being cherry-picked/reverted, when Git recorded a
    /// parseable one. Best-effort, mirrors [`RebaseOperation::onto`].
    pub target: Option<CommitHash>,
    pub conflicted_files: Vec<ConflictedFile>,
    pub capabilities: Vec<OperationCapability>,
}

/// A `git bisect` run in progress (`.git/BISECT_LOG` present).
///
/// This only covers the cheap, file-presence-based detection the story's
/// own Definition of Done allows ("BisectRun (se for barato detectar; senão
/// documente que não cobre bisect)"): it does not parse
/// `BISECT_START`/`BISECT_TERMS` for the good/bad boundary or the original
/// branch, since that is not needed to answer "is a bisect in progress"
/// and a future bisect-specific story can extend this struct without
/// touching [`InProgressOperation`]'s own shape.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BisectOperation {
    pub conflicted_files: Vec<ConflictedFile>,
    pub capabilities: Vec<OperationCapability>,
}

/// Which multi-step Git operation, if any, is currently in progress in a
/// repository (US-078).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InProgressOperation {
    /// No multi-step operation is in progress.
    None,
    Merge(MergeOperation),
    Rebase(RebaseOperation),
    CherryPick(SequencerOperation),
    Revert(SequencerOperation),
    BisectRun(BisectOperation),
}

impl InProgressOperation {
    /// Whether no multi-step operation is in progress — the state any
    /// mutation that wants to refuse running "on top of" another operation
    /// (US-078 criterion 3) checks first.
    pub fn is_none(&self) -> bool {
        matches!(self, InProgressOperation::None)
    }

    /// A short, stable label naming the kind of operation in progress, for
    /// a presentation layer or an error message — `None` when
    /// [`Self::is_none`] is true.
    pub fn kind_label(&self) -> Option<&'static str> {
        match self {
            InProgressOperation::None => None,
            InProgressOperation::Merge(_) => Some("merge"),
            InProgressOperation::Rebase(_) => Some("rebase"),
            InProgressOperation::CherryPick(_) => Some("cherry-pick"),
            InProgressOperation::Revert(_) => Some("revert"),
            InProgressOperation::BisectRun(_) => Some("bisect"),
        }
    }

    pub fn conflicted_files(&self) -> &[ConflictedFile] {
        match self {
            InProgressOperation::None => &[],
            InProgressOperation::Merge(op) => &op.conflicted_files,
            InProgressOperation::Rebase(op) => &op.conflicted_files,
            InProgressOperation::CherryPick(op) | InProgressOperation::Revert(op) => {
                &op.conflicted_files
            }
            InProgressOperation::BisectRun(op) => &op.conflicted_files,
        }
    }

    pub fn has_conflicts(&self) -> bool {
        !self.conflicted_files().is_empty()
    }

    pub fn capabilities(&self) -> &[OperationCapability] {
        match self {
            InProgressOperation::None => &[],
            InProgressOperation::Merge(op) => &op.capabilities,
            InProgressOperation::Rebase(op) => &op.capabilities,
            InProgressOperation::CherryPick(op) | InProgressOperation::Revert(op) => {
                &op.capabilities
            }
            InProgressOperation::BisectRun(op) => &op.capabilities,
        }
    }

    /// Whether `capability` is offered for this state — the primitive a
    /// future merge/rebase/cherry-pick/revert mutation calls before acting
    /// (US-078 criterion 3's own "the type allows this checking" scope).
    pub fn supports(&self, capability: OperationCapability) -> bool {
        self.capabilities().contains(&capability)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hash(s: &str) -> CommitHash {
        CommitHash::new(s.to_string()).unwrap()
    }

    #[test]
    fn none_reports_no_conflicts_no_capabilities_and_no_label() {
        let op = InProgressOperation::None;
        assert!(op.is_none());
        assert!(op.kind_label().is_none());
        assert!(op.conflicted_files().is_empty());
        assert!(!op.has_conflicts());
        assert!(op.capabilities().is_empty());
        assert!(!op.supports(OperationCapability::Abort));
    }

    #[test]
    fn merge_never_supports_skip() {
        let op = InProgressOperation::Merge(MergeOperation {
            heads: vec![hash("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa")],
            conflicted_files: vec![ConflictedFile {
                path: "a.txt".into(),
                stage: ConflictStage::BothModified,
            }],
            capabilities: vec![OperationCapability::Continue, OperationCapability::Abort],
        });

        assert!(!op.is_none());
        assert_eq!(op.kind_label(), Some("merge"));
        assert!(op.has_conflicts());
        assert!(op.supports(OperationCapability::Continue));
        assert!(op.supports(OperationCapability::Abort));
        assert!(
            !op.supports(OperationCapability::Skip),
            "a merge has no further step to skip past"
        );
    }

    #[test]
    fn rebase_supports_continue_skip_and_abort() {
        let op = InProgressOperation::Rebase(RebaseOperation {
            interactive: true,
            onto: Some(hash("bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb")),
            conflicted_files: vec![],
            capabilities: vec![
                OperationCapability::Continue,
                OperationCapability::Skip,
                OperationCapability::Abort,
            ],
        });

        assert_eq!(op.kind_label(), Some("rebase"));
        assert!(!op.has_conflicts());
        for capability in [
            OperationCapability::Continue,
            OperationCapability::Skip,
            OperationCapability::Abort,
        ] {
            assert!(op.supports(capability));
        }
    }

    #[test]
    fn cherry_pick_and_revert_share_the_sequencer_shape_but_remain_distinct_variants() {
        let cherry_pick = InProgressOperation::CherryPick(SequencerOperation {
            target: Some(hash("cccccccccccccccccccccccccccccccccccccccc")),
            conflicted_files: vec![],
            capabilities: vec![OperationCapability::Continue],
        });
        let revert = InProgressOperation::Revert(SequencerOperation {
            target: Some(hash("cccccccccccccccccccccccccccccccccccccccc")),
            conflicted_files: vec![],
            capabilities: vec![OperationCapability::Continue],
        });

        assert_eq!(cherry_pick.kind_label(), Some("cherry-pick"));
        assert_eq!(revert.kind_label(), Some("revert"));
        assert_ne!(cherry_pick, revert);
    }

    #[test]
    fn bisect_run_never_supports_continue() {
        let op = InProgressOperation::BisectRun(BisectOperation {
            conflicted_files: vec![],
            capabilities: vec![OperationCapability::Skip, OperationCapability::Abort],
        });

        assert_eq!(op.kind_label(), Some("bisect"));
        assert!(!op.supports(OperationCapability::Continue));
    }
}
