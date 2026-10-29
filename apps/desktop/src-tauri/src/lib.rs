//! Tauri shell for PlatypusDB: exposes the core crates to the web UI as commands.

mod agent;
mod applog;
mod commands;
mod copy;
mod display;
mod error;
mod export;
mod formats;
mod jobs;
mod profiles;
mod querylog;
mod scheduler;
mod windows;

use std::path::{Path, PathBuf};
use std::sync::Arc;

use platypus_store::{
    CatalogStore, ConnectionStore, HistoryStore, KeyringSecrets, SettingsStore, VaultSecrets,
};
use tauri::Manager;

use crate::commands::AppState;

/// The argument that makes the app an MCP server for agents instead of opening its window.
pub const MCP_FLAG: &str = "--mcp";

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
/// use it to run against a throwaway profile. A portable copy keeps everything in its
/// `data` folder.
fn dir(app: &tauri::App, config: bool) -> Result<PathBuf, String> {
    if let Some(dir) = std::env::var_os("PLATYPUS_DATA_DIR") {
        return Ok(PathBuf::from(dir));
    }
    if let Some(dir) = platypus_mcp::profile::portable_dir() {
        return Ok(dir);
    }
    let path = app.path();
    if config {
        path.app_config_dir()
    } else {
        path.app_data_dir()
    }
    .map_err(|e| e.to_string())
}

/// The saved passwords file of the master password, next to the connections.
fn vault_path(app: &tauri::App) -> Result<PathBuf, String> {
    Ok(dir(app, true)?.join("secrets.vault"))
}

/// The connections, with their passwords in the keychain, or in the vault at `vault_path`
/// (locked until the master password is entered) when there is one.
fn open_store(
    app: &tauri::App,
    vault_path: Option<&Path>,
) -> (Result<ConnectionStore, String>, Option<Arc<VaultSecrets>>) {
    let vault = vault_path
        .filter(|p| p.exists())
        .map(|p| Arc::new(VaultSecrets::locked(p)));
    let secrets: Arc<dyn platypus_store::SecretStore> = match &vault {
        Some(v) => v.clone(),
        None => Arc::new(KeyringSecrets::default()),
    };
    let store = dir(app, true).and_then(|dir| {
        ConnectionStore::open(dir.join("connections.toml"), secrets).map_err(|e| e.to_string())
    });
    (store, vault)
}

fn open_settings(app: &tauri::App) -> Result<SettingsStore, String> {
    let dir = dir(app, true)?;
    SettingsStore::open(dir.join("settings.toml")).map_err(|e| e.to_string())
}

fn open_catalog(app: &tauri::App) -> Result<CatalogStore, String> {
    let dir = dir(app, false)?;
    CatalogStore::open(&dir.join("catalog.db")).map_err(|e| e.to_string())
}

fn open_history(app: &tauri::App) -> Result<HistoryStore, String> {
    let dir = dir(app, false)?;
    let history = HistoryStore::open(&dir.join("history.db")).map_err(|e| e.to_string())?;
    history.prune(HISTORY_LIMIT).map_err(|e| e.to_string())?;
    Ok(history)
}

/// Creates the window declared in `tauri.conf.json` (with `create: false`), with the system's
/// title bar only when `system_title_bar` (the setting) or on macOS: otherwise the app draws
/// its own in the top bar.
/// `PLATYPUS_DEVTOOLS_PORT` opens the web view's developer tools protocol on that port for the
/// end-to-end tests: WebView2 does not always honour `WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS`
/// when the app passes browser arguments of its own.
fn open_main_window(app: &tauri::App, system_title_bar: bool) -> tauri::Result<()> {
    let Some(mut config) = windows::window_config(app.handle(), "main") else {
        return Ok(());
    };
    config.decorations = system_title_bar || cfg!(target_os = "macos");
    windows::build(app.handle(), &config)
}

/// Opens the stores and the window.
fn setup(app: &mut tauri::App) -> Result<(), Box<dyn std::error::Error>> {
    let vault_path = vault_path(app);
    let settings = open_settings(app);
    // `titleBarSetting` in theme.ts: the key and its "system" value.
    let system_title_bar = settings
        .as_ref()
        .ok()
        .and_then(|s| s.get::<String>("appearance.title_bar"))
        .is_some_and(|v| v == "system");
    let state = AppState::new(
        open_store(app, vault_path.as_deref().ok()),
        vault_path,
        open_history(app),
        open_catalog(app),
        settings,
        dir(app, false).map(|d| d.join("scripts")),
    );
    state.prune_catalog();
    app.manage(state);
    app.manage(windows::Detached::default());
    // Downloaded drivers live with the data; removals that Windows refused are finished.
    if let Ok(data) = dir(app, false) {
        applog::init(&data);
        platypus_drivers::set_data_dir(&data);
        platypus_drivers::purge_removed(&data);
    }
    // After the log and the drivers' folder: a task due at the start may run at once.
    app.manage(scheduler::Scheduler::open(
        dir(app, false).ok().map(|d| d.join("schedules.json")),
    ));
    app.manage(profiles::Profiles::open(
        dir(app, false)
            .ok()
            .map(|d| d.join("transfer-profiles.json")),
    ));
    commands::schedules::start(app.handle().clone());
    agent::start(app.handle(), dir(app, false).ok());
    open_main_window(app, system_title_bar)?;
    Ok(())
}

