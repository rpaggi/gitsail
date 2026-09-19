//! True snapshot tests for T-255/US-122 criterion 1 ("TUI testa update/
//! state e snapshots seletivos desde v0.2"): every other integration test
//! in this crate renders a frame and only probes it with
//! `text.contains(...)` substring checks (see `tests/smoke.rs`,
//! `tests/commit_graph.rs`, etc.) — useful, but not a snapshot in the sense
//! of "this exact frame must render to this exact text." These three tests
//! close that gap for the screens the story calls out by name: the commit
//! graph panel, the merge-conflicts overlay, and the empty-repository
//! frame — comparing a rendered [`ratatui::backend::TestBackend`] buffer
//! against a fixed, hand-written expected string via a plain `assert_eq!`,
//! no external snapshot-testing crate.
//!
//! Two determinism hazards a byte-for-byte comparison must avoid, since
//! every fixture here is a real, temporary Git repository (never a mock,
//! same convention as every other file in this directory), running on
//! whatever OS/temp-directory layout the three `rust` CI matrix legs use
//! (`docs/architecture/ci-policy.md`):
//! - **Commit hashes** are content-derived and therefore different on
//!   every run (author/committer timestamps are part of what Git hashes).
//!   The graph-panel snapshot queries the real hash right after creating
//!   the commit and substitutes a fixed placeholder for it before
//!   comparing, so the expected string itself never embeds a hash.
//! - **The Repository panel's path row** embeds the fixture's absolute
//!   temporary-directory path, which varies in both content and length
//!   across runs and operating systems. Every fixture here therefore puts
//!   its repository in a `repo/` subdirectory of the `TempDir`, so the
//!   *name* the panel shows is always the fixed string `repo`, and
//!   [`mask_repo_path`] then blanks whatever prefix of the absolute path
//!   survived truncation into a fixed run of `·` of the same width — so
//!   the assertion never depends on where `TempDir` happens to live.
//! - **The merge-conflicts overlay** is asserted only over its own popup
//!   region (computed with the same `centered_rect` math `ui.rs` uses
//!   internally), not the whole screen — the screen behind a floating
//!   overlay still renders the Graph panel, which *would* otherwise leak a
//!   real commit hash into the comparison for no reason relevant to this
//!   test.

mod support;

use gitsail_application::{GetRepositoryStatus, ListBranches, MergeResult, OpenRepository};
use gitsail_tui::{ui, Action, App, Command};
use ratatui::backend::TestBackend;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::Terminal;
use support::{buffer_text, git, init_repo_with_initial_commit, read_port, write_port, TempDir};

/// Mirrors `ui.rs`'s private `centered_rect` exactly (same constraint
/// math), so a test can locate the floating popup's rectangle without that
/// helper needing to become `pub` just for tests — this never asserts
/// anything about `centered_rect` itself, it only needs the same
/// coordinates `render_conflicts_overlay` drew into.
fn centered_rect(percent_x: u16, percent_y: u16, area: Rect) -> Rect {
    let vertical = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(area);
    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(vertical[1])[1]
}

/// Reads one rectangular region of the backend's buffer as text, one line
/// per row (no trimming — every row in a whole-frame capture is already
/// padded to the terminal's full width, and the popup capture below relies
/// on that fixed width to stay aligned with its bordered box).
fn region_text(terminal: &Terminal<TestBackend>, rect: Rect) -> String {
    let buffer = terminal.backend().buffer();
    let mut lines = Vec::new();
    for y in rect.y..rect.y + rect.height {
        let mut line = String::new();
        for x in rect.x..rect.x + rect.width {
            line.push_str(buffer[(x, y)].symbol());
        }
        lines.push(line);
    }
    lines.join("\n")
}

/// The fixed stand-in for a content-derived commit hash. Long enough to
/// cover any abbreviation length Git might choose; callers cut it to the
/// exact width of the value they are replacing.
const PLACEHOLDER_HASH: &str = "abcd1234abcd1234";

