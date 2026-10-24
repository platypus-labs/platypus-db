//! The master password: saved passwords in an encrypted file instead of the OS keychain.

use std::collections::HashMap;
use std::sync::Arc;

use platypus_store::{KeyringSecrets, VaultSecrets};
use serde::Serialize;
use tauri::State;

use super::{AppState, lock};
use crate::error::{CommandError, CommandResult};

#[derive(Debug, Serialize)]
pub struct VaultStatus {
    /// Saved passwords are in the encrypted file.
    enabled: bool,
    /// The master password was entered in this run of the app.
    unlocked: bool,
}

fn status(state: &AppState) -> VaultStatus {
    match &*lock(&state.vault) {
        Some(v) => VaultStatus {
            enabled: true,
            unlocked: v.is_unlocked(),
        },
        None => VaultStatus {
            enabled: false,
            unlocked: false,
        },
    }
}

fn vault(state: &AppState) -> CommandResult<Arc<VaultSecrets>> {
    lock(&state.vault)
        .clone()
        .ok_or_else(|| CommandError::new("vault", "no master password is set"))
}

/// The vault's key comes from Argon2, which takes a moment: off the UI thread.
async fn blocking<T: Send + 'static>(
    f: impl FnOnce() -> CommandResult<T> + Send + 'static,
) -> CommandResult<T> {
    tauri::async_runtime::spawn_blocking(f)
        .await
        .map_err(|e| CommandError::new("other", e.to_string()))?
}

#[tauri::command]
pub fn vault_status(state: State<'_, AppState>) -> VaultStatus {
    status(&state)
}

#[tauri::command]
pub async fn unlock_vault(
    state: State<'_, AppState>,
    password: String,
) -> CommandResult<VaultStatus> {
    let v = vault(&state)?;
    blocking(move || Ok(v.unlock(&password)?)).await?;
    Ok(status(&state))
}

#[tauri::command]
pub fn lock_vault(state: State<'_, AppState>) -> CommandResult<VaultStatus> {
    vault(&state)?.lock();
    Ok(status(&state))
}

/// Moves the saved passwords from the keychain into a new vault.
#[tauri::command]
pub async fn enable_vault(
    state: State<'_, AppState>,
    password: String,
) -> CommandResult<VaultStatus> {
    if lock(&state.vault).is_some() {
        return Err(CommandError::new(
            "vault",
            "a master password is set already",
        ));
    }
    let path = state
        .vault_path
        .clone()
        .map_err(|e| CommandError::new("store", e))?;
    let created =
        blocking(move || Ok(VaultSecrets::create(path, &password, HashMap::new())?)).await?;
    let created = Arc::new(created);
    state.with_store(|s| s.move_secrets(created.clone()))?;
    *lock(&state.vault) = Some(created);
    Ok(status(&state))
}

/// Moves the saved passwords back to the keychain and deletes the vault.
#[tauri::command]
pub async fn disable_vault(
    state: State<'_, AppState>,
    password: String,
) -> CommandResult<VaultStatus> {
    let v = vault(&state)?;
    // The master password is asked again, even when unlocked.
    let check = VaultSecrets::locked(v.path());
    let target = Arc::clone(&v);
    blocking(move || {
        check.unlock(&password)?;
        if !target.is_unlocked() {
            target.unlock(&password)?;
        }
        Ok(())
    })
    .await?;
    state.with_store(|s| s.move_secrets(Arc::new(KeyringSecrets::default())))?;
    std::fs::remove_file(v.path())
        .map_err(|e| CommandError::new("io", format!("{}: {e}", v.path().display())))?;
    *lock(&state.vault) = None;
    Ok(status(&state))
}

#[tauri::command]
pub async fn change_vault_password(
    state: State<'_, AppState>,
    current: String,
    password: String,
) -> CommandResult<VaultStatus> {
    let v = vault(&state)?;
    let check = VaultSecrets::locked(v.path());
    let target = Arc::clone(&v);
    blocking(move || {
        check.unlock(&current)?;
        if !target.is_unlocked() {
            target.unlock(&current)?;
        }
        Ok(target.change_password(&password)?)
    })
    .await?;
    Ok(status(&state))
}
