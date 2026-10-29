//! The master password: saved passwords in an encrypted file instead of the OS keychain.

use std::collections::HashMap;
use std::sync::Arc;

use platypus_store::{KeyringSecrets, VaultSecrets};
use serde::Serialize;
use tauri::State;

use super::{AppState, lock};
use crate::error::{CommandError, CommandResult};
use crate::profiles::Profiles;

#[derive(Debug, Serialize)]
pub struct VaultStatus {
    /// Saved passwords are in the encrypted file.
    enabled: bool,
    /// The master password was entered in this run of the app.
    unlocked: bool,
}

fn status(state: &AppState) -> VaultStatus {
    let vault = lock(&state.vault);
    VaultStatus {
        enabled: vault.is_some(),
        unlocked: vault.as_ref().is_some_and(|v| v.is_unlocked()),
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

/// Checks `password` against the vault file even when `vault` is unlocked already, and
/// leaves it unlocked. A locked vault is checked by unlocking it: one key derivation.
fn confirm_password(vault: &VaultSecrets, password: &str) -> CommandResult<()> {
    if vault.is_unlocked() {
        VaultSecrets::locked(vault.path()).unlock(password)?;
    } else {
        vault.unlock(password)?;
    }
    Ok(())
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

/// Moves the saved passwords (and the profiles' secrets) from the keychain into a new vault.
#[tauri::command]
pub async fn enable_vault(
    state: State<'_, AppState>,
    profiles: State<'_, Profiles>,
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
    let created = Arc::new(
        blocking(move || Ok(VaultSecrets::create(path, &password, HashMap::new())?)).await?,
    );
    // A vault file left behind would be read at the next start instead of the keychain.
    if let Err(e) =
        state.with_store(|s| s.move_secrets_with(created.clone(), &profiles.secret_ids()))
    {
        let _ = std::fs::remove_file(created.path());
        return Err(e);
    }
    *lock(&state.vault) = Some(created);
    Ok(status(&state))
}

/// Moves the saved passwords (and the profiles' secrets) back to the keychain and deletes the vault.
#[tauri::command]
pub async fn disable_vault(
    state: State<'_, AppState>,
    profiles: State<'_, Profiles>,
    password: String,
) -> CommandResult<VaultStatus> {
    let v = vault(&state)?;
    // The master password is asked again, even when unlocked.
    let target = Arc::clone(&v);
    blocking(move || confirm_password(&target, &password)).await?;
    state.with_store(|s| {
        s.move_secrets_with(Arc::new(KeyringSecrets::default()), &profiles.secret_ids())
    })?;
    // The passwords are in the keychain now, whatever happens to the file.
    *lock(&state.vault) = None;
    // A file still there would be read at the next start: renamed when it cannot go.
    if let Err(e) = std::fs::remove_file(v.path()) {
        let aside = v.path().with_extension("vault.off");
        std::fs::rename(v.path(), &aside)
            .map_err(|_| CommandError::new("io", format!("{}: {e}", v.path().display())))?;
    }
    Ok(status(&state))
}

#[tauri::command]
pub async fn change_vault_password(
    state: State<'_, AppState>,
    current: String,
    password: String,
) -> CommandResult<VaultStatus> {
    let v = vault(&state)?;
    blocking(move || {
        confirm_password(&v, &current)?;
        Ok(v.change_password(&password)?)
    })
    .await?;
    Ok(status(&state))
}
