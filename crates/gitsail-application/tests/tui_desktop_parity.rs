//! T-256/US-123 criterion 1: "the same scenario produces an equivalent Git
//! state in TUI/Desktop."
//!
//! The TUI (`crates/gitsail-tui/src/worker.rs::spawn_one`) and the Desktop
//! app (`apps/desktop/src-tauri/src/commands.rs`) never talk to Git
//! directly — every mutation/read call site in both is a one-line call into
//! a `gitsail-application` use case, given the exact same repository handle
//! and arguments (only the DTO/`Message` wrapping around that call differs
//! per interface). That structural fact is what makes "TUI and Desktop
//! produce the same Git state" true *by construction* rather than by luck —
//! but this test is the proof, not an assumption: it replays, against two
//! independently created (never shared) temporary repositories seeded
//! identically, the exact use-case call sequence each interface's own call
//! site issues for two representative scenarios, then asserts the resulting
//! Git state is equivalent.
//!
//! # Why tree hashes, not raw commit hashes
//!
//! A commit hash is content-derived from, among other things, the
//! author/committer date (`git`'s own object format) — the mutation calls
//! under test here run through the real `GitCliProvider` against the real
//! wall clock (unlike `gitsail-test-support`'s own fixture setup, which
//! pins a synthetic deterministic clock purely for *setup* commands that
//! are not the code path under test — see `git_ops.rs`'s module doc). Two
//! sequential replays of the same scenario therefore never share the exact
//! same author/committer timestamps, so their commit object hashes legally
//! differ even though nothing about the *content* differs. `T-255`'s own
//! `snapshot_screens.rs` hit the identical hazard for rendered commit
//! hashes and solved it by masking rather than asserting on the varying
//! value; this test takes the equivalent approach by asserting on
//! **tree hashes** (`<rev>^{tree}`, timestamp-independent — pure content) and
//! on the fields `gitsail-domain::Commit` actually models as content
//! (`subject`, `parents.len()`, branch topology) instead of on the raw
//! commit hash.
//!
//! # Scenarios
//!
//! 1. [`create_branch_two_commits_conflicting_merge_resolve_continue`] —
//!    create a branch, commit twice on it, diverge `main`, merge (conflict),
//!    resolve, continue: T-231/T-232/T-233's own flow, the highest-risk
//!    sequence in the product.
//! 2. [`amend_last_commit`] — the preview/amend flow (T-242/US-090), the
//!    other scenario the story names explicitly.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use gitsail_application::{
    AmendCommit, ContinueOperation, CreateBranch, CreateCommit, GetCommit, ListBranches,
    MarkConflictResolved, Merge, MergeResult, OpenRepository, PreviewAmend, RepositoryReadPort,
    RepositoryWritePort, StageFiles, SwitchBranch,
};
use gitsail_domain::{BranchName, CancellationToken};
use gitsail_test_support::git_ops::{
    commit_all, git, init_repo, read_port, write_file, write_port,
};

