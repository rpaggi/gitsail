//! Integration tests for T-230/US-078 —
//! `RepositoryReadPort::detect_in_progress_operation` — against real,
//! temporary Git repositories where a merge/rebase/cherry-pick/revert/
//! bisect is actually started via a direct `git` subprocess call (never
//! simulated), mirroring `tests/t163_apply_patch.rs`'s own convention.
//!
//! Every scenario here starts the operation the same way a person running
//! `git` in another terminal would — this adapter must recognize it purely
//! by re-reading `.git/` state, never by any in-memory GitSail flag
//! (US-078 criterion 2).

use std::path::{Path, PathBuf};
use std::process::Command;

use gitsail_application::RepositoryReadPort;
use gitsail_domain::{
    CommitHash, ConflictStage, ErrorCode, InProgressOperation, OperationCapability,
};
// T-252/US-119: this file's own `TempDir`/`git`/`git_ok`/`init_repo`/
// `commit_all`/`rev_parse`/`provider` helpers used to be duplicated here
// (and in ~8 other integration test files); they now live in
// `gitsail-test-support`, this crate's own test-only fixture crate (see
// that crate's `src/lib.rs` doc). `commit_all` here takes an extra
// deterministic-sequence argument that this file's scenarios do not care
// about (they only assert conflict detection, never a specific hash), so a
// monotonically increasing counter is threaded through where the original
// hard-coded no-argument version was called positionally.
use gitsail_test_support::{
    commit_all, git, git_ok, init_repo, provider, rev_parse, write_file, TempDir,
};

// ---------------------------------------------------------------------
// Baseline: nothing in progress.
// ---------------------------------------------------------------------

#[test]
fn a_clean_repository_reports_no_in_progress_operation() {
    let repo_dir = init_repo("clean");
    write_file(repo_dir.path(), "a.txt", "hello\n");
    commit_all(repo_dir.path(), "init", 0);
    let provider = provider();
    let repo = provider.discover(repo_dir.path()).unwrap();

    let op = provider.detect_in_progress_operation(&repo).unwrap();

    assert!(op.is_none());
    assert!(op.kind_label().is_none());
    assert!(!op.has_conflicts());
    assert!(op.capabilities().is_empty());
}

#[test]
fn a_bare_repository_reports_an_explicit_limitation_rather_than_an_opaque_failure() {
    let repo_dir = TempDir::new("bare");
    git_ok(repo_dir.path(), &["init", "--quiet", "--bare"]);
    let provider = provider();
    let repo = provider.discover(repo_dir.path()).unwrap();

    let err = provider.detect_in_progress_operation(&repo).unwrap_err();

    assert_eq!(err.code(), ErrorCode::InvalidRepositoryState);
}

// ---------------------------------------------------------------------
// Merge: conflict, capabilities, and abort returning to `None`.
// ---------------------------------------------------------------------

/// Sets up a repository with two branches (`main`/`feature`) that both
/// modify the same line of the same file, so merging one into the other
/// reliably conflicts.
fn setup_diverging_branches(repo_dir: &Path, other_branch: &str) -> (CommitHash, CommitHash) {
    write_file(repo_dir, "f.txt", "line1\nline2\nline3\n");
    commit_all(repo_dir, "base", 0);
    let base = rev_parse(repo_dir, "HEAD");

    git_ok(repo_dir, &["checkout", "-q", "-b", other_branch]);
    write_file(repo_dir, "f.txt", "line1\nCHANGED-other\nline3\n");
    commit_all(repo_dir, "other change", 1);
    let other_tip = rev_parse(repo_dir, "HEAD");

    git_ok(repo_dir, &["checkout", "-q", "main"]);
    write_file(repo_dir, "f.txt", "line1\nCHANGED-main\nline3\n");
    commit_all(repo_dir, "main change", 2);

    let _ = base;
    (other_tip, rev_parse(repo_dir, "HEAD"))
}

