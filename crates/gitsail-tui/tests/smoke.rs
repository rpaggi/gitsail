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
//! layout change that drops one of the required panel regions, the branch
//! name, or the loading/empty/error indicators breaks a test here.
//!
//! The five regions are asserted by the *label* each panel carries in the
//! rendered frame, not by the internal `Panel` variant name: the layout
//! redesign renamed what the boxes say ("Commits", "Changes", "Branches")
//! without touching which panels exist or how focus moves between them.

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
    for region in ["Branches", "Commits", "Changes", "Diff", "Tags"] {
        assert!(text.contains(region), "missing {region} region:\n{text}");
    }
    assert!(
        text.contains("loading"),
        "loading state not visible:\n{text}"
    );
    assert!(
        text.contains("Help") && text.contains("Quit"),
        "missing shortcuts bar:\n{text}"
    );
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

    assert!(
        text.contains("Current branch"),
        "current-branch card not shown:\n{text}"
    );
    assert!(
        text.contains("Clean"),
        "clean working tree not stated:\n{text}"
    );
    // The filled dot is the current branch's marker in the Branches panel
    // (a hollow one means "local, not current"), and it is a *shape*, so
    // this still holds with color off.
    assert!(
        text.contains("● main"),
        "current branch not marked in the Branches panel:\n{text}"
    );
}

/// US-043 criterion 3 / `NO_COLOR`: with color switched off, every state
/// the colored frame distinguishes must still be readable from the text
/// alone. `crate::theme`'s unit tests prove each *primitive* degrades
/// (every icon has an ASCII form, no role emits a color, the three branch
/// dots stay distinct shapes); this one proves the primitives are actually
/// wired through to a rendered frame.
#[test]
fn the_low_color_frame_still_carries_every_state_as_text() {
    let dir = TempDir::new("smoke-ascii");
    support::init_repo_with_initial_commit(dir.path());
    std::fs::write(dir.path().join("README.md"), "hello\nworld\n").unwrap();

    let port = read_port();
    let (mut app, _commands) = App::new(dir.path().to_path_buf(), port.clone(), true);
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
    app.on_status_refreshed(
        ticket,
        GetRepositoryStatus::new(port.clone()).execute(&repo),
    );
    let generation = app.session().unwrap().generation();
    app.on_branches_loaded(generation, ListBranches::new(port).execute(&repo));

    let mut terminal = Terminal::new(TestBackend::new(100, 30)).unwrap();
    terminal.draw(|frame| ui::render(frame, &app)).unwrap();
    let text = buffer_text(&terminal);

    for (marker, what) in [
        ("* main", "the current branch's filled marker"),
        (">", "the selection cursor"),
        ("W README.md", "the worktree scope tag on a changed file"),
        ("! 1 changed", "the dirty working-tree verdict"),
        ("|?| Help", "the ASCII keycap strip"),
    ] {
        assert!(
            text.contains(marker),
            "low-color frame lost {what} ({marker:?}):\n{text}"
        );
    }
    // No cell may carry a color when `low_color` is set.
    let buffer = terminal.backend().buffer();
    for cell in buffer.content() {
        assert_eq!(
            cell.fg,
            ratatui::style::Color::Reset,
            "colored cell: {cell:?}"
        );
        assert_eq!(
            cell.bg,
            ratatui::style::Color::Reset,
            "colored cell: {cell:?}"
        );
    }
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
        !text.contains("Commits"),
        "the full layout must not attempt to render:\n{text}"
    );
}
