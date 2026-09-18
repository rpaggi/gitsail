//! Integration tests for US-045 ("Explorar histórico e detalhes na TUI")
//! against a real, temporary Git repository via `GitCliProvider` (never a
//! mock), following `tests/commit_graph.rs`'s fixture convention — the same
//! branch-plus-merge fixture, reused here for search and details.

mod support;

use gitsail_application::{CommitQuery, GetCommitHistory, GetRepositoryStatus};
use gitsail_tui::{ui, Action, App, Command, Panel};
use ratatui::backend::TestBackend;
use ratatui::Terminal;
use support::{buffer_text, git, init_repo_with_initial_commit, read_port, TempDir};

/// Creates a merge commit: a `feature` branch diverges from `main`, `main`
/// gets its own commit, then `feature` is merged back — the smallest DAG
/// that exercises a second lane and a real convergence (mirrors
/// `tests/commit_graph.rs::init_repo_with_a_merge`).
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

/// Opens `dir`, drives `app` to the `Loaded` phase, and runs every
/// `LoadCommitGraph` command outstanding so far against the real adapter —
/// mirrors `tests/commit_graph.rs`'s `open_and_capture_graph_command`, but
/// drains *all* graph commands (a search restarts the graph before the app
/// even reaches this helper in some tests) rather than assuming exactly one.
fn open_and_load(app: &mut App, dir: &std::path::Path) {
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
    let status = GetRepositoryStatus::new(port.clone())
        .execute(&repo)
        .unwrap();
    app.on_status_refreshed(ticket, Ok(status));

    for command in commands {
        run_graph_command(app, port.clone(), command);
    }
}

fn run_graph_command(
    app: &mut App,
    port: std::sync::Arc<dyn gitsail_application::RepositoryReadPort>,
    command: Command,
) {
    if let Command::LoadCommitGraph(request_id, repo, query) = command {
        let result = GetCommitHistory::new(port).execute(&repo, &query);
        app.on_commit_graph_page_loaded(request_id, result);
    }
}

/// Submits `text` as a commit search from the Graph panel and runs the
/// resulting `LoadCommitGraph` command against the real adapter, exactly
/// like the main loop would once `worker::dispatch` reports its result back.
fn submit_search(app: &mut App, dir: &std::path::Path, text: &str) {
    let port = read_port();
    let repo = gitsail_application::OpenRepository::new(port.clone())
        .execute(dir)
        .unwrap();

    app.update(Action::StartSearch);
    // `StartSearch` pre-fills the box with whatever filter is already
    // active (so re-opening search to edit it does not lose the previous
    // text) — clear that out first, exactly like backspacing through it,
    // before typing this call's own query.
    while app.commit_search().is_some_and(|s| !s.is_empty()) {
        app.update(Action::CommitSearchBackspace);
    }
    for c in text.chars() {
        app.update(Action::CommitSearchInput(c));
    }
    let commands = app.update(Action::CommitSearchSubmit);
    let (request_id, query) = match commands.as_slice() {
        [Command::LoadCommitGraph(id, _, q)] => (*id, q.clone()),
        other => panic!("expected exactly one LoadCommitGraph command, got {other:?}"),
    };
    let result = GetCommitHistory::new(port).execute(&repo, &query);
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

// -- Criterion 1: the graph (the TUI's only commit list) keeps a stable
// selection by hash across pagination and across a search's fresh load. ----

#[test]
fn the_graph_cursor_keeps_pointing_at_the_same_commit_hash_across_a_page_boundary() {
    let dir = TempDir::new("history-selection-pagination");
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

    focus_graph(&mut app);
    let selected_hash = app.selected_graph_commit().unwrap().hash.clone();

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
        app.selected_graph_commit().unwrap().hash,
        selected_hash,
        "the cursor must still point at the same commit hash after a page is appended"
    );
}

// -- Criterion 2: search restarts the paginated query without ever blocking
// navigation, and each of message/author/branch/hash resolves through the
// existing `CommitQuery` filters (US-017) against a real Git process. ------

#[test]
fn a_message_search_narrows_the_graph_to_matching_commits() {
    let dir = TempDir::new("history-search-message");
    init_repo_with_a_merge(dir.path());
    let (mut app, _commands) = App::new(dir.path().to_path_buf(), read_port(), false);
    open_and_load(&mut app, dir.path());
    focus_graph(&mut app);

    let full_count = app.commit_graph().rows().len();
    assert!(
        full_count >= 4,
        "the merge fixture has at least four commits"
    );

    submit_search(&mut app, dir.path(), "feature commit");

    assert_eq!(
        app.active_commit_filter(),
        Some("feature commit"),
        "the submitted text must be shown as the active filter"
    );
    assert_eq!(
        app.commit_graph().rows().len(),
        1,
        "only the matching commit must remain"
    );
    assert_eq!(app.graph_commits()[0].subject, "feature commit");
    assert_eq!(
        app.graph_cursor(),
        0,
        "a new search result starts with the selection at the top"
    );

    let text = render(&app);
    assert!(
        text.contains("filter: feature commit"),
        "the active filter must be visible in the Graph panel's title:\n{text}"
    );
}

