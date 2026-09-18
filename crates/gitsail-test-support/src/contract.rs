//! A reusable contract test suite for `RepositoryReadPort` (T-253/US-120
//! criterion 2).
//!
//! Every function here is generic over `P: RepositoryReadPort`, built from a
//! caller-supplied `make_provider: impl Fn() -> P` factory — never hard-coded
//! against `gitsail_git::GitCliProvider`. Today `GitCliProvider` is this
//! trait's only implementation, so `gitsail-git/tests/contract.rs` is the
//! only caller, but a future second implementation (e.g. a `libgit2`-backed
//! adapter) plugs into these exact same functions unchanged: it only needs to
//! supply its own `make_provider` factory to each one.
//!
//! # Why a set of functions, not a single "run everything" entry point
//!
//! Each function is called from its own `#[test]` in the consuming crate
//! (see `gitsail-git/tests/contract.rs`), so a single failing contract check
//! is reported (and can be re-run) independently, with `cargo test`'s normal
//! per-test output — rather than one monolithic test whose first failure
//! hides every other result. [`RepositoryWritePort`]'s side of this contract
//! (mutations) is a documented, non-blocking follow-up: T-253/US-120's own
//! DoD scopes the required contract coverage to v0.1's read queries
//! (discovery, status, diff, blame, commit history — every function below
//! maps to exactly one of those), which are all already implemented; adding
//! an equivalent `RepositoryWritePort` suite is free to reuse this same
//! generic-function pattern when that becomes a requirement.
//!
//! # Locale independence (US-120 criterion 3)
//!
//! `GitCliProvider` — the only production implementation this suite runs
//! against today — pins `LC_ALL=C`/`LANG=C` on *every* invocation
//! unconditionally (`GitCliProvider::run`/`run_cancellable`, both routing
//! through `Self::locale_env()`; confirmed by reading
//! `gitsail-git/src/provider.rs`), so this suite's own assertions against
//! parsed domain values are never coupled to the host's locale. The lower
//! level `GitProcessRunner`/`run_process` primitive deliberately does *not*
//! hard-code a locale itself — it is also used for non-`git` executables in
//! `gitsail-git`'s own tests (a `sleep` command, a fake credential helper
//! script), where a `git`-specific locale override would be meaningless;
//! locale pinning is `GitCliProvider`'s concern alone, and it already
//! existed before this task. Every raw `git` call this crate's own
//! [`crate::git_ops`] module makes for fixture setup pins the same
//! `LC_ALL=C`/`LANG=C` explicitly too, matching the convention already
//! established by every pre-existing `gitsail-git` integration test file.

use std::path::Path;

use gitsail_application::{
    BlameRequest, CommitQuery, DiffRequest, LineHistoryRequest, RepositoryReadPort,
};
use gitsail_domain::{
    BlameOrigin, CancellationToken, ChangeType, ErrorCode, FileContentKind, HeadState,
};

use crate::git_ops::{commit_all, git_ok, init_repo, rev_parse, write_binary_file, write_file};

// ---------------------------------------------------------------------
// Discovery (RepositoryReadPort::discover).
// ---------------------------------------------------------------------

/// `discover` against a fresh repository reports a non-bare repository whose
/// root matches the directory it was opened from, with an unborn `HEAD` and
/// no current branch.
pub fn discover_reports_root_and_unborn_head_for_a_fresh_repository<P: RepositoryReadPort>(
    make_provider: impl Fn() -> P,
) {
    let dir = init_repo("contract-discover-fresh");
    let provider = make_provider();

    let repo = provider
        .discover(dir.path())
        .expect("discover must succeed for a real repository");

    assert!(!repo.is_bare);
    assert_eq!(repo.head_state, HeadState::Unborn);
    assert!(repo.current_branch.is_none());
    assert_eq!(
        repo.root_path.canonicalize().ok(),
        dir.path().canonicalize().ok()
    );
}