/// Blanks the fixture's absolute path wherever it appears, with a run of
/// `'·'` of the same display width — see the module doc comment for why the
/// raw path can never appear in a snapshot's expected string.
///
/// The Repository panel truncates the path to the box's inner width, so
/// what actually reaches the buffer is some *prefix* of it; this looks for
/// the longest prefix present on each line rather than the whole string.
/// Eight characters is the shortest prefix considered, below which a match
/// would be coincidence rather than the path.
fn mask_repo_path(text: &str, repo_path: &std::path::Path) -> String {
    let full = repo_path.display().to_string();
    text.lines()
        .map(|line| {
            for len in (8..=full.chars().count()).rev() {
                let prefix: String = full.chars().take(len).collect();
                if let Some(idx) = line.find(&prefix) {
                    return format!(
                        "{}{}{}",
                        &line[..idx],
                        "·".repeat(prefix.chars().count()),
                        &line[idx + prefix.len()..]
                    );
                }
            }
            line.to_string()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// A repository inside a fixed-name subdirectory of `dir`, so the
/// Repository panel's *name* row is stable across runs (see the module doc
/// comment).
fn fixture_root(dir: &TempDir) -> std::path::PathBuf {
    let root = dir.path().join("repo");
    std::fs::create_dir_all(&root).expect("create fixture root");
    root
}

#[test]
fn a_single_commit_graph_panel_frame_matches_the_recorded_snapshot() {
    let dir = TempDir::new("snapshot-graph");
    let root = fixture_root(&dir);
    init_repo_with_initial_commit(&root);

    let port = read_port();
    let (mut app, _commands) = App::new(root.clone(), port.clone(), false);
    let repo = OpenRepository::new(port.clone()).execute(&root).unwrap();
    let open_commands = app.on_repository_opened(Ok(repo.clone()));

    let ticket = open_commands
        .iter()
        .find_map(|c| match c {
            Command::RefreshStatus(t, _) => Some(*t),
            _ => None,
        })
        .expect("RefreshStatus command");
    let status = GetRepositoryStatus::new(port.clone())
        .execute(&repo)
        .unwrap();
    app.on_status_refreshed(ticket, Ok(status));

    let generation = app.session().unwrap().generation();
    let branches = ListBranches::new(port.clone()).execute(&repo).unwrap();
    app.on_branches_loaded(generation, Ok(branches));

    let graph_command = open_commands
        .into_iter()
        .find(|c| matches!(c, Command::LoadCommitGraph(_, _, _)))
        .expect("on_repository_opened must request the first commit-graph page");
    let Command::LoadCommitGraph(request_id, repo_for_graph, query) = graph_command else {
        unreachable!()
    };
    let page = gitsail_application::GetCommitHistory::new(port)
        .execute(&repo_for_graph, &query)
        .unwrap();
    app.on_commit_graph_page_loaded(request_id, Ok(page));

    let mut terminal = Terminal::new(TestBackend::new(100, 30)).unwrap();
    terminal.draw(|frame| ui::render(frame, &app)).unwrap();
    let raw = buffer_text(&terminal);

    // The two non-deterministic ingredients: the real commit hash and the
    // fixture's temp-directory path (see module doc comment).
    // Two abbreviations of the same hash reach the buffer: the Commits
    // panel prints `Commit::short_hash` (whatever width Git abbreviated to)
    // and the Branches panel its own 7-character form. Each placeholder is
    // cut to the exact width of what it replaces — a longer one would shift
    // every column to its right and make the snapshot depend on Git's
    // abbreviation length.
    let short_hash = app.graph_commits()[0].short_hash.as_str().to_string();
    let placeholder: String = PLACEHOLDER_HASH
        .chars()
        .take(short_hash.chars().count())
        .collect();
    let shorter: String = short_hash.chars().take(7).collect();
    let shorter_placeholder: String = PLACEHOLDER_HASH.chars().take(7).collect();
    let text = mask_repo_path(
        &raw.replace(&short_hash, &placeholder)
            .replace(&shorter, &shorter_placeholder),
        &root,
    );
    assert_eq!(
        text.lines().map(|l| l.chars().count()).max(),
        raw.lines().map(|l| l.chars().count()).max(),
        "masking must preserve every row's width"
    );

    let expected = "\
╭──────────────────────────╮╭ ◉ Commits ─────────────────── 1 commit ╮╭ ▣ Repository ──────────────╮
│        ╱▌ GitSail        ││▸○ abcd123 (HEAD, main) initial c… now T││▣ repo                ↳ main│
│──────────────────────────││                                        ││···························…│
│ ◉ Commits               1││                                        ││────────────────────────────│
│▸↳ Branches              1││                                        ││◉ Commits                  1│
│ ▦ Changes               0││                                        ││↳ Branches                 1│
│ ± Diff                   ││                                        ││▤ Stashes                  0│
│ ≡ Blame                  ││                                        ││⇄ Remotes                  0│
│ ▤ Stashes               0││                                        ││◆ Tags                     0│
│ ⇄ Remotes               0││                                        ││                            │
│ ◆ Tags                  0││                                        │╰────────────────────────────╯
│ ↺ Reflog                0││                                        │╭»↳ Branches ───────────── 1 ╮
│                          ││                                        ││▸● main              abcd123│
│                          ││                                        ││                            │
│                          │╰────────────────────────────────────────╯│                            │
│                          │╭ ± Diff ────────────────────────────────╮│                            │
│                          ││╭ ▦ Changes ───────────────────────────╮││                            │
│                          │││No changes.                           │││                            │
│╭ Current branch ────────╮││╰──────────────────────────────────────╯││                            │
││↳ main                 ●│││No file selected — press Enter on a     ││                            │
││────────────────────────│││Changes entry.                          ││                            │
││Working tree            │││                                        │╰────────────────────────────╯
││✓ Clean                 │││                                        │╭ ◆ Tags  (t cycles) ───── 0 ╮
│╰────────────────────────╯││                                        ││No tags.                    │
│──────────────────────────││                                        ││                            │
│ ?  Help    q  Quit       ││                                        ││                            │
╰──────────────────────────╯╰────────────────────────────────────────╯╰────────────────────────────╯
╭──╮           ╭─╮       ╭───╮        ╭─╮         ╭─╮       ╭─╮                                     
│↑↓│ Navigate  │⏎│ Open  │Tab│ Focus  │/│ Search  │?│ Help  │q│ Quit                                
╰──╯           ╰─╯       ╰───╯        ╰─╯         ╰─╯       ╰─╯                                     "
        .to_string();

    if std::env::var_os("GITSAIL_SNAPSHOT_DUMP").is_some() {
        println!("<<<GRAPH>>>\n{text}\n<<<END>>>");
    }
    assert_eq!(
        text, expected,
        "rendered graph-panel frame drifted from the recorded snapshot:\n{text}"
    );
}

#[test]
fn the_merge_conflicts_overlay_popup_matches_the_recorded_snapshot() {
    let dir = TempDir::new("snapshot-conflicts");
    let root = fixture_root(&dir);
    setup_conflicting_divergence(&root);

    let port = read_port();
    let (mut app, _commands) = App::new(root.clone(), port.clone(), false);
    open_and_load(&mut app, &root);

    select_branch(&mut app, "feature");
    app.update(Action::RequestMerge);
    let commands = app.update(Action::Activate);
    run_mutation(&mut app, commands);
    assert!(matches!(
        app.last_merge_result(),
        Some(MergeResult::Conflict { .. })
    ));

    // The merge's own "Done — press any key to dismiss" result overlay
    // takes render priority over the conflicts overlay until dismissed
    // (`ui.rs::render_overlays`'s own ordering) — a pure-state test never
    // notices this (state-only tests like `tests/merge_conflicts.rs` never
    // render a frame), but a snapshot of the actual popup must dismiss it
    // first, exactly like a real user would.
    app.update(Action::Dismiss);
    app.update(Action::ToggleConflictsPanel);
    assert!(app.conflicts_open());

    let mut terminal = Terminal::new(TestBackend::new(100, 30)).unwrap();
    terminal.draw(|frame| ui::render(frame, &app)).unwrap();
    let popup = centered_rect(70, 70, terminal.backend().buffer().area);
    let text = region_text(&terminal, popup);

    let expected = "\
╭ Conflicts ─────────────────────────────────────────────────────────╮
│merge — 1 conflicted file                                           │
│                                                                    │
│> f.txt (both modified)                                             │
│                                                                    │
│Enter inspects the highlighted file's sides.                        │
│                                                                    │
│r resolves · o/t take ours/theirs · c continues · a aborts · Esc/q  │
│closes                                                              │
│                                                                    │
│                                                                    │
│                                                                    │
│                                                                    │
│                                                                    │
│                                                                    │
│                                                                    │
│                                                                    │
│                                                                    │
│                                                                    │
│                                                                    │
╰────────────────────────────────────────────────────────────────────╯"
        .to_string();

    if std::env::var_os("GITSAIL_SNAPSHOT_DUMP").is_some() {
        println!("<<<CONFLICTS>>>\n{text}\n<<<END>>>");
    }
    assert_eq!(
        text, expected,
        "rendered conflicts-overlay popup drifted from the recorded snapshot:\n{text}"
    );
}

#[test]
fn an_empty_repository_frame_matches_the_recorded_snapshot() {
    let dir = TempDir::new("snapshot-empty");
    let root = fixture_root(&dir);
    git(&root, &["init", "--quiet", "--initial-branch=main"]);
    git(&root, &["config", "user.name", "Test User"]);
    git(&root, &["config", "user.email", "test@example.com"]);

    let port = read_port();
    let (mut app, _commands) = App::new(root.clone(), port.clone(), false);
    let repo = OpenRepository::new(port.clone()).execute(&root).unwrap();
    let open_commands = app.on_repository_opened(Ok(repo.clone()));
    let ticket = open_commands
        .iter()
        .find_map(|c| match c {
            Command::RefreshStatus(t, _) => Some(*t),
            _ => None,
        })
        .expect("RefreshStatus command");
    let status = GetRepositoryStatus::new(port).execute(&repo).unwrap();
    assert!(
        matches!(status.head_state, gitsail_domain::HeadState::Unborn),
        "a freshly initialized repository with no commits must report an unborn HEAD: {status:?}"
    );
    app.on_status_refreshed(ticket, Ok(status));
    assert_eq!(app.view_phase(), gitsail_tui::ViewPhase::Empty);

    let mut terminal = Terminal::new(TestBackend::new(100, 30)).unwrap();
    terminal.draw(|frame| ui::render(frame, &app)).unwrap();
    let text = mask_repo_path(&buffer_text(&terminal), &root);

    let expected = "\
╭──────────────────────────╮╭ ◉ Commits ────────────────── 0 commits ╮╭ ▣ Repository ──────────────╮
│        ╱▌ GitSail        ││Nothing to show — empty repository.     ││▣ repo          ↳ (detached)│
│──────────────────────────││                                        ││···························…│
│ ◉ Commits               0││                                        ││────────────────────────────│
│▸↳ Branches              0││                                        ││◉ Commits                  0│
│ ▦ Changes               0││                                        ││↳ Branches                 0│
│ ± Diff                   ││                                        ││▤ Stashes                  0│
│ ≡ Blame                  ││                                        ││⇄ Remotes                  0│
│ ▤ Stashes               0││                                        ││◆ Tags                     0│
│ ⇄ Remotes               0││                                        ││                            │
│ ◆ Tags                  0││                                        │╰────────────────────────────╯
│ ↺ Reflog                0││                                        │╭»↳ Branches ───────────── 0 ╮
│                          ││                                        ││Nothing to show — empty     │
│                          ││                                        ││repository.                 │
│                          │╰────────────────────────────────────────╯│                            │
│                          │╭ ± Diff ────────────────────────────────╮│                            │
│                          ││Nothing to show — empty repository.     ││                            │
│                          ││                                        ││                            │
│╭ Current branch ────────╮││                                        ││                            │
││↳ (detached HEAD)      ●│││                                        ││                            │
││────────────────────────│││                                        ││                            │
││Working tree            │││                                        │╰────────────────────────────╯
││(no commits yet)        │││                                        │╭ ◆ Tags  (t cycles) ───── 0 ╮
│╰────────────────────────╯││                                        ││Nothing to show — empty     │
│──────────────────────────││                                        ││repository.                 │
│ ?  Help    q  Quit       ││                                        ││                            │
╰──────────────────────────╯╰────────────────────────────────────────╯╰────────────────────────────╯
╭──╮           ╭─╮       ╭───╮        ╭─╮         ╭─╮       ╭─╮                                     
│↑↓│ Navigate  │⏎│ Open  │Tab│ Focus  │/│ Search  │?│ Help  │q│ Quit                                
╰──╯           ╰─╯       ╰───╯        ╰─╯         ╰─╯       ╰─╯                                     "
        .to_string();

    if std::env::var_os("GITSAIL_SNAPSHOT_DUMP").is_some() {
        println!("<<<EMPTY>>>\n{text}\n<<<END>>>");
    }
    assert_eq!(
        text, expected,
        "rendered empty-repository frame drifted from the recorded snapshot:\n{text}"
    );
}

// -- Shared fixture/drive-loop helpers, mirroring `tests/merge_conflicts.rs`
// exactly (see that file's own doc comments for the full rationale) --------

fn open_and_load(app: &mut App, dir: &std::path::Path) {
    let port = read_port();
    let repo = OpenRepository::new(port.clone()).execute(dir).unwrap();
    let open_commands = app.on_repository_opened(Ok(repo.clone()));
    let ticket = open_commands
        .iter()
        .find_map(|c| match c {
            Command::RefreshStatus(t, _) => Some(*t),
            _ => None,
        })
        .expect("RefreshStatus command");
    let status = GetRepositoryStatus::new(port.clone())
        .execute(&repo)
        .unwrap();
    app.on_status_refreshed(ticket, Ok(status));

    let generation = app.session().unwrap().generation();
    let branches = ListBranches::new(port.clone()).execute(&repo).unwrap();
    app.on_branches_loaded(generation, Ok(branches));
    app.on_in_progress_operation_loaded(generation, port.detect_in_progress_operation(&repo));
}

fn run_mutation(app: &mut App, commands: Vec<Command>) {
    let write = write_port();
    let read = read_port();
    let mut queue = commands;
    while let Some(command) = queue.pop() {
        let follow_up = match command {
            Command::Merge(repo, target) => {
                let result = gitsail_application::Merge::new(write.clone()).execute(&repo, &target);
                app.on_merge_finished(result)
            }
            Command::RefreshStatus(ticket, repo) => {
                let result = GetRepositoryStatus::new(read.clone()).execute(&repo);
                app.on_status_refreshed(ticket, result);
                Vec::new()
            }
            Command::LoadBranches(generation, repo) => {
                let result = ListBranches::new(read.clone()).execute(&repo);
                app.on_branches_loaded(generation, result);
                Vec::new()
            }
            Command::LoadTags(generation, repo) => {
                app.on_tags_loaded(generation, read.list_tags(&repo));
                Vec::new()
            }
            Command::LoadRemotes(generation, repo) => {
                app.on_remotes_loaded(generation, read.list_remotes(&repo));
                Vec::new()
            }
            Command::LoadStashEntries(generation, repo) => {
                app.on_stash_entries_loaded(generation, read.list_stash_entries(&repo));
                Vec::new()
            }
            Command::LoadReflog(generation, repo) => {
                app.on_reflog_loaded(generation, read.reflog(&repo));
                Vec::new()
            }
            Command::LoadInProgressOperation(generation, repo) => {
                app.on_in_progress_operation_loaded(
                    generation,
                    read.detect_in_progress_operation(&repo),
                );
                Vec::new()
            }
            other => panic!("unexpected command: {other:?}"),
        };
        queue.extend(follow_up);
    }
}

fn init_repo(dir: &std::path::Path) {
    git(dir, &["init", "--quiet", "--initial-branch=main"]);
    git(dir, &["config", "user.name", "Test User"]);
    git(dir, &["config", "user.email", "test@example.com"]);
}

/// Two branches that both modify the same line of the same file, so
/// merging one into the other reliably conflicts.
fn setup_conflicting_divergence(dir: &std::path::Path) {
    init_repo(dir);
    std::fs::write(dir.join("f.txt"), "line1\nline2\nline3\n").unwrap();
    git(dir, &["add", "-A"]);
    git(dir, &["commit", "--quiet", "-m", "base"]);

    git(dir, &["checkout", "-q", "-b", "feature"]);
    std::fs::write(dir.join("f.txt"), "line1\nCHANGED-feature\nline3\n").unwrap();
    git(dir, &["add", "-A"]);
    git(dir, &["commit", "--quiet", "-m", "feature change"]);

    git(dir, &["checkout", "-q", "main"]);
    std::fs::write(dir.join("f.txt"), "line1\nCHANGED-main\nline3\n").unwrap();
    git(dir, &["add", "-A"]);
    git(dir, &["commit", "--quiet", "-m", "main change"]);
}

fn select_branch(app: &mut App, name: &str) {
    for _ in 0..app.filtered_branches().len() {
        if app
            .filtered_branches()
            .get(app.sidebar_cursor())
            .map(|b| b.name.as_str() == name)
            .unwrap_or(false)
        {
            return;
        }
        app.update(Action::MoveDown);
    }
    panic!("branch '{name}' was never found under the sidebar cursor");
}
