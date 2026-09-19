//! Regression tests for the operation confirmation/progress overlay's own
//! keyboard contract (T-267, GitHub issue #1): every modal state must be
//! reachable *and* leavable with a real key, and no key may leak through it
//! to the panels underneath.
//!
//! Every interaction here goes through `keymap::action_for` with an actual
//! `KeyEvent`, never a hand-built [`Action`]. That distinction is the whole
//! point: the bug this file guards against was invisible to the existing
//! state-level tests precisely because they dispatched `Action::Dismiss`
//! directly, while no key press in the overlay's context could ever produce
//! one.

mod support;

use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyEventState, KeyModifiers};
use gitsail_application::{GetRepositoryStatus, ListBranches, OpenRepository};
use gitsail_tui::keymap;
use gitsail_tui::{ui, Action, App, Command, InputContext, OperationKind, OperationState};
use ratatui::backend::TestBackend;
use ratatui::Terminal;
use support::{buffer_text, git, init_repo_with_initial_commit, read_port, TempDir};

fn branch_exists(dir: &std::path::Path, name: &str) -> bool {
    std::process::Command::new("git")
        .args(["branch", "--list", name])
        .current_dir(dir)
        .output()
        .map(|out| !out.stdout.is_empty())
        .unwrap_or(false)
}

fn press(code: KeyCode) -> KeyEvent {
    KeyEvent {
        code,
        modifiers: KeyModifiers::NONE,
        kind: KeyEventKind::Press,
        state: KeyEventState::NONE,
    }
}

fn ctrl(code: KeyCode) -> KeyEvent {
    KeyEvent {
        modifiers: KeyModifiers::CONTROL,
        ..press(code)
    }
}

/// Feeds one key through the same path `runtime.rs` uses — resolve against
/// the app's current context, then update — and reports whether the key had
/// any meaning at all.
fn send(app: &mut App, key: KeyEvent) -> (Option<Action>, Vec<Command>) {
    match keymap::action_for(key, app.input_context()) {
        Some(action) => {
            let commands = app.update(action);
            (Some(action), commands)
        }
        None => (None, Vec::new()),
    }
}

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
    let branches = ListBranches::new(port).execute(&repo).unwrap();
    app.on_branches_loaded(generation, Ok(branches));
}

