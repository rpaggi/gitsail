//! GitSail Desktop shell (SAD §17; ADR-006; US-051).
//!
//! Wires the same Git CLI adapter `gitsail-cli` uses
//! (`GitProcessRunner` + `GitCliProvider`) into a Tauri v2 application,
//! exposing it to the Vue 3 frontend only through the thin commands in
//! [`commands`].

#![forbid(unsafe_code)]

mod browser;
mod commands;
mod keybindings_store;
mod preferences_store;
mod recent_repositories_store;
mod state;
mod version;

use std::sync::Arc;

use gitsail_application::{
    ForgeCredentialPort, PreferencesPort, PullRequestQueryPort, RecentRepositoriesPort,
    RepositoryReadPort, RepositoryWritePort, UpdateCheckPort,
};
use gitsail_git::{GitCliProvider, GitProcessRunner, GitProcessRunnerConfig};

use preferences_store::JsonFilePreferencesStore;
use recent_repositories_store::JsonFileRecentRepositoriesStore;
use state::{AppState, StartupIntent};

/// Parses this process' own argv for the `--repo <path> --commit <hash>`
/// handoff contract `apps/vscode/src/desktopHandoff.ts` already builds and
/// validates (T-210/US-077) — the Desktop-side half of that contract that
/// was, until now, a documented but unimplemented gap (EPIC-15's own
/// finding: "Desktop hoje ignora esses argumentos"). Unknown flags/values
/// are ignored rather than rejected: a GUI app's argv can pick up
/// OS/launcher-injected arguments (macOS' `-psn_...`, etc.) that must never
/// prevent it from starting.
///
/// Either flag may appear alone (`--repo` with no `--commit` just opens the
/// repository without selecting anything); a flag with no following value
/// (argv ends right after it) is treated the same as if the flag were
/// absent, never as an empty-string path/hash.
fn parse_startup_args(args: impl Iterator<Item = String>) -> StartupIntent {
    let mut args = args.skip(1); // argv[0] is this executable's own path.
    let mut repo_path = None;
    let mut commit_hash = None;
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--repo" => repo_path = args.next(),
            "--commit" => commit_hash = args.next(),
            _ => {}
        }
    }
    StartupIntent {
        repo_path,
        commit_hash,
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let runner = GitProcessRunner::new(GitProcessRunnerConfig {
        executable: None,
        default_cwd: None,
        default_timeout: None,
    })
    .expect("git executable discovery failed");
    // One adapter instance implements both read and write ports (ADR-006);
    // each `Arc` clone below is a separate trait-object view over the same
    // underlying `GitCliProvider`, matching `write_ports.rs`'s own doc
    // comment ("`GitCliProvider` implements both traits, but application
    // code depends on whichever capability it actually needs").
    let provider = Arc::new(GitCliProvider::new(runner));
    let port: Arc<dyn RepositoryReadPort> = provider.clone();
    let write_port: Arc<dyn RepositoryWritePort> = provider;

    let recent_repositories_path = JsonFileRecentRepositoriesStore::default_location()
        .expect("could not resolve the recent repositories file location");
    let recent_repositories: Arc<dyn RecentRepositoriesPort> = Arc::new(
        JsonFileRecentRepositoriesStore::new(recent_repositories_path),
    );

    // T-244/US-102: OS-secure-storage-backed forge (GitHub/GitLab) token
    // store — see `gitsail-forge`'s crate docs for the backend chosen per
    // platform and why it needs manual verification per target OS.
    let forge_credentials: Arc<dyn ForgeCredentialPort> =
        Arc::new(gitsail_forge::KeyringForgeCredentialStore::new());

    // T-245/US-103: PR/MR listing, dispatched per detected forge (GitHub vs
    // GitLab) — see `gitsail-forge`'s crate docs for the HTTP client
    // choice (`ureq`) and why this is not a complete PR/MR client.
    let pull_request_query: Arc<dyn PullRequestQueryPort> =
        Arc::new(gitsail_forge::CompositePullRequestQueryPort::production());

    // T-248/US-106: theme preference, connecting T-247's until-now-unused
    // `JsonFilePreferencesStore` the same way `recent_repositories` is wired
    // above.
    let preferences_path = JsonFilePreferencesStore::default_location()
        .expect("could not resolve the preferences file location");
    let preferences: Arc<dyn PreferencesPort> =
        Arc::new(JsonFilePreferencesStore::new(preferences_path));

    // T-249/US-107: keyboard shortcut overrides — a Desktop-only concern,
    // deliberately not a `PreferencesPort` (see `keybindings_store`'s own
    // module doc for why).
    let keybindings_path = keybindings_store::JsonFileKeybindingsStore::default_location()
        .expect("could not resolve the keybindings file location");
    let keybindings = Arc::new(keybindings_store::JsonFileKeybindingsStore::new(
        keybindings_path,
    ));

    // T-260/US-127: GitHub Releases update checking — see `version.rs` for
    // how this build knows its own release tag, and
    // `gitsail_application::update_check`'s module doc for why this is a
    // check-only port (never an auto-installer).
    let update_check: Arc<dyn UpdateCheckPort> =
        Arc::new(gitsail_forge::GitHubReleaseUpdateAdapter::new(Arc::new(
            gitsail_forge::UreqHttpClient::new(),
        )));

    let app_state = AppState::new(
        port,
        write_port,
        recent_repositories,
        forge_credentials,
        pull_request_query,
        preferences,
        keybindings,
        update_check,
    );
    app_state.set_startup_intent(parse_startup_args(std::env::args()));

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(app_state)
        .invoke_handler(tauri::generate_handler![
            commands::open_repository,
            commands::get_repository_status,
            commands::get_commit_graph_page,
            commands::list_recent_repositories,
            commands::forget_recent_repository,
            commands::export_patch,
            commands::save_text_file,
            commands::read_text_file,
            commands::preview_patch_application,
            commands::apply_patch,
            commands::take_startup_intent,
            commands::list_branches,
            commands::get_commit,
            commands::search_commits,
            commands::get_diff,
            commands::stage_paths,
            commands::unstage_paths,
            commands::stage_hunks,
            commands::unstage_hunks,
            commands::create_commit,
            commands::preview_amend,
            commands::amend_commit,
            commands::create_branch,
            commands::switch_branch,
            commands::delete_branch,
            commands::rename_branch,
            commands::list_remotes,
            commands::list_tags,
            commands::list_stash_entries,
            commands::get_blame,
            commands::get_forge_link,
            commands::open_forge_link,
            commands::forge_connection_status,
            commands::connect_forge_account,
            commands::disconnect_forge_account,
            commands::list_pull_requests,
            commands::open_pull_request_link,
            commands::resolve_sync_target,
            commands::fetch,
            commands::pull,
            commands::push,
            commands::detect_in_progress_operation,
            commands::merge,
            commands::get_conflict_sides,
            commands::mark_conflict_resolved,
            commands::take_conflict_side,
            commands::continue_operation,
            commands::abort_operation,
            commands::rebase,
            commands::skip_operation,
            commands::plan_rebase,
            commands::execute_rebase_plan,
            commands::cherry_pick,
            commands::revert,
            commands::reset,
            commands::get_preferences,
            commands::set_theme,
            commands::get_keybinding_overrides,
            commands::set_keybinding_override,
            commands::reset_keybinding_override,
            commands::reset_all_keybinding_overrides,
            commands::check_for_update,
            commands::set_check_for_updates,
            commands::open_update_link,
        ])
        .run(tauri::generate_context!())
        .expect("error while running the GitSail Desktop application");
}

