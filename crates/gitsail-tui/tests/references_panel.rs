//! Integration tests for T-183/US-050 ("Consultar tags, remotes e stash na
//! TUI") against a real, temporary Git repository via `GitCliProvider`
//! (never a mock). Read-only: no test here creates/applies/deletes a stash
//! or a tag through the TUI itself (that is explicitly out of scope for
//! this story — see US-050 criterion 3), only through the `git` CLI as
//! fixture setup, then inspected via the app.

mod support;

use gitsail_application::{GetCommit, GetRepositoryStatus, OpenRepository};
use gitsail_tui::{ui, Action, App, Command, Panel, ReferenceView};
use ratatui::backend::TestBackend;
use ratatui::Terminal;
use support::{git, init_repo_with_initial_commit, read_port, TempDir};

/// Opens `dir` and drains every command `on_repository_opened` issues
/// (status, branches, tags, remotes, stash, the first commit-graph page)
/// against the real adapter, so the References panel genuinely reflects
/// the repository's on-disk tags/remotes/stash.
fn open_and_load(app: &mut App, dir: &std::path::Path) {
    let read = read_port();
    let repo = OpenRepository::new(read.clone()).execute(dir).unwrap();
    let commands = app.on_repository_opened(Ok(repo.clone()));

    let ticket = commands
        .iter()
        .find_map(|c| match c {
            Command::RefreshStatus(t, _) => Some(*t),
            _ => None,
        })
        .expect("RefreshStatus command");
    let status = GetRepositoryStatus::new(read.clone()).execute(&repo).unwrap();
    app.on_status_refreshed(ticket, Ok(status));

    for command in commands {
        match command {
            Command::LoadTags(generation, repo) => {
                app.on_tags_loaded(generation, read.list_tags(&repo));
            }
            Command::LoadRemotes(generation, repo) => {
                app.on_remotes_loaded(generation, read.list_remotes(&repo));
            }
            Command::LoadStashEntries(generation, repo) => {
                app.on_stash_entries_loaded(generation, read.list_stash_entries(&repo));
            }
            Command::LoadReflog(generation, repo) => {
                app.on_reflog_loaded(generation, read.reflog(&repo));
            }
            _ => {}
        }
    }
}

fn focus_references(app: &mut App) {
    for _ in 0..4 {
        // Sidebar -> Graph -> Details -> Diff -> References
        app.update(Action::FocusNext);
    }
    assert_eq!(app.focus(), Panel::References);
}

fn render(app: &App) -> String {
    let mut terminal = Terminal::new(TestBackend::new(140, 32)).unwrap();
    terminal.draw(|frame| ui::render(frame, app)).unwrap();
    support::buffer_text(&terminal)
}

#[test]
fn a_repository_with_no_tags_remotes_or_stash_shows_an_explicit_empty_state_for_each() {
    let dir = TempDir::new("references-empty");
    init_repo_with_initial_commit(dir.path());

    let (mut app, _commands) = App::new(dir.path().to_path_buf(), read_port(), false);
    open_and_load(&mut app, dir.path());
    focus_references(&mut app);

    assert!(app.tags().is_empty());
    assert!(app.remotes().is_empty());
    assert!(app.stashes().is_empty());

    assert_eq!(app.reference_view(), ReferenceView::Tags);
    let text = render(&app);
    assert!(text.contains("No tags."), "missing empty tags state:\n{text}");

    app.update(Action::CycleReferenceView);
    assert_eq!(app.reference_view(), ReferenceView::Remotes);
    let text = render(&app);
    assert!(
        text.contains("No remotes configured."),
        "missing empty remotes state:\n{text}"
    );

    app.update(Action::CycleReferenceView);
    assert_eq!(app.reference_view(), ReferenceView::Stash);
    let text = render(&app);
    assert!(
        text.contains("No stash entries."),
        "missing empty stash state:\n{text}"
    );

    // Activating an empty list must never open a details overlay for
    // nothing (US-050 criterion 3: empty is an explicit state, not a
    // silently broken selection).
    app.update(Action::Activate);
    assert!(!app.reference_details_open());
}

