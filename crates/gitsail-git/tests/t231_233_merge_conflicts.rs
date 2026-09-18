//! Integration tests for T-231/US-079 (`RepositoryWritePort::merge`),
//! T-232/US-080 (`RepositoryReadPort::conflict_sides`,
//! `RepositoryWritePort::mark_conflict_resolved`/`take_conflict_side`), and
//! T-233/US-081 (`RepositoryWritePort::continue_operation`/
//! `abort_operation`) against real, temporary Git repositories — mirroring
//! `tests/t230_in_progress_operation.rs`'s own fixture conventions.
//!
//! Every merge scenario exercises real `git merge` behavior end to end:
//! fast-forward, a genuine merge commit, and a genuine conflict are always
//! asserted as three distinct [`MergeResult`] variants (never collapsed into
//! one another, and a conflict never reported as success) — the exact
//! guarantee T-231's own acceptance criteria require.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use gitsail_application::{MergeResult, RepositoryReadPort, RepositoryWritePort};
use gitsail_domain::{
    CommitHash, ConflictSide, ConflictSideContent, ErrorCode, InProgressOperation,
};
use gitsail_git::{GitCliProvider, GitProcessRunner, GitProcessRunnerConfig};

/// A uniquely named temporary directory, removed on drop.
struct TempDir(PathBuf);

