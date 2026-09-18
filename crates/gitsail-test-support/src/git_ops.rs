//! Low-level `git` plumbing shared by [`crate::fixture::Fixture`] and
//! directly usable by tests that need finer control than a canned fixture
//! offers (mirrors how `tests/t230_in_progress_operation.rs` and others used
//! their own private `git`/`git_ok`/`init_repo`/`commit_all`/`rev_parse`
//! helpers before this crate existed).
//!
//! Every call here always pins `LC_ALL=C`/`LANG=C`, matching
//! `gitsail_git::GitCliProvider`'s own locale policy (SAD §11) — a fixture's
//! setup output must never depend on the host's locale either, even though
//! this setup code itself is never parsed by GitSail's adapter (it just
//! shells out directly, exactly like a real external actor would).
//!
//! # Determinism (T-252/US-119 DoD)
//!
//! Every commit-creating call here ([`commit_all`], and any caller of
//! [`git_ok_at`]) pins author/committer identity *and* date to a fixed,
//! monotonically increasing synthetic clock (see [`env_for_sequence`])
//! instead of the wall clock. Given the same sequence of calls (which every
//! [`crate::fixture::Fixture`] constructor is — no randomness, no host
//! clock), this makes the resulting commit hashes byte-for-byte identical
//! across repeated runs, not merely "equivalent in shape" — verified by
//! `fixture::tests::repeated_construction_is_deterministic`.

use std::path::Path;
use std::process::{Command, Output};
use std::sync::Arc;

use gitsail_application::{RepositoryReadPort, RepositoryWritePort};
use gitsail_domain::CommitHash;
use gitsail_git::{GitCliProvider, GitProcessRunner, GitProcessRunnerConfig};

use crate::temp_dir::TempDir;

/// Identity every fixture commits under. Not a real person — this is
/// synthetic, deterministic test data (see this module's doc).
pub const FIXTURE_AUTHOR_NAME: &str = "GitSail Fixture";
pub const FIXTURE_AUTHOR_EMAIL: &str = "fixture@gitsail.test";

/// Base of the synthetic commit clock (2023-11-14T22:13:20Z), and the fixed
/// step (seconds) between consecutive sequence numbers. Values themselves
/// are arbitrary; what matters is that they never depend on the wall clock.
const FIXTURE_EPOCH_BASE: i64 = 1_700_000_000;
const FIXTURE_COMMIT_STEP: i64 = 60;

