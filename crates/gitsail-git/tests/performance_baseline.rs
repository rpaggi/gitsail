//! Performance baseline measurements across small/medium/large synthetic
//! repositories (SAD §32; T-226/US-115 criterion "benchmark inicial";
//! T-229/US-118).
//!
//! This is a reproducible **measurement** harness, not a pass/fail
//! correctness test and not a frozen SLA (SAD §32: "No hard millisecond SLA
//! is frozen before representative benchmarks exist" — this file is how
//! those benchmarks get produced, and the numbers it prints are recorded,
//! with the environment they were measured on, in
//! `docs/architecture/performance-baseline.md`). It is `#[ignore]`d by
//! default: generating a multi-thousand-commit repository takes real
//! wall-clock time unsuitable for every `cargo test --workspace` run.
//!
//! Reproduce it with:
//!
//! ```text
//! cargo test -p gitsail-git --test performance_baseline -- --ignored --nocapture
//! ```

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use gitsail_application::{BlameRequest, CommitQuery, DiffRequest, RepositoryReadPort};
use gitsail_domain::{CancellationToken, GraphCommit};
use gitsail_git::{GitCliProvider, GitProcessRunner, GitProcessRunnerConfig};

/// A uniquely named temporary directory, removed on drop (mirrors
/// `tests/provider.rs`'s fixture helper — each integration test file in
/// this crate keeps its own copy rather than sharing one, matching the
/// existing convention here).
struct TempDir(PathBuf);