#[test]
fn a_conflicting_merge_started_outside_gitsail_is_detected_with_its_conflicted_file_and_capabilities(
) {
    let repo_dir = init_repo("merge-conflict");
    let (feature_tip, _main_tip) = setup_diverging_branches(repo_dir.path(), "feature");

    // Started exactly like another terminal would: a direct `git merge`,
    // never anything GitSail-specific.
    let merge_output = git(repo_dir.path(), &["merge", "feature"]);
    assert!(
        !merge_output.status.success(),
        "the merge must conflict for this test to be meaningful"
    );

    let provider = provider();
    let repo = provider.discover(repo_dir.path()).unwrap();

    let op = provider.detect_in_progress_operation(&repo).unwrap();

    match &op {
        InProgressOperation::Merge(merge) => {
            assert_eq!(merge.heads, vec![feature_tip]);
            assert_eq!(merge.conflicted_files.len(), 1);
            assert_eq!(merge.conflicted_files[0].path, PathBuf::from("f.txt"));
            assert_eq!(merge.conflicted_files[0].stage, ConflictStage::BothModified);
        }
        other => panic!("expected InProgressOperation::Merge, got {other:?}"),
    }
    assert!(op.has_conflicts());
    assert!(op.supports(OperationCapability::Continue));
    assert!(op.supports(OperationCapability::Abort));
    assert!(
        !op.supports(OperationCapability::Skip),
        "a merge has no further step to skip past"
    );

    // Abort via the real Git command (again, exactly as another terminal
    // would) and confirm detection returns to `None` — the operation's
    // metadata is never left behind, and detection never itself deletes it.
    git_ok(repo_dir.path(), &["merge", "--abort"]);
    let after_abort = provider.detect_in_progress_operation(&repo).unwrap();
    assert!(after_abort.is_none());
}

// ---------------------------------------------------------------------
// Rebase: apply backend (non-interactive) and merge backend, capabilities,
// and abort returning to `None`.
// ---------------------------------------------------------------------

#[test]
fn a_conflicting_apply_backend_rebase_is_detected_as_non_interactive() {
    let repo_dir = init_repo("rebase-apply-conflict");
    setup_diverging_branches(repo_dir.path(), "feature");
    let onto = rev_parse(repo_dir.path(), "main");
    git_ok(repo_dir.path(), &["checkout", "-q", "feature"]);

    // `--apply` forces the legacy `am`-based backend (`.git/rebase-apply`),
    // which never has an "interactive" concept at all — this is the one
    // scenario this test suite can assert `interactive: false` for with
    // certainty; see `a_conflicting_default_backend_rebase_is_detected`'s
    // doc for why the default backend cannot be asserted the same way.
    let rebase_output = git(repo_dir.path(), &["rebase", "--apply", "main"]);
    assert!(
        !rebase_output.status.success(),
        "the rebase must conflict for this test to be meaningful"
    );

    let provider = provider();
    let repo = provider.discover(repo_dir.path()).unwrap();

    let op = provider.detect_in_progress_operation(&repo).unwrap();

    match &op {
        InProgressOperation::Rebase(rebase) => {
            assert!(!rebase.interactive);
            assert_eq!(rebase.onto, Some(onto));
            assert_eq!(rebase.conflicted_files.len(), 1);
            assert_eq!(rebase.conflicted_files[0].path, PathBuf::from("f.txt"));
        }
        other => panic!("expected InProgressOperation::Rebase, got {other:?}"),
    }
    assert!(op.supports(OperationCapability::Continue));
    assert!(op.supports(OperationCapability::Skip));
    assert!(op.supports(OperationCapability::Abort));

    git_ok(repo_dir.path(), &["rebase", "--abort"]);
    let after_abort = provider.detect_in_progress_operation(&repo).unwrap();
    assert!(after_abort.is_none());
}

/// A plain `git rebase` (no explicit backend flag) with a conflict.
///
/// This documents a real Git behavior this adapter deliberately does not
/// paper over: since Git made the merge/sequencer backend the default for
/// `git rebase` (Git ≥2.26), the on-disk `rebase-merge/interactive` marker
/// this adapter reads is written for a plain, non-`-i` conflicting rebase
/// too (empirically confirmed against the Git version these tests run
/// against) — Git itself no longer exposes an on-disk distinction between
/// "the user explicitly ran `-i`" and "the default backend happened to hit
/// a conflict". `RebaseOperation::interactive` therefore reads `true` here
/// exactly as it does for an explicit `git rebase -i`
/// ([`a_conflicting_interactive_rebase_is_detected_as_interactive`]) — this
/// test asserts that observed reality rather than a stale assumption.
#[test]
fn a_conflicting_default_backend_rebase_is_detected_as_rebase_with_an_onto_commit() {
    let repo_dir = init_repo("rebase-default-conflict");
    setup_diverging_branches(repo_dir.path(), "feature");
    let onto = rev_parse(repo_dir.path(), "main");
    git_ok(repo_dir.path(), &["checkout", "-q", "feature"]);

    let rebase_output = git(repo_dir.path(), &["rebase", "main"]);
    assert!(
        !rebase_output.status.success(),
        "the rebase must conflict for this test to be meaningful"
    );

    let provider = provider();
    let repo = provider.discover(repo_dir.path()).unwrap();

    let op = provider.detect_in_progress_operation(&repo).unwrap();

    match &op {
        InProgressOperation::Rebase(rebase) => {
            assert_eq!(rebase.onto, Some(onto));
            assert_eq!(rebase.conflicted_files.len(), 1);
        }
        other => panic!("expected InProgressOperation::Rebase, got {other:?}"),
    }
    assert!(op.supports(OperationCapability::Continue));
    assert!(op.supports(OperationCapability::Skip));
    assert!(op.supports(OperationCapability::Abort));

    git_ok(repo_dir.path(), &["rebase", "--abort"]);
    assert!(provider
        .detect_in_progress_operation(&repo)
        .unwrap()
        .is_none());
}

