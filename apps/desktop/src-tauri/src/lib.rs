//! Tauri shell for PlatypusDB: exposes the core crates to the web UI as commands.

mod commands;
mod copy;
mod display;
mod error;
mod export;
mod formats;
mod jobs;

use std::path::PathBuf;
use std::sync::Arc;

use platypus_store::{ConnectionStore, HistoryStore, KeyringSecrets, SettingsStore};
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

/// Where connections and settings (`config: true`) or history live. `PLATYPUS_DATA_DIR`
/// puts everything in that one folder instead of the OS locations; the end-to-end tests
/// use it to run against a throwaway profile.
fn dir(app: &tauri::App, config: bool) -> Result<PathBuf, String> {
    if let Some(dir) = std::env::var_os("PLATYPUS_DATA_DIR") {
        return Ok(PathBuf::from(dir));
    }
    let path = app.path();
    if config {
        path.app_config_dir()
    } else {
        path.app_data_dir()
    }
    .map_err(|e| e.to_string())
}

fn open_store(app: &tauri::App) -> Result<ConnectionStore, String> {
    let dir = dir(app, true)?;
    ConnectionStore::open(
        dir.join("connections.toml"),
        Arc::new(KeyringSecrets::default()),
    )
    .map_err(|e| e.to_string())
}

fn open_settings(app: &tauri::App) -> Result<SettingsStore, String> {
    let dir = dir(app, true)?;
    SettingsStore::open(dir.join("settings.toml")).map_err(|e| e.to_string())
}

fn open_history(app: &tauri::App) -> Result<HistoryStore, String> {
    let dir = dir(app, false)?;
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
            let state = AppState::new(open_store(app), open_history(app), open_settings(app));
            app.manage(state);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::app_info,
            commands::connections::list_connections,
            commands::connections::default_connection,
            commands::connections::save_connection,
            commands::connections::delete_connection,
            commands::connections::set_virtual_key,
            commands::connections::test_connection,
            commands::connections::open_session,
            commands::connections::close_session,
            commands::query::run_query,
            commands::query::fetch_rows,
            commands::query::fetch_all,
            commands::query::cancel_query,
            commands::query::close_query,
            commands::meta::completion_catalog,
            commands::meta::query_value,
            commands::meta::list_schemas,
            commands::meta::list_objects,
            commands::meta::table_details,
            commands::meta::referencing_keys,
            commands::meta::lookup_values,
            commands::data::preview_changes,
            commands::data::save_changes,
            commands::data::row_values,
            commands::grid::copy_rows,
            commands::grid::sql_literals,
            commands::grid::cell_bytes,
            commands::grid::save_cell,
            commands::grid::search_rows,
            commands::summary::selection_stats,
            commands::summary::group_counts,
            commands::export::export_query,
            commands::export::export_rows,
            commands::export::cancel_export,
            commands::transactions::transaction_state,
            commands::transactions::set_autocommit,
            commands::transactions::end_transaction,
            commands::sql::analyze_sql,
            commands::sql::query_source,
            commands::history::search_history,
            commands::history::delete_history,
            commands::files::read_text_file,
            commands::files::write_text_file,
            commands::files::read_clipboard,
            commands::settings::get_settings,
            commands::settings::set_setting,
        ])
        .run(tauri::generate_context!())
        .expect("error while running PlatypusDB");
}