impl TempDir {
    fn new(label: &str) -> Self {
        static COUNTER: AtomicU32 = AtomicU32::new(0);
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let n = COUNTER.fetch_add(1, Ordering::SeqCst);
        let path = std::env::temp_dir().join(format!("gitsail-git-merge-{label}-{nanos}-{n}"));
        std::fs::create_dir_all(&path).expect("create temp dir");
        Self(path)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// Runs `git <args>` directly (test fixture setup only — production code in
/// this crate must always go through `GitProcessRunner`).
fn git(dir: &Path, args: &[&str]) -> std::process::Output {
    Command::new(dir_git_binary())
        .args(args)
        .current_dir(dir)
        .env("LC_ALL", "C")
        .env("LANG", "C")
        .output()
        .unwrap_or_else(|e| panic!("failed to spawn git {args:?}: {e}"))
}

fn dir_git_binary() -> &'static str {
    "git"
}

fn git_ok(dir: &Path, args: &[&str]) {
    let output = git(dir, args);
    assert!(
        output.status.success(),
        "git {args:?} failed in {dir:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn init_repo(label: &str) -> TempDir {
    let dir = TempDir::new(label);
    git_ok(dir.path(), &["init", "--quiet", "--initial-branch=main"]);
    git_ok(dir.path(), &["config", "user.name", "Test User"]);
    git_ok(dir.path(), &["config", "user.email", "test@example.com"]);
    dir
}

fn write_file(dir: &Path, name: &str, contents: &str) {
    std::fs::write(dir.join(name), contents).unwrap();
}

fn write_binary_file(dir: &Path, name: &str, contents: &[u8]) {
    std::fs::write(dir.join(name), contents).unwrap();
}

fn commit_all(dir: &Path, message: &str) {
    git_ok(dir, &["add", "-A"]);
    git_ok(dir, &["commit", "--quiet", "-m", message]);
}

fn rev_parse(dir: &Path, revision: &str) -> CommitHash {
    let output = git(dir, &["rev-parse", revision]);
    assert!(output.status.success(), "git rev-parse {revision} failed");
    let hash = String::from_utf8(output.stdout).unwrap().trim().to_string();
    CommitHash::new(hash).unwrap()
}

fn provider() -> GitCliProvider {
    let runner = GitProcessRunner::new(GitProcessRunnerConfig::default())
        .expect("git must be installed to run these integration tests");
    GitCliProvider::new(runner)
}

// ---------------------------------------------------------------------
// T-231/US-079: merge — fast-forward, merge commit, conflict.
// ---------------------------------------------------------------------

#[test]
fn merge_fast_forwards_when_the_current_branch_has_no_divergent_work() {
    let repo_dir = init_repo("ff");
    write_file(repo_dir.path(), "f.txt", "line1\n");
    commit_all(repo_dir.path(), "base");

    git_ok(repo_dir.path(), &["checkout", "-q", "-b", "feature"]);
    write_file(repo_dir.path(), "f.txt", "line1\nline2\n");
    commit_all(repo_dir.path(), "feature change");
    let feature_tip = rev_parse(repo_dir.path(), "HEAD");

    git_ok(repo_dir.path(), &["checkout", "-q", "main"]);

    let provider = provider();
    let repo = provider.discover(repo_dir.path()).unwrap();

    let result = RepositoryWritePort::merge(&provider, &repo, "feature").unwrap();

    match result {
        MergeResult::FastForwarded { new_head } => assert_eq!(new_head, feature_tip),
        other => panic!("expected FastForwarded, got {other:?}"),
    }
    assert_eq!(rev_parse(repo_dir.path(), "HEAD"), feature_tip);
    assert!(provider
        .detect_in_progress_operation(&repo)
        .unwrap()
        .is_none());
}

#[test]
fn merge_already_up_to_date_still_reports_a_fast_forward_at_the_unchanged_head() {
    let repo_dir = init_repo("already-up-to-date");
    write_file(repo_dir.path(), "f.txt", "line1\n");
    commit_all(repo_dir.path(), "base");
    let head = rev_parse(repo_dir.path(), "HEAD");

    let provider = provider();
    let repo = provider.discover(repo_dir.path()).unwrap();

    // Merging `HEAD` into itself: nothing to integrate.
    let result = RepositoryWritePort::merge(&provider, &repo, "HEAD").unwrap();

    match result {
        MergeResult::FastForwarded { new_head } => assert_eq!(new_head, head),
        other => panic!("expected FastForwarded, got {other:?}"),
    }
}

#[test]
fn merge_creates_a_two_parent_merge_commit_for_diverging_non_conflicting_work() {
    let repo_dir = init_repo("merge-commit");
    write_file(repo_dir.path(), "a.txt", "a\n");
    write_file(repo_dir.path(), "b.txt", "b\n");
    commit_all(repo_dir.path(), "base");
    let base = rev_parse(repo_dir.path(), "HEAD");

    git_ok(repo_dir.path(), &["checkout", "-q", "-b", "feature"]);
    write_file(repo_dir.path(), "a.txt", "a\nfeature change\n");
    commit_all(repo_dir.path(), "feature change");
    let feature_tip = rev_parse(repo_dir.path(), "HEAD");

    git_ok(repo_dir.path(), &["checkout", "-q", "main"]);
    write_file(repo_dir.path(), "b.txt", "b\nmain change\n");
    commit_all(repo_dir.path(), "main change");
    let main_tip = rev_parse(repo_dir.path(), "HEAD");

    let provider = provider();
    let repo = provider.discover(repo_dir.path()).unwrap();

    let result = RepositoryWritePort::merge(&provider, &repo, "feature").unwrap();

    let hash = match result {
        MergeResult::MergeCommitCreated { hash } => hash,
        other => panic!("expected MergeCommitCreated, got {other:?}"),
    };
    assert_eq!(rev_parse(repo_dir.path(), "HEAD"), hash);

    let parents_output = git(repo_dir.path(), &["rev-parse", "HEAD^1", "HEAD^2"]);
    assert!(parents_output.status.success());
    let parents = String::from_utf8(parents_output.stdout).unwrap();
    assert!(parents.contains(main_tip.as_str()));
    assert!(parents.contains(feature_tip.as_str()));
    let _ = base;

    assert!(provider
        .detect_in_progress_operation(&repo)
        .unwrap()
        .is_none());
}

/// Sets up two branches that both modify the same line of the same file, so
/// merging one into the other reliably conflicts — mirrors
/// `t230_in_progress_operation.rs::setup_diverging_branches`.
fn setup_diverging_branches(repo_dir: &Path, other_branch: &str) -> CommitHash {
    write_file(repo_dir, "f.txt", "line1\nline2\nline3\n");
    commit_all(repo_dir, "base");

    git_ok(repo_dir, &["checkout", "-q", "-b", other_branch]);
    write_file(repo_dir, "f.txt", "line1\nCHANGED-other\nline3\n");
    commit_all(repo_dir, "other change");
    let other_tip = rev_parse(repo_dir, "HEAD");

    git_ok(repo_dir, &["checkout", "-q", "main"]);
    write_file(repo_dir, "f.txt", "line1\nCHANGED-main\nline3\n");
    commit_all(repo_dir, "main change");

    other_tip
}

#[test]
fn merge_reports_a_conflict_distinctly_never_as_a_completed_success() {
    let repo_dir = init_repo("merge-conflict-result");
    setup_diverging_branches(repo_dir.path(), "feature");

    let provider = provider();
    let repo = provider.discover(repo_dir.path()).unwrap();

    let result = RepositoryWritePort::merge(&provider, &repo, "feature").unwrap();

    match result {
        MergeResult::Conflict { files } => {
            assert_eq!(files.len(), 1);
            assert_eq!(files[0].path, PathBuf::from("f.txt"));
        }
        other => panic!("a conflict must never be reported as {other:?}"),
    }

    // The repository is genuinely left with a pending merge for T-232/T-233
    // to pick up — this call never silently cleans anything up.
    let op = provider.detect_in_progress_operation(&repo).unwrap();
    assert!(matches!(op, InProgressOperation::Merge(_)));

    git_ok(repo_dir.path(), &["merge", "--abort"]);
}

#[test]
fn merge_refuses_to_start_when_a_merge_is_already_in_progress() {
    let repo_dir = init_repo("merge-refuses-on-existing-op");
    setup_diverging_branches(repo_dir.path(), "feature");

    let provider = provider();
    let repo = provider.discover(repo_dir.path()).unwrap();

    // Leave a real conflicting merge pending (started directly, as another
    // terminal would).
    let merge_output = git(repo_dir.path(), &["merge", "feature"]);
    assert!(!merge_output.status.success());

    let err = RepositoryWritePort::merge(&provider, &repo, "feature").unwrap_err();
    assert_eq!(err.code(), ErrorCode::OperationConflict);

    // Never silently proceeded, discarded metadata, or ran a second merge on
    // top of the first.
    assert!(matches!(
        provider.detect_in_progress_operation(&repo).unwrap(),
        InProgressOperation::Merge(_)
    ));

    git_ok(repo_dir.path(), &["merge", "--abort"]);
}

#[test]
fn merge_rejects_an_unresolvable_target_with_a_clear_error() {
    let repo_dir = init_repo("merge-bad-target");
    write_file(repo_dir.path(), "f.txt", "line1\n");
    commit_all(repo_dir.path(), "base");

    let provider = provider();
    let repo = provider.discover(repo_dir.path()).unwrap();

    let err = RepositoryWritePort::merge(&provider, &repo, "does-not-exist").unwrap_err();

    assert_eq!(err.code(), ErrorCode::RepositoryNotFound);
}

// ---------------------------------------------------------------------
// T-232/US-080: conflict_sides, mark_conflict_resolved, take_conflict_side.
// ---------------------------------------------------------------------

#[test]
fn conflict_sides_reads_base_ours_and_theirs_for_a_textual_conflict() {
    let repo_dir = init_repo("conflict-sides-text");
    setup_diverging_branches(repo_dir.path(), "feature");
    let merge_output = git(repo_dir.path(), &["merge", "feature"]);
    assert!(!merge_output.status.success());

    let provider = provider();
    let repo = provider.discover(repo_dir.path()).unwrap();

    let sides = provider
        .conflict_sides(&repo, Path::new("f.txt"))
        .unwrap();

    assert_eq!(sides.path, PathBuf::from("f.txt"));
    assert_eq!(
        sides.base,
        ConflictSideContent::Text("line1\nline2\nline3\n".to_string())
    );
    assert_eq!(
        sides.ours,
        ConflictSideContent::Text("line1\nCHANGED-main\nline3\n".to_string())
    );
    assert_eq!(
        sides.theirs,
        ConflictSideContent::Text("line1\nCHANGED-other\nline3\n".to_string())
    );

    git_ok(repo_dir.path(), &["merge", "--abort"]);
}

#[test]
fn conflict_sides_reports_absent_for_a_stage_a_both_added_conflict_has_no_base_for() {
    let repo_dir = init_repo("conflict-sides-both-added");
    write_file(repo_dir.path(), "shared.txt", "root\n");
    commit_all(repo_dir.path(), "root");

    git_ok(repo_dir.path(), &["checkout", "-q", "-b", "feature"]);
    write_file(repo_dir.path(), "new.txt", "feature version\n");
    commit_all(repo_dir.path(), "feature adds new.txt");

    git_ok(repo_dir.path(), &["checkout", "-q", "main"]);
    write_file(repo_dir.path(), "new.txt", "main version\n");
    commit_all(repo_dir.path(), "main adds new.txt");

    let merge_output = git(repo_dir.path(), &["merge", "feature"]);
    assert!(!merge_output.status.success());

    let provider = provider();
    let repo = provider.discover(repo_dir.path()).unwrap();

    let sides = provider
        .conflict_sides(&repo, Path::new("new.txt"))
        .unwrap();

    assert_eq!(
        sides.base,
        ConflictSideContent::Absent,
        "a file added independently on both sides has no common-ancestor stage"
    );
    assert_eq!(
        sides.ours,
        ConflictSideContent::Text("main version\n".to_string())
    );
    assert_eq!(
        sides.theirs,
        ConflictSideContent::Text("feature version\n".to_string())
    );

    git_ok(repo_dir.path(), &["merge", "--abort"]);
}

#[test]
fn mark_conflict_resolved_stages_the_working_trees_current_content_only_on_explicit_call() {
    let repo_dir = init_repo("mark-resolved-text");
    setup_diverging_branches(repo_dir.path(), "feature");
    let merge_output = git(repo_dir.path(), &["merge", "feature"]);
    assert!(!merge_output.status.success());

    let provider = provider();
    let repo = provider.discover(repo_dir.path()).unwrap();

    // Never resolved automatically just because it "looks" resolvable —
    // still conflicted until the explicit call below.
    let before = provider.detect_in_progress_operation(&repo).unwrap();
    assert!(before.has_conflicts());

    // Simulates external resolution: another editor (outside GitSail)
    // rewrites the file, and GitSail only observes it on the next explicit
    // action — never requiring the edit to happen inside GitSail (US-080
    // criterion 2).
    write_file(repo_dir.path(), "f.txt", "line1\nRESOLVED\nline3\n");

    RepositoryWritePort::mark_conflict_resolved(&provider, &repo, Path::new("f.txt")).unwrap();

    let status = provider.status(&repo).unwrap();
    let entry = status
        .files
        .iter()
        .find(|f| f.path == Path::new("f.txt"))
        .expect("f.txt must still be reported");
    assert_ne!(
        entry.index_status,
        gitsail_domain::FileStatusCode::Unmodified,
        "resolving must have staged the file"
    );

    // Still a pending merge (this call only touches the index for this one
    // path) — continuing is what actually concludes it (T-233).
    assert!(matches!(
        provider.detect_in_progress_operation(&repo).unwrap(),
        InProgressOperation::Merge(_)
    ));

    git_ok(repo_dir.path(), &["merge", "--abort"]);
}

#[test]
fn take_conflict_side_resolves_a_binary_conflict_by_choosing_one_side_wholesale() {
    let repo_dir = init_repo("take-conflict-side-binary");
    // Each variant embeds a NUL byte so `classify_file_content`'s binary
    // sniff (an embedded NUL, mirroring Git's own heuristic) reliably
    // classifies it as binary rather than as valid (if unusual) UTF-8 text.
    write_binary_file(repo_dir.path(), "img.bin", &[0, 1, 2, 3]);
    commit_all(repo_dir.path(), "base");

    git_ok(repo_dir.path(), &["checkout", "-q", "-b", "feature"]);
    write_binary_file(repo_dir.path(), "img.bin", &[0, 9, 9, 9]);
    commit_all(repo_dir.path(), "feature binary change");

    git_ok(repo_dir.path(), &["checkout", "-q", "main"]);
    write_binary_file(repo_dir.path(), "img.bin", &[0, 5, 5, 5]);
    commit_all(repo_dir.path(), "main binary change");

    let merge_output = git(repo_dir.path(), &["merge", "feature"]);
    assert!(!merge_output.status.success());

    let provider = provider();
    let repo = provider.discover(repo_dir.path()).unwrap();

    let sides = provider
        .conflict_sides(&repo, Path::new("img.bin"))
        .unwrap();
    assert_eq!(sides.ours, ConflictSideContent::Binary);
    assert_eq!(sides.theirs, ConflictSideContent::Binary);

    RepositoryWritePort::take_conflict_side(
        &provider,
        &repo,
        Path::new("img.bin"),
        ConflictSide::Theirs,
    )
    .unwrap();

    let on_disk = std::fs::read(repo_dir.path().join("img.bin")).unwrap();
    assert_eq!(on_disk, vec![0, 9, 9, 9], "working tree must hold theirs' content");

    let status = provider.status(&repo).unwrap();
    let entry = status
        .files
        .iter()
        .find(|f| f.path == Path::new("img.bin"))
        .expect("img.bin must still be reported");
    assert_ne!(entry.index_status, gitsail_domain::FileStatusCode::Unmodified);

    RepositoryWritePort::continue_operation(&provider, &repo).unwrap();
    assert!(provider
        .detect_in_progress_operation(&repo)
        .unwrap()
        .is_none());
    let final_content = std::fs::read(repo_dir.path().join("img.bin")).unwrap();
    assert_eq!(final_content, vec![0, 9, 9, 9]);
}

// ---------------------------------------------------------------------
// T-233/US-081: continue_operation / abort_operation.
// ---------------------------------------------------------------------

#[test]
fn continue_operation_refuses_while_conflicted_files_remain() {
    let repo_dir = init_repo("continue-refuses-unresolved");
    setup_diverging_branches(repo_dir.path(), "feature");
    let merge_output = git(repo_dir.path(), &["merge", "feature"]);
    assert!(!merge_output.status.success());

    let provider = provider();
    let repo = provider.discover(repo_dir.path()).unwrap();

    let err = RepositoryWritePort::continue_operation(&provider, &repo).unwrap_err();
    assert_eq!(err.code(), ErrorCode::OperationConflict);

    // Never presumed success: still exactly the same pending merge.
    assert!(matches!(
        provider.detect_in_progress_operation(&repo).unwrap(),
        InProgressOperation::Merge(_)
    ));

    git_ok(repo_dir.path(), &["merge", "--abort"]);
}

#[test]
fn continue_operation_completes_the_merge_commit_once_conflicts_are_resolved() {
    let repo_dir = init_repo("continue-completes-merge");
    setup_diverging_branches(repo_dir.path(), "feature");
    let merge_output = git(repo_dir.path(), &["merge", "feature"]);
    assert!(!merge_output.status.success());

    let provider = provider();
    let repo = provider.discover(repo_dir.path()).unwrap();

    write_file(repo_dir.path(), "f.txt", "line1\nRESOLVED\nline3\n");
    RepositoryWritePort::mark_conflict_resolved(&provider, &repo, Path::new("f.txt")).unwrap();

    RepositoryWritePort::continue_operation(&provider, &repo).unwrap();

    // The real result is reinspected rather than presumed (US-081 criterion
    // 3): the pending merge is genuinely gone, and a real merge commit
    // exists at HEAD.
    assert!(provider
        .detect_in_progress_operation(&repo)
        .unwrap()
        .is_none());
    let parents_output = git(repo_dir.path(), &["rev-parse", "HEAD^1", "HEAD^2"]);
    assert!(
        parents_output.status.success(),
        "HEAD must be a two-parent merge commit after continue"
    );
    assert_eq!(
        std::fs::read_to_string(repo_dir.path().join("f.txt")).unwrap(),
        "line1\nRESOLVED\nline3\n"
    );
}

#[test]
fn continue_operation_refuses_when_nothing_is_pending() {
    let repo_dir = init_repo("continue-refuses-nothing-pending");
    write_file(repo_dir.path(), "f.txt", "line1\n");
    commit_all(repo_dir.path(), "base");

    let provider = provider();
    let repo = provider.discover(repo_dir.path()).unwrap();

    let err = RepositoryWritePort::continue_operation(&provider, &repo).unwrap_err();
    assert_eq!(err.code(), ErrorCode::InvalidRepositoryState);
}

#[test]
fn continue_operation_refuses_for_an_operation_that_does_not_support_it() {
    let repo_dir = init_repo("continue-refuses-bisect");
    write_file(repo_dir.path(), "f.txt", "v1\n");
    commit_all(repo_dir.path(), "c1");
    let first = rev_parse(repo_dir.path(), "HEAD");
    write_file(repo_dir.path(), "f.txt", "v2\n");
    commit_all(repo_dir.path(), "c2");

    git_ok(repo_dir.path(), &["bisect", "start"]);
    git_ok(repo_dir.path(), &["bisect", "bad", "HEAD"]);
    git_ok(repo_dir.path(), &["bisect", "good", first.as_str()]);

    let provider = provider();
    let repo = provider.discover(repo_dir.path()).unwrap();

    let err = RepositoryWritePort::continue_operation(&provider, &repo).unwrap_err();
    assert_eq!(err.code(), ErrorCode::InvalidRepositoryState);

    git_ok(repo_dir.path(), &["bisect", "reset"]);
}

#[test]
fn abort_operation_restores_the_pre_merge_head_and_leaves_unrelated_work_intact() {
    let repo_dir = init_repo("abort-restores-head");
    setup_diverging_branches(repo_dir.path(), "feature");
    let head_before_merge = rev_parse(repo_dir.path(), "HEAD");

    // Unrelated, unstaged local work that must survive the abort untouched.
    write_file(repo_dir.path(), "unrelated.txt", "unrelated work\n");

    let merge_output = git(repo_dir.path(), &["merge", "feature"]);
    assert!(!merge_output.status.success());

    let provider = provider();
    let repo = provider.discover(repo_dir.path()).unwrap();

    RepositoryWritePort::abort_operation(&provider, &repo).unwrap();

    // Reinspected, not presumed (US-081 criterion 3).
    assert!(provider
        .detect_in_progress_operation(&repo)
        .unwrap()
        .is_none());
    assert_eq!(rev_parse(repo_dir.path(), "HEAD"), head_before_merge);
    assert_eq!(
        std::fs::read_to_string(repo_dir.path().join("unrelated.txt")).unwrap(),
        "unrelated work\n",
        "abort must never discard unrelated local work"
    );
}

#[test]
fn abort_operation_refuses_when_nothing_is_pending() {
    let repo_dir = init_repo("abort-refuses-nothing-pending");
    write_file(repo_dir.path(), "f.txt", "line1\n");
    commit_all(repo_dir.path(), "base");

    let provider = provider();
    let repo = provider.discover(repo_dir.path()).unwrap();

    let err = RepositoryWritePort::abort_operation(&provider, &repo).unwrap_err();
    assert_eq!(err.code(), ErrorCode::InvalidRepositoryState);
}
