//! Canned, representative Git repository fixtures (T-252/US-119).
//!
//! Each [`Fixture`] constructor builds a real, temporary, non-shared Git
//! repository via direct `git` subprocess calls (never simulated, never
//! through `GitCliProvider` itself — a fixture is independent of the code it
//! exists to test) and returns it paired with a [`FixtureState`] declaring
//! exactly what it contains (US-119 criterion 3: "cada cenário declara o
//! estado esperado"). The backing directory is a uniquely named
//! [`TempDir`], recursively removed when the `Fixture` is dropped — every
//! fixture is fully isolated from every other, including two built
//! concurrently by parallel test threads (US-119 DoD).
//!
//! This consolidates conflict/in-progress-operation setup that had been
//! duplicated (each with its own copy, sometimes explicitly commented as
//! "mirrors tests/t230_in_progress_operation.rs::setup_diverging_branches")
//! across `tests/t230_in_progress_operation.rs`,
//! `tests/t231_233_merge_conflicts.rs`, and `tests/t235_237_rebase.rs`
//! (EPIC-16/EPIC-17) — see [`Fixture::with_conflict`]/
//! [`Fixture::with_rebase_conflict`].

use std::path::Path;
use std::process::Output;

use crate::git_ops::{
    commit_all, git, git_ok, git_ok_at, init_repo, rev_parse, write_binary_file, write_file,
    FIXTURE_AUTHOR_EMAIL, FIXTURE_AUTHOR_NAME,
};
use crate::temp_dir::TempDir;

/// Which kind of multi-step operation a fixture was left in the middle of
/// (mirrors the two shapes `gitsail_domain::InProgressOperation` covers that
/// setup code needs to construct explicitly; the read side already has that
/// richer, adapter-produced type — this is just fixture metadata, not a
/// re-implementation of it).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InProgressOperationKind {
    Merge,
    Rebase,
}

/// Declares exactly what a [`Fixture`] contains, so a test can assert
/// against the fixture's own stated contract instead of re-deriving it from
/// the setup code (US-119 criterion 3).
///
/// Fields default to the "nothing of this kind" value
/// ([`Default::default`]), so each constructor only needs to state the
/// fields that make it distinctive.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct FixtureState {
    /// Number of commits [`Fixture::path`]'s current `HEAD` (or, for
    /// [`Fixture::shallow_clone`], the shallow history) can reach. `0` for
    /// an unborn `HEAD` ([`Fixture::empty_repo`], [`Fixture::bare_repo`]).
    pub commit_count: usize,
    /// Every local branch the fixture created, in creation order.
    pub branches: Vec<String>,
    /// The branch `HEAD` is attached to, or `None` for an unborn or detached
    /// `HEAD` (see [`Self::unborn_head`]/[`Self::detached_head`] to
    /// distinguish those two).
    pub current_branch: Option<String>,
    /// `true` when no commit exists yet (`HeadState::Unborn`).
    pub unborn_head: bool,
    /// `true` when `HEAD` points directly at a commit rather than a branch.
    pub detached_head: bool,
    pub bare: bool,
    /// `true` for a shallow clone (`git clone --depth`) — history beyond
    /// [`Self::commit_count`] deliberately does not exist locally.
    pub shallow: bool,
    /// `true` when the working tree and/or index has uncommitted changes.
    pub dirty: bool,
    /// `true` when the repository has an unresolved merge/rebase conflict
    /// (see [`Self::in_progress_operation`] for which).
    pub has_conflict: bool,
    pub in_progress_operation: Option<InProgressOperationKind>,
    /// `(old_path, new_path)` pairs for every rename the fixture committed.
    pub renamed_files: Vec<(String, String)>,
    /// Paths of every non-UTF-8/binary file the fixture committed.
    pub binary_files: Vec<String>,
}

/// A single, self-contained temporary Git repository scenario. See this
/// module's doc for the isolation/cleanup guarantee, and each constructor's
/// own doc for exactly what it builds.
pub struct Fixture {
    dir: TempDir,
    pub state: FixtureState,
}

impl Fixture {
    /// The repository's root directory (or, for [`Fixture::bare_repo`], the
    /// bare repository directory itself).
    pub fn path(&self) -> &Path {
        self.dir.path()
    }