/// `discover` reports a structured, classified error — never a panic, never
/// creating any file — for a path that is not inside any Git repository.
pub fn discover_fails_for_a_path_outside_any_repository<P: RepositoryReadPort>(
    make_provider: impl Fn() -> P,
) {
    // A fresh, empty (non-Git) temp directory — deliberately not created via
    // `init_repo`.
    let dir = crate::temp_dir::TempDir::new("contract-discover-not-a-repo");
    let provider = make_provider();

    let err = provider
        .discover(dir.path())
        .expect_err("a plain directory is not a Git repository");

    assert_eq!(err.code(), ErrorCode::RepositoryNotFound);
    assert!(
        std::fs::read_dir(dir.path()).unwrap().next().is_none(),
        "discover must never write into the path it failed to open"
    );
}

/// `discover` reports `HeadState::Detached` (and no current branch) for a
/// repository whose `HEAD` points directly at a commit.
pub fn discover_detects_a_detached_head<P: RepositoryReadPort>(make_provider: impl Fn() -> P) {
    let dir = init_repo("contract-discover-detached");
    write_file(dir.path(), "a.txt", "one\n");
    commit_all(dir.path(), "first", 0);
    let first = rev_parse(dir.path(), "HEAD");
    write_file(dir.path(), "a.txt", "two\n");
    commit_all(dir.path(), "second", 1);
    git_ok(dir.path(), &["checkout", "-q", first.as_str()]);

    let provider = make_provider();
    let repo = provider.discover(dir.path()).unwrap();

    match repo.head_state {
        HeadState::Detached { commit } => assert_eq!(commit, first),
        other => panic!("expected Detached, got {other:?}"),
    }
    assert!(repo.current_branch.is_none());
}

// ---------------------------------------------------------------------
// Status (RepositoryReadPort::status).
// ---------------------------------------------------------------------

/// A repository with a fresh commit and no pending changes reports a clean
/// status.
pub fn status_is_clean_right_after_a_commit<P: RepositoryReadPort>(make_provider: impl Fn() -> P) {
    let dir = init_repo("contract-status-clean");
    write_file(dir.path(), "a.txt", "content\n");
    commit_all(dir.path(), "initial", 0);

    let provider = make_provider();
    let repo = provider.discover(dir.path()).unwrap();
    let status = provider.status(&repo).unwrap();

    assert!(status.is_clean());
    assert_eq!(status.branch.as_ref().map(|b| b.as_str()), Some("main"));
}

/// An unstaged modification and a new untracked file are both reported by
/// `status`, each under the correct [`ChangeType`].
pub fn status_reports_an_unstaged_modification_and_an_untracked_file<P: RepositoryReadPort>(
    make_provider: impl Fn() -> P,
) {
    let dir = init_repo("contract-status-dirty");
    write_file(dir.path(), "tracked.txt", "original\n");
    commit_all(dir.path(), "initial", 0);
    write_file(dir.path(), "tracked.txt", "modified\n");
    write_file(dir.path(), "untracked.txt", "new\n");

    let provider = make_provider();
    let repo = provider.discover(dir.path()).unwrap();
    let status = provider.status(&repo).unwrap();

    assert!(!status.is_clean());
    let modified = status
        .files
        .iter()
        .find(|f| f.path == Path::new("tracked.txt"))
        .expect("tracked.txt must be reported");
    assert_eq!(modified.change_type, ChangeType::Modified);
    let untracked = status
        .files
        .iter()
        .find(|f| f.path == Path::new("untracked.txt"))
        .expect("untracked.txt must be reported");
    assert_eq!(untracked.change_type, ChangeType::Untracked);
}

// ---------------------------------------------------------------------
// Commit history (RepositoryReadPort::commits/commit).
// ---------------------------------------------------------------------

