//! Smoke test for opening a repository and rendering the TUI's initial
//! structure (US-040 DoD: "Smoke test de abertura e estado inicial usa
//! fixture conhecida; referência visual TUI registrada").
//!
//! Renders against `ratatui::backend::TestBackend` — an in-memory grid of
//! cells, the standard way to assert a terminal UI's actual output without
//! a real terminal — against a real, temporary Git repository created via
//! the `git` CLI (never a mock), following the same fixture convention as
//! `gitsail-git`'s and `gitsail-cli`'s integration tests. The buffer
//! assertions below *are* the recorded visual reference: any future
//! layout change that drops one of the five required regions, the branch
//! name, or the loading/empty/error indicators breaks a test here.

mod support;

use std::path::PathBuf;

use gitsail_application::{GetRepositoryStatus, ListBranches, OpenRepository};
use gitsail_tui::{ui, App};
use ratatui::backend::TestBackend;
use ratatui::Terminal;
use support::{buffer_text, git, read_port, TempDir};

#[test]
fn initial_frame_shows_all_five_regions_in_the_loading_phase() {
    let dir = TempDir::new("smoke-loading");
    git(dir.path(), &["init", "--quiet", "--initial-branch=main"]);

    let (app, _commands) = App::new(dir.path().to_path_buf(), read_port(), false);
    let mut terminal = Terminal::new(TestBackend::new(100, 30)).unwrap();
    terminal.draw(|frame| ui::render(frame, &app)).unwrap();

    let text = buffer_text(&terminal);
    assert!(text.contains("Sidebar"), "missing sidebar region:\n{text}");
    assert!(text.contains("Graph"), "missing graph region:\n{text}");
    assert!(text.contains("Details"), "missing details region:\n{text}");
    assert!(text.contains("Diff"), "missing diff region:\n{text}");
    assert!(
        text.contains("loading"),
        "loading state not visible:\n{text}"
    );
    assert!(text.contains("? help"), "missing shortcuts bar:\n{text}");
}

#[test]
fn opening_a_known_fixture_repository_reaches_the_loaded_phase_with_its_branch() {
    let dir = TempDir::new("smoke-loaded");
    support::init_repo_with_initial_commit(dir.path());

    let port = read_port();
    let (mut app, _commands) = App::new(dir.path().to_path_buf(), port.clone(), false);

    let repo = OpenRepository::new(port.clone())
        .execute(dir.path())
        .unwrap();
    let open_commands = app.on_repository_opened(Ok(repo.clone()));
    let ticket = open_commands
        .iter()
        .find_map(|c| match c {
            gitsail_tui::Command::RefreshStatus(t, _) => Some(*t),
            _ => None,
        })
        .expect("RefreshStatus command");
    let status = GetRepositoryStatus::new(port.clone())
        .execute(&repo)
        .unwrap();
    app.on_status_refreshed(ticket, Ok(status));

    let generation = app.session().unwrap().generation();
    let branches = ListBranches::new(port).execute(&repo).unwrap();
    app.on_branches_loaded(generation, Ok(branches));

    let mut terminal = Terminal::new(TestBackend::new(100, 30)).unwrap();
    terminal.draw(|frame| ui::render(frame, &app)).unwrap();
    let text = buffer_text(&terminal);

    assert!(text.contains("branch: main"), "branch not shown:\n{text}");
    assert!(text.contains("status: clean"), "status not shown:\n{text}");
    assert!(
        text.contains("* main"),
        "current branch not listed in sidebar:\n{text}"
    );
}

#[test]
fn opening_a_missing_repository_reaches_the_error_phase() {
    let dir = TempDir::new("smoke-not-a-repo");

    let (mut app, _commands) = App::new(dir.path().to_path_buf(), read_port(), false);
    let result = OpenRepository::new(read_port()).execute(dir.path());
    assert!(
        result.is_err(),
        "a plain directory must not resolve as a repository"
    );
    app.on_repository_opened(result);

    let mut terminal = Terminal::new(TestBackend::new(100, 30)).unwrap();
    terminal.draw(|frame| ui::render(frame, &app)).unwrap();
    let text = buffer_text(&terminal);

    assert!(text.contains("error"), "error state not visible:\n{text}");
}

#[test]
fn a_terminal_below_the_minimum_size_shows_a_resize_message_instead_of_the_layout() {
    let (app, _commands) = App::new(PathBuf::from("."), read_port(), false);
    let mut terminal = Terminal::new(TestBackend::new(40, 10)).unwrap();
    terminal.draw(|frame| ui::render(frame, &app)).unwrap();
    let text = buffer_text(&terminal);

    assert!(
        text.contains("too small"),
        "expected a minimum-size message:\n{text}"
    );
    assert!(
        !text.contains("Sidebar"),
        "the full layout must not attempt to render:\n{text}"
    );
}