    /// Runs `git <args>` against this fixture directly (never through
    /// `GitCliProvider`) — for a test that needs to inspect or further
    /// mutate the fixture's raw Git state.
    pub fn git(&self, args: &[&str]) -> Output {
        git(self.path(), args)
    }

    /// Like [`Self::git`], but asserts success.
    pub fn git_ok(&self, args: &[&str]) {
        git_ok(self.path(), args);
    }

    // -------------------------------------------------------------------
    // US-119 criterion 1: empty, one commit, branches, merge commit,
    // detached, dirty, rename, binary.
    // -------------------------------------------------------------------

    /// A freshly initialized repository with no commits at all — `HEAD` is
    /// unborn, there are no branches yet (Git does not create `refs/heads/
    /// main` until the first commit).
    pub fn empty_repo() -> Self {
        let dir = init_repo("fixture-empty-repo");
        Self {
            dir,
            state: FixtureState {
                unborn_head: true,
                ..Default::default()
            },
        }
    }

    /// A repository with exactly one commit on `main`.
    pub fn with_one_commit() -> Self {
        let dir = init_repo("fixture-one-commit");
        write_file(dir.path(), "README.md", "hello, gitsail\n");
        commit_all(dir.path(), "initial commit", 0);
        Self {
            dir,
            state: FixtureState {
                commit_count: 1,
                branches: vec!["main".to_string()],
                current_branch: Some("main".to_string()),
                ..Default::default()
            },
        }
    }

    /// `main` plus two more local branches (`feature`, `develop`), each with
    /// its own commit not present on the others — three branches, three
    /// commits total, no merge and no conflict. `HEAD` ends back on `main`.
    pub fn with_branches() -> Self {
        let dir = init_repo("fixture-branches");
        write_file(dir.path(), "base.txt", "base\n");
        commit_all(dir.path(), "base", 0);

        git_ok(dir.path(), &["checkout", "-q", "-b", "feature"]);
        write_file(dir.path(), "feature.txt", "feature work\n");
        commit_all(dir.path(), "feature work", 1);

        git_ok(dir.path(), &["checkout", "-q", "main"]);
        git_ok(dir.path(), &["checkout", "-q", "-b", "develop"]);
        write_file(dir.path(), "develop.txt", "develop work\n");
        commit_all(dir.path(), "develop work", 2);

        git_ok(dir.path(), &["checkout", "-q", "main"]);

        Self {
            dir,
            state: FixtureState {
                commit_count: 3,
                branches: vec!["main".to_string(), "feature".to_string(), "develop".to_string()],
                current_branch: Some("main".to_string()),
                ..Default::default()
            },
        }
    }

    /// Two branches with diverging, non-conflicting changes, merged with
    /// `--no-ff` into a genuine two-parent merge commit — completed, not
    /// in-progress or conflicted (see [`Self::with_conflict`] for that).
    pub fn with_merge_commit() -> Self {
        let dir = init_repo("fixture-merge-commit");
        write_file(dir.path(), "a.txt", "a\n");
        write_file(dir.path(), "b.txt", "b\n");
        commit_all(dir.path(), "base", 0);

        git_ok(dir.path(), &["checkout", "-q", "-b", "feature"]);
        write_file(dir.path(), "a.txt", "a\nfeature change\n");
        commit_all(dir.path(), "feature change", 1);

        git_ok(dir.path(), &["checkout", "-q", "main"]);
        write_file(dir.path(), "b.txt", "b\nmain change\n");
        commit_all(dir.path(), "main change", 2);

        git_ok_at(
            dir.path(),
            &["merge", "--no-ff", "--no-edit", "feature"],
            3,
        );

        Self {
            dir,
            state: FixtureState {
                commit_count: 4,
                branches: vec!["main".to_string(), "feature".to_string()],
                current_branch: Some("main".to_string()),
                ..Default::default()
            },
        }
    }

