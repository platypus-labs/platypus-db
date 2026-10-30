//! WebAssembly plugins (Settings › Plugins): inspecting a package before the user approves
//! its permissions, installing, turning on and off, removing, and running their actions.
//! The sandbox and the packages are in `platypus-plugins`; their drivers open through
//! [`super::connect`].

use std::path::PathBuf;

use platypus_plugins::{ActionOutcome, Inspected, Installed, PluginError, Plugins};
use serde_json::Value;

use crate::error::{CommandError, CommandResult};

fn error(e: &PluginError) -> CommandError {
    let kind = match e {
        PluginError::Denied(_) => "plugin_denied",
        PluginError::OutOfFuel | PluginError::Timeout | PluginError::Trap(_) => "plugin_stopped",
        _ => "plugin",
    };
    CommandError::new(kind, e.to_string())
}

#[expect(
    clippy::needless_pass_by_value,
    reason = "used directly as a `map_err` callback"
)]
fn failed(e: PluginError) -> CommandError {
    error(&e)
}

fn plugins() -> CommandResult<Plugins> {
    Plugins::shared().map_err(failed)
}

#[tauri::command]
pub fn plugins_list() -> CommandResult<Vec<Installed>> {
    plugins()?.list().map_err(failed)
}

/// Reads a package (a folder or a `.zip`) for the user to review its permissions.
#[tauri::command]
pub async fn plugin_inspect(path: PathBuf) -> CommandResult<Inspected> {
    plugins()?.inspect(&path).await.map_err(failed)
}

/// Installs a package the user reviewed; its permissions are approved by this call.
#[tauri::command]
pub async fn plugin_install(path: PathBuf) -> CommandResult<Installed> {
    plugins()?.install(&path).await.map_err(failed)
}

#[tauri::command]
pub fn plugin_set_enabled(id: String, enabled: bool) -> CommandResult<()> {
    plugins()?.set_enabled(&id, enabled).map_err(failed)
}

#[tauri::command]
pub fn plugin_remove(id: String) -> CommandResult<()> {
    plugins()?.remove(&id).map_err(failed)
}

/// Runs a plugin's action on what the page gives it (the editor's text, the grid's rows,
/// the navigator's object); the page applies the outcome.
#[tauri::command]
pub async fn plugin_run_action(
    plugin: String,
    action: String,
    context: Value,
) -> CommandResult<ActionOutcome> {
    plugins()?
        .run_action(&plugin, &action, context)
        .await
        .map_err(failed)
}
