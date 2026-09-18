//! Integration tests for US-066 ("Renderizar graph na TUI") against a real,
//! temporary Git repository via `GitCliProvider` (never a mock), following
//! `tests/status_and_diff.rs`'s fixture convention.

mod support;

use gitsail_application::{CommitQuery, GetCommitHistory, GetRepositoryStatus};
use gitsail_tui::{ui, Action, App, Command, Panel};
use ratatui::backend::TestBackend;
use ratatui::Terminal;
use support::{buffer_text, git, init_repo_with_initial_commit, read_port, TempDir};

/// Opens `dir`, drives `app` to the `Loaded` phase (status refreshed, like
/// `tests/status_and_diff.rs`'s `open_and_load`), and returns the
/// `LoadCommitGraph` command `on_repository_opened` issued (US-065,
/// US-066's initial page load) so a test can run it against the real port.
fn open_and_capture_graph_command(app: &mut App, dir: &std::path::Path) -> Command {
    let port = read_port();
    let repo = gitsail_application::OpenRepository::new(port.clone())
        .execute(dir)
        .unwrap();
    let commands = app.on_repository_opened(Ok(repo.clone()));

    let ticket = commands
        .iter()
        .find_map(|c| match c {
            Command::RefreshStatus(t, _) => Some(*t),
            _ => None,
        })
        .expect("RefreshStatus command");
    let status = GetRepositoryStatus::new(port).execute(&repo).unwrap();
    app.on_status_refreshed(ticket, Ok(status));

    commands
        .into_iter()
        .find(|c| matches!(c, Command::LoadCommitGraph(_, _, _)))
        .expect("on_repository_opened must request the first commit-graph page")
}

fn run_graph_command(app: &mut App, command: Command) {
    let Command::LoadCommitGraph(request_id, repo, query) = command else {
        panic!("expected a LoadCommitGraph command");
    };
    let result = GetCommitHistory::new(read_port()).execute(&repo, &query);
    app.on_commit_graph_page_loaded(request_id, result);
}

fn focus_graph(app: &mut App) {
    app.update(Action::FocusNext); // Sidebar -> Graph
    assert_eq!(app.focus(), Panel::Graph);
}

fn render(app: &App) -> String {
    let mut terminal = Terminal::new(TestBackend::new(140, 30)).unwrap();
    terminal.draw(|frame| ui::render(frame, app)).unwrap();
    buffer_text(&terminal)
}

/// Creates a merge commit: a `feature` branch diverges from `main`, `main`
/// gets its own commit, then `feature` is merged back — the smallest DAG
/// that exercises a second lane and a real convergence.
fn init_repo_with_a_merge(dir: &std::path::Path) {
    init_repo_with_initial_commit(dir);
    git(dir, &["checkout", "--quiet", "-b", "feature"]);
    std::fs::write(dir.join("feature.txt"), "feature work\n").unwrap();
    git(dir, &["add", "feature.txt"]);
    git(dir, &["commit", "--quiet", "-m", "feature commit"]);
    git(dir, &["checkout", "--quiet", "main"]);
    std::fs::write(dir.join("main.txt"), "main work\n").unwrap();
    git(dir, &["add", "main.txt"]);
    git(dir, &["commit", "--quiet", "-m", "main commit"]);
    git(
        dir,
        &[
            "merge",
            "--quiet",
            "--no-ff",
            "-m",
            "merge feature",
            "feature",
        ],
    );
}

#[test]
fn opening_a_repository_loads_the_commit_graph_and_associates_rows_with_commits() {
    let dir = TempDir::new("graph-basic");
    init_repo_with_initial_commit(dir.path());

    let (mut app, _commands) = App::new(dir.path().to_path_buf(), read_port(), false);
    let graph_command = open_and_capture_graph_command(&mut app, dir.path());
    run_graph_command(&mut app, graph_command);

    assert_eq!(
        app.commit_graph().rows().len(),
        1,
        "the single initial commit must be loaded"
    );
    assert_eq!(app.graph_commits().len(), 1);
    assert_eq!(
        app.commit_graph().rows()[0].commit,
        app.graph_commits()[0].hash
    );
    assert!(!app.graph_has_more());
    assert!(app.graph_error().is_none());

    let text = render(&app);
    assert!(
        text.contains("initial commit"),
        "the commit's subject must be visible in the rendered graph panel:\n{text}"
    );
}

#[test]
fn a_merge_commit_opens_a_second_lane_and_renders_a_legible_connector() {
    let dir = TempDir::new("graph-merge");
    init_repo_with_a_merge(dir.path());

    let (mut app, _commands) = App::new(dir.path().to_path_buf(), read_port(), false);
    let graph_command = open_and_capture_graph_command(&mut app, dir.path());
    run_graph_command(&mut app, graph_command);

    assert!(
        app.commit_graph().lane_count() >= 2,
        "a merge must open a second lane: {:?}",
        app.commit_graph().rows()
    );
    assert!(
        app.commit_graph().rows().iter().any(|r| r.lane > 0),
        "at least one row must sit on a non-mainline lane"
    );

    let text = render(&app);
    assert!(
        text.contains("merge feature"),
        "the merge commit's subject must be visible:\n{text}"
    );
    // Legibility must not depend on color alone (US-066 criterion 2): a
    // lane-changing connector is drawn as a distinct glyph in plain text.
    assert!(
        text.contains('\\') || text.contains('/'),
        "a merge/branch point must be shown with a connector glyph, not color alone:\n{text}"
    );
}