    /// Two commits on `main`, with `HEAD` then detached at the first one.
    pub fn detached_head() -> Self {
        let dir = init_repo("fixture-detached-head");
        write_file(dir.path(), "a.txt", "first\n");
        commit_all(dir.path(), "first", 0);
        let first = rev_parse(dir.path(), "HEAD");
        write_file(dir.path(), "a.txt", "second\n");
        commit_all(dir.path(), "second", 1);

        git_ok(dir.path(), &["checkout", "-q", first.as_str()]);

        Self {
            dir,
            state: FixtureState {
                commit_count: 2,
                branches: vec!["main".to_string()],
                current_branch: None,
                detached_head: true,
                ..Default::default()
            },
        }
    }

    /// One commit, then an unstaged modification to the tracked file *and*
    /// a new untracked file — both a dirty index-relative working tree and
    /// an untracked path in one fixture.
    pub fn dirty_working_tree() -> Self {
        let dir = init_repo("fixture-dirty-working-tree");
        write_file(dir.path(), "tracked.txt", "original content\n");
        commit_all(dir.path(), "initial commit", 0);

        write_file(dir.path(), "tracked.txt", "modified, uncommitted content\n");
        write_file(dir.path(), "untracked.txt", "never committed\n");

        Self {
            dir,
            state: FixtureState {
                commit_count: 1,
                branches: vec!["main".to_string()],
                current_branch: Some("main".to_string()),
                dirty: true,
                ..Default::default()
            },
        }
    }

    /// A file committed under one name, then renamed (`git mv`) and
    /// committed again — `git log --follow`/rename detection has something
    /// real to find.
    pub fn with_renamed_file() -> Self {
        let dir = init_repo("fixture-renamed-file");
        write_file(dir.path(), "old_name.txt", "content that survives the rename\n");
        commit_all(dir.path(), "add old_name.txt", 0);

        git_ok(dir.path(), &["mv", "old_name.txt", "new_name.txt"]);
        commit_all(dir.path(), "rename old_name.txt to new_name.txt", 1);

        Self {
            dir,
            state: FixtureState {
                commit_count: 2,
                branches: vec!["main".to_string()],
                current_branch: Some("main".to_string()),
                renamed_files: vec![("old_name.txt".to_string(), "new_name.txt".to_string())],
                ..Default::default()
            },
        }
    }

    /// A committed file whose content is deliberately not valid UTF-8 text
    /// (a PNG-style magic header followed by non-text bytes, including a
    /// NUL) — Git's own binary-detection heuristic reports it as binary.
    pub fn with_binary_file() -> Self {
        let dir = init_repo("fixture-binary-file");
        let bytes: &[u8] = &[
            0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, // PNG magic
            0x00, 0x01, 0x02, 0x03, 0xff, 0xfe, 0xfd, 0x00,
        ];
        write_binary_file(dir.path(), "image.bin", bytes);
        commit_all(dir.path(), "add binary file", 0);

        Self {
            dir,
            state: FixtureState {
                commit_count: 1,
                branches: vec!["main".to_string()],
                current_branch: Some("main".to_string()),
                binary_files: vec!["image.bin".to_string()],
                ..Default::default()
            },
        }
    }

    // -------------------------------------------------------------------
    // US-119 criterion 2: shallow clone, bare repo.
    // -------------------------------------------------------------------

    /// A `--depth 1` clone of a freshly built three-commit source
    /// repository: only the single most recent commit is reachable locally,
    /// exactly like a CI checkout.
    pub fn shallow_clone() -> Self {
        let source = init_repo("fixture-shallow-source");
        for (sequence, message) in ["c1", "c2", "c3"].into_iter().enumerate() {
            write_file(source.path(), "a.txt", &format!("{message}\n"));
            commit_all(source.path(), message, sequence as u64);
        }

        let clone_dir = TempDir::new("fixture-shallow-clone");
        let cwd = std::env::current_dir().expect("current directory is accessible");
        // `--depth` is silently ignored for a plain local-path source (Git's
        // own "local clone" optimization hardlinks the whole object store
        // instead) — a `file://` URL forces the real (non-local) transport,
        // which actually honors it. Verified empirically: without this, the
        // clone below ends up with all three source commits, not one.
        let source_url = format!("file://{}", source.path().display());
        git_ok(
            &cwd,
            &[
                "clone",
                "--quiet",
                "--depth",
                "1",
                "--",
                &source_url,
                clone_dir
                    .path()
                    .to_str()
                    .expect("temp dir path is valid UTF-8"),
            ],
        );
        git_ok(clone_dir.path(), &["config", "user.name", FIXTURE_AUTHOR_NAME]);
        git_ok(clone_dir.path(), &["config", "user.email", FIXTURE_AUTHOR_EMAIL]);
        // `source` is dropped (and its directory removed) here: a local
        // clone's objects are its own, independent copy (or hardlinks that
        // remain valid after one side is unlinked), never a reference back
        // to `source`'s directory.

        Self {
            dir: clone_dir,
            state: FixtureState {
                commit_count: 1,
                branches: vec!["main".to_string()],
                current_branch: Some("main".to_string()),
                shallow: true,
                ..Default::default()
            },
        }
    }