/// `GIT_{AUTHOR,COMMITTER}_{NAME,EMAIL,DATE}` for the `sequence`-th
/// deterministic commit/merge/etc. Two calls with the same `sequence` always
/// produce the same date, so replaying the same call sequence reproduces the
/// same object hashes.
fn env_for_sequence(sequence: u64) -> [(&'static str, String); 6] {
    let timestamp = FIXTURE_EPOCH_BASE + (sequence as i64) * FIXTURE_COMMIT_STEP;
    let date = format!("{timestamp} +0000");
    [
        ("GIT_AUTHOR_NAME", FIXTURE_AUTHOR_NAME.to_string()),
        ("GIT_AUTHOR_EMAIL", FIXTURE_AUTHOR_EMAIL.to_string()),
        ("GIT_AUTHOR_DATE", date.clone()),
        ("GIT_COMMITTER_NAME", FIXTURE_AUTHOR_NAME.to_string()),
        ("GIT_COMMITTER_EMAIL", FIXTURE_AUTHOR_EMAIL.to_string()),
        ("GIT_COMMITTER_DATE", date),
    ]
}

/// Runs `git <args>` in `dir`, tolerating a non-zero exit (some fixture
/// setups, e.g. a deliberately conflicting merge, *expect* one). Test
/// fixture setup only — production code must always go through
/// `GitProcessRunner`, never a direct `Command`.
pub fn git(dir: &Path, args: &[&str]) -> Output {
    Command::new("git")
        .args(args)
        .current_dir(dir)
        .env("LC_ALL", "C")
        .env("LANG", "C")
        .output()
        .unwrap_or_else(|err| panic!("failed to spawn git {args:?} in {dir:?}: {err}"))
}

/// Like [`git`], but panics with `stderr` on a non-zero exit — for setup
/// steps that must always succeed.
pub fn git_ok(dir: &Path, args: &[&str]) {
    let output = git(dir, args);
    assert!(
        output.status.success(),
        "git {args:?} failed in {dir:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

/// Like [`git`], but with the deterministic author/committer identity and
/// date for `sequence` applied (see this module's doc) — for any invocation
/// that creates a commit object (`commit`, `merge`, `cherry-pick`,
/// `revert`, ...).
pub fn git_at(dir: &Path, args: &[&str], sequence: u64) -> Output {
    let mut command = Command::new("git");
    command
        .args(args)
        .current_dir(dir)
        .env("LC_ALL", "C")
        .env("LANG", "C");
    for (key, value) in env_for_sequence(sequence) {
        command.env(key, value);
    }
    command
        .output()
        .unwrap_or_else(|err| panic!("failed to spawn git {args:?} in {dir:?}: {err}"))
}

/// Like [`git_at`], but panics with `stderr` on a non-zero exit.
pub fn git_ok_at(dir: &Path, args: &[&str], sequence: u64) {
    let output = git_at(dir, args, sequence);
    assert!(
        output.status.success(),
        "git {args:?} failed in {dir:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

/// Initializes a fresh, non-bare repository on branch `main`, with the
/// fixture identity configured (a fallback for any raw `git` call in a
/// fixture that does not go through [`git_at`]/[`git_ok_at`], e.g. `git mv`,
/// `git checkout`). No commits yet — `HEAD` is unborn.
pub fn init_repo(label: &str) -> TempDir {
    let dir = TempDir::new(label);
    git_ok(dir.path(), &["init", "--quiet", "--initial-branch=main"]);
    git_ok(dir.path(), &["config", "user.name", FIXTURE_AUTHOR_NAME]);
    git_ok(dir.path(), &["config", "user.email", FIXTURE_AUTHOR_EMAIL]);
    dir
}

/// Writes UTF-8 text `contents` to `dir/name`, creating or overwriting it.
pub fn write_file(dir: &Path, name: &str, contents: &str) {
    std::fs::write(dir.join(name), contents)
        .unwrap_or_else(|err| panic!("failed to write {name}: {err}"));
}

/// Writes raw `contents` to `dir/name` — for fixtures whose content is
/// deliberately not valid UTF-8/text (US-119 criterion 1's "binary
/// content").
pub fn write_binary_file(dir: &Path, name: &str, contents: &[u8]) {
    std::fs::write(dir.join(name), contents)
        .unwrap_or_else(|err| panic!("failed to write {name}: {err}"));
}

/// Stages every change (`git add -A`) and commits with `message`, using the
/// deterministic identity/date for `sequence` (see this module's doc).
pub fn commit_all(dir: &Path, message: &str, sequence: u64) {
    git_ok(dir, &["add", "-A"]);
    git_ok_at(dir, &["commit", "--quiet", "-m", message], sequence);
}

/// Resolves `revision` to a [`CommitHash`], panicking if it does not
/// resolve — test setup/assertion helper only.
pub fn rev_parse(dir: &Path, revision: &str) -> CommitHash {
    let output = git(dir, &["rev-parse", revision]);
    assert!(
        output.status.success(),
        "git rev-parse {revision} failed in {dir:?}"
    );
    let hash = String::from_utf8(output.stdout)
        .expect("git rev-parse output is valid UTF-8")
        .trim()
        .to_string();
    CommitHash::new(hash).expect("git rev-parse output is a valid commit hash")
}

/// A [`GitCliProvider`] wired to the real, locally installed `git`
/// executable — the only production implementation of
/// `RepositoryReadPort`/`RepositoryWritePort` today (T-253/US-120's contract
/// suite is written against the trait, not this concrete type, so a future
/// second implementation plugs into the same suite unchanged).
pub fn provider() -> GitCliProvider {
    let runner = GitProcessRunner::new(GitProcessRunnerConfig::default())
        .expect("git must be installed to run these tests");
    GitCliProvider::new(runner)
}

/// [`provider`], boxed behind the read port trait object — convenient for
/// call sites that only need read access and want to depend on the port,
/// not the concrete adapter.
pub fn read_port() -> Arc<dyn RepositoryReadPort> {
    Arc::new(provider())
}

/// [`provider`], boxed behind the write port trait object.
pub fn write_port() -> Arc<dyn RepositoryWritePort> {
    Arc::new(provider())
}
