//! Integration tests for US-046 ("Inspecionar status, diff e blame na
//! TUI") against a real, temporary Git repository via `GitCliProvider`
//! (never a mock), following `tests/smoke.rs`'s fixture convention.

mod support;

use std::sync::Arc;

use gitsail_application::{GetRepositoryStatus, OpenRepository};
use gitsail_tui::{ui, Action, App, ClipboardPort, Command, FakeClipboard, PatchExportOutcome};
use ratatui::backend::TestBackend;
use ratatui::Terminal;
use support::{buffer_text, git, init_repo_with_initial_commit, read_port, TempDir};

/// Opens `dir` and drives `app` to the `Loaded` phase using the real port,
/// returning the port for further real calls (e.g. dispatching a
/// `Command`).
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
    let status = GetRepositoryStatus::new(port).execute(&repo).unwrap();
    app.on_status_refreshed(ticket, Ok(status));
}

fn focus_details(app: &mut App) {
    app.update(Action::FocusNext); // Sidebar -> Graph
    app.update(Action::FocusNext); // Graph -> Details
}

/// Runs the single `LoadDiff`/`LoadBlame` command a test expects and feeds
/// its real result back into `app`, exactly as `main.rs`'s loop would.
fn run_diff_or_blame_command(app: &mut App, commands: Vec<Command>) {
    let read_port = read_port();
    for command in commands {
        match command {
            Command::LoadDiff(id, repo, request) => {
                let result = gitsail_application::GetDiff::new(read_port.clone())
                    .execute(&repo, &request, &gitsail_domain::CancellationToken::new());
                app.on_diff_loaded(id, result);
            }
            Command::LoadBlame(id, repo, request, content_version) => {
                let result = gitsail_application::GetFileBlame::new(read_port.clone()).execute(
                    &repo,
                    &request,
                    content_version,
                    &gitsail_domain::CancellationToken::new(),
                );
                app.on_blame_loaded(id, result);
            }
            other => panic!("unexpected command: {other:?}"),
        }
    }
}

fn render(app: &App) -> String {
    let mut terminal = Terminal::new(TestBackend::new(120, 30)).unwrap();
    terminal.draw(|frame| ui::render(frame, app)).unwrap();
    buffer_text(&terminal)
}

#[test]
fn selecting_a_modified_file_loads_and_shows_its_unstaged_diff() {
    let dir = TempDir::new("diff-modified");
    init_repo_with_initial_commit(dir.path());
    std::fs::write(dir.path().join("README.md"), "hello\nworld\n").unwrap();

    let (mut app, _commands) = App::new(dir.path().to_path_buf(), read_port(), false);
    open_and_load(&mut app, dir.path());
    focus_details(&mut app);

    let commands = app.update(Action::Activate);
    assert!(
        matches!(commands.as_slice(), [Command::LoadDiff(_, _, _)]),
        "selecting the entry must request its diff: {commands:?}"
    );
    run_diff_or_blame_command(&mut app, commands);

    let text = render(&app);
    assert!(
        text.contains("+world"),
        "the added line must appear in the unstaged diff:\n{text}"
    );
}

#[test]
fn a_binary_file_shows_a_banner_instead_of_fabricated_hunks() {
    let dir = TempDir::new("diff-binary");
    init_repo_with_initial_commit(dir.path());
    std::fs::write(dir.path().join("image.bin"), [0x89u8, b'P', b'N', b'G', 0x00, 0x01]).unwrap();
    git(dir.path(), &["add", "image.bin"]);

    let (mut app, _commands) = App::new(dir.path().to_path_buf(), read_port(), false);
    open_and_load(&mut app, dir.path());
    focus_details(&mut app);

    // Move onto the staged entry for the new binary file (index 0, the only
    // entry: a freshly staged untracked file has no unstaged counterpart).
    let commands = app.update(Action::Activate);
    run_diff_or_blame_command(&mut app, commands);

    let text = render(&app);
    assert!(
        text.contains("[binary file]"),
        "a binary diff must show its banner, not fabricated hunks:\n{text}"
    );
    assert!(
        !text.contains("PNG"),
        "raw binary bytes must never be rendered as text:\n{text}"
    );
}

#[cfg(unix)]
#[test]
fn a_malicious_file_name_is_sanitized_before_rendering() {
    use std::ffi::OsStr;
    use std::os::unix::ffi::OsStrExt;

    let dir = TempDir::new("diff-malicious-name");
    init_repo_with_initial_commit(dir.path());

    let mut raw_name = b"evil".to_vec();
    raw_name.push(0x1b); // ESC
    raw_name.extend_from_slice(b"[31mred.txt");
    let file_path = dir.path().join(OsStr::from_bytes(&raw_name));
    std::fs::write(&file_path, "content\n").unwrap();
    git(dir.path(), &["add", "-A"]);

    let (mut app, _commands) = App::new(dir.path().to_path_buf(), read_port(), false);
    open_and_load(&mut app, dir.path());

    let text = render(&app);
    assert!(
        !text.contains('\u{1b}'),
        "a raw escape byte from a repository-controlled file name must never reach the terminal:\n{text:?}"
    );
    assert!(
        text.contains("red.txt"),
        "the rest of the (sanitized) file name must still be visible:\n{text}"
    );
}

