//! A uniquely named temporary directory, removed on drop.
//!
//! Consolidates the identical `struct TempDir` hand-copied into every one of
//! `gitsail-git`'s ~9 integration test files (`tests/provider.rs`,
//! `tests/runner.rs`, `tests/t230_in_progress_operation.rs`,
//! `tests/t231_233_merge_conflicts.rs`, `tests/t235_237_rebase.rs`,
//! `tests/t238_240_cherry_pick_revert_reset.rs`, `tests/t241_reflog.rs`,
//! `tests/epic18_stash_tags_worktrees.rs`,
//! `tests/epic19_remote_operations.rs`, `tests/t163_apply_patch.rs`,
//! `tests/performance_baseline.rs`) plus `gitsail-tui`'s
//! `tests/support/mod.rs` — every copy was byte-for-byte the same shape,
//! differing only in the directory name prefix.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

/// Process-wide counter, combined with a nanosecond timestamp, so two
/// [`TempDir`]s created in the same process (even from concurrently running
/// tests, per `cargo test`'s default parallelism) never collide on a path —
/// this is what keeps every fixture's cleanup isolated to its own directory
/// (T-252/US-119 DoD: "nenhuma fixture usa um caminho fixo compartilhado
/// entre execuções paralelas").
static COUNTER: AtomicU32 = AtomicU32::new(0);

/// A directory under [`std::env::temp_dir`] unique to this instance,
/// recursively removed when the instance is dropped. `label` is purely for a
/// human skimming `/tmp` while debugging a failing test; it plays no role in
/// uniqueness.
pub struct TempDir(PathBuf);

impl TempDir {
    pub fn new(label: &str) -> Self {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock is after the Unix epoch")
            .as_nanos();
        let n = COUNTER.fetch_add(1, Ordering::SeqCst);
        let path =
            std::env::temp_dir().join(format!("gitsail-test-support-{label}-{nanos}-{n}"));
        std::fs::create_dir_all(&path).expect("create temp dir");
        Self(path)
    }

    pub fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_instance_gets_its_own_unique_directory() {
        let a = TempDir::new("dup-check");
        let b = TempDir::new("dup-check");
        assert_ne!(a.path(), b.path());
        assert!(a.path().is_dir());
        assert!(b.path().is_dir());
    }

    #[test]
    fn the_directory_is_removed_on_drop() {
        let path = {
            let dir = TempDir::new("drop-check");
            let path = dir.path().to_path_buf();
            assert!(path.is_dir());
            path
        };
        assert!(!path.exists(), "directory must not survive the TempDir being dropped");
    }
}