/// `commits` returns every commit, newest first, matching the order they
/// were created in.
pub fn commits_lists_history_newest_first<P: RepositoryReadPort>(make_provider: impl Fn() -> P) {
    let dir = init_repo("contract-commits-order");
    for (sequence, message) in ["first", "second", "third"].into_iter().enumerate() {
        write_file(dir.path(), "a.txt", message);
        commit_all(dir.path(), message, sequence as u64);
    }

    let provider = make_provider();
    let repo = provider.discover(dir.path()).unwrap();
    let page = provider.commits(&repo, &CommitQuery::default()).unwrap();

    let subjects: Vec<&str> = page.items.iter().map(|c| c.subject.as_str()).collect();
    assert_eq!(subjects, vec!["third", "second", "first"]);
    assert!(!page.has_more);
}

/// A `limit` smaller than the true history size is honored exactly, and
/// `has_more`/`next_cursor` reflect that more history remains.
pub fn commits_respects_a_limit_and_reports_more_remain<P: RepositoryReadPort>(
    make_provider: impl Fn() -> P,
) {
    let dir = init_repo("contract-commits-limit");
    for sequence in 0..5u64 {
        write_file(dir.path(), "a.txt", &sequence.to_string());
        commit_all(dir.path(), &format!("commit {sequence}"), sequence);
    }

    let provider = make_provider();
    let repo = provider.discover(dir.path()).unwrap();
    let query = CommitQuery {
        limit: Some(2),
        ..Default::default()
    };
    let page = provider.commits(&repo, &query).unwrap();

    assert_eq!(page.items.len(), 2);
    assert!(page.has_more);
    assert!(page.next_cursor.is_some());
}

/// `commit` reads a single commit by hash, matching the same subject
/// `commits` reported it with, and its parent-count-derived predicates
/// behave as documented.
pub fn commit_reads_a_single_commit_matching_its_history_entry<P: RepositoryReadPort>(
    make_provider: impl Fn() -> P,
) {
    let dir = init_repo("contract-commit-single");
    write_file(dir.path(), "a.txt", "content\n");
    commit_all(dir.path(), "the only commit", 0);
    let hash = rev_parse(dir.path(), "HEAD");

    let provider = make_provider();
    let repo = provider.discover(dir.path()).unwrap();
    let commit = provider.commit(&repo, &hash).unwrap();

    assert_eq!(commit.hash, hash);
    assert_eq!(commit.subject, "the only commit");
    assert!(commit.is_root());
    assert!(!commit.is_merge());
}

/// `commit` fails with a classified error for a hash that does not resolve
/// to any object in the repository — never a fabricated/default `Commit`.
pub fn commit_fails_for_an_unknown_hash<P: RepositoryReadPort>(make_provider: impl Fn() -> P) {
    let dir = init_repo("contract-commit-unknown");
    write_file(dir.path(), "a.txt", "content\n");
    commit_all(dir.path(), "initial", 0);

    let provider = make_provider();
    let repo = provider.discover(dir.path()).unwrap();
    let unknown = gitsail_domain::CommitHash::new("f".repeat(40)).unwrap();

    let err = provider.commit(&repo, &unknown).unwrap_err();
    assert!(matches!(
        err.code(),
        ErrorCode::RepositoryNotFound | ErrorCode::ProcessFailure | ErrorCode::ParseFailure
    ));
}

// ---------------------------------------------------------------------
// Branches (RepositoryReadPort::branches).
// ---------------------------------------------------------------------