#[cfg(test)]
mod tests {
    use super::parse_startup_args;

    #[test]
    fn no_arguments_beyond_argv0_yields_an_empty_intent() {
        let intent = parse_startup_args(vec!["gitsail-desktop".to_string()].into_iter());

        assert_eq!(intent.repo_path, None);
        assert_eq!(intent.commit_hash, None);
    }

    #[test]
    fn parses_both_repo_and_commit_flags_regardless_of_order() {
        let intent = parse_startup_args(
            vec![
                "gitsail-desktop".to_string(),
                "--repo".to_string(),
                "/home/user/project".to_string(),
                "--commit".to_string(),
                "deadbeef".to_string(),
            ]
            .into_iter(),
        );

        assert_eq!(intent.repo_path.as_deref(), Some("/home/user/project"));
        assert_eq!(intent.commit_hash.as_deref(), Some("deadbeef"));
    }

    #[test]
    fn a_flag_with_no_following_value_is_ignored_rather_than_treated_as_empty() {
        let intent = parse_startup_args(
            vec!["gitsail-desktop".to_string(), "--repo".to_string()].into_iter(),
        );

        assert_eq!(intent.repo_path, None);
    }

    #[test]
    fn unknown_arguments_are_ignored_rather_than_rejected() {
        let intent = parse_startup_args(
            vec![
                "gitsail-desktop".to_string(),
                "-psn_0_12345".to_string(),
                "--repo".to_string(),
                "/repo".to_string(),
            ]
            .into_iter(),
        );

        assert_eq!(intent.repo_path.as_deref(), Some("/repo"));
    }

    #[test]
    fn repo_only_leaves_commit_hash_absent() {
        let intent = parse_startup_args(
            vec![
                "gitsail-desktop".to_string(),
                "--repo".to_string(),
                "/repo".to_string(),
            ]
            .into_iter(),
        );

        assert_eq!(intent.repo_path.as_deref(), Some("/repo"));
        assert_eq!(intent.commit_hash, None);
    }
}