#[test]
fn blame_shows_author_and_commit_without_terminal_control_codes() {
    let dir = TempDir::new("blame-basic");
    init_repo_with_initial_commit(dir.path());
    std::fs::write(dir.path().join("README.md"), "hello\nworld\n").unwrap();
    git(dir.path(), &["add", "README.md"]);
    git(dir.path(), &["commit", "--quiet", "-m", "add a line"]);

    let (mut app, _commands) = App::new(dir.path().to_path_buf(), read_port(), false);
    open_and_load(&mut app, dir.path());
    focus_details(&mut app);

    // The repository is clean after that commit, so there is nothing to
    // select in the status panel; select via the Diff panel's blame toggle
    // directly is not possible without a selection, so re-dirty the file
    // first to have something to select and blame.
    std::fs::write(dir.path().join("README.md"), "hello\nworld\nagain\n").unwrap();
    open_and_load(&mut app, dir.path()); // re-open re-reads status for real

    let commands = app.update(Action::Activate);
    run_diff_or_blame_command(&mut app, commands);

    let commands = app.update(Action::ToggleBlameView);
    run_diff_or_blame_command(&mut app, commands);

    let text = render(&app);
    assert!(text.contains("Test User"), "author must be visible:\n{text}");
    assert!(!text.contains('\u{1b}'), "no raw control codes:\n{text}");
    assert!(
        text.contains("local"),
        "the uncommitted line must be attributed as local, not a fabricated commit:\n{text}"
    );
}

// ---------------------------------------------------------------------
// Copy or export a patch (US-029/T-162).
// ---------------------------------------------------------------------

#[test]
fn exporting_a_patch_copies_it_to_the_clipboard_and_names_its_scope() {
    let dir = TempDir::new("export-patch-clipboard");
    init_repo_with_initial_commit(dir.path());
    std::fs::write(dir.path().join("README.md"), "hello\nworld\n").unwrap();

    let clipboard: Arc<FakeClipboard> = Arc::new(FakeClipboard::default());
    let (mut app, _commands) = App::new_with_clipboard(
        dir.path().to_path_buf(),
        read_port(),
        false,
        clipboard.clone() as Arc<dyn ClipboardPort>,
    );
    open_and_load(&mut app, dir.path());
    focus_details(&mut app);
    let commands = app.update(Action::Activate);
    run_diff_or_blame_command(&mut app, commands);
    app.update(Action::FocusNext); // Details -> Diff
    assert_eq!(app.focus(), gitsail_tui::Panel::Diff);

    app.update(Action::ExportPatch);

    let copied = clipboard
        .last_set
        .lock()
        .unwrap()
        .clone()
        .expect("the patch must have been written to the clipboard");
    assert!(
        copied.contains("+world") && copied.starts_with("--- a/README.md\n+++ b/README.md\n"),
        "the copied text must be the real git-apply-compatible patch:\n{copied}"
    );

    let text = render(&app);
    assert!(
        text.contains("copied") && text.contains("README.md") && text.contains("unstaged"),
        "the UI must state the patch's origin and scope (US-029 criterion 1):\n{text}"
    );
}

#[test]
fn exporting_a_patch_falls_back_to_a_file_when_the_clipboard_is_unavailable_and_it_applies_cleanly()
{
    let dir = TempDir::new("export-patch-fallback");
    init_repo_with_initial_commit(dir.path());
    std::fs::write(dir.path().join("README.md"), "hello\nworld\n").unwrap();

    let failing_clipboard: Arc<dyn ClipboardPort> = Arc::new(FakeClipboard {
        fail: true,
        ..Default::default()
    });
    let (mut app, _commands) = App::new_with_clipboard(
        dir.path().to_path_buf(),
        read_port(),
        false,
        failing_clipboard,
    );
    open_and_load(&mut app, dir.path());
    focus_details(&mut app);
    let commands = app.update(Action::Activate);
    run_diff_or_blame_command(&mut app, commands);
    app.update(Action::FocusNext); // Details -> Diff

    app.update(Action::ExportPatch);

    let path = match app.patch_export() {
        Some(PatchExportOutcome::SavedToFile { path, .. }) => path.clone(),
        other => panic!("expected the clipboard-unavailable fallback, got {other:?}"),
    };
    let text = render(&app);
    assert!(
        text.contains("saved to") && text.contains("README.md"),
        "the UI must name the fallback file and the patch's scope:\n{text}"
    );

    // DoD: round-trip through a real `git apply` reproduces the diff this
    // patch was exported from, closing the loop for the TUI exactly like
    // `gitsail-git`'s own round-trip tests do for the underlying renderer.
    git(dir.path(), &["checkout", "--", "README.md"]);
    assert_eq!(
        std::fs::read_to_string(dir.path().join("README.md")).unwrap(),
        "hello\n"
    );
    git(
        dir.path(),
        &["apply", path.to_str().expect("utf-8 patch path")],
    );
    assert_eq!(
        std::fs::read_to_string(dir.path().join("README.md")).unwrap(),
        "hello\nworld\n",
        "applying the saved patch file must reproduce the original diff"
    );

    std::fs::remove_file(&path).expect("clean up the fallback patch file created by this test");
}