/// `branches` lists every local branch, with exactly the checked-out one
/// flagged `is_current`.
pub fn branches_lists_local_branches_with_exactly_one_current<P: RepositoryReadPort>(
    make_provider: impl Fn() -> P,
) {
    let dir = init_repo("contract-branches");
    write_file(dir.path(), "a.txt", "base\n");
    commit_all(dir.path(), "base", 0);
    git_ok(dir.path(), &["checkout", "-q", "-b", "feature"]);
    git_ok(dir.path(), &["checkout", "-q", "main"]);

    let provider = make_provider();
    let repo = provider.discover(dir.path()).unwrap();
    let branches = provider.branches(&repo).unwrap();

    let names: Vec<&str> = branches.iter().map(|b| b.name.as_str()).collect();
    assert!(names.contains(&"main"));
    assert!(names.contains(&"feature"));
    let current: Vec<&str> = branches
        .iter()
        .filter(|b| b.is_current)
        .map(|b| b.name.as_str())
        .collect();
    assert_eq!(current, vec!["main"]);
}

// ---------------------------------------------------------------------
// Diff (RepositoryReadPort::diff).
// ---------------------------------------------------------------------

/// An unstaged modification shows up in the working-tree diff
/// (`from`/`to: None, staged: false`), with a real hunk (never fabricated,
/// never withheld for a plain small text change).
pub fn diff_working_tree_reports_an_unstaged_modification<P: RepositoryReadPort>(
    make_provider: impl Fn() -> P,
) {
    let dir = init_repo("contract-diff-working-tree");
    write_file(dir.path(), "a.txt", "line1\nline2\n");
    commit_all(dir.path(), "initial", 0);
    write_file(dir.path(), "a.txt", "line1\nCHANGED\n");

    let provider = make_provider();
    let repo = provider.discover(dir.path()).unwrap();
    let diff = provider
        .diff(&repo, &DiffRequest::default(), &CancellationToken::new())
        .unwrap();

    assert_eq!(diff.files.len(), 1);
    let file = &diff.files[0];
    assert_eq!(file.path, Path::new("a.txt"));
    assert!(!file.is_binary);
    assert!(!file.truncated);
    assert!(!file.hunks.is_empty());
}

/// `staged: true` reports only what is in the index, never an unstaged
/// change to the same or another file.
pub fn diff_staged_reports_only_the_index_not_the_working_tree<P: RepositoryReadPort>(
    make_provider: impl Fn() -> P,
) {
    let dir = init_repo("contract-diff-staged");
    write_file(dir.path(), "a.txt", "line1\n");
    write_file(dir.path(), "b.txt", "line1\n");
    commit_all(dir.path(), "initial", 0);
    write_file(dir.path(), "a.txt", "line1\nstaged change\n");
    git_ok(dir.path(), &["add", "a.txt"]);
    // Unstaged-only change to a different file — must never appear.
    write_file(dir.path(), "b.txt", "line1\nunstaged change\n");

    let provider = make_provider();
    let repo = provider.discover(dir.path()).unwrap();
    let request = DiffRequest {
        staged: true,
        ..Default::default()
    };
    let diff = provider
        .diff(&repo, &request, &CancellationToken::new())
        .unwrap();

    let paths: Vec<&Path> = diff.files.iter().map(|f| f.path.as_path()).collect();
    assert_eq!(paths, vec![Path::new("a.txt")]);
}

/// Diffing two explicit, resolved revisions against each other reports
/// exactly the files that changed between them.
pub fn diff_between_two_explicit_revisions<P: RepositoryReadPort>(make_provider: impl Fn() -> P) {
    let dir = init_repo("contract-diff-revisions");
    write_file(dir.path(), "a.txt", "v1\n");
    commit_all(dir.path(), "v1", 0);
    let first = rev_parse(dir.path(), "HEAD");
    write_file(dir.path(), "a.txt", "v2\n");
    commit_all(dir.path(), "v2", 1);
    let second = rev_parse(dir.path(), "HEAD");

    let provider = make_provider();
    let repo = provider.discover(dir.path()).unwrap();
    let request = DiffRequest {
        from: Some(first),
        to: Some(second),
        ..Default::default()
    };
    let diff = provider
        .diff(&repo, &request, &CancellationToken::new())
        .unwrap();

    assert_eq!(diff.files.len(), 1);
    assert_eq!(diff.files[0].path, Path::new("a.txt"));
}

