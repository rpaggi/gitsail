//! Cooperative cancellation signal (SAD §32: "large diffs ... should
//! support cancellation"; US-027 criterion 3).
//!
//! Lives in the domain crate rather than `gitsail-git` so read ports
//! (`RepositoryReadPort`) can accept one without `gitsail-application`
//! depending on a specific adapter (SAD §5 dependency rule). The adapter
//! that actually watches it while a process runs is `gitsail-git`.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

/// A cancellation flag shared between the caller requesting cancellation
/// and the adapter code polling for it. Cloning shares the same underlying
/// flag, so a token can be handed to a background thread while the
/// operation it guards is blocking on another.
#[derive(Debug, Clone, Default)]
pub struct CancellationToken(Arc<AtomicBool>);

impl CancellationToken {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn cancel(&self) {
        self.0.store(true, Ordering::SeqCst);
    }

    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::SeqCst)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cancellation_token_reflects_cancel_across_clones() {
        let token = CancellationToken::new();
        let clone = token.clone();
        assert!(!clone.is_cancelled());
        token.cancel();
        assert!(clone.is_cancelled());
    }
}
