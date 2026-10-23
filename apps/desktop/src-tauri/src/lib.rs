//! Tauri shell for PlatypusDB: exposes the core crates to the web UI as commands.

mod commands;
mod display;
mod error;
mod export;
mod jobs;

use std::sync::Arc;

use platypus_store::{ConnectionStore, HistoryStore, KeyringSecrets};
use tauri::Manager;

use crate::commands::AppState;

/// History entries kept across restarts.
const HISTORY_LIMIT: usize = 50_000;

/// Builds before 0.1.0 used the identifier `dev.platypusdb.app`, which macOS bundles reject.
/// Their folders (connections, history, the web view's storage) move over once, before the
/// web view creates the new ones. Those builds only ever ran on Windows.
fn migrate_legacy_dirs() {
    if !cfg!(windows) {
        return;
    }
    for base in ["APPDATA", "LOCALAPPDATA"]
        .into_iter()
        .filter_map(std::env::var_os)
    {
        let base = std::path::PathBuf::from(base);
        let (legacy, current) = (
            base.join("dev.platypusdb.app"),
            base.join("dev.platypusdb.desktop"),
        );
        if legacy.is_dir() && !current.exists() {
            let _ = std::fs::rename(&legacy, &current);
        }
    }
}

fn open_store(app: &tauri::App) -> Result<ConnectionStore, String> {
    let dir = app.path().app_config_dir().map_err(|e| e.to_string())?;
    ConnectionStore::open(
        dir.join("connections.toml"),
        Arc::new(KeyringSecrets::default()),
    )
    .map_err(|e| e.to_string())
}

fn open_history(app: &tauri::App) -> Result<HistoryStore, String> {
    let dir = app.path().app_data_dir().map_err(|e| e.to_string())?;
    let history = HistoryStore::open(&dir.join("history.db")).map_err(|e| e.to_string())?;
    history.prune(HISTORY_LIMIT).map_err(|e| e.to_string())?;
    Ok(history)
}

/// Builds and runs the Tauri application.
///
/// # Panics
///
/// Panics if the Tauri runtime fails to start (e.g. missing WebView2 on Windows).
pub fn run() {
    migrate_legacy_dirs();
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_process::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .setup(|app| {
            let state = AppState::new(open_store(app), open_history(app));
            app.manage(state);
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
            commands::completion_catalog,
            commands::query_value,
            commands::export_query,
            commands::export_rows,
            commands::cancel_export,
            commands::transaction_state,
            commands::set_autocommit,
            commands::end_transaction,
            commands::list_schemas,
            commands::list_objects,
            commands::table_details,
            commands::analyze_sql,
            commands::search_history,
            commands::delete_history,
            commands::read_text_file,
            commands::write_text_file,
        ])
        .run(tauri::generate_context!())
        .expect("error while running PlatypusDB");
}