/// A binary file's change is reported truthfully (`is_binary: true`) without
/// fabricating textual hunks for content that never was text (Diff & Blame
/// Semantics Rules #3).
pub fn diff_reports_a_binary_file_without_fabricating_hunks<P: RepositoryReadPort>(
    make_provider: impl Fn() -> P,
) {
    let dir = init_repo("contract-diff-binary");
    write_binary_file(dir.path(), "image.bin", &[0, 1, 2, 255, 254, 0, 3]);
    commit_all(dir.path(), "add binary", 0);
    write_binary_file(dir.path(), "image.bin", &[9, 8, 7, 255, 254, 0, 6]);

    let provider = make_provider();
    let repo = provider.discover(dir.path()).unwrap();
    let diff = provider
        .diff(&repo, &DiffRequest::default(), &CancellationToken::new())
        .unwrap();

    assert_eq!(diff.files.len(), 1);
    let file = &diff.files[0];
    assert!(file.is_binary);
    assert!(
        file.hunks.is_empty(),
        "a binary file must never carry fabricated textual hunks"
    );
}

// ---------------------------------------------------------------------
// resolve_revision.
// ---------------------------------------------------------------------

/// `resolve_revision` resolves a branch name to its tip commit.
pub fn resolve_revision_resolves_a_branch_name_to_its_tip<P: RepositoryReadPort>(
    make_provider: impl Fn() -> P,
) {
    let dir = init_repo("contract-resolve-revision");
    write_file(dir.path(), "a.txt", "content\n");
    commit_all(dir.path(), "initial", 0);
    let head = rev_parse(dir.path(), "HEAD");

    let provider = make_provider();
    let repo = provider.discover(dir.path()).unwrap();
    let resolved = provider.resolve_revision(&repo, "main").unwrap();

    assert_eq!(resolved, head);
}

/// A revision that does not resolve to anything fails outright — never
/// falling back to some other, unrelated commit (US-028 criterion 3).
pub fn resolve_revision_fails_for_an_unresolvable_revision<P: RepositoryReadPort>(
    make_provider: impl Fn() -> P,
) {
    let dir = init_repo("contract-resolve-revision-unknown");
    write_file(dir.path(), "a.txt", "content\n");
    commit_all(dir.path(), "initial", 0);

    let provider = make_provider();
    let repo = provider.discover(dir.path()).unwrap();

    let err = provider
        .resolve_revision(&repo, "definitely-not-a-real-revision")
        .unwrap_err();
    assert_eq!(err.code(), ErrorCode::RepositoryNotFound);
}

// ---------------------------------------------------------------------
// Blame (RepositoryReadPort::blame).
// ---------------------------------------------------------------------

/// Every line of a committed file is attributed to the commit that
/// introduced it, with [`BlameOrigin::Committed`].
pub fn blame_attributes_every_line_to_the_commit_that_introduced_it<P: RepositoryReadPort>(
    make_provider: impl Fn() -> P,
) {
    let dir = init_repo("contract-blame-committed");
    write_file(dir.path(), "a.txt", "line1\nline2\n");
    commit_all(dir.path(), "initial", 0);
    let hash = rev_parse(dir.path(), "HEAD");

    let provider = make_provider();
    let repo = provider.discover(dir.path()).unwrap();
    let request = BlameRequest {
        file: "a.txt".into(),
        revision: Some(hash.clone()),
        line_range: None,
        buffer_contents: None,
    };
    let blame = provider
        .blame(&repo, &request, &CancellationToken::new())
        .unwrap();

    assert_eq!(blame.lines.len(), 2);
    for line in &blame.lines {
        assert_eq!(line.commit, hash);
        assert_eq!(line.origin, BlameOrigin::Committed);
    }
}