#[test]
fn an_author_search_uses_the_author_prefix() {
    let dir = TempDir::new("history-search-author");
    init_repo_with_initial_commit(dir.path());
    git(dir.path(), &["config", "user.name", "Ada Lovelace"]);
    git(dir.path(), &["config", "user.email", "ada@example.com"]);
    std::fs::write(dir.path().join("ada.txt"), "hello\n").unwrap();
    git(dir.path(), &["add", "ada.txt"]);
    git(dir.path(), &["commit", "--quiet", "-m", "ada's commit"]);

    let (mut app, _commands) = App::new(dir.path().to_path_buf(), read_port(), false);
    open_and_load(&mut app, dir.path());
    focus_graph(&mut app);

    submit_search(&mut app, dir.path(), "author:Ada");

    assert_eq!(app.commit_graph().rows().len(), 1);
    assert_eq!(app.graph_commits()[0].author.name, "Ada Lovelace");
}

#[test]
fn a_branch_search_uses_the_branch_prefix() {
    let dir = TempDir::new("history-search-branch");
    init_repo_with_a_merge(dir.path());
    // `feature` still has its own commit even after being merged into
    // `main` — searching `branch:feature` must resolve to that branch's
    // own history, not whatever the graph happened to show before.
    let (mut app, _commands) = App::new(dir.path().to_path_buf(), read_port(), false);
    open_and_load(&mut app, dir.path());
    focus_graph(&mut app);

    submit_search(&mut app, dir.path(), "branch:feature");

    assert!(
        app.graph_commits()
            .iter()
            .any(|c| c.subject == "feature commit"),
        "branch:feature must include that branch's own commit: {:?}",
        app.graph_commits()
    );
    assert!(
        !app.graph_commits()
            .iter()
            .any(|c| c.subject == "merge feature"),
        "branch:feature must not include a commit only reachable from main: {:?}",
        app.graph_commits()
    );
}

#[test]
fn a_hash_search_jumps_the_graph_to_that_commit() {
    let dir = TempDir::new("history-search-hash");
    init_repo_with_a_merge(dir.path());
    let (mut app, _commands) = App::new(dir.path().to_path_buf(), read_port(), false);
    open_and_load(&mut app, dir.path());
    focus_graph(&mut app);

    let target = app
        .graph_commits()
        .iter()
        .find(|c| c.subject == "feature commit")
        .unwrap()
        .hash
        .clone();

    submit_search(&mut app, dir.path(), target.as_str());

    assert_eq!(
        app.commit_graph().rows()[0].commit,
        target,
        "a hash search must land on that exact commit first"
    );
}

#[test]
fn clearing_a_search_with_an_empty_submission_returns_to_the_unfiltered_log() {
    let dir = TempDir::new("history-search-clear");
    init_repo_with_a_merge(dir.path());
    let (mut app, _commands) = App::new(dir.path().to_path_buf(), read_port(), false);
    open_and_load(&mut app, dir.path());
    focus_graph(&mut app);
    let full_count = app.commit_graph().rows().len();

    submit_search(&mut app, dir.path(), "feature commit");
    assert_eq!(app.commit_graph().rows().len(), 1);

    submit_search(&mut app, dir.path(), "");
    assert!(
        app.active_commit_filter().is_none(),
        "an empty submission must clear the active filter"
    );
    assert_eq!(
        app.commit_graph().rows().len(),
        full_count,
        "clearing the search must restore the full history"
    );
}

#[test]
fn starting_a_commit_search_never_blocks_navigation_of_the_still_loaded_graph() {
    let dir = TempDir::new("history-search-nonblocking");
    init_repo_with_a_merge(dir.path());
    let (mut app, _commands) = App::new(dir.path().to_path_buf(), read_port(), false);
    open_and_load(&mut app, dir.path());
    focus_graph(&mut app);

    // Opening the search box and typing into it must never itself dispatch
    // a `Command` (US-045 criterion 2's "sem travar navegação") — only the
    // final `CommitSearchSubmit` does, and until then the previously loaded
    // graph (and its cursor) stay exactly as they were, fully navigable.
    let open_commands = app.update(Action::StartSearch);
    assert!(open_commands.is_empty());
    for c in "abc".chars() {
        let commands = app.update(Action::CommitSearchInput(c));
        assert!(
            commands.is_empty(),
            "typing into the search box must not dispatch anything"
        );
    }
    assert_eq!(app.commit_search(), Some("abc"));
    assert!(
        app.commit_graph().rows().len() >= 4,
        "the graph loaded before search must be untouched"
    );
}