#[test]
fn scrolling_to_the_last_loaded_row_does_not_fetch_more_once_history_is_exhausted() {
    let dir = TempDir::new("graph-scroll-end");
    init_repo_with_a_merge(dir.path());

    let (mut app, _commands) = App::new(dir.path().to_path_buf(), read_port(), false);
    let graph_command = open_and_capture_graph_command(&mut app, dir.path());
    run_graph_command(&mut app, graph_command);
    assert!(
        !app.graph_has_more(),
        "the fixture repository is far smaller than one page"
    );

    focus_graph(&mut app);
    let row_count = app.commit_graph().rows().len();
    assert!(
        row_count >= 3,
        "the merge fixture has at least three commits"
    );

    let mut extra_commands = Vec::new();
    for _ in 0..row_count + 2 {
        extra_commands.extend(app.update(Action::MoveDown));
    }

    assert_eq!(
        app.graph_cursor(),
        row_count - 1,
        "the cursor clamps at the last loaded row"
    );
    assert!(
        extra_commands.is_empty(),
        "no further page must be requested once has_more is false: {extra_commands:?}"
    );
}

#[test]
fn appending_a_second_page_resolves_the_first_pages_continuation_and_preserves_its_row() {
    let dir = TempDir::new("graph-pagination");
    init_repo_with_a_merge(dir.path());

    let (mut app, _commands) = App::new(dir.path().to_path_buf(), read_port(), false);
    let port = read_port();
    let repo = gitsail_application::OpenRepository::new(port.clone())
        .execute(dir.path())
        .unwrap();
    let commands = app.on_repository_opened(Ok(repo.clone()));
    let request_id = commands
        .iter()
        .find_map(|c| match c {
            Command::LoadCommitGraph(id, _, _) => Some(*id),
            _ => None,
        })
        .expect("an initial LoadCommitGraph command must be issued");

    // Load the very first page with a deliberately tiny limit so the
    // fixture's small history still exercises a real page boundary
    // (US-065), using the real adapter rather than a synthetic fixture.
    let first_query = CommitQuery {
        limit: Some(1),
        ..CommitQuery::default()
    };
    let first_page = GetCommitHistory::new(port.clone())
        .execute(&repo, &first_query)
        .unwrap();
    assert!(
        first_page.has_more,
        "the merge fixture has more than one commit"
    );
    app.on_commit_graph_page_loaded(request_id, Ok(first_page.clone()));

    let selected_hash = app.commit_graph().rows()[0].commit.clone();
    let row_index_before = app.commit_graph().row_index_of(&selected_hash).unwrap();
    let lane_before = app.commit_graph().rows()[row_index_before].lane;
    // The merge/root commit's own parent has not loaded yet — exactly the
    // continuation case US-065 criterion 1 describes.
    if !app.commit_graph().rows()[row_index_before].edges.is_empty() {
        assert!(
            app.commit_graph().rows()[row_index_before]
                .edges
                .iter()
                .any(|e| !e.resolved),
            "a parent outside the loaded page must be an unresolved continuation"
        );
    }

    let second_query = CommitQuery {
        limit: Some(10),
        cursor: first_page.next_cursor.clone(),
        ..CommitQuery::default()
    };
    let second_page = GetCommitHistory::new(port)
        .execute(&repo, &second_query)
        .unwrap();
    app.on_commit_graph_page_loaded(request_id, Ok(second_page));

    assert_eq!(
        app.commit_graph().row_index_of(&selected_hash),
        Some(row_index_before),
        "a selection tracked by hash must resolve to the same row after a page is appended"
    );
    assert_eq!(
        app.commit_graph().rows()[row_index_before].lane,
        lane_before
    );
    assert_eq!(
        app.graph_commits().len(),
        app.commit_graph().rows().len(),
        "commit metadata must stay index-aligned with the graph rows across pages"
    );
}

/// T-255/US-122 criterion 3 ("conteúdo malicioso"): `tests/status_and_diff.rs`
/// already covers a raw escape byte in a *file name*, but nothing exercised
/// a hostile *commit subject* reaching the Graph panel — repository content
/// an attacker fully controls (unlike a file name, a commit subject never
/// needs to be a valid path, so nothing about creating one is unusual). A
/// terminal control byte in a commit message is plain UTF-8 (unlike a raw
/// non-UTF-8 file name), so this can go through `support::git` directly.
#[test]
fn a_malicious_commit_subject_is_sanitized_before_rendering_in_the_graph_panel() {
    let dir = TempDir::new("graph-malicious-subject");
    init_repo_with_initial_commit(dir.path());
    std::fs::write(dir.path().join("a.txt"), "content\n").unwrap();
    git(dir.path(), &["add", "a.txt"]);
    let malicious_subject = "evil\u{1b}[31mred\u{7}alert subject";
    git(dir.path(), &["commit", "--quiet", "-m", malicious_subject]);

    let (mut app, _commands) = App::new(dir.path().to_path_buf(), read_port(), false);
    let graph_command = open_and_capture_graph_command(&mut app, dir.path());
    run_graph_command(&mut app, graph_command);

    let text = render(&app);
    assert!(
        !text.contains('\u{1b}') && !text.contains('\u{7}'),
        "a raw escape/bell byte from a repository-controlled commit subject must never reach the terminal:\n{text:?}"
    );
    assert!(
        text.contains("red") && text.contains("alert subject"),
        "the rest of the (sanitized) commit subject must still be visible:\n{text}"
    );
}