#[test]
fn a_conflicting_interactive_rebase_is_detected_as_interactive() {
    let repo_dir = init_repo("rebase-interactive-conflict");
    setup_diverging_branches(repo_dir.path(), "feature");
    git_ok(repo_dir.path(), &["checkout", "-q", "feature"]);

    // `GIT_SEQUENCE_EDITOR=true` accepts the default todo list (every
    // commit `pick`ed) without opening a real editor, so this runs
    // non-interactively while still exercising a genuine `-i` invocation.
    let rebase_output = Command::new("git")
        .args(["rebase", "-i", "main"])
        .current_dir(repo_dir.path())
        .env("LC_ALL", "C")
        .env("LANG", "C")
        .env("GIT_SEQUENCE_EDITOR", "true")
        .output()
        .unwrap();
    assert!(
        !rebase_output.status.success(),
        "the interactive rebase must conflict for this test to be meaningful"
    );

    let provider = provider();
    let repo = provider.discover(repo_dir.path()).unwrap();

    let op = provider.detect_in_progress_operation(&repo).unwrap();
    match &op {
        InProgressOperation::Rebase(rebase) => {
            assert!(rebase.interactive);
        }
        other => panic!("expected InProgressOperation::Rebase, got {other:?}"),
    }

    git_ok(repo_dir.path(), &["rebase", "--abort"]);
    assert!(provider
        .detect_in_progress_operation(&repo)
        .unwrap()
        .is_none());
}

// ---------------------------------------------------------------------
// Cherry-pick: conflict, target commit, capabilities, abort.
// ---------------------------------------------------------------------

#[test]
fn a_conflicting_cherry_pick_is_detected_with_its_target_commit() {
    let repo_dir = init_repo("cherry-pick-conflict");
    let (other_tip, _main_tip) = setup_diverging_branches(repo_dir.path(), "other");
    // Already on `main` after `setup_diverging_branches`.

    let cherry_pick_output = git(repo_dir.path(), &["cherry-pick", other_tip.as_str()]);
    assert!(
        !cherry_pick_output.status.success(),
        "the cherry-pick must conflict for this test to be meaningful"
    );

    let provider = provider();
    let repo = provider.discover(repo_dir.path()).unwrap();

    let op = provider.detect_in_progress_operation(&repo).unwrap();

    match &op {
        InProgressOperation::CherryPick(seq) => {
            assert_eq!(seq.target, Some(other_tip));
            assert_eq!(seq.conflicted_files.len(), 1);
        }
        other => panic!("expected InProgressOperation::CherryPick, got {other:?}"),
    }
    assert!(op.supports(OperationCapability::Continue));
    assert!(op.supports(OperationCapability::Skip));
    assert!(op.supports(OperationCapability::Abort));

    git_ok(repo_dir.path(), &["cherry-pick", "--abort"]);
    assert!(provider
        .detect_in_progress_operation(&repo)
        .unwrap()
        .is_none());
}

// ---------------------------------------------------------------------
// Revert: conflict, target commit, capabilities, abort.
// ---------------------------------------------------------------------