/// Blaming the working tree (`revision: None`) reports
/// [`BlameOrigin::Local`] for a line changed since the last commit, never
/// silently attributing it to the prior commit as if unchanged (US-033
/// criterion 1).
pub fn blame_working_tree_reports_local_origin_for_an_uncommitted_change<P: RepositoryReadPort>(
    make_provider: impl Fn() -> P,
) {
    let dir = init_repo("contract-blame-local");
    write_file(dir.path(), "a.txt", "line1\nline2\n");
    commit_all(dir.path(), "initial", 0);
    write_file(dir.path(), "a.txt", "line1\nCHANGED\n");

    let provider = make_provider();
    let repo = provider.discover(dir.path()).unwrap();
    let request = BlameRequest {
        file: "a.txt".into(),
        revision: None,
        line_range: None,
        buffer_contents: None,
    };
    let blame = provider
        .blame(&repo, &request, &CancellationToken::new())
        .unwrap();

    assert_eq!(blame.lines.len(), 2);
    assert_eq!(blame.lines[0].origin, BlameOrigin::Committed);
    assert_eq!(blame.lines[1].origin, BlameOrigin::Local);
}

// ---------------------------------------------------------------------
// Line history (RepositoryReadPort::line_history).
// ---------------------------------------------------------------------

/// `line_history` reports one entry per commit that touched the queried
/// range, oldest details aside — at minimum, both commits that changed line
/// 1 of the file show up.
pub fn line_history_traces_every_commit_that_touched_the_range<P: RepositoryReadPort>(
    make_provider: impl Fn() -> P,
) {
    let dir = init_repo("contract-line-history");
    write_file(dir.path(), "a.txt", "v1\n");
    commit_all(dir.path(), "v1", 0);
    write_file(dir.path(), "a.txt", "v2\n");
    commit_all(dir.path(), "v2", 1);
    let head = rev_parse(dir.path(), "HEAD");

    let provider = make_provider();
    let repo = provider.discover(dir.path()).unwrap();
    let request = LineHistoryRequest {
        file: "a.txt".into(),
        revision: Some(head),
        range: gitsail_domain::LineRange::new(1, 1),
    };
    let history = provider
        .line_history(&repo, &request, &CancellationToken::new())
        .unwrap();

    assert_eq!(history.entries.len(), 2);
    let subjects: Vec<&str> = history
        .entries
        .iter()
        .map(|entry| entry.commit.subject.as_str())
        .collect();
    assert!(subjects.contains(&"v1"));
    assert!(subjects.contains(&"v2"));
}

// ---------------------------------------------------------------------
// File content at a revision (RepositoryReadPort::file_content).
// ---------------------------------------------------------------------

/// Reading a file's content as of an old revision returns exactly what was
/// committed then, not the file's later (or current) content.
pub fn file_content_reads_historical_text_content<P: RepositoryReadPort>(
    make_provider: impl Fn() -> P,
) {
    let dir = init_repo("contract-file-content-historical");
    write_file(dir.path(), "a.txt", "first version\n");
    commit_all(dir.path(), "v1", 0);
    let first = rev_parse(dir.path(), "HEAD");
    write_file(dir.path(), "a.txt", "second version\n");
    commit_all(dir.path(), "v2", 1);

    let provider = make_provider();
    let repo = provider.discover(dir.path()).unwrap();
    let content = provider
        .file_content(&repo, &first, Path::new("a.txt"))
        .unwrap();

    assert_eq!(
        content.kind,
        FileContentKind::Text("first version\n".to_string())
    );
}

