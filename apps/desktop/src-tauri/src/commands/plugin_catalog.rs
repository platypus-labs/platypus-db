//! The plugin catalog (Settings › Plugin catalog): the index's address and the trusted keys, kept
//! in the settings, and the download of a package up to the review a manual install has too. The
//! checks are in `platypus_plugins::catalog`. Nothing here runs by itself: each command is the page
//! answering the user, and an empty address contacts nothing.

use platypus_plugins::catalog::{self, CatalogLimits};
use platypus_plugins::{CatalogRequest, CatalogView, IndexEntry, Installed, Prepared, TrustedKey};
use tauri::AppHandle;

use super::AppState;
use super::plugins::{failed, plugins, plugins_changed};
use crate::error::CommandResult;

/// The index's address in `settings.toml`; none until the user sets it.
const INDEX_URL: &str = "plugins.catalog_url";
/// The keys the user trusts in `settings.toml`.
const TRUSTED_KEYS: &str = "plugins.trusted_keys";

fn index_url(state: &AppState) -> CommandResult<String> {
    state.with_settings(|s| Ok(s.get::<String>(INDEX_URL).unwrap_or_default()))
}

fn trusted_keys(state: &AppState) -> CommandResult<Vec<TrustedKey>> {
    state.with_settings(|s| Ok(s.get::<Vec<TrustedKey>>(TRUSTED_KEYS).unwrap_or_default()))
}

fn save_keys(state: &AppState, keys: &[TrustedKey]) -> CommandResult<()> {
    let value = (!keys.is_empty())
        .then(|| {
            platypus_store::toml::Value::try_from(keys)
                .map_err(|e| platypus_store::StoreError::Invalid(e.to_string()))
        })
        .transpose();
    state.with_settings(|s| s.set(TRUSTED_KEYS, value?))
}

/// The catalog of the index the user set, read now. With no address it is empty and nothing is
/// contacted.
#[tauri::command]
pub async fn plugin_catalog_fetch(state: tauri::State<'_, AppState>) -> CommandResult<CatalogView> {
    let url = index_url(&state)?;
    plugins()?
        .catalog_fetch(&url, CatalogLimits::default())
        .await
        .map_err(failed)
}

/// Saves the index's address, refused when it is not one the catalog may use; empty clears it. The
/// address it holds now goes back, so that the page shows what is saved.
#[tauri::command]
pub fn plugin_catalog_set_url(
    state: tauri::State<'_, AppState>,
    url: String,
) -> CommandResult<String> {
    let url = catalog::check_index_url(&url).map_err(failed)?;
    let value = (!url.is_empty()).then(|| platypus_store::toml::Value::String(url.clone()));
    state.with_settings(|s| s.set(INDEX_URL, value))?;
    Ok(url)
}

/// The address saved, without contacting it.
#[tauri::command]
pub fn plugin_catalog_url(state: tauri::State<'_, AppState>) -> CommandResult<String> {
    index_url(&state)
}

#[tauri::command]
pub fn plugin_catalog_keys(state: tauri::State<'_, AppState>) -> CommandResult<Vec<TrustedKey>> {
    trusted_keys(&state)
}

/// Trusts a public key the user pasted; a key already there takes the new name.
#[tauri::command]
pub fn plugin_catalog_trust(
    state: tauri::State<'_, AppState>,
    name: String,
    key: String,
) -> CommandResult<Vec<TrustedKey>> {
    let key = TrustedKey::parse(&name, &key).map_err(failed)?;
    let mut keys = trusted_keys(&state)?;
    keys.retain(|k| k.key != key.key);
    keys.push(key);
    save_keys(&state, &keys)?;
    Ok(keys)
}

#[tauri::command]
pub fn plugin_catalog_untrust(
    state: tauri::State<'_, AppState>,
    key: String,
) -> CommandResult<Vec<TrustedKey>> {
    let mut keys = trusted_keys(&state)?;
    keys.retain(|k| k.key != key);
    save_keys(&state, &keys)?;
    Ok(keys)
}

/// Downloads and checks a package the user asked to install, for the review. `allowUnsigned` is the
/// answer to the separate question about a package with no valid signature, and `allowDowngrade`
/// the one about an older version: refused without them, each with its own error kind.
#[tauri::command]
pub async fn plugin_catalog_prepare(
    state: tauri::State<'_, AppState>,
    entry: IndexEntry,
    allow_unsigned: bool,
    allow_downgrade: bool,
) -> CommandResult<Prepared> {
    let request = CatalogRequest {
        entry,
        index_url: index_url(&state)?,
        keys: trusted_keys(&state)?,
        allow_unsigned,
        allow_downgrade,
    };
    plugins()?
        .catalog_prepare(request, CatalogLimits::default())
        .await
        .map_err(failed)
}

/// Installs the package the user reviewed: as `plugin_install`, from its download.
#[tauri::command]
pub async fn plugin_catalog_install(
    app: AppHandle,
    ticket: String,
    hash: String,
) -> CommandResult<Installed> {
    let plugins = plugins()?;
    let installed = plugins
        .catalog_install(&ticket, &hash)
        .await
        .map_err(failed)?;
    plugins_changed(&app, &plugins, &installed.manifest.id)?;
    Ok(installed)
}

/// Forgets a download the user turned down.
#[tauri::command]
pub fn plugin_catalog_discard(ticket: String) -> CommandResult<()> {
    plugins()?.catalog_discard(&ticket);
    Ok(())
}