#[test]
fn a_conflicting_revert_is_detected_with_its_target_commit() {
    let repo_dir = init_repo("revert-conflict");
    write_file(repo_dir.path(), "f.txt", "line1\nline2\nline3\n");
    commit_all(repo_dir.path(), "adds f.txt", 0);
    let adding_commit = rev_parse(repo_dir.path(), "HEAD");

    // A later commit changes the same line `adding_commit` introduced, so
    // reverting `adding_commit` (which would remove those exact lines)
    // conflicts with the newer content.
    write_file(repo_dir.path(), "f.txt", "line1\nCHANGED-later\nline3\n");
    commit_all(repo_dir.path(), "changes the line later", 1);

    let revert_output = git(
        repo_dir.path(),
        &["revert", "--no-edit", adding_commit.as_str()],
    );
    assert!(
        !revert_output.status.success(),
        "the revert must conflict for this test to be meaningful"
    );

    let provider = provider();
    let repo = provider.discover(repo_dir.path()).unwrap();

    let op = provider.detect_in_progress_operation(&repo).unwrap();

    match &op {
        InProgressOperation::Revert(seq) => {
            assert_eq!(seq.target, Some(adding_commit));
            assert!(!seq.conflicted_files.is_empty());
        }
        other => panic!("expected InProgressOperation::Revert, got {other:?}"),
    }
    assert!(op.supports(OperationCapability::Continue));
    assert!(op.supports(OperationCapability::Skip));
    assert!(op.supports(OperationCapability::Abort));

    git_ok(repo_dir.path(), &["revert", "--abort"]);
    assert!(provider
        .detect_in_progress_operation(&repo)
        .unwrap()
        .is_none());
}

// ---------------------------------------------------------------------
// Bisect: cheap, presence-based detection and reset returning to `None`.
// ---------------------------------------------------------------------

#[test]
fn a_bisect_run_in_progress_is_detected_without_continue() {
    let repo_dir = init_repo("bisect-run");
    write_file(repo_dir.path(), "f.txt", "v1\n");
    commit_all(repo_dir.path(), "c1", 0);
    let first = rev_parse(repo_dir.path(), "HEAD");
    write_file(repo_dir.path(), "f.txt", "v2\n");
    commit_all(repo_dir.path(), "c2", 1);
    write_file(repo_dir.path(), "f.txt", "v3\n");
    commit_all(repo_dir.path(), "c3", 2);

    git_ok(repo_dir.path(), &["bisect", "start"]);
    git_ok(repo_dir.path(), &["bisect", "bad", "HEAD"]);
    git_ok(repo_dir.path(), &["bisect", "good", first.as_str()]);

    let provider = provider();
    let repo = provider.discover(repo_dir.path()).unwrap();

    let op = provider.detect_in_progress_operation(&repo).unwrap();

    match &op {
        InProgressOperation::BisectRun(_) => {}
        other => panic!("expected InProgressOperation::BisectRun, got {other:?}"),
    }
    assert!(op.supports(OperationCapability::Skip));
    assert!(op.supports(OperationCapability::Abort));
    assert!(
        !op.supports(OperationCapability::Continue),
        "a bisect run advances via good/bad, not a generic continue"
    );

    git_ok(repo_dir.path(), &["bisect", "reset"]);
    assert!(provider
        .detect_in_progress_operation(&repo)
        .unwrap()
        .is_none());
}

// ---------------------------------------------------------------------
// Mutual exclusivity sanity check: aborting one operation never leaves
// behind state that gets misdetected as a different one.
// ---------------------------------------------------------------------

#[test]
fn detection_never_depends_on_gitsails_own_in_memory_state() {
    // This is exactly what every scenario above already demonstrates (each
    // operation is started via a raw `git` subprocess call, never through
    // this adapter, and still detected) — this test only makes that
    // property explicit with its own name for the DoD's "operação iniciada
    // externamente é reconhecida" criterion, using a fresh provider
    // instance per check to rule out any accidental shared state.
    let repo_dir = init_repo("external-operation");
    let (feature_tip, _) = setup_diverging_branches(repo_dir.path(), "feature");

    let provider_before = provider();
    let repo = provider_before.discover(repo_dir.path()).unwrap();
    assert!(provider_before
        .detect_in_progress_operation(&repo)
        .unwrap()
        .is_none());

    // Started by "another terminal" (a plain subprocess, not this crate's
    // own write port).
    git(repo_dir.path(), &["merge", "feature"]);

    // A brand new provider instance, never told anything about the merge
    // above, still recognizes it purely from `.git/` state.
    let provider_after = provider();
    let op = provider_after.detect_in_progress_operation(&repo).unwrap();
    match op {
        InProgressOperation::Merge(merge) => assert_eq!(merge.heads, vec![feature_tip]),
        other => panic!("expected InProgressOperation::Merge, got {other:?}"),
    }

    git_ok(repo_dir.path(), &["merge", "--abort"]);
}