/// Builds and runs the Tauri application.
///
/// # Panics
///
/// Panics if the Tauri runtime fails to start (e.g. missing WebView2 on Windows).
pub fn run() {
    applog::install_panic_hook();
    migrate_legacy_dirs();
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_process::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .setup(setup)
        .invoke_handler(handlers())
        .build(tauri::generate_context!())
        .expect("error while running PlatypusDB")
        .run(|app, event| {
            if let tauri::RunEvent::Exit = event {
                agent::stop(app);
            }
        });
}

/// Every command the web UI can call.
#[expect(clippy::too_many_lines, reason = "one line per command")]
fn handlers() -> impl Fn(tauri::ipc::Invoke) -> bool + Send + Sync + 'static {
    tauri::generate_handler![
        commands::app_info,
        commands::agents::agent_setup,
        agent::agent_bridge,
        agent::restart_agent_bridge,
        agent::regenerate_agent_token,
        agent::answer_agent_write,
        agent::agent_writes_waiting,
        agent::agent_answer_context,
        agent::agent_contexts_waiting,
        agent::connected_agents,
        commands::agents::agent_blockers,
        commands::agents::stale_agent_registrations,
        commands::agents::agent_calls,
        commands::agents::agent_sessions,
        commands::agents::agent_calls_export,
        commands::agents::clear_agent_calls,
        commands::agents::agent_clients,
        commands::agents::install_agent_client,
        commands::agents::take_agent_proposals,
        commands::diagnostics::diagnostics_info,
        commands::diagnostics::dismiss_crash,
        commands::diagnostics::log_error,
        commands::diagnostics::save_diagnostics,
        commands::connections::list_connections,
        commands::sample::open_sample_database,
        commands::connections::default_connection,
        commands::connections::parse_connection_url,
        commands::connections::connection_url,
        commands::connections::form_connection_url,
        commands::connections::connection_has_password,
        commands::connections::remember_password,
        commands::connections::duplicate_connection,
        commands::connections::save_connection,
        commands::connections::delete_connection,
        commands::connections::set_virtual_key,
        commands::projects::list_projects,
        commands::projects::save_project,
        commands::projects::delete_project,
        commands::projects::set_project_membership,
        commands::drivers::driver_status,
        commands::drivers::install_driver,
        commands::drivers::cancel_driver_install,
        commands::drivers::list_drivers,
        commands::drivers::remove_driver,
        commands::vendor::vendor_driver_status,
        commands::vendor::install_vendor_driver,
        commands::vendor::cancel_vendor_install,
        commands::vendor::remove_vendor_driver,
        commands::vendor::odbc_system,
        commands::projects::create_connection_folder,
        commands::projects::rename_connection_folder,
        commands::projects::move_connection_folder,
        commands::projects::delete_connection_folder,
        commands::projects::place_connection,
        commands::projects::arrange_connection,
        commands::projects::arrange_connection_folder,
        commands::projects::group_connections,
        commands::projects::reorder_connection,
        commands::projects::list_project_folders,
        commands::projects::group_projects,
        commands::projects::save_project_folder,
        commands::projects::set_project_folder,
        commands::projects::ungroup_projects,
        commands::projects::reorder_projects,
        commands::projects::project_initials,
        commands::connections::detect_imports,
        commands::connections::read_import,
        commands::connections::save_imported,
        commands::connections::forget_import,
        commands::connections::test_connection,
        commands::connections::open_session,
        commands::connections::close_session,
        commands::connections::list_network_profiles,
        commands::connections::save_network_profile,
        commands::connections::delete_network_profile,
        commands::health::ping_session,
        commands::vault::vault_status,
        commands::vault::unlock_vault,
        commands::vault::lock_vault,
        commands::vault::enable_vault,
        commands::vault::disable_vault,
        commands::vault::change_vault_password,
        commands::query::run_query,
        commands::query::fetch_rows,
        commands::query::fetch_all,
        commands::query::cancel_query,
        commands::query::close_query,
        commands::analysis::sql::fetch_whole,
        commands::analysis::sql::stop_fetch,
        commands::analysis::sql::open_result_db,
        commands::analysis::sql::reload_result_db,
        commands::analysis::sql::close_result_db,
        commands::meta::completion_catalog,
        commands::meta::query_value,
        commands::meta::estimate_rows,
        commands::meta::list_schemas,
        commands::meta::list_objects,
        commands::meta::table_details,
        commands::meta::generated_columns,
        commands::meta::referencing_keys,
        commands::meta::lookup_values,
        commands::search::search_metadata,
        commands::search::search_table_data,
        commands::data::preview_changes,
        commands::data::save_changes,
        commands::data::row_values,
        commands::data::split_literal,
        commands::data::join_literal,
        commands::grid::copy_rows,
        commands::grid::sql_literals,
        commands::grid::cell_bytes,
        commands::grid::save_cell,
        commands::grid::file_size,
        commands::grid::read_cell_file,
        commands::grid::search_rows,
        commands::summary::selection_stats,
        commands::summary::column_stats,
        commands::analysis::stats::column_profile,
        commands::summary::group_counts,
        commands::geometry::geometries,
        commands::analysis::chart::chart_data,
        commands::export::export_query,
        commands::export::export_rows,
        commands::export::cancel_export,
        commands::analysis::diff::keep_previous_run,
        commands::analysis::diff::forget_previous_run,
        commands::analysis::diff::diff_previous,
        commands::export::export_tables,
        commands::transfer::preview_import,
        commands::transfer::import_file,
        commands::transfer::preview_transfer,
        commands::transfer::transfer_table,
        commands::transfer::migrate_tables,
        commands::transfer::run_sql_file,
        commands::transfer::dump_sql,
        windows::open_tab_window,
        windows::take_window_tab,
        windows::open_quick_window,
        windows::set_tray,
        commands::snapshots::take_snapshot,
        commands::snapshots::list_snapshots,
        commands::snapshots::snapshot_restore,
        commands::snapshots::delete_snapshot,
        commands::transfer::fake_rows,
        commands::transactions::transaction_state,
        commands::transactions::set_autocommit,
        commands::transactions::end_transaction,
        commands::sql::analyze_sql,
        commands::sql::format_sql,
        commands::sql::query_source,
        commands::sql::lint_sql,
        commands::sql::sql_order_by,
        commands::sql::sql_count_query,
        commands::sql::sql_add_limit,
        commands::sql::sql_sortable,
        commands::sql::sql_add_condition,
        commands::sql::sql_order_clause,
        commands::sql::sql_filter_columns,
        commands::scripts::list_scripts,
        commands::scripts::create_script,
        commands::scripts::create_script_folder,
        commands::scripts::rename_script,
        commands::scripts::delete_script,
        commands::scripts::link_script,
        commands::cli::run_cli,
        commands::cli::cancel_cli,
        commands::catalog::catalog_entries,
        commands::catalog::save_catalog_entry,
        commands::catalog::catalog_tags,
        commands::catalog::rename_catalog_key,
        commands::catalog::comment_sql,
        commands::catalog::catalog_display_columns,
        commands::catalog::set_catalog_display_column,
        commands::catalog::export_docs,
        commands::catalog::cancel_docs,
        commands::history::search_history,
        commands::history::delete_history,
        commands::files::read_text_file,
        commands::files::write_text_file,
        commands::files::write_binary_file,
        commands::files::read_clipboard,
        commands::files::clipboard_to_file,
        commands::files::discard_pasted_file,
        commands::settings::get_settings,
        commands::settings::set_setting,
        commands::explain::explain,
        commands::explain::read_plan,
        commands::admin::server_sessions,
        commands::admin::server_locks,
        commands::admin::end_server_session,
        commands::admin::top_queries,
        commands::admin::object_statistics,
        commands::admin::maintain_table,
        commands::backup::backup_database,
        commands::backup::restore_database,
        commands::schedules::list_schedules,
        commands::schedules::save_schedule,
        commands::schedules::delete_schedule,
        commands::schedules::run_schedule_now,
        commands::schedules::time_zones,
        commands::profiles::list_transfer_profiles,
        commands::profiles::save_transfer_profile,
        commands::profiles::delete_transfer_profile,
        commands::profiles::run_transfer_profile,
        commands::querylog::query_log,
        commands::querylog::clear_query_log,
        commands::objects::object_ddl,
        commands::objects::object_properties,
        commands::objects::list_databases,
        commands::objects::execute_statement,
        commands::objects::generate_sql,
        commands::objects::dialect_support,
        commands::schema::table_design,
        commands::schema::table_script,
        commands::schema::apply_table_script,
        commands::schema::schema_tables,
        commands::schema::lint_schema,
        commands::schema::compare_schemas,
    ]
}