/// A path that did not exist yet at the queried revision reports
/// [`FileContentKind::Missing`] — a legitimate outcome, never a
/// [`gitsail_domain::GitSailError`].
pub fn file_content_reports_missing_for_a_path_added_later<P: RepositoryReadPort>(
    make_provider: impl Fn() -> P,
) {
    let dir = init_repo("contract-file-content-missing");
    write_file(dir.path(), "a.txt", "content\n");
    commit_all(dir.path(), "v1", 0);
    let first = rev_parse(dir.path(), "HEAD");
    write_file(dir.path(), "b.txt", "added later\n");
    commit_all(dir.path(), "v2 adds b.txt", 1);

    let provider = make_provider();
    let repo = provider.discover(dir.path()).unwrap();
    let content = provider
        .file_content(&repo, &first, Path::new("b.txt"))
        .unwrap();

    assert_eq!(content.kind, FileContentKind::Missing);
}

// ---------------------------------------------------------------------
// Golden parsing: selective edge cases (US-120 criterion 3).
// ---------------------------------------------------------------------

/// A file whose name contains spaces and non-ASCII (accented) characters is
/// reported with that exact name, not a mangled or quoted-escaped one —
/// exercises `GitCliProvider`'s `core.quotePath=false` handling (US-008)
/// through the real port, not just its internal parser in isolation.
pub fn golden_parsing_preserves_a_file_name_with_special_characters<P: RepositoryReadPort>(
    make_provider: impl Fn() -> P,
) {
    let dir = init_repo("contract-golden-special-name");
    let name = "café notes (draft).txt";
    write_file(dir.path(), name, "hello\n");
    commit_all(
        dir.path(),
        "add file with special characters in its name",
        0,
    );
    write_file(dir.path(), name, "hello\nmodified\n");

    let provider = make_provider();
    let repo = provider.discover(dir.path()).unwrap();
    let diff = provider
        .diff(&repo, &DiffRequest::default(), &CancellationToken::new())
        .unwrap();

    assert_eq!(diff.files.len(), 1);
    assert_eq!(diff.files[0].path, Path::new(name));
}

/// A commit's multi-line body (subject, blank line, then a multi-paragraph
/// body) is read back verbatim, including embedded newlines — this is
/// exactly why `LOG_FORMAT`'s record/field separators are ASCII control
/// characters that can never occur in ordinary commit text, rather than a
/// character (comma, pipe, ...) a message could plausibly contain.
pub fn golden_parsing_preserves_a_multi_line_commit_message<P: RepositoryReadPort>(
    make_provider: impl Fn() -> P,
) {
    let dir = init_repo("contract-golden-multiline-message");
    write_file(dir.path(), "a.txt", "content\n");
    git_ok(dir.path(), &["add", "-A"]);
    let message =
        "subject line\n\nfirst body paragraph.\n\nsecond body paragraph, with more detail.";
    crate::git_ops::git_ok_at(dir.path(), &["commit", "--quiet", "-m", message], 0);

    let provider = make_provider();
    let repo = provider.discover(dir.path()).unwrap();
    let hash = rev_parse(dir.path(), "HEAD");
    let commit = provider.commit(&repo, &hash).unwrap();

    assert_eq!(commit.subject, "subject line");
    assert!(commit.body.contains("first body paragraph."));
    assert!(commit
        .body
        .contains("second body paragraph, with more detail."));
}

/// A commit message containing non-ASCII UTF-8 text (accented characters
/// and an emoji) round-trips exactly — this parser must never assume ASCII
/// or transliterate/drop non-ASCII bytes.
pub fn golden_parsing_preserves_non_ascii_commit_message_content<P: RepositoryReadPort>(
    make_provider: impl Fn() -> P,
) {
    let dir = init_repo("contract-golden-non-ascii-message");
    write_file(dir.path(), "a.txt", "content\n");
    commit_all(
        dir.path(),
        "café \u{2615} — fix acentuação e emoji \u{1F680}",
        0,
    );

    let provider = make_provider();
    let repo = provider.discover(dir.path()).unwrap();
    let hash = rev_parse(dir.path(), "HEAD");
    let commit = provider.commit(&repo, &hash).unwrap();

    assert_eq!(
        commit.subject,
        "café \u{2615} — fix acentuação e emoji \u{1F680}"
    );
}
