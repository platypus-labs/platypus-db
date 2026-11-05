//! User preferences. Keys and defaults are declared by the UI (`settings.ts`); the backend
//! only stores the values.

use platypus_store::toml;
use tauri::State;

use super::AppState;
use crate::error::{CommandError, CommandResult};

/// Every stored value, as nested objects (`{ "editor": { "font_size": 14 } }`).
#[tauri::command]
pub fn get_settings(state: State<'_, AppState>) -> CommandResult<serde_json::Value> {
    state.with_settings(|s| Ok(serde_json::to_value(s.all()).unwrap_or_default()))
}

/// Sets a dotted key; `null` removes it so the default applies again.
#[tauri::command]
pub fn set_setting(
    state: State<'_, AppState>,
    key: String,
    value: serde_json::Value,
) -> CommandResult<()> {
    let value = if value.is_null() {
        None
    } else {
        Some(
            toml::Value::try_from(value)
                .map_err(|e| CommandError::new("invalid", format!("{key}: {e}")))?,
        )
    };
    let before = state.setting::<String>(&key);
    state.with_settings(|s| s.set(&key, value.clone()))?;
    // What agents may do, chosen here, is the user's (see `agent_policy`).
    state.approve_agent_setting(&key, before.as_deref(), value.as_ref());
    Ok(())
}