    /// A bare repository (`git init --bare`) with no commits — standing in
    /// for a remote, or for exercising a bare-repository-specific
    /// limitation (e.g. `RepositoryReadPort::detect_in_progress_operation`
    /// against a bare repo, T-230/US-078's own explicit "no worktree"
    /// scenario).
    pub fn bare_repo() -> Self {
        let dir = TempDir::new("fixture-bare-repo");
        git_ok(
            dir.path(),
            &["init", "--quiet", "--bare", "--initial-branch=main"],
        );
        Self {
            dir,
            state: FixtureState {
                bare: true,
                unborn_head: true,
                ..Default::default()
            },
        }
    }

    // -------------------------------------------------------------------
    // US-119 criterion 2: conflict/rebase-in-progress fixtures, for reading
    // and for future write operations (continue/abort/skip). Consolidates
    // the `setup_diverging_branches` helper duplicated (with that exact
    // name, and an explicit "mirrors ..." doc comment) across
    // `tests/t230_in_progress_operation.rs`, `tests/t231_233_merge_conflicts.rs`
    // and `tests/t235_237_rebase.rs`.
    // -------------------------------------------------------------------

    /// Two branches that both modify the same line of the same file
    /// (`main`, then a `main`/other-branch commit each changing line 2 of
    /// `f.txt`), leaving `HEAD` on `main`. Shared by [`Fixture::with_conflict`]
    /// and [`Fixture::with_rebase_conflict`], and directly reusable by a
    /// test that needs the same reliably-diverging setup without either
    /// fixture's own outer `git merge`/`git rebase` step.
    fn diverging_branches(dir: &Path, other_branch: &str) {
        write_file(dir, "f.txt", "line1\nline2\nline3\n");
        commit_all(dir, "base", 0);

        git_ok(dir, &["checkout", "-q", "-b", other_branch]);
        write_file(dir, "f.txt", "line1\nCHANGED-other\nline3\n");
        commit_all(dir, "other change", 1);

        git_ok(dir, &["checkout", "-q", "main"]);
        write_file(dir, "f.txt", "line1\nCHANGED-main\nline3\n");
        commit_all(dir, "main change", 2);
    }

    /// A merge left in progress, conflicted, on `main` (`git merge feature`
    /// against [`Self::diverging_branches`]'s setup). Read-only fixtures for
    /// `conflict_sides`/`detect_in_progress_operation`, and a starting point
    /// for exercising `continue_operation`/`abort_operation`/
    /// `mark_conflict_resolved`/`take_conflict_side`.
    pub fn with_conflict() -> Self {
        let dir = init_repo("fixture-merge-conflict");
        Self::diverging_branches(dir.path(), "feature");

        let merge_output = git(dir.path(), &["merge", "feature"]);
        assert!(
            !merge_output.status.success(),
            "the merge must conflict for this fixture to be meaningful"
        );

        Self {
            dir,
            state: FixtureState {
                commit_count: 3,
                branches: vec!["main".to_string(), "feature".to_string()],
                current_branch: Some("main".to_string()),
                has_conflict: true,
                in_progress_operation: Some(InProgressOperationKind::Merge),
                ..Default::default()
            },
        }
    }

