//! GitSail Desktop shell (SAD §17; ADR-006; US-051).
//!
//! Wires the same Git CLI adapter `gitsail-cli` uses
//! (`GitProcessRunner` + `GitCliProvider`) into a Tauri v2 application,
//! exposing it to the Vue 3 frontend only through the thin commands in
//! [`commands`].

#![forbid(unsafe_code)]

mod commands;
mod state;

use std::sync::Arc;

use gitsail_application::RepositoryReadPort;
use gitsail_git::{GitCliProvider, GitProcessRunner, GitProcessRunnerConfig};

use state::AppState;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let runner = GitProcessRunner::new(GitProcessRunnerConfig {
        executable: None,
        default_cwd: None,
        default_timeout: None,
    })
    .expect("git executable discovery failed");
    let port: Arc<dyn RepositoryReadPort> = Arc::new(GitCliProvider::new(runner));

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(AppState::new(port))
        .invoke_handler(tauri::generate_handler![
            commands::open_repository,
            commands::get_repository_status,
            commands::get_commit_graph_page,
        ])
        .run(tauri::generate_context!())
        .expect("error while running the GitSail Desktop application");
}
