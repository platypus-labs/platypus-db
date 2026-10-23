//! Tauri shell for PlatypusDB: exposes the core crates to the web UI as commands.

mod commands;
mod display;
mod error;
mod jobs;

use std::sync::Arc;

use platypus_store::{ConnectionStore, KeyringSecrets};
use tauri::Manager;

use crate::commands::AppState;

fn open_store(app: &tauri::App) -> Result<ConnectionStore, String> {
    let dir = app.path().app_config_dir().map_err(|e| e.to_string())?;
    ConnectionStore::open(
        dir.join("connections.toml"),
        Arc::new(KeyringSecrets::default()),
    )
    .map_err(|e| e.to_string())
}

/// Builds and runs the Tauri application.
///
/// # Panics
///
/// Panics if the Tauri runtime fails to start (e.g. missing WebView2 on Windows).
pub fn run() {
    tauri::Builder::default()
        .setup(|app| {
            let store = open_store(app);
            app.manage(AppState::new(store));
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::app_info,
            commands::list_connections,
            commands::default_connection,
            commands::save_connection,
            commands::delete_connection,
            commands::test_connection,
            commands::open_session,
            commands::close_session,
            commands::run_query,
            commands::fetch_rows,
            commands::fetch_all,
            commands::cancel_query,
            commands::close_query,
        ])
        .run(tauri::generate_context!())
        .expect("error while running PlatypusDB");
}
