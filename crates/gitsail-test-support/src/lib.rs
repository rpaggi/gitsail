//! Shared, temporary Git fixture machinery for GitSail's integration tests
//! (T-252/US-119, EPIC-24).
//!
//! # Why this crate exists
//!
//! Before this crate, nine `gitsail-git` integration test files (`tests/
//! provider.rs`, `runner.rs`, `t230_in_progress_operation.rs`,
//! `t231_233_merge_conflicts.rs`, `t235_237_rebase.rs`,
//! `t238_240_cherry_pick_revert_reset.rs`, `t241_reflog.rs`,
//! `epic18_stash_tags_worktrees.rs`, `epic19_remote_operations.rs`,
//! `t163_apply_patch.rs`, `performance_baseline.rs`) plus `gitsail-tui`'s
//! own `tests/support/mod.rs` each hand-rolled their own copy of the same
//! `struct TempDir` + `fn init_repo`/`fn git`/`fn git_ok`/`fn commit_all`/
//! `fn rev_parse` shape, and several also duplicated a `setup_diverging_branches`
//! helper for reliably producing a merge/rebase conflict. This crate is the
//! single place that logic now lives.
//!
//! # Design choice: a dedicated crate, not a `#[cfg(test)]` module
//!
//! `gitsail-git`'s own integration tests live in `tests/*.rs`, each compiled
//! as its own separate crate — a `#[cfg(test)]` module inside
//! `gitsail-git/src/` is *not* visible to them (only to `gitsail-git`'s own
//! unit tests), and a `test-support` Cargo feature gating a `pub` module
//! would leak fixture-building code (and its `std::process::Command` use)
//! into every normal, non-test build unless every downstream consumer
//! remembered to disable the feature. A separate crate, added only as a
//! `[dev-dependencies]` entry, is visible to `tests/*.rs` binaries in any
//! crate that depends on it (today `gitsail-git`; `gitsail-tui` and
//! `gitsail-cli` can migrate their own test support to this crate
//! incrementally, see the module docs below for what has and has not moved
//! yet) and adds zero cost/surface to any production build.
//!
//! This does introduce a dependency-graph shape worth calling out: this
//! crate has a normal dependency on `gitsail-git` (it wires up a real
//! `GitCliProvider` for [`git_ops::provider`]/[`git_ops::read_port`]/
//! [`git_ops::write_port`]), while `gitsail-git`'s `[dev-dependencies]`
//! depends back on this crate. That is a cycle through dev-dependencies
//! only, which Cargo explicitly supports (dev-dependencies are excluded from
//! the graph used to build the library artifact itself, only wired in for
//! test/example/bench targets) — the same pattern used whenever a
//! workspace's own test-utility crate needs to construct the very type it
//! helps test.
//!
//! # What has been migrated so far
//!
//! `gitsail-git/tests/runner.rs` and `gitsail-git/tests/t230_in_progress_operation.rs`
//! now use this crate instead of their own local copies, as a proof of
//! concept (T-252/US-119 task note: migrating the remaining ~8 files is an
//! explicitly non-blocking, incremental follow-up — see this task's session
//! report for the reasoning).

pub mod contract;
pub mod fixture;
pub mod git_ops;
pub mod temp_dir;

pub use fixture::{Fixture, FixtureState, InProgressOperationKind};
pub use git_ops::{
    commit_all, git, git_at, git_ok, git_ok_at, init_repo, provider, read_port, rev_parse,
    write_binary_file, write_file, write_port, FIXTURE_AUTHOR_EMAIL, FIXTURE_AUTHOR_NAME,
};
pub use temp_dir::TempDir;
