//! Integration tests for EPIC-18 (Stash, Tags & Worktrees; Takumi E-43):
//! T-216/US-091 (tag/remote/stash inspection), T-217/US-092 (stash
//! creation), T-218/US-093 (stash apply/pop/drop), T-219/US-094 (local tag
//! create/delete) and T-220/US-095 (worktree list/create/remove).
//!
//! Mirrors `tests/provider.rs`'s own convention: every test creates a real,
//! temporary Git repository via the `git` CLI (never a mock) and exercises
//! [`GitCliProvider`] through `RepositoryReadPort`/`RepositoryWritePort`.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use gitsail_application::{
    RepositoryReadPort, RepositoryWritePort, StashScope, TagAnnotation, WorktreeBranchSpec,
};
use gitsail_domain::{BranchName, CommitHash, ErrorCode, TagKind, WorktreeHead};
use gitsail_git::{GitCliProvider, GitProcessRunner, GitProcessRunnerConfig};

// ---------------------------------------------------------------------
// Fixture plumbing (mirrors `tests/provider.rs`'s own helpers).
// ---------------------------------------------------------------------

struct TempDir(PathBuf);

impl TempDir {
    fn new(label: &str) -> Self {
        static COUNTER: AtomicU32 = AtomicU32::new(0);
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let n = COUNTER.fetch_add(1, Ordering::SeqCst);
        let path = std::env::temp_dir().join(format!("gitsail-git-epic18-{label}-{nanos}-{n}"));
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

fn git(dir: &Path, args: &[&str]) {
    let status = Command::new("git")
        .args(args)
        .current_dir(dir)
        .env("LC_ALL", "C")
        .env("LANG", "C")
        .status()
        .unwrap_or_else(|e| panic!("failed to spawn git {args:?}: {e}"));
    assert!(status.success(), "git {args:?} failed in {dir:?}");
}

fn git_output(dir: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .args(args)
        .current_dir(dir)
        .env("LC_ALL", "C")
        .env("LANG", "C")
        .output()
        .unwrap_or_else(|e| panic!("failed to spawn git {args:?}: {e}"));
    assert!(output.status.success(), "git {args:?} failed in {dir:?}");
    String::from_utf8(output.stdout).unwrap().trim().to_string()
}

fn init_repo(label: &str) -> TempDir {
    let dir = TempDir::new(label);
    git(dir.path(), &["init", "--quiet", "--initial-branch=main"]);
    git(dir.path(), &["config", "user.name", "Test User"]);
    git(dir.path(), &["config", "user.email", "test@example.com"]);
    dir
}

fn write_file(dir: &Path, name: &str, contents: &str) {
    std::fs::write(dir.join(name), contents).unwrap();
}

fn commit_all(dir: &Path, message: &str) {
    git(dir, &["add", "-A"]);
    git(dir, &["commit", "--quiet", "-m", message]);
}

fn provider() -> GitCliProvider {
    let runner = GitProcessRunner::new(GitProcessRunnerConfig::default())
        .expect("git must be installed to run these integration tests");
    GitCliProvider::new(runner)
}

// =======================================================================
// T-216/US-091: list_tags / list_remotes / list_stash_entries /
// list_worktrees.
// =======================================================================

#[test]
fn list_tags_is_empty_on_a_repository_with_no_tags() {
    let repo_dir = init_repo("tags-empty");
    write_file(repo_dir.path(), "a.txt", "one\n");
    commit_all(repo_dir.path(), "first commit");
    let provider = provider();
    let repo = provider.discover(repo_dir.path()).unwrap();

    let tags = provider
        .list_tags(&repo)
        .expect("an empty tag list is a valid state, not an error");

    assert!(tags.is_empty());
}

#[test]
fn list_tags_reports_lightweight_and_annotated_tags_with_their_metadata() {
    let repo_dir = init_repo("tags-both-kinds");
    write_file(repo_dir.path(), "a.txt", "one\n");
    commit_all(repo_dir.path(), "first commit");
    let head = current_head(repo_dir.path());

    git(repo_dir.path(), &["tag", "--", "light-v1"]);
    git(
        repo_dir.path(),
        &["tag", "-a", "-m", "release message", "--", "annotated-v1"],
    );

    let provider = provider();
    let repo = provider.discover(repo_dir.path()).unwrap();
    let mut tags = provider.list_tags(&repo).unwrap();
    tags.sort_by(|a, b| a.name.cmp(&b.name));

    assert_eq!(tags.len(), 2);
    let annotated = tags.iter().find(|t| t.name == "annotated-v1").unwrap();
    assert_eq!(annotated.target.as_str(), head);
    match &annotated.kind {
        TagKind::Annotated {
            message, tagger, ..
        } => {
            assert_eq!(message, "release message");
            assert_eq!(tagger.email, "test@example.com");
        }
        TagKind::Lightweight => panic!("expected an annotated tag"),
    }

    let light = tags.iter().find(|t| t.name == "light-v1").unwrap();
    assert_eq!(light.target.as_str(), head);
    assert_eq!(light.kind, TagKind::Lightweight);
}

#[test]
fn list_remotes_is_empty_with_no_remotes_configured() {
    let repo_dir = init_repo("remotes-empty");
    let provider = provider();
    let repo = provider.discover(repo_dir.path()).unwrap();

    let remotes = provider.list_remotes(&repo).unwrap();

    assert!(remotes.is_empty());
}

#[test]
fn list_remotes_distinguishes_fetch_and_push_urls_and_redacts_credentials() {
    let repo_dir = init_repo("remotes-fetch-push");
    // A synthetic (non-real, never-dialed) credential in the URL, purely to
    // exercise redaction — never a real secret.
    git(
        repo_dir.path(),
        &[
            "remote",
            "add",
            "origin",
            "https://synthetic-user:synthetic-pass@example.invalid/org/repo.git",
        ],
    );
    git(
        repo_dir.path(),
        &[
            "remote",
            "set-url",
            "--push",
            "origin",
            "https://push.example.invalid/org/repo.git",
        ],
    );
    git(
        repo_dir.path(),
        &[
            "remote",
            "add",
            "single-url",
            "git@example.invalid:org/repo2.git",
        ],
    );

    let provider = provider();
    let repo = provider.discover(repo_dir.path()).unwrap();
    let mut remotes = provider.list_remotes(&repo).unwrap();
    remotes.sort_by(|a, b| a.name.cmp(&b.name));

    assert_eq!(remotes.len(), 2);
    let origin = remotes.iter().find(|r| r.name == "origin").unwrap();
    assert!(origin.fetch_url.as_str().contains("synthetic-user"));
    assert_ne!(origin.fetch_url.as_str(), origin.push_url.as_str());
    assert_eq!(
        origin.push_url.as_str(),
        "https://push.example.invalid/org/repo.git"
    );
    // The raw credential is retrievable via `as_str` (needed to actually
    // invoke Git), but never through `Display`/`Debug` (US-091 criterion
    // 3): this is `RemoteUrl`'s own guarantee, exercised here at the
    // adapter boundary rather than only in `gitsail-domain`'s unit tests.
    assert!(!origin.fetch_url.to_string().contains("synthetic-pass"));
    assert!(!format!("{:?}", origin.fetch_url).contains("synthetic-pass"));

    let single = remotes.iter().find(|r| r.name == "single-url").unwrap();
    assert_eq!(single.fetch_url.as_str(), single.push_url.as_str());
}

#[test]
fn list_stash_entries_is_empty_with_no_stash() {
    let repo_dir = init_repo("stash-empty");
    write_file(repo_dir.path(), "a.txt", "one\n");
    commit_all(repo_dir.path(), "first commit");
    let provider = provider();
    let repo = provider.discover(repo_dir.path()).unwrap();

    let entries = provider.list_stash_entries(&repo).unwrap();

    assert!(entries.is_empty());
}

#[test]
fn list_stash_entries_reports_index_commit_message_and_date_newest_first() {
    let repo_dir = init_repo("stash-list-order");
    write_file(repo_dir.path(), "a.txt", "one\n");
    commit_all(repo_dir.path(), "first commit");

    write_file(repo_dir.path(), "a.txt", "two\n");
    git(
        repo_dir.path(),
        &["stash", "push", "-q", "-m", "first stash"],
    );
    write_file(repo_dir.path(), "a.txt", "three\n");
    git(
        repo_dir.path(),
        &["stash", "push", "-q", "-m", "second stash"],
    );

    let provider = provider();
    let repo = provider.discover(repo_dir.path()).unwrap();
    let entries = provider.list_stash_entries(&repo).unwrap();

    assert_eq!(entries.len(), 2);
    assert_eq!(entries[0].index, 0);
    assert!(entries[0].message.contains("second stash"));
    assert_eq!(entries[1].index, 1);
    assert!(entries[1].message.contains("first stash"));
    assert!(entries[0].date.seconds_since_epoch > 0);
    for entry in &entries {
        assert!(!entry.commit.as_str().is_empty());
    }
}

#[test]
fn list_worktrees_reports_main_and_linked_worktrees_with_state() {
    let repo_dir = init_repo("worktrees-list");
    write_file(repo_dir.path(), "a.txt", "one\n");
    commit_all(repo_dir.path(), "first commit");
    git(repo_dir.path(), &["branch", "wt-branch"]);

    let linked_dir = TempDir::new("worktrees-list-linked");
    std::fs::remove_dir(linked_dir.path()).unwrap(); // `worktree add` must create it.
    git(
        repo_dir.path(),
        &[
            "worktree",
            "add",
            "-q",
            linked_dir.path().to_str().unwrap(),
            "wt-branch",
        ],
    );
    git(
        repo_dir.path(),
        &["worktree", "lock", linked_dir.path().to_str().unwrap()],
    );

    let provider = provider();
    let repo = provider.discover(repo_dir.path()).unwrap();
    let worktrees = provider.list_worktrees(&repo).unwrap();

    assert_eq!(worktrees.len(), 2);
    let main = worktrees
        .iter()
        .find(|w| w.is_main)
        .expect("a main worktree");
    assert_eq!(main.path, repo_dir.path().canonicalize().unwrap());
    assert!(matches!(&main.head, WorktreeHead::Attached { branch } if branch.as_str() == "main"));

    let linked = worktrees
        .iter()
        .find(|w| !w.is_main)
        .expect("a linked worktree");
    assert_eq!(linked.path, linked_dir.path().canonicalize().unwrap());
    assert!(
        matches!(&linked.head, WorktreeHead::Attached { branch } if branch.as_str() == "wt-branch")
    );
    assert!(linked.is_locked);
    assert!(!linked.is_prunable);
}

// =======================================================================
// T-217/US-092: create_stash.
// =======================================================================

#[test]
fn create_stash_default_scope_captures_tracked_changes_and_leaves_the_working_tree_clean() {
    let repo_dir = init_repo("stash-create-default");
    write_file(repo_dir.path(), "tracked.txt", "committed\n");
    write_file(repo_dir.path(), "untracked.txt", "not tracked\n");
    commit_all(repo_dir.path(), "first commit");
    // `untracked.txt` was written after the commit above, so it stays
    // untracked; re-create it after commit_all to keep it out of the
    // commit while still present on disk before the stash runs.
    write_file(repo_dir.path(), "untracked.txt", "not tracked\n");
    write_file(repo_dir.path(), "tracked.txt", "modified\n");

    let provider = provider();
    let repo = provider.discover(repo_dir.path()).unwrap();

    let stash = provider
        .create_stash(&repo, Some("wip"), StashScope::default())
        .expect("there is a tracked change to stash");

    assert_eq!(stash.index, 0);
    assert!(stash.message.contains("wip"));
    assert_eq!(
        std::fs::read_to_string(repo_dir.path().join("tracked.txt")).unwrap(),
        "committed\n",
        "the tracked file must be restored to HEAD's content"
    );
    assert!(
        repo_dir.path().join("untracked.txt").exists(),
        "an untracked file must be preserved when include_untracked/all are not set"
    );
}

#[test]
fn create_stash_with_keep_index_preserves_staged_changes() {
    let repo_dir = init_repo("stash-create-keep-index");
    write_file(repo_dir.path(), "a.txt", "one\n");
    commit_all(repo_dir.path(), "first commit");
    write_file(repo_dir.path(), "a.txt", "two\n");
    git(repo_dir.path(), &["add", "a.txt"]);

    let provider = provider();
    let repo = provider.discover(repo_dir.path()).unwrap();
    provider
        .create_stash(
            &repo,
            None,
            StashScope {
                keep_index: true,
                ..StashScope::default()
            },
        )
        .unwrap();

    let status = git_output(repo_dir.path(), &["status", "--porcelain"]);
    assert_eq!(status, "M  a.txt", "staged content must remain staged");
}

#[test]
fn create_stash_with_include_untracked_captures_untracked_files_but_not_ignored_ones() {
    let repo_dir = init_repo("stash-create-untracked");
    write_file(repo_dir.path(), "a.txt", "one\n");
    write_file(repo_dir.path(), ".gitignore", "ignored.txt\n");
    commit_all(repo_dir.path(), "first commit");
    write_file(repo_dir.path(), "untracked.txt", "new\n");
    write_file(repo_dir.path(), "ignored.txt", "ignored content\n");

    let provider = provider();
    let repo = provider.discover(repo_dir.path()).unwrap();
    provider
        .create_stash(
            &repo,
            None,
            StashScope {
                include_untracked: true,
                ..StashScope::default()
            },
        )
        .unwrap();

    assert!(
        !repo_dir.path().join("untracked.txt").exists(),
        "include_untracked must capture untracked files"
    );
    assert!(
        repo_dir.path().join("ignored.txt").exists(),
        "an ignored file must never be captured without the explicit `all` flag"
    );
}

#[test]
fn create_stash_with_all_also_captures_ignored_files() {
    let repo_dir = init_repo("stash-create-all");
    write_file(repo_dir.path(), "a.txt", "one\n");
    write_file(repo_dir.path(), ".gitignore", "ignored.txt\n");
    commit_all(repo_dir.path(), "first commit");
    write_file(repo_dir.path(), "ignored.txt", "ignored content\n");

    let provider = provider();
    let repo = provider.discover(repo_dir.path()).unwrap();
    provider
        .create_stash(
            &repo,
            None,
            StashScope {
                all: true,
                ..StashScope::default()
            },
        )
        .unwrap();

    assert!(
        !repo_dir.path().join("ignored.txt").exists(),
        "the explicit `all` flag must capture ignored files too"
    );
}

#[test]
fn create_stash_fails_when_there_is_nothing_to_stash() {
    let repo_dir = init_repo("stash-create-nothing");
    write_file(repo_dir.path(), "a.txt", "one\n");
    commit_all(repo_dir.path(), "first commit");

    let provider = provider();
    let repo = provider.discover(repo_dir.path()).unwrap();

    let err = provider
        .create_stash(&repo, None, StashScope::default())
        .unwrap_err();

    assert_eq!(err.code(), ErrorCode::InvalidRepositoryState);
}

// =======================================================================
// T-218/US-093: apply_stash / pop_stash / drop_stash.
// =======================================================================

fn stash_after_change(repo_dir: &Path, message: &str) -> gitsail_domain::Stash {
    let provider = provider();
    let repo = provider.discover(repo_dir).unwrap();
    provider
        .create_stash(&repo, Some(message), StashScope::default())
        .unwrap()
}

#[test]
fn apply_stash_restores_changes_without_removing_the_entry() {
    let repo_dir = init_repo("stash-apply-keeps-entry");
    write_file(repo_dir.path(), "a.txt", "one\n");
    commit_all(repo_dir.path(), "first commit");
    write_file(repo_dir.path(), "a.txt", "two\n");
    let stash = stash_after_change(repo_dir.path(), "wip");

    let provider = provider();
    let repo = provider.discover(repo_dir.path()).unwrap();
    let outcome = provider.apply_stash(&repo, &stash).unwrap();

    assert!(!outcome.had_conflicts);
    assert_eq!(
        std::fs::read_to_string(repo_dir.path().join("a.txt")).unwrap(),
        "two\n"
    );
    assert_eq!(provider.list_stash_entries(&repo).unwrap().len(), 1);
}

#[test]
fn pop_stash_restores_changes_and_removes_the_entry() {
    let repo_dir = init_repo("stash-pop-removes-entry");
    write_file(repo_dir.path(), "a.txt", "one\n");
    commit_all(repo_dir.path(), "first commit");
    write_file(repo_dir.path(), "a.txt", "two\n");
    let stash = stash_after_change(repo_dir.path(), "wip");

    let provider = provider();
    let repo = provider.discover(repo_dir.path()).unwrap();
    let outcome = provider.pop_stash(&repo, &stash).unwrap();

    assert!(!outcome.had_conflicts);
    assert_eq!(
        std::fs::read_to_string(repo_dir.path().join("a.txt")).unwrap(),
        "two\n"
    );
    assert!(provider.list_stash_entries(&repo).unwrap().is_empty());
}

/// Builds a repository where restoring `stash@{0}` (created by
/// `stash_after_change`) is guaranteed to conflict: the same line changed
/// two different ways on the stash side and on `HEAD`.
fn build_conflicting_stash(repo_dir: &Path) -> gitsail_domain::Stash {
    write_file(repo_dir, "f.txt", "line1\n");
    let provider = provider();
    let repo = provider.discover(repo_dir).unwrap();
    // (commit_all/git helpers operate on the raw path; provider is unused
    // here beyond discovery, kept for symmetry with other fixtures.)
    let _ = repo;
    commit_all(repo_dir, "base");
    write_file(repo_dir, "f.txt", "stash-side-change\n");
    let stash = stash_after_change(repo_dir, "conflicting stash");
    write_file(repo_dir, "f.txt", "head-side-change\n");
    commit_all(repo_dir, "conflicting head commit");
    stash
}

#[test]
fn apply_stash_reports_conflicts_and_preserves_the_entry() {
    let repo_dir = init_repo("stash-apply-conflict");
    let stash = build_conflicting_stash(repo_dir.path());

    let provider = provider();
    let repo = provider.discover(repo_dir.path()).unwrap();
    let outcome = provider
        .apply_stash(&repo, &stash)
        .expect("a conflict is a legitimate outcome, not an Err");

    assert!(
        outcome.had_conflicts,
        "a conflicted apply must never be reported as a plain success"
    );
    assert_eq!(
        provider.list_stash_entries(&repo).unwrap().len(),
        1,
        "the stash must be preserved when applying it conflicts"
    );
}

#[test]
fn pop_stash_reports_conflicts_and_never_drops_the_entry_on_conflict() {
    let repo_dir = init_repo("stash-pop-conflict");
    let stash = build_conflicting_stash(repo_dir.path());

    let provider = provider();
    let repo = provider.discover(repo_dir.path()).unwrap();
    let outcome = provider
        .pop_stash(&repo, &stash)
        .expect("a conflict is a legitimate outcome, not an Err");

    assert!(
        outcome.had_conflicts,
        "a conflicted pop must never be presented as a completed restoration"
    );
    assert_eq!(
        provider.list_stash_entries(&repo).unwrap().len(),
        1,
        "US-093 criterion 2: pop must never drop the stash when applying it conflicted"
    );
}

#[test]
fn drop_stash_deletes_the_entry_without_applying_it() {
    let repo_dir = init_repo("stash-drop");
    write_file(repo_dir.path(), "a.txt", "one\n");
    commit_all(repo_dir.path(), "first commit");
    write_file(repo_dir.path(), "a.txt", "two\n");
    let stash = stash_after_change(repo_dir.path(), "to be dropped");

    let provider = provider();
    let repo = provider.discover(repo_dir.path()).unwrap();
    provider.drop_stash(&repo, &stash).unwrap();

    assert!(provider.list_stash_entries(&repo).unwrap().is_empty());
    assert_eq!(
        std::fs::read_to_string(repo_dir.path().join("a.txt")).unwrap(),
        "one\n",
        "drop must never apply the stash's content"
    );
}

#[test]
fn apply_stash_refuses_when_the_stash_index_changed_externally() {
    let repo_dir = init_repo("stash-apply-stale-index");
    write_file(repo_dir.path(), "a.txt", "one\n");
    commit_all(repo_dir.path(), "first commit");
    write_file(repo_dir.path(), "a.txt", "two\n");
    // Previewed as stash@{0} at the moment this is captured.
    let previewed = stash_after_change(repo_dir.path(), "first");

    // Something external (another terminal/editor, or a concurrent GitSail
    // session) pushes a second stash, shifting `previewed` to stash@{1}.
    write_file(repo_dir.path(), "a.txt", "three\n");
    git(repo_dir.path(), &["stash", "push", "-q", "-m", "second"]);

    let provider = provider();
    let repo = provider.discover(repo_dir.path()).unwrap();

    let err = provider
        .apply_stash(&repo, &previewed)
        .expect_err("a stale stash identity must never authorize acting on the wrong entry");

    assert_eq!(err.code(), ErrorCode::OperationConflict);
    // Nothing was applied or removed.
    assert_eq!(provider.list_stash_entries(&repo).unwrap().len(), 2);
}

#[test]
fn drop_stash_refuses_when_the_entry_no_longer_exists() {
    let repo_dir = init_repo("stash-drop-stale-index");
    write_file(repo_dir.path(), "a.txt", "one\n");
    commit_all(repo_dir.path(), "first commit");
    write_file(repo_dir.path(), "a.txt", "two\n");
    let previewed = stash_after_change(repo_dir.path(), "only stash");

    let provider = provider();
    let repo = provider.discover(repo_dir.path()).unwrap();
    // Drop it once already (e.g. from another session).
    provider.drop_stash(&repo, &previewed).unwrap();

    let err = provider
        .drop_stash(&repo, &previewed)
        .expect_err("dropping an already-gone stash must be refused, not silently no-op");

    assert_eq!(err.code(), ErrorCode::OperationConflict);
}

// =======================================================================
// T-219/US-094: create_tag / delete_tag.
// =======================================================================

fn current_head(dir: &Path) -> String {
    git_output(dir, &["rev-parse", "HEAD"])
}

#[test]
fn create_tag_creates_a_lightweight_tag_at_head() {
    let repo_dir = init_repo("tag-create-lightweight");
    write_file(repo_dir.path(), "a.txt", "one\n");
    commit_all(repo_dir.path(), "first commit");
    let head = current_head(repo_dir.path());

    let provider = provider();
    let repo = provider.discover(repo_dir.path()).unwrap();
    provider
        .create_tag(&repo, "v1.0", None, TagAnnotation::Lightweight)
        .unwrap();

    let tags = provider.list_tags(&repo).unwrap();
    assert_eq!(tags.len(), 1);
    assert_eq!(tags[0].name, "v1.0");
    assert_eq!(tags[0].target.as_str(), head);
    assert_eq!(tags[0].kind, TagKind::Lightweight);
}

#[test]
fn create_tag_creates_an_annotated_tag_with_message_at_an_explicit_target() {
    let repo_dir = init_repo("tag-create-annotated");
    write_file(repo_dir.path(), "a.txt", "one\n");
    commit_all(repo_dir.path(), "first commit");
    let first_head = current_head(repo_dir.path());
    write_file(repo_dir.path(), "a.txt", "two\n");
    commit_all(repo_dir.path(), "second commit");

    let provider = provider();
    let repo = provider.discover(repo_dir.path()).unwrap();
    let target = CommitHash::new(first_head.clone()).unwrap();
    provider
        .create_tag(
            &repo,
            "v1.0",
            Some(&target),
            TagAnnotation::Annotated {
                message: "first release".to_string(),
            },
        )
        .unwrap();

    let tags = provider.list_tags(&repo).unwrap();
    assert_eq!(tags.len(), 1);
    assert_eq!(tags[0].target.as_str(), first_head);
    match &tags[0].kind {
        TagKind::Annotated { message, .. } => assert_eq!(message, "first release"),
        TagKind::Lightweight => panic!("expected an annotated tag"),
    }
}

#[test]
fn create_tag_refuses_to_overwrite_an_existing_tag_name() {
    let repo_dir = init_repo("tag-create-collision");
    write_file(repo_dir.path(), "a.txt", "one\n");
    commit_all(repo_dir.path(), "first commit");

    let provider = provider();
    let repo = provider.discover(repo_dir.path()).unwrap();
    provider
        .create_tag(&repo, "v1.0", None, TagAnnotation::Lightweight)
        .unwrap();

    let err = provider
        .create_tag(&repo, "v1.0", None, TagAnnotation::Lightweight)
        .unwrap_err();

    assert_eq!(err.code(), ErrorCode::InvalidRepositoryState);
    assert_eq!(provider.list_tags(&repo).unwrap().len(), 1);
}

#[test]
fn delete_tag_removes_the_local_tag() {
    let repo_dir = init_repo("tag-delete");
    write_file(repo_dir.path(), "a.txt", "one\n");
    commit_all(repo_dir.path(), "first commit");
    git(repo_dir.path(), &["tag", "--", "v1.0"]);

    let provider = provider();
    let repo = provider.discover(repo_dir.path()).unwrap();
    provider.delete_tag(&repo, "v1.0").unwrap();

    assert!(provider.list_tags(&repo).unwrap().is_empty());
}

#[test]
fn delete_tag_fails_for_a_nonexistent_tag() {
    let repo_dir = init_repo("tag-delete-missing");
    write_file(repo_dir.path(), "a.txt", "one\n");
    commit_all(repo_dir.path(), "first commit");

    let provider = provider();
    let repo = provider.discover(repo_dir.path()).unwrap();

    let err = provider.delete_tag(&repo, "does-not-exist").unwrap_err();

    assert_eq!(err.code(), ErrorCode::RepositoryNotFound);
}

#[test]
fn tag_mutations_never_contact_or_change_a_remote() {
    let remote_dir = TempDir::new("tag-no-remote-bare");
    git(
        remote_dir.path(),
        &["init", "--quiet", "--bare", "--initial-branch=main"],
    );

    let repo_dir = init_repo("tag-no-remote-clone");
    write_file(repo_dir.path(), "a.txt", "one\n");
    commit_all(repo_dir.path(), "first commit");
    git(
        repo_dir.path(),
        &[
            "remote",
            "add",
            "origin",
            remote_dir.path().to_str().unwrap(),
        ],
    );
    git(repo_dir.path(), &["push", "-q", "origin", "main"]);

    let provider = provider();
    let repo = provider.discover(repo_dir.path()).unwrap();
    provider
        .create_tag(&repo, "v1.0", None, TagAnnotation::Lightweight)
        .unwrap();
    provider.delete_tag(&repo, "v1.0").unwrap();

    let remote_tags = git_output(remote_dir.path(), &["tag", "-l"]);
    assert!(
        remote_tags.is_empty(),
        "local tag create/delete must never reach the remote"
    );
}

// =======================================================================
// T-220/US-095: list_worktrees / create_worktree / remove_worktree.
// =======================================================================

#[test]
fn create_worktree_with_a_new_branch() {
    let repo_dir = init_repo("worktree-create-new-branch");
    write_file(repo_dir.path(), "a.txt", "one\n");
    commit_all(repo_dir.path(), "first commit");

    let target_dir = TempDir::new("worktree-create-new-branch-target");
    std::fs::remove_dir(target_dir.path()).unwrap();

    let provider = provider();
    let repo = provider.discover(repo_dir.path()).unwrap();
    let branch = BranchName::new("feature").unwrap();

    let worktree = provider
        .create_worktree(
            &repo,
            target_dir.path(),
            WorktreeBranchSpec::NewBranch {
                name: branch.clone(),
                start_point: None,
            },
        )
        .unwrap();

    assert!(matches!(&worktree.head, WorktreeHead::Attached { branch: b } if b == &branch));
    assert!(!worktree.is_main);

    let listed = provider.list_worktrees(&repo).unwrap();
    assert_eq!(listed.len(), 2);
}

#[test]
fn create_worktree_with_an_existing_branch() {
    let repo_dir = init_repo("worktree-create-existing-branch");
    write_file(repo_dir.path(), "a.txt", "one\n");
    commit_all(repo_dir.path(), "first commit");
    git(repo_dir.path(), &["branch", "existing"]);

    let target_dir = TempDir::new("worktree-create-existing-branch-target");
    std::fs::remove_dir(target_dir.path()).unwrap();

    let provider = provider();
    let repo = provider.discover(repo_dir.path()).unwrap();
    let worktree = provider
        .create_worktree(
            &repo,
            target_dir.path(),
            WorktreeBranchSpec::ExistingBranch(BranchName::new("existing").unwrap()),
        )
        .unwrap();

    assert!(
        matches!(&worktree.head, WorktreeHead::Attached { branch } if branch.as_str() == "existing")
    );
}

#[test]
fn create_worktree_refuses_a_branch_already_checked_out_elsewhere() {
    let repo_dir = init_repo("worktree-branch-in-use");
    write_file(repo_dir.path(), "a.txt", "one\n");
    commit_all(repo_dir.path(), "first commit");

    let provider = provider();
    let repo = provider.discover(repo_dir.path()).unwrap();

    // `main` is already checked out in the main worktree itself.
    let target_dir = TempDir::new("worktree-branch-in-use-target");
    std::fs::remove_dir(target_dir.path()).unwrap();

    let err = provider
        .create_worktree(
            &repo,
            target_dir.path(),
            WorktreeBranchSpec::ExistingBranch(BranchName::new("main").unwrap()),
        )
        .unwrap_err();

    assert_eq!(err.code(), ErrorCode::InvalidRepositoryState);
}

#[test]
fn create_worktree_refuses_an_already_existing_nonempty_path() {
    let repo_dir = init_repo("worktree-existing-path");
    write_file(repo_dir.path(), "a.txt", "one\n");
    commit_all(repo_dir.path(), "first commit");

    let target_dir = TempDir::new("worktree-existing-path-target");
    write_file(target_dir.path(), "occupied.txt", "already here\n");

    let provider = provider();
    let repo = provider.discover(repo_dir.path()).unwrap();

    let err = provider
        .create_worktree(
            &repo,
            target_dir.path(),
            WorktreeBranchSpec::NewBranch {
                name: BranchName::new("feature").unwrap(),
                start_point: None,
            },
        )
        .unwrap_err();

    assert_eq!(err.code(), ErrorCode::InvalidRepositoryState);
}

#[test]
fn remove_worktree_refuses_a_dirty_worktree_without_force() {
    let repo_dir = init_repo("worktree-remove-dirty-refused");
    write_file(repo_dir.path(), "a.txt", "one\n");
    commit_all(repo_dir.path(), "first commit");

    let target_dir = TempDir::new("worktree-remove-dirty-refused-target");
    std::fs::remove_dir(target_dir.path()).unwrap();
    git(
        repo_dir.path(),
        &[
            "worktree",
            "add",
            "-q",
            target_dir.path().to_str().unwrap(),
            "-b",
            "dirty-branch",
        ],
    );
    write_file(target_dir.path(), "dirty.txt", "uncommitted\n");

    let provider = provider();
    let repo = provider.discover(repo_dir.path()).unwrap();

    let err = provider
        .remove_worktree(&repo, target_dir.path(), false)
        .unwrap_err();

    assert_eq!(err.code(), ErrorCode::OperationConflict);
    assert_eq!(provider.list_worktrees(&repo).unwrap().len(), 2);
}

#[test]
fn remove_worktree_with_force_removes_a_dirty_worktree() {
    let repo_dir = init_repo("worktree-remove-dirty-forced");
    write_file(repo_dir.path(), "a.txt", "one\n");
    commit_all(repo_dir.path(), "first commit");

    let target_dir = TempDir::new("worktree-remove-dirty-forced-target");
    std::fs::remove_dir(target_dir.path()).unwrap();
    git(
        repo_dir.path(),
        &[
            "worktree",
            "add",
            "-q",
            target_dir.path().to_str().unwrap(),
            "-b",
            "dirty-branch",
        ],
    );
    write_file(target_dir.path(), "dirty.txt", "uncommitted\n");

    let provider = provider();
    let repo = provider.discover(repo_dir.path()).unwrap();

    provider
        .remove_worktree(&repo, target_dir.path(), true)
        .unwrap();

    assert_eq!(provider.list_worktrees(&repo).unwrap().len(), 1);
}

#[test]
fn remove_worktree_reflects_in_the_listing_for_a_clean_worktree() {
    let repo_dir = init_repo("worktree-remove-clean");
    write_file(repo_dir.path(), "a.txt", "one\n");
    commit_all(repo_dir.path(), "first commit");

    let target_dir = TempDir::new("worktree-remove-clean-target");
    std::fs::remove_dir(target_dir.path()).unwrap();
    git(
        repo_dir.path(),
        &[
            "worktree",
            "add",
            "-q",
            target_dir.path().to_str().unwrap(),
            "-b",
            "clean-branch",
        ],
    );

    let provider = provider();
    let repo = provider.discover(repo_dir.path()).unwrap();
    assert_eq!(provider.list_worktrees(&repo).unwrap().len(), 2);

    provider
        .remove_worktree(&repo, target_dir.path(), false)
        .unwrap();

    assert_eq!(provider.list_worktrees(&repo).unwrap().len(), 1);
}