#[test]
fn a_repository_with_tags_remotes_and_stash_lists_and_navigates_all_three() {
    let dir = TempDir::new("references-populated");
    init_repo_with_initial_commit(dir.path());

    // Two tags: one lightweight, one annotated with its own message/tagger/
    // date (US-050 criterion 2: both kinds' available metadata are shown).
    git(dir.path(), &["tag", "v1.0"]);
    git(
        dir.path(),
        &["tag", "-a", "v2.0", "-m", "second release notes"],
    );

    // A remote (listing only reads configuration — it never needs to be
    // reachable).
    git(
        dir.path(),
        &["remote", "add", "origin", "https://example.test/repo.git"],
    );

    // A stash entry: a tracked-file edit stashed without committing.
    std::fs::write(dir.path().join("README.md"), "hello\nchanged\n").unwrap();
    git(dir.path(), &["stash", "push", "-m", "work in progress"]);

    let (mut app, _commands) = App::new(dir.path().to_path_buf(), read_port(), false);
    open_and_load(&mut app, dir.path());
    focus_references(&mut app);

    // -- Tags --------------------------------------------------------
    assert_eq!(app.tags().len(), 2);
    let text = render(&app);
    assert!(text.contains("v1.0") && text.contains("v2.0"), "tags not listed:\n{text}");

    app.update(Action::MoveDown); // v1.0 -> v2.0 (order not asserted; just move once)
    app.update(Action::Activate);
    assert!(app.reference_details_open());
    let details = render(&app);
    assert!(
        details.contains("tag") && (details.contains("lightweight") || details.contains("annotated")),
        "tag details must show its kind:\n{details}"
    );
    app.update(Action::Dismiss);
    assert!(!app.reference_details_open());

    // Find the annotated tag specifically and check its message surfaces —
    // navigate to it from wherever the cursor currently sits, using the
    // cyclic cursor's own wraparound rather than assuming a starting
    // position.
    let annotated_index = app
        .tags()
        .iter()
        .position(|t| t.name == "v2.0")
        .expect("v2.0 must be listed");
    let len = app.tags().len();
    let steps = (annotated_index + len - app.reference_cursor()) % len;
    for _ in 0..steps {
        app.update(Action::MoveDown);
    }
    assert_eq!(app.reference_cursor(), annotated_index);
    app.update(Action::Activate);
    let annotated_details = render(&app);
    assert!(
        annotated_details.contains("second release notes"),
        "annotated tag's message must be shown:\n{annotated_details}"
    );
    app.update(Action::Dismiss);

    // -- Remotes -------------------------------------------------------
    app.update(Action::CycleReferenceView);
    assert_eq!(app.reference_view(), ReferenceView::Remotes);
    assert_eq!(app.remotes().len(), 1);
    assert_eq!(app.remotes()[0].name, "origin");
    let text = render(&app);
    assert!(text.contains("origin"), "remote not listed:\n{text}");

    app.update(Action::Activate);
    assert!(app.reference_details_open());
    let details = render(&app);
    assert!(
        details.contains("example.test"),
        "remote details must show its URL:\n{details}"
    );
    app.update(Action::Dismiss);

    // -- Stash -----------------------------------------------------------
    app.update(Action::CycleReferenceView);
    assert_eq!(app.reference_view(), ReferenceView::Stash);
    assert_eq!(app.stashes().len(), 1);
    assert!(
        app.stashes()[0].message.contains("work in progress"),
        "stash message not captured: {:?}",
        app.stashes()[0].message
    );
    // The panel's own narrow column may visually truncate a long message
    // (a real layout constraint, not a bug); "stash@{0}" itself must still
    // be visible in the list regardless.
    let text = render(&app);
    assert!(text.contains("stash@{0}"), "stash entry not listed:\n{text}");

    app.update(Action::Activate);
    assert!(app.reference_details_open());
    let details = render(&app);
    assert!(
        details.contains("work in progress"),
        "stash details must show its message:\n{details}"
    );
    app.update(Action::Dismiss);
    assert!(!app.reference_details_open());

    // -- Reflog (T-241/US-089) --------------------------------------------
    // Read-only: this never runs `reset` or checks anything out (History
    // Editing Rules #10) — only the one commit made above shows up.
    let read = read_port();
    app.update(Action::CycleReferenceView);
    assert_eq!(app.reference_view(), ReferenceView::Reflog);
    // The initial commit plus the `stash push` above (which records its own
    // "reset: moving to HEAD" entry, even though the commit itself never
    // changes) — verified empirically against real Git.
    assert_eq!(app.reflog().len(), 2);
    assert!(app.reflog().iter().all(|entry| entry.is_available()));
    let text = render(&app);
    assert!(text.contains("HEAD@{0}"), "reflog entry not listed:\n{text}");

    // Selecting it dispatches a real `GetCommit` read (US-089 criterion 2:
    // reuses the existing use case, never a parallel implementation) —
    // this is the one sub-view where activating an entry issues a
    // `Command`, unlike Tags/Remotes/Stash above.
    let commands = app.update(Action::Activate);
    assert!(app.reflog_details_open());
    assert_eq!(commands.len(), 1, "activating a reflog entry issues exactly one read");
    for command in commands {
        if let Command::LoadReflogCommit(repo, hash) = command {
            let result = GetCommit::new(read.clone()).execute(&repo, &hash);
            app.on_reflog_commit_loaded(hash, result);
        } else {
            panic!("expected Command::LoadReflogCommit, got {command:?}");
        }
    }
    let details = render(&app);
    assert!(
        details.contains("initial commit"),
        "reflog entry details must show the real commit's own subject:\n{details}"
    );
    app.update(Action::Dismiss);
    assert!(!app.reflog_details_open());
}

#[test]
fn cycling_reference_view_resets_the_cursor_so_a_shorter_lists_selection_stays_valid() {
    let dir = TempDir::new("references-cursor-reset");
    init_repo_with_initial_commit(dir.path());
    git(dir.path(), &["tag", "v1.0"]);
    git(dir.path(), &["tag", "v2.0"]);
    git(dir.path(), &["tag", "v3.0"]);

    let (mut app, _commands) = App::new(dir.path().to_path_buf(), read_port(), false);
    open_and_load(&mut app, dir.path());
    focus_references(&mut app);

    app.update(Action::MoveDown);
    app.update(Action::MoveDown);
    assert_eq!(app.reference_cursor(), 2);

    // Remotes has zero entries in this fixture; cycling to it must not
    // leave a stale, out-of-bounds cursor behind.
    app.update(Action::CycleReferenceView);
    assert_eq!(app.reference_view(), ReferenceView::Remotes);
    assert_eq!(app.reference_cursor(), 0);
}