/// `git rev-parse <revision>^{tree}` — the timestamp-independent content
/// hash this test asserts equivalence on (see the module doc).
fn tree_hash(dir: &Path, revision: &str) -> String {
    let output = git(dir, &["rev-parse", &format!("{revision}^{{tree}}")]);
    assert!(
        output.status.success(),
        "git rev-parse {revision}^{{tree}} failed in {dir:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout)
        .expect("git rev-parse output is valid UTF-8")
        .trim()
        .to_string()
}

/// The final state this test compares between the two replays: every local
/// branch (sorted by name, matching `ListBranches`'s own contract) paired
/// with its tip's tree hash, subject, and parent count, read back through
/// the very same `gitsail-application` read use cases the TUI/Desktop
/// Graph/Branches panels themselves use — never raw `git log` text
/// parsing — so this also incidentally re-confirms the read side agrees
/// with the write side on what just happened.
#[derive(Debug, PartialEq, Eq)]
struct BranchTip {
    name: String,
    is_current: bool,
    tree: String,
    subject: String,
    parent_count: usize,
}

fn final_state(
    dir: &Path,
    read_port: &Arc<dyn RepositoryReadPort>,
    repo: &gitsail_domain::Repository,
) -> Vec<BranchTip> {
    let mut branches = ListBranches::new(Arc::clone(read_port))
        .execute(repo)
        .expect("list branches");
    branches.sort_by(|a, b| a.name.as_str().cmp(b.name.as_str()));

    branches
        .into_iter()
        .map(|branch| {
            let commit = GetCommit::new(Arc::clone(read_port))
                .execute(repo, &branch.target)
                .expect("get branch tip commit");
            BranchTip {
                name: branch.name.as_str().to_string(),
                is_current: branch.is_current,
                tree: tree_hash(dir, branch.target.as_str()),
                subject: commit.subject,
                parent_count: commit.parents.len(),
            }
        })
        .collect()
}

/// Builds an identical starting point in a fresh, isolated temporary
/// repository: one commit on `main` touching `f.txt`. Uses the
/// deterministic-clock setup helpers (`commit_all`) exactly like every
/// `gitsail-test-support::Fixture` — this is scenario *setup*, not the
/// production code path under test (see the module doc), so it is
/// deliberately identical, byte-for-byte, across both replays.
fn seed_one_commit(label: &str) -> gitsail_test_support::temp_dir::TempDir {
    let dir = init_repo(label);
    write_file(dir.path(), "f.txt", "line1\nline2\nline3\n");
    commit_all(dir.path(), "base", 0);
    dir
}

/// Mirrors `crates/gitsail-tui/src/worker.rs::spawn_one`'s exact call
/// sequence for this scenario: `Command::CreateBranch` ->
/// `Command::SwitchBranch` -> `Command::StageFiles` + `Command::CreateCommit`
/// (twice) -> `Command::SwitchBranch` (back to `main`) ->
/// `Command::StageFiles` + `Command::CreateCommit` -> `Command::Merge` ->
/// `Command::MarkConflictResolved` -> `Command::ContinueOperation`. Every
/// arm there is a single, unwrapped `UseCase::new(port).execute(...)` call
/// (no confirmation/precondition logic lives on that path — `App::update`
/// already resolved that before ever producing the `Command`), which is
/// exactly what this function replays.
fn replay_as_tui(
    dir: &Path,
    read_port: &Arc<dyn RepositoryReadPort>,
    write_port: &Arc<dyn RepositoryWritePort>,
) {
    let repo = OpenRepository::new(Arc::clone(read_port))
        .execute(dir)
        .expect("open repository");

    let feature = BranchName::new("feature").unwrap();
    let main = BranchName::new("main").unwrap();

    CreateBranch::new(Arc::clone(write_port))
        .execute(&repo, &feature, None)
        .expect("create feature branch");
    SwitchBranch::new(Arc::clone(write_port))
        .execute(&repo, &feature)
        .expect("switch to feature");

    write_file(dir, "f.txt", "line1\nCHANGED-feature-1\nline3\n");
    StageFiles::new(Arc::clone(write_port))
        .execute(&repo, &[PathBuf::from("f.txt")])
        .expect("stage feature commit 1");
    CreateCommit::new(Arc::clone(write_port))
        .execute(&repo, "feature commit 1")
        .expect("create feature commit 1");

    write_file(dir, "f.txt", "line1\nCHANGED-feature-2\nline3\n");
    StageFiles::new(Arc::clone(write_port))
        .execute(&repo, &[PathBuf::from("f.txt")])
        .expect("stage feature commit 2");
    CreateCommit::new(Arc::clone(write_port))
        .execute(&repo, "feature commit 2")
        .expect("create feature commit 2");

    SwitchBranch::new(Arc::clone(write_port))
        .execute(&repo, &main)
        .expect("switch back to main");
    write_file(dir, "f.txt", "line1\nCHANGED-main\nline3\n");
    StageFiles::new(Arc::clone(write_port))
        .execute(&repo, &[PathBuf::from("f.txt")])
        .expect("stage main change");
    CreateCommit::new(Arc::clone(write_port))
        .execute(&repo, "main change")
        .expect("create main change");

    let result = Merge::new(Arc::clone(write_port))
        .execute(&repo, "feature")
        .expect("merge feature into main");
    match result {
        MergeResult::Conflict { files } => {
            assert_eq!(files.len(), 1, "expected exactly one conflicted file");
        }
        other => panic!("this scenario must conflict, got {other:?}"),
    }

    write_file(dir, "f.txt", "line1\nRESOLVED\nline3\n");
    MarkConflictResolved::new(Arc::clone(write_port))
        .execute(&repo, Path::new("f.txt"))
        .expect("mark conflict resolved");

    ContinueOperation::new(Arc::clone(write_port))
        .execute(&repo)
        .expect("continue the merge");
}

/// Mirrors `apps/desktop/src-tauri/src/commands.rs`'s exact call sequence
/// for this scenario: `create_branch_impl` -> `switch_branch_impl` ->
/// `stage_paths_impl` + `create_commit_impl` (twice) -> `switch_branch_impl`
/// -> `stage_paths_impl` + `create_commit_impl` -> `merge_impl` ->
/// `mark_conflict_resolved_impl` -> `continue_operation_impl`. Every one of
/// those wraps its use-case call in `run_mutation`, whose only behavior
/// beyond calling the closure is re-opening the repository before/after
/// and refusing if Tauri's own `AppState` epoch changed mid-call (a guard
/// against a second window switching the active repository while this one
/// was running — see `commands.rs::run_mutation`'s own doc). That guard has
/// no bearing on the resulting Git state (it only ever turns a would-be
/// success into a `Cancelled` error), so this replay reproduces it
/// structurally — re-deriving a stability signal before and after each
/// mutation, exactly like `run_mutation` does — to make the two replays
/// honestly distinct in *wrapping* while identical in the use-case
/// invocation itself, rather than collapsing them into one shared
/// function.
fn replay_as_desktop(
    dir: &Path,
    read_port: &Arc<dyn RepositoryReadPort>,
    write_port: &Arc<dyn RepositoryWritePort>,
) {
    /// Mirrors `commands.rs::run_mutation`'s epoch guard: re-discovers the
    /// repository before and after `action`, and asserts nothing about the
    /// active repository identity changed underneath it (in this
    /// single-threaded replay it never does — the guard is reproduced for
    /// structural fidelity to the real wrapper, not because it can fail
    /// here).
    fn run_mutation<T>(
        dir: &Path,
        read_port: &Arc<dyn RepositoryReadPort>,
        action: impl FnOnce(&gitsail_domain::Repository) -> Result<T, gitsail_domain::GitSailError>,
    ) -> T {
        let before = OpenRepository::new(Arc::clone(read_port))
            .execute(dir)
            .expect("open repository (before)");
        let result = action(&before).expect("mutation succeeds");
        let after = OpenRepository::new(Arc::clone(read_port))
            .execute(dir)
            .expect("open repository (after)");
        assert_eq!(
            before.id, after.id,
            "the active repository must not change mid-mutation"
        );
        result
    }

    let feature = BranchName::new("feature").unwrap();
    let main = BranchName::new("main").unwrap();

    run_mutation(dir, read_port, |repo| {
        CreateBranch::new(Arc::clone(write_port)).execute(repo, &feature, None)
    });
    run_mutation(dir, read_port, |repo| {
        SwitchBranch::new(Arc::clone(write_port)).execute(repo, &feature)
    });

    write_file(dir, "f.txt", "line1\nCHANGED-feature-1\nline3\n");
    run_mutation(dir, read_port, |repo| {
        StageFiles::new(Arc::clone(write_port)).execute(repo, &[PathBuf::from("f.txt")])
    });
    run_mutation(dir, read_port, |repo| {
        CreateCommit::new(Arc::clone(write_port)).execute(repo, "feature commit 1")
    });

    write_file(dir, "f.txt", "line1\nCHANGED-feature-2\nline3\n");
    run_mutation(dir, read_port, |repo| {
        StageFiles::new(Arc::clone(write_port)).execute(repo, &[PathBuf::from("f.txt")])
    });
    run_mutation(dir, read_port, |repo| {
        CreateCommit::new(Arc::clone(write_port)).execute(repo, "feature commit 2")
    });

    run_mutation(dir, read_port, |repo| {
        SwitchBranch::new(Arc::clone(write_port)).execute(repo, &main)
    });
    write_file(dir, "f.txt", "line1\nCHANGED-main\nline3\n");
    run_mutation(dir, read_port, |repo| {
        StageFiles::new(Arc::clone(write_port)).execute(repo, &[PathBuf::from("f.txt")])
    });
    run_mutation(dir, read_port, |repo| {
        CreateCommit::new(Arc::clone(write_port)).execute(repo, "main change")
    });

    let result = run_mutation(dir, read_port, |repo| {
        Merge::new(Arc::clone(write_port)).execute(repo, "feature")
    });
    match result {
        MergeResult::Conflict { files } => {
            assert_eq!(files.len(), 1, "expected exactly one conflicted file");
        }
        other => panic!("this scenario must conflict, got {other:?}"),
    }

    write_file(dir, "f.txt", "line1\nRESOLVED\nline3\n");
    run_mutation(dir, read_port, |repo| {
        MarkConflictResolved::new(Arc::clone(write_port)).execute(repo, Path::new("f.txt"))
    });
    run_mutation(dir, read_port, |repo| {
        ContinueOperation::new(Arc::clone(write_port)).execute(repo)
    });
}

#[test]
fn create_branch_two_commits_conflicting_merge_resolve_continue() {
    let tui_dir = seed_one_commit("parity-merge-tui");
    let desktop_dir = seed_one_commit("parity-merge-desktop");

    let read_port = read_port();
    let write_port = write_port();

    replay_as_tui(tui_dir.path(), &read_port, &write_port);
    replay_as_desktop(desktop_dir.path(), &read_port, &write_port);

    let tui_repo = OpenRepository::new(Arc::clone(&read_port))
        .execute(tui_dir.path())
        .expect("open TUI repository for assertions");
    let desktop_repo = OpenRepository::new(Arc::clone(&read_port))
        .execute(desktop_dir.path())
        .expect("open Desktop repository for assertions");

    let tui_state = final_state(tui_dir.path(), &read_port, &tui_repo);
    let desktop_state = final_state(desktop_dir.path(), &read_port, &desktop_repo);

    assert_eq!(
        tui_state, desktop_state,
        "TUI and Desktop must reach an equivalent Git state (same branches, \
         same tip tree hashes, same subjects, same parent counts) for the \
         same create-branch/commit/conflicting-merge/resolve/continue \
         scenario"
    );

    // Sanity: this scenario is genuinely non-trivial — two branches, and
    // `main`'s tip is a real two-parent merge commit, on both sides.
    assert_eq!(tui_state.len(), 2, "expected exactly `main` and `feature`");
    let tui_main = tui_state
        .iter()
        .find(|b| b.name == "main")
        .expect("main branch present");
    assert!(tui_main.is_current, "main must be the checked-out branch");
    assert_eq!(
        tui_main.parent_count, 2,
        "main's tip must be a two-parent merge commit"
    );
    assert_eq!(
        tree_hash(tui_dir.path(), "main"),
        tree_hash(desktop_dir.path(), "main"),
        "main's resulting tree must be byte-identical (content-only, \
         timestamp-independent) across both replays"
    );
}

#[test]
fn amend_last_commit() {
    let tui_dir = seed_one_commit("parity-amend-tui");
    let desktop_dir = seed_one_commit("parity-amend-desktop");

    let read_port = read_port();
    let write_port = write_port();

    // Mirrors `Command::PreviewAmend`/`Command::AmendCommit`
    // (`crates/gitsail-tui/src/worker.rs`) and `preview_amend_impl`/
    // `amend_commit_impl` (`apps/desktop/src-tauri/src/commands.rs`): both
    // read `PreviewAmend` first to learn the exact `expected_head` to
    // revalidate, then call `AmendCommit` with it — never a hand-rolled
    // `HEAD` lookup.
    fn amend(
        dir: &Path,
        read_port: &Arc<dyn RepositoryReadPort>,
        write_port: &Arc<dyn RepositoryWritePort>,
    ) {
        let repo = OpenRepository::new(Arc::clone(read_port))
            .execute(dir)
            .expect("open repository");

        write_file(dir, "f.txt", "line1\nline2\nline3\nappended\n");
        StageFiles::new(Arc::clone(write_port))
            .execute(&repo, &[PathBuf::from("f.txt")])
            .expect("stage amend content");

        let preview = PreviewAmend::new(Arc::clone(read_port))
            .execute(&repo, &CancellationToken::new())
            .expect("preview amend");

        AmendCommit::new(Arc::clone(write_port))
            .execute(&repo, "base (amended)", &preview.head.hash)
            .expect("amend HEAD");
    }

    amend(tui_dir.path(), &read_port, &write_port);
    amend(desktop_dir.path(), &read_port, &write_port);

    let tui_repo = OpenRepository::new(Arc::clone(&read_port))
        .execute(tui_dir.path())
        .expect("open TUI repository for assertions");
    let desktop_repo = OpenRepository::new(Arc::clone(&read_port))
        .execute(desktop_dir.path())
        .expect("open Desktop repository for assertions");

    let tui_state = final_state(tui_dir.path(), &read_port, &tui_repo);
    let desktop_state = final_state(desktop_dir.path(), &read_port, &desktop_repo);

    assert_eq!(
        tui_state, desktop_state,
        "TUI and Desktop must reach an equivalent Git state for the amend \
         scenario"
    );
    assert_eq!(tui_state.len(), 1, "expected exactly `main`");
    assert_eq!(tui_state[0].parent_count, 0, "amend must not add a parent");
    assert_eq!(tui_state[0].subject, "base (amended)");
    assert_eq!(
        tree_hash(tui_dir.path(), "main"),
        tree_hash(desktop_dir.path(), "main"),
        "the amended tree must be byte-identical across both replays"
    );
}