impl TempDir {
    fn new(label: &str) -> Self {
        static COUNTER: AtomicU32 = AtomicU32::new(0);
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let n = COUNTER.fetch_add(1, Ordering::SeqCst);
        let path = std::env::temp_dir().join(format!("gitsail-git-perf-{label}-{nanos}-{n}"));
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

fn provider() -> GitCliProvider {
    let runner = GitProcessRunner::new(GitProcessRunnerConfig::default())
        .expect("git must be installed to run this benchmark");
    GitCliProvider::new(runner)
}

/// Builds a linear-history fixture of `commit_count` commits, each touching
/// `files_per_commit` small files, via `git fast-import` — far faster than
/// `commit_count` real `git commit` invocations for the "large" scale this
/// harness needs (thousands of commits). The working tree is deliberately
/// left unpopulated (`fast-import` only writes objects/refs): every
/// operation this harness measures targets explicit revisions, none of
/// which need a checked-out working tree.
fn build_fixture(label: &str, commit_count: u32, files_per_commit: u32) -> TempDir {
    let dir = TempDir::new(label);
    git(dir.path(), &["init", "--quiet", "--initial-branch=main"]);
    git(dir.path(), &["config", "user.name", "Bench User"]);
    git(dir.path(), &["config", "user.email", "bench@example.com"]);

    let mut import = String::new();
    for i in 0..commit_count {
        let message = format!("commit {i}");
        import.push_str("commit refs/heads/main\n");
        import.push_str(&format!("mark :{}\n", i + 1));
        import.push_str(&format!(
            "committer Bench User <bench@example.com> {} +0000\n",
            1_700_000_000 + i as i64
        ));
        import.push_str(&format!("data {}\n{}\n", message.len(), message));
        if i > 0 {
            import.push_str(&format!("from :{i}\n"));
        }
        for f in 0..files_per_commit {
            let content = format!("content for file {f} at commit {i}\n");
            import.push_str(&format!("M 100644 inline file{f}.txt\n"));
            import.push_str(&format!("data {}\n{}", content.len(), content));
        }
        import.push('\n');
    }

    let mut child = Command::new("git")
        .args(["fast-import", "--quiet"])
        .current_dir(dir.path())
        .env("LC_ALL", "C")
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn git fast-import");
    child
        .stdin
        .take()
        .unwrap()
        .write_all(import.as_bytes())
        .unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "git fast-import failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    dir
}

/// Best-effort resident memory (Linux `/proc/self/status` `VmRSS`), in
/// kilobytes. `None` off Linux, or if the file is ever unreadable — this is
/// a "rough" measurement (explicitly allowed: SAD/T-229 asks for "uso de
/// memória de forma grosseira"), reflecting this whole test process's
/// cumulative usage, not one isolated operation.
fn resident_memory_kb() -> Option<u64> {
    #[cfg(target_os = "linux")]
    {
        let status = std::fs::read_to_string("/proc/self/status").ok()?;
        for line in status.lines() {
            if let Some(rest) = line.strip_prefix("VmRSS:") {
                return rest.trim().trim_end_matches(" kB").trim().parse().ok();
            }
        }
        None
    }
    #[cfg(not(target_os = "linux"))]
    {
        None
    }
}

struct ScaleReport {
    label: &'static str,
    commit_count: u32,
    files_per_commit: u32,
    first_page_log: Duration,
    first_page_graph: Duration,
    full_history_diff: Duration,
    blame_hot_file: Duration,
    cancel_diff: Duration,
    resident_kb_before: Option<u64>,
    resident_kb_after: Option<u64>,
}

fn measure_scale(label: &'static str, commit_count: u32, files_per_commit: u32) -> ScaleReport {
    let resident_kb_before = resident_memory_kb();
    let repo_dir = build_fixture(label, commit_count, files_per_commit);
    let provider = provider();
    let repo = RepositoryReadPort::discover(&provider, repo_dir.path()).expect("discover fixture");

    // 1. First page of history (T-226/US-115 criterion 1: must not
    // materialize full history — `GitCliProvider::commits` already passes
    // `--skip`/`-n` to `git log` rather than fetching everything).
    let start = Instant::now();
    let page = RepositoryReadPort::commits(&provider, &repo, &CommitQuery::default())
        .expect("first page of commits");
    let first_page_log = start.elapsed();
    assert!(
        page.items.len() <= 50,
        "the default first page must stay bounded regardless of total history size"
    );

    // 2. First page of the commit graph, built from that same page (US-064/
    // US-065's incremental `CommitGraph::append_page`).
    let start = Instant::now();
    let graph_commits: Vec<GraphCommit> = page.items.iter().map(GraphCommit::from).collect();
    let mut graph = gitsail_domain::CommitGraph::new();
    graph.append_page(&graph_commits);
    let first_page_graph = start.elapsed();
    assert_eq!(graph.rows().len(), graph_commits.len());

    // 3. Diff spanning the entire history (first commit -> HEAD): the
    // heaviest diff this fixture can produce, deliberately never cancelled.
    let head = RepositoryReadPort::resolve_revision(&provider, &repo, "HEAD").unwrap();
    // `main~<n>` addresses the oldest commit directly, avoiding a full log
    // walk just to find it.
    let oldest_expr = if commit_count > 1 {
        format!("main~{}", commit_count - 1)
    } else {
        "HEAD".to_string()
    };
    let oldest = RepositoryReadPort::resolve_revision(&provider, &repo, &oldest_expr).unwrap();
    let start = Instant::now();
    let diff = RepositoryReadPort::diff(
        &provider,
        &repo,
        &DiffRequest {
            from: Some(oldest),
            to: Some(head.clone()),
            staged: false,
            path_filter: None,
            context_lines: Some(3),
        },
        &CancellationToken::new(),
    )
    .expect("full-history diff");
    let full_history_diff = start.elapsed();
    assert!(!diff.files.is_empty());

    // 4. Blame of the file every commit touches (the worst case: as many
    // distinct attributions as commits).
    let start = Instant::now();
    let _blame = RepositoryReadPort::blame(
        &provider,
        &repo,
        &BlameRequest {
            file: PathBuf::from("file0.txt"),
            revision: Some(head.clone()),
            line_range: None,
            buffer_contents: None,
        },
        &CancellationToken::new(),
    )
    .expect("blame the hot file");
    let blame_hot_file = start.elapsed();

    // 5. Cancellation responsiveness (T-226/US-115 criterion 2/DoD: "um
    // teste comprova cancelamento responsivo"): a pre-cancelled diff request
    // over the same full-history span must return promptly with a distinct
    // `Cancelled` error, never a slow full computation and never a
    // truncated-but-`Ok` result mistaken for success.
    let cancel = CancellationToken::new();
    cancel.cancel();
    let start = Instant::now();
    let cancelled = RepositoryReadPort::diff(
        &provider,
        &repo,
        &DiffRequest {
            from: Some(RepositoryReadPort::resolve_revision(&provider, &repo, "HEAD").unwrap()),
            to: None,
            staged: false,
            path_filter: None,
            context_lines: Some(3),
        },
        &cancel,
    );
    let cancel_diff = start.elapsed();
    assert_eq!(
        cancelled.unwrap_err().code(),
        gitsail_domain::ErrorCode::Cancelled,
        "a cancelled diff must surface as a distinct error, never a partial success"
    );

    let resident_kb_after = resident_memory_kb();

    ScaleReport {
        label,
        commit_count,
        files_per_commit,
        first_page_log,
        first_page_graph,
        full_history_diff,
        blame_hot_file,
        cancel_diff,
        resident_kb_before,
        resident_kb_after,
    }
}

/// Runs the small/medium/large scales and prints a table (run with
/// `--nocapture` to see it). See the module doc for the reproduction
/// command; results from one representative run are recorded in
/// `docs/architecture/performance-baseline.md`.
#[test]
#[ignore = "generates multi-thousand-commit fixtures; run explicitly with --ignored --nocapture"]
fn measures_log_graph_diff_and_blame_across_scales() {
    let scales: &[(&str, u32, u32)] =
        &[("small", 100, 3), ("medium", 1_000, 3), ("large", 5_000, 3)];

    println!(
        "\n{:<8} {:>8} {:>6} {:>14} {:>14} {:>16} {:>14} {:>14} {:>14} {:>14}",
        "scale",
        "commits",
        "files",
        "first_page_log",
        "first_page_graph",
        "full_history_diff",
        "blame_hot_file",
        "cancel_diff",
        "rss_before_kb",
        "rss_after_kb"
    );
    for (label, commit_count, files_per_commit) in scales.iter().copied() {
        let report = measure_scale(label, commit_count, files_per_commit);
        println!(
            "{:<8} {:>8} {:>6} {:>14?} {:>14?} {:>16?} {:>14?} {:>14?} {:>14} {:>14}",
            report.label,
            report.commit_count,
            report.files_per_commit,
            report.first_page_log,
            report.first_page_graph,
            report.full_history_diff,
            report.blame_hot_file,
            report.cancel_diff,
            report
                .resident_kb_before
                .map(|kb| kb.to_string())
                .unwrap_or_else(|| "n/a".to_string()),
            report
                .resident_kb_after
                .map(|kb| kb.to_string())
                .unwrap_or_else(|| "n/a".to_string()),
        );
    }
}
