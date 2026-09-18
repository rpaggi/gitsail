//! Reflog entry (SAD §8; T-241/US-089): a read-only record of where a
//! reference (typically `HEAD`) pointed at some point in the past.
//!
//! Inspecting a reflog is never a mutation — listing entries never runs
//! `git reset`/checks anything out (History Editing Rules #10: "Reflog
//! inspection is read-only"). Offering "go back to this state" from a
//! reflog entry would mean invoking `reset`, a distinct, already-existing,
//! explicit mutation (T-240/US-088); this module intentionally has no such
//! capability of its own.

use crate::commit::GitTimestamp;
use crate::ids::CommitHash;

/// Whether the commit object a [`ReflogEntry`] names can still be read from
/// the object database (US-089 criterion 3).
///
/// A reflog entry is normally what keeps its own target commit reachable
/// during Git's reflog-expiry grace period; once the entry itself has
/// expired (`git reflog expire`) and the object has been collected (`git gc
/// --prune`), the commit can no longer be inspected. This must never fail
/// the whole reflog listing — only mark that one entry's own object as
/// unavailable, so the entry itself (reference, hash, date, message) still
/// shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReflogObjectState {
    /// The commit object still exists and can be read (e.g. via
    /// `RepositoryReadPort::commit`) — a caller may open its full details.
    Present,
    /// The commit object no longer exists in the object database. Opening
    /// details for this entry is not possible; a caller must say so clearly
    /// rather than silently omitting the entry or failing the whole query.
    Missing,
}

/// One entry of a ref's reflog (US-089), newest (index 0, matching Git's own
/// `@{0}` selector) first.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReflogEntry {
    /// This entry's position, matching Git's own `@{n}` selector syntax
    /// (`HEAD@{0}` is the most recent entry) — mirrors [`crate::Stash`]'s own
    /// `index` convention.
    pub index: u32,
    /// The commit hash the reference pointed at, at this point in time.
    pub commit: CommitHash,
    /// The reflog's own message for this entry (e.g. `"commit: message"`,
    /// `"checkout: moving from a to b"`, `"reset: moving to HEAD~1"`) — this
    /// is Git's own reflog subject, distinct from the target commit's own
    /// message.
    pub message: String,
    pub date: GitTimestamp,
    /// Whether [`Self::commit`] can still be read (US-089 criterion 3).
    pub object_state: ReflogObjectState,
}

impl ReflogEntry {
    /// Git's own `@{n}` selector for this entry relative to `reference`
    /// (e.g. `"HEAD@{5}"`), for display (US-089 criterion 1).
    pub fn selector(&self, reference: &str) -> String {
        format!("{reference}@{{{}}}", self.index)
    }

    /// Whether [`Self::commit`] can still be inspected (US-089 criterion 2).
    pub fn is_available(&self) -> bool {
        matches!(self.object_state, ReflogObjectState::Present)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(state: ReflogObjectState) -> ReflogEntry {
        ReflogEntry {
            index: 5,
            commit: CommitHash::new("deadbeefdeadbeefdeadbeefdeadbeefdeadbeef").unwrap(),
            message: "commit: did a thing".to_string(),
            date: GitTimestamp::new(0, 0),
            object_state: state,
        }
    }

    #[test]
    fn selector_matches_gits_own_at_syntax() {
        let entry = sample(ReflogObjectState::Present);
        assert_eq!(entry.selector("HEAD"), "HEAD@{5}");
    }

    #[test]
    fn is_available_reflects_object_state() {
        assert!(sample(ReflogObjectState::Present).is_available());
        assert!(!sample(ReflogObjectState::Missing).is_available());
    }
}