// -- Criterion 3: Enter opens commit details with hash, author, dates, and
// the full message. -------------------------------------------------------

#[test]
fn enter_on_the_graph_opens_commit_details_with_hash_author_and_message() {
    let dir = TempDir::new("history-details");
    init_repo_with_initial_commit(dir.path());
    // A multi-line body, to prove the *complete* message is shown, not just
    // the subject line.
    git(
        dir.path(),
        &[
            "commit",
            "--quiet",
            "--allow-empty",
            "-m",
            "add feature\n\nDetailed body line.",
        ],
    );

    let (mut app, _commands) = App::new(dir.path().to_path_buf(), read_port(), false);
    open_and_load(&mut app, dir.path());
    focus_graph(&mut app);

    assert!(!app.commit_details_open());
    let commands = app.update(Action::Activate);
    assert!(
        commands.is_empty(),
        "opening details reuses already-loaded data, no Command needed"
    );
    assert!(app.commit_details_open());

    let commit = app.selected_graph_commit().unwrap().clone();
    assert_eq!(commit.subject, "add feature");
    assert_eq!(commit.body.trim(), "Detailed body line.");

    let text = render(&app);
    assert!(
        text.contains("Commit Details"),
        "the overlay must be visible:\n{text}"
    );
    assert!(
        text.contains(commit.hash.as_str()),
        "the full hash must be shown:\n{text}"
    );
    assert!(
        text.contains(&commit.author.name),
        "the author's name must be shown:\n{text}"
    );
    assert!(
        text.contains(&commit.author.email),
        "the author's email must be shown:\n{text}"
    );
    assert!(
        text.contains("add feature"),
        "the subject must be shown:\n{text}"
    );
    assert!(
        text.contains("Detailed body line."),
        "the full body must be shown:\n{text}"
    );

    // Esc closes the overlay again without touching anything else.
    app.update(Action::Dismiss);
    assert!(!app.commit_details_open());
}

#[test]
fn commit_details_shows_the_committer_separately_when_it_differs_from_the_author() {
    let dir = TempDir::new("history-details-committer");
    init_repo_with_initial_commit(dir.path());
    std::fs::write(dir.path().join("cherry.txt"), "picked\n").unwrap();
    git(dir.path(), &["add", "cherry.txt"]);
    // `GIT_COMMITTER_*` differing from `user.name`/`user.email` is the same
    // real-world case a rebase/cherry-pick/`am` produces.
    let status = std::process::Command::new("git")
        .args(["commit", "--quiet", "-m", "authored by someone else"])
        .current_dir(dir.path())
        .env("LC_ALL", "C")
        .env("LANG", "C")
        .env("GIT_AUTHOR_NAME", "Ada Lovelace")
        .env("GIT_AUTHOR_EMAIL", "ada@example.com")
        .env("GIT_COMMITTER_NAME", "Grace Hopper")
        .env("GIT_COMMITTER_EMAIL", "grace@example.com")
        .status()
        .unwrap();
    assert!(status.success());

    let (mut app, _commands) = App::new(dir.path().to_path_buf(), read_port(), false);
    open_and_load(&mut app, dir.path());
    focus_graph(&mut app);
    app.update(Action::Activate);

    let text = render(&app);
    assert!(
        text.contains("Ada Lovelace"),
        "the author must be shown:\n{text}"
    );
    assert!(
        text.contains("Grace Hopper"),
        "a differing committer must be shown:\n{text}"
    );
}

#[test]
fn pressing_enter_with_no_loaded_commits_does_not_open_details() {
    let dir = TempDir::new("history-details-empty");
    git(dir.path(), &["init", "--quiet", "--initial-branch=main"]);
    git(dir.path(), &["config", "user.name", "Test User"]);
    git(dir.path(), &["config", "user.email", "test@example.com"]);

    let (mut app, _commands) = App::new(dir.path().to_path_buf(), read_port(), false);
    open_and_load(&mut app, dir.path());
    focus_graph(&mut app);

    app.update(Action::Activate);
    assert!(
        !app.commit_details_open(),
        "there is no commit under the cursor to show details for"
    );
}
