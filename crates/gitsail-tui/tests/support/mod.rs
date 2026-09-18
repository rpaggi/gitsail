//! Shared fixture helpers for `gitsail-tui`'s integration tests. Every test
//! drives a real, temporary Git repository via the `git` CLI (never a
//! mock), against the real [`GitCliProvider`] adapter — the same
//! convention `gitsail-git`'s and `gitsail-cli`'s integration tests use.
//!
//! Not every test binary in `tests/` uses every helper here, so unused ones
//! in a given binary are expected rather than a sign of dead code.
#![allow(dead_code)]

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use gitsail_application::{RepositoryReadPort, RepositoryWritePort};
use gitsail_git::{GitCliProvider, GitProcessRunner, GitProcessRunnerConfig};
use ratatui::backend::TestBackend;
use ratatui::Terminal;

pub struct TempDir(PathBuf);

impl TempDir {
    pub fn new(label: &str) -> Self {
        static COUNTER: AtomicU32 = AtomicU32::new(0);
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let n = COUNTER.fetch_add(1, Ordering::SeqCst);
        let path = std::env::temp_dir().join(format!("gitsail-tui-{label}-{nanos}-{n}"));
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

pub fn git(dir: &Path, args: &[&str]) {
    let status = Command::new("git")
        .args(args)
        .current_dir(dir)
        .env("LC_ALL", "C")
        .env("LANG", "C")
        .status()
        .unwrap_or_else(|e| panic!("failed to spawn git {args:?}: {e}"));
    assert!(status.success(), "git {args:?} failed in {dir:?}");
}

/// A repository with an initial commit, ready for tests that need existing
/// history to modify/stage/branch from.
pub fn init_repo_with_initial_commit(dir: &Path) {
    git(dir, &["init", "--quiet", "--initial-branch=main"]);
    git(dir, &["config", "user.name", "Test User"]);
    git(dir, &["config", "user.email", "test@example.com"]);
    std::fs::write(dir.join("README.md"), "hello\n").unwrap();
    git(dir, &["add", "README.md"]);
    git(dir, &["commit", "--quiet", "-m", "initial commit"]);
}

/// A bare repository standing in for a real remote (T-182's DoD: "E2E com
/// remote fixture", mirroring `gitsail-git`'s own EPIC-19 integration tests
/// — never a real network). Bare because a non-bare repository refuses a
/// push to its checked-out branch by default; a real Git hosting service's
/// repositories are bare for the same reason.
pub fn init_bare_remote(label: &str) -> TempDir {
    let dir = TempDir::new(label);
    git(
        dir.path(),
        &["init", "--quiet", "--bare", "--initial-branch=main"],
    );
    dir
}

/// Clones `remote` into a fresh temporary directory, configuring a test
/// identity so commits made in the clone succeed.
pub fn clone_repo(remote: &Path, label: &str) -> TempDir {
    let dir = TempDir::new(label);
    git(
        dir.path().parent().unwrap(),
        &[
            "clone",
            "--quiet",
            "--",
            remote.to_str().unwrap(),
            dir.path().to_str().unwrap(),
        ],
    );
    git(dir.path(), &["config", "user.name", "Test User"]);
    git(dir.path(), &["config", "user.email", "test@example.com"]);
    dir
}

pub fn read_port() -> Arc<dyn RepositoryReadPort> {
    let runner = GitProcessRunner::new(GitProcessRunnerConfig::default()).expect("git runner");
    Arc::new(GitCliProvider::new(runner))
}

pub fn write_port() -> Arc<dyn RepositoryWritePort> {
    let runner = GitProcessRunner::new(GitProcessRunnerConfig::default()).expect("git runner");
    Arc::new(GitCliProvider::new(runner))
}

pub fn buffer_text(terminal: &Terminal<TestBackend>) -> String {
    let buffer = terminal.backend().buffer();
    let mut lines = Vec::new();
    for y in 0..buffer.area.height {
        let mut line = String::new();
        for x in 0..buffer.area.width {
            line.push_str(buffer[(x, y)].symbol());
        }
        lines.push(line);
    }
    lines.join("\n")
}