    /// A rebase (default/merge backend) left in progress, conflicted, with
    /// `feature` being replayed onto `main` (against
    /// [`Self::diverging_branches`]'s same setup). Mirrors
    /// [`Self::with_conflict`] for the rebase case.
    pub fn with_rebase_conflict() -> Self {
        let dir = init_repo("fixture-rebase-conflict");
        Self::diverging_branches(dir.path(), "feature");
        git_ok(dir.path(), &["checkout", "-q", "feature"]);

        let rebase_output = git(dir.path(), &["rebase", "main"]);
        assert!(
            !rebase_output.status.success(),
            "the rebase must conflict for this fixture to be meaningful"
        );

        Self {
            dir,
            state: FixtureState {
                commit_count: 3,
                branches: vec!["main".to_string(), "feature".to_string()],
                // Mid-rebase, Git detaches HEAD onto the commit being
                // replayed — there is no current branch to report, mirroring
                // `Fixture::detached_head`'s own convention.
                current_branch: None,
                has_conflict: true,
                in_progress_operation: Some(InProgressOperationKind::Rebase),
                ..Default::default()
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::git_ops::rev_parse;

    fn commit_count(dir: &Path, revision: &str) -> usize {
        let output = git(dir, &["rev-list", "--count", revision]);
        assert!(output.status.success(), "git rev-list --count failed");
        String::from_utf8(output.stdout)
            .unwrap()
            .trim()
            .parse()
            .unwrap()
    }

    fn local_branches(dir: &Path) -> Vec<String> {
        let output = git(dir, &["branch", "--format=%(refname:short)"]);
        assert!(output.status.success(), "git branch failed");
        String::from_utf8(output.stdout)
            .unwrap()
            .lines()
            .map(str::to_string)
            .collect()
    }

    #[test]
    fn empty_repo_has_an_unborn_head_and_no_branches() {
        let fixture = Fixture::empty_repo();
        assert!(fixture.state.unborn_head);
        assert!(local_branches(fixture.path()).is_empty());
        // Nothing to `rev-list` yet either — this asserts the same fact from
        // the Git side, not only the declared state.
        let head = git(fixture.path(), &["rev-parse", "HEAD"]);
        assert!(!head.status.success());
    }

    #[test]
    fn with_one_commit_matches_its_declared_state() {
        let fixture = Fixture::with_one_commit();
        assert_eq!(
            commit_count(fixture.path(), "HEAD"),
            fixture.state.commit_count
        );
        assert_eq!(local_branches(fixture.path()), fixture.state.branches);
    }

    #[test]
    fn with_branches_creates_exactly_the_declared_branches() {
        let fixture = Fixture::with_branches();
        let mut branches = local_branches(fixture.path());
        branches.sort();
        let mut expected = fixture.state.branches.clone();
        expected.sort();
        assert_eq!(branches, expected);
        assert_eq!(
            commit_count(fixture.path(), "--all"),
            fixture.state.commit_count
        );
    }

    #[test]
    fn with_merge_commit_produces_a_real_two_parent_commit() {
        let fixture = Fixture::with_merge_commit();
        let parents = git(fixture.path(), &["rev-parse", "HEAD^1", "HEAD^2"]);
        assert!(
            parents.status.success(),
            "HEAD must have two parents (a real merge commit)"
        );
        assert_eq!(
            commit_count(fixture.path(), "--all"),
            fixture.state.commit_count
        );
    }

    #[test]
    fn detached_head_is_not_on_any_branch() {
        let fixture = Fixture::detached_head();
        let symbolic = git(fixture.path(), &["symbolic-ref", "-q", "HEAD"]);
        assert!(
            !symbolic.status.success(),
            "HEAD must not resolve to a branch while detached"
        );
        assert!(fixture.state.detached_head);
        assert!(fixture.state.current_branch.is_none());
    }

    #[test]
    fn dirty_working_tree_has_both_a_modification_and_an_untracked_file() {
        let fixture = Fixture::dirty_working_tree();
        let status = git(fixture.path(), &["status", "--porcelain"]);
        let status = String::from_utf8(status.stdout).unwrap();
        assert!(status.contains(" M tracked.txt"), "status was: {status}");
        assert!(status.contains("?? untracked.txt"), "status was: {status}");
        assert!(fixture.state.dirty);
    }

    #[test]
    fn with_renamed_file_is_detected_as_a_rename_by_git_itself() {
        let fixture = Fixture::with_renamed_file();
        let log = git(
            fixture.path(),
            &["log", "-M", "--summary", "--format=", "-1"],
        );
        let log = String::from_utf8(log.stdout).unwrap();
        assert!(log.contains("rename"), "log --summary was: {log}");
        assert_eq!(
            fixture.state.renamed_files,
            vec![("old_name.txt".to_string(), "new_name.txt".to_string())]
        );
    }

    #[test]
    fn with_binary_file_is_reported_as_binary_by_git_itself() {
        let fixture = Fixture::with_binary_file();
        let diff = git(
            fixture.path(),
            &["show", "--numstat", "--format=", "HEAD"],
        );
        let diff = String::from_utf8(diff.stdout).unwrap();
        // `git show --numstat` reports `-\t-\t<path>` (dashes instead of
        // line counts) for a binary file, never fabricated textual line
        // counts.
        assert!(diff.trim().starts_with("-\t-\t"), "numstat was: {diff}");
        assert_eq!(fixture.state.binary_files, vec!["image.bin".to_string()]);
    }

    #[test]
    fn shallow_clone_only_has_one_commit_locally() {
        let fixture = Fixture::shallow_clone();
        assert!(fixture.state.shallow);
        assert_eq!(commit_count(fixture.path(), "HEAD"), 1);
        let shallow_marker = fixture.path().join(".git").join("shallow");
        assert!(
            shallow_marker.exists(),
            "a shallow clone must record its shallow boundary"
        );
    }

    #[test]
    fn bare_repo_has_no_worktree() {
        let fixture = Fixture::bare_repo();
        assert!(fixture.state.bare);
        assert!(!fixture.path().join(".git").exists(), "a bare repo has no .git subdirectory");
        assert!(fixture.path().join("HEAD").exists());
    }

    #[test]
    fn with_conflict_leaves_a_real_unresolved_merge_conflict() {
        let fixture = Fixture::with_conflict();
        let status = git(fixture.path(), &["status", "--porcelain=v2"]);
        let status = String::from_utf8(status.stdout).unwrap();
        assert!(status.contains("u "), "expected an unmerged entry, got: {status}");
        assert!(fixture.path().join(".git").join("MERGE_HEAD").exists());
        assert!(fixture.state.has_conflict);
        assert_eq!(
            fixture.state.in_progress_operation,
            Some(InProgressOperationKind::Merge)
        );
    }

    #[test]
    fn with_rebase_conflict_leaves_a_real_unresolved_rebase() {
        let fixture = Fixture::with_rebase_conflict();
        let has_rebase_merge = fixture.path().join(".git").join("rebase-merge").exists();
        let has_rebase_apply = fixture.path().join(".git").join("rebase-apply").exists();
        assert!(
            has_rebase_merge || has_rebase_apply,
            "expected on-disk rebase-in-progress state"
        );
        assert!(fixture.state.has_conflict);
        assert_eq!(
            fixture.state.in_progress_operation,
            Some(InProgressOperationKind::Rebase)
        );
    }

    /// T-252/US-119 DoD: "execuções repetidas produzem resultados
    /// equivalentes" — verified here as the strongest possible form,
    /// byte-for-byte identical commit hashes, not merely "same shape".
    #[test]
    fn repeated_construction_is_deterministic() {
        let first = Fixture::with_merge_commit();
        let second = Fixture::with_merge_commit();
        assert_eq!(
            rev_parse(first.path(), "HEAD"),
            rev_parse(second.path(), "HEAD")
        );
        assert_eq!(
            rev_parse(first.path(), "HEAD^2"),
            rev_parse(second.path(), "HEAD^2")
        );
    }

    /// Every fixture is backed by its own directory, independent of every
    /// other — building two of the same kind concurrently must never
    /// collide (US-119 DoD).
    #[test]
    fn two_fixtures_never_share_a_directory() {
        let a = Fixture::with_one_commit();
        let b = Fixture::with_one_commit();
        assert_ne!(a.path(), b.path());
    }
}