/// Runs `commands` for real, feeding each result back into `app` exactly as
/// `main.rs`'s loop would (mirrors `tests/branch_management.rs`).
fn run_mutation(app: &mut App, commands: Vec<Command>) {
    let write = support::write_port();
    let read = read_port();
    let mut queue = commands;
    while let Some(command) = queue.pop() {
        let follow_up = match command {
            Command::DeleteBranch(repo, name, force) => {
                let result = gitsail_application::DeleteBranch::new(write.clone())
                    .execute(&repo, &name, force);
                app.on_operation_finished(result)
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
            // The rest of the post-mutation refresh set is irrelevant here:
            // these tests are about the overlay's keyboard, not about what
            // a refresh reloads.
            _ => Vec::new(),
        };
        queue.extend(follow_up);
    }
}

fn select_branch(app: &mut App, name: &str) {
    let index = app
        .filtered_branches()
        .iter()
        .position(|b| b.name.as_str() == name)
        .unwrap_or_else(|| panic!("{name} must be listed"));
    for _ in 0..index {
        app.update(Action::MoveDown);
    }
}

fn rendered(app: &App) -> String {
    let mut terminal = Terminal::new(TestBackend::new(100, 30)).unwrap();
    terminal.draw(|frame| ui::render(frame, app)).unwrap();
    buffer_text(&terminal)
}

/// A pending confirmation is the state the overlay's own text promises can
/// be cancelled ("Esc cancels"). Before T-267 the overlay fell through to
/// `InputContext::Normal`, which binds no `Esc` at all, so the promise was
/// simply false and the overlay could not be closed at all.
#[test]
fn esc_cancels_a_pending_confirmation_without_mutating_anything() {
    let dir = TempDir::new("modal-esc-cancels");
    init_repo_with_initial_commit(dir.path());
    git(dir.path(), &["branch", "feature"]);

    let (mut app, _commands) = App::new(dir.path().to_path_buf(), read_port(), false);
    open_and_load(&mut app, dir.path());

    select_branch(&mut app, "feature");
    app.update(Action::RequestDeleteBranch);
    assert_eq!(app.input_context(), InputContext::OperationConfirm);
    assert!(matches!(
        app.operation(),
        OperationState::Confirming(OperationKind::DeleteBranch { .. })
    ));

    // The shortcuts strip must stop advertising the panel keys the modal is
    // now swallowing, and say what the modal itself accepts instead.
    let confirming = rendered(&app);
    assert!(
        confirming.contains("Delete branch 'feature'"),
        "the prompt must name the action, not just its target:\n{confirming}"
    );
    assert!(
        confirming.contains("Enter confirms · Esc/q cancels · nothing has changed yet"),
        "the status bar must describe the modal's own keys:\n{confirming}"
    );
    assert!(
        !confirming.contains("Navigate"),
        "the ordinary shortcuts strip must not be advertised behind a modal:\n{confirming}"
    );

    let (action, commands) = send(&mut app, press(KeyCode::Esc));
    assert_eq!(action, Some(Action::Dismiss), "Esc must reach the overlay");
    assert!(
        commands.is_empty(),
        "cancelling must dispatch nothing: {commands:?}"
    );
    assert!(app.operation().is_idle(), "the overlay must be closed");
    assert!(
        branch_exists(dir.path(), "feature"),
        "cancelling must never mutate anything"
    );
    assert!(
        !rendered(&app).contains("risk: "),
        "the overlay must be gone from the screen too"
    );
}

/// The same for `q`, the key every other overlay in this crate also accepts
/// as "close me" — and, crucially, it closes the overlay instead of quitting
/// the whole app.
#[test]
fn q_closes_a_pending_confirmation_instead_of_quitting_the_app() {
    let dir = TempDir::new("modal-q-cancels");
    init_repo_with_initial_commit(dir.path());
    git(dir.path(), &["branch", "feature"]);

    let (mut app, _commands) = App::new(dir.path().to_path_buf(), read_port(), false);
    open_and_load(&mut app, dir.path());

    select_branch(&mut app, "feature");
    app.update(Action::RequestDeleteBranch);

    send(&mut app, press(KeyCode::Char('q')));
    assert!(app.operation().is_idle());
    assert!(!app.should_quit(), "q must close the modal, not the app");
    assert!(branch_exists(dir.path(), "feature"));
}

/// A modal owns the keyboard: while a confirmation is pending, the panel
/// underneath must not keep accepting shortcuts that would stack a *second*
/// mutation behind the first one.
#[test]
fn a_pending_confirmation_swallows_every_other_shortcut() {
    let dir = TempDir::new("modal-swallows-keys");
    init_repo_with_initial_commit(dir.path());
    git(dir.path(), &["branch", "feature"]);

    let (mut app, _commands) = App::new(dir.path().to_path_buf(), read_port(), false);
    open_and_load(&mut app, dir.path());

    select_branch(&mut app, "feature");
    app.update(Action::RequestDeleteBranch);
    let pending = format!("{:?}", app.operation());

    for code in [
        KeyCode::Char('d'),
        KeyCode::Char('z'),
        KeyCode::Char('m'),
        KeyCode::Char('c'),
        KeyCode::Char('A'),
        KeyCode::Char('?'),
        KeyCode::Char('j'),
        KeyCode::Tab,
    ] {
        let (action, commands) = send(&mut app, press(code));
        assert_eq!(action, None, "{code:?} must mean nothing while confirming");
        assert!(commands.is_empty());
    }
    assert_eq!(
        format!("{:?}", app.operation()),
        pending,
        "no key may replace or advance the pending confirmation"
    );
    assert!(
        !app.help_visible(),
        "'?' must not open help behind the modal"
    );
}

/// Work that already started cannot be un-started (`OperationState::cancel`'s
/// own rule), so the running state deliberately answers to nothing but the
/// usual `Ctrl+C` escape hatch.
#[test]
fn a_running_operation_ignores_keys_except_ctrl_c() {
    let dir = TempDir::new("modal-running");
    init_repo_with_initial_commit(dir.path());
    git(dir.path(), &["branch", "feature"]);

    let (mut app, _commands) = App::new(dir.path().to_path_buf(), read_port(), false);
    open_and_load(&mut app, dir.path());

    select_branch(&mut app, "feature");
    app.update(Action::RequestDeleteBranch);
    // Confirm, but never run the resulting command: the operation stays
    // `InProgress`, exactly as it would while Git is still working.
    let _commands = app.update(Action::Activate);
    assert_eq!(app.input_context(), InputContext::OperationRunning);

    for code in [KeyCode::Esc, KeyCode::Enter, KeyCode::Char('q')] {
        assert_eq!(
            send(&mut app, press(code)).0,
            None,
            "{code:?} must not pretend to cancel work already in flight"
        );
    }
    assert!(matches!(app.operation(), OperationState::InProgress(_)));

    assert_eq!(
        send(&mut app, ctrl(KeyCode::Char('c'))).0,
        Some(Action::Quit)
    );
    assert!(app.should_quit());
}

/// A success was already confirmed before anything mutated, so it needs no
/// second acknowledgement: the overlay closes itself and the status bar
/// reports what happened (T-267).
#[test]
fn a_successful_operation_closes_its_own_overlay_and_reports_in_the_status_bar() {
    let dir = TempDir::new("modal-success-closes");
    init_repo_with_initial_commit(dir.path());
    git(dir.path(), &["branch", "feature"]);

    let (mut app, _commands) = App::new(dir.path().to_path_buf(), read_port(), false);
    open_and_load(&mut app, dir.path());

    select_branch(&mut app, "feature");
    app.update(Action::RequestDeleteBranch);
    let commands = app.update(Action::Activate);
    run_mutation(&mut app, commands);

    assert!(!branch_exists(dir.path(), "feature"));
    assert!(
        app.operation().is_idle(),
        "a success must not leave an overlay behind: {:?}",
        app.operation()
    );
    assert!(matches!(
        app.last_operation_outcome(),
        Some(OperationKind::DeleteBranch { .. })
    ));

    let screen = rendered(&app);
    assert!(
        !screen.contains("risk: "),
        "the operation overlay must be gone:\n{screen}"
    );
    assert!(
        screen.contains("Done — deleted branch 'feature'"),
        "the status bar must report the outcome instead:\n{screen}"
    );

    // And the report itself is dismissible, rather than pinned forever.
    send(&mut app, press(KeyCode::Esc));
    assert!(app.last_operation_outcome().is_none());
    assert!(!rendered(&app).contains("Done — deleted branch"));
}

/// A failure is the one terminal state that still holds the screen (US-044
/// criterion 3: the error and its remediation must be readable) — but it is
/// now leavable, and it still swallows stray shortcuts while it is up.
#[test]
fn a_failed_operation_stays_on_screen_until_a_key_dismisses_it() {
    let dir = TempDir::new("modal-failure-stays");
    init_repo_with_initial_commit(dir.path());
    // A branch with work of its own that `main` does not have: Git itself
    // refuses a plain (non-forced) delete of it.
    git(dir.path(), &["checkout", "--quiet", "-b", "topic"]);
    std::fs::write(dir.path().join("topic.txt"), "topic\n").unwrap();
    git(dir.path(), &["add", "topic.txt"]);
    git(dir.path(), &["commit", "--quiet", "-m", "topic work"]);
    git(dir.path(), &["checkout", "--quiet", "main"]);

    let (mut app, _commands) = App::new(dir.path().to_path_buf(), read_port(), false);
    open_and_load(&mut app, dir.path());

    select_branch(&mut app, "topic");
    app.update(Action::RequestDeleteBranch);
    let commands = app.update(Action::Activate);
    run_mutation(&mut app, commands);

    assert!(
        matches!(app.operation(), OperationState::Failed(_, _)),
        "expected a failure, got {:?}",
        app.operation()
    );
    assert_eq!(app.input_context(), InputContext::OperationResult);
    assert!(rendered(&app).contains("Nothing was changed"));
    assert_eq!(
        send(&mut app, press(KeyCode::Char('d'))).0,
        None,
        "a stray shortcut must not start a new operation over an error"
    );

    send(&mut app, press(KeyCode::Esc));
    assert!(app.operation().is_idle());
    assert!(
        branch_exists(dir.path(), "topic"),
        "a refused delete must leave the branch alone"
    );
    assert!(!rendered(&app).contains("Nothing was changed"));
}
