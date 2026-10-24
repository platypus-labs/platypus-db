//! Passwords kept in a file encrypted with a master password, for machines without a
//! keychain or users who want one password over all of them. The key comes from Argon2id;
//! the secrets are sealed with AES-256-GCM, a fresh nonce on every write.

use std::collections::HashMap;
use std::fmt::Write as _;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use argon2::{Algorithm, Argon2, Params, Version};
use platypus_core::Secret;
use ring::aead::{AES_256_GCM, Aad, LessSafeKey, NONCE_LEN, Nonce, UnboundKey};
use ring::rand::{SecureRandom, SystemRandom};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{Result, SecretStore, StoreError};

const FILE_VERSION: u32 = 1;
const SALT_LEN: usize = 16;
const KEY_LEN: usize = 32;

/// What the vault file holds; everything but the KDF settings is encrypted.
#[derive(Serialize, Deserialize)]
struct VaultFile {
    version: u32,
    /// Argon2id memory (KiB), iterations and lanes, so stronger settings can come later.
    m_cost: u32,
    t_cost: u32,
    p_cost: u32,
    salt: String,
    nonce: String,
    data: String,
}

struct Unlocked {
    key: [u8; KEY_LEN],
    salt: [u8; SALT_LEN],
    secrets: HashMap<Uuid, String>,
}

impl Drop for Unlocked {
    fn drop(&mut self) {
        // Leaves no copy of the key behind once locked.
        self.key.fill(0);
    }
}

/// The encrypted secrets file. Starts locked; [`VaultSecrets::unlock`] reads it.
pub struct VaultSecrets {
    path: PathBuf,
    open: Mutex<Option<Unlocked>>,
}

impl std::fmt::Debug for VaultSecrets {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("VaultSecrets")
            .field("path", &self.path)
            .field("unlocked", &self.is_unlocked())
            .finish_non_exhaustive()
    }
}

fn locked() -> StoreError {
    StoreError::Locked
}

fn corrupt(path: &Path, message: &str) -> StoreError {
    StoreError::Format {
        path: path.to_owned(),
        message: message.to_owned(),
    }
}

fn hex(bytes: &[u8]) -> String {
    bytes
        .iter()
        .fold(String::with_capacity(bytes.len() * 2), |mut s, b| {
            let _ = write!(s, "{b:02x}");
            s
        })
}

fn unhex(text: &str) -> Option<Vec<u8>> {
    if !text.len().is_multiple_of(2) {
        return None;
    }
    (0..text.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(text.get(i..i + 2)?, 16).ok())
        .collect()
}

fn random<const N: usize>() -> Result<[u8; N]> {
    let mut out = [0; N];
    SystemRandom::new()
        .fill(&mut out)
        .map_err(|_| StoreError::Invalid("no random numbers from the system".to_owned()))?;
    Ok(out)
}

fn params() -> (u32, u32, u32) {
    (
        Params::DEFAULT_M_COST,
        Params::DEFAULT_T_COST,
        Params::DEFAULT_P_COST,
    )
}

fn derive(password: &str, salt: &[u8], (m, t, p): (u32, u32, u32)) -> Result<[u8; KEY_LEN]> {
    let params = Params::new(m, t, p, Some(KEY_LEN))
        .map_err(|e| StoreError::Invalid(format!("vault settings: {e}")))?;
    let mut key = [0; KEY_LEN];
    Argon2::new(Algorithm::Argon2id, Version::V0x13, params)
        .hash_password_into(password.as_bytes(), salt, &mut key)
        .map_err(|e| StoreError::Invalid(format!("vault key: {e}")))?;
    Ok(key)
}

fn sealing_key(key: &[u8; KEY_LEN]) -> LessSafeKey {
    // A 32-byte key is always valid for AES-256.
    LessSafeKey::new(UnboundKey::new(&AES_256_GCM, key).unwrap_or_else(|_| unreachable!()))
}

impl VaultSecrets {
    /// The vault at `path`, locked. Nothing is read until [`Self::unlock`].
    #[must_use]
    pub fn locked(path: impl Into<PathBuf>) -> Self {
        Self {
            path: path.into(),
            open: Mutex::new(None),
        }
    }

    /// Creates the vault file with `secrets`, encrypted with `password`; returns it unlocked.
    pub fn create(
        path: impl Into<PathBuf>,
        password: &str,
        secrets: HashMap<Uuid, Secret>,
    ) -> Result<Self> {
        if password.is_empty() {
            return Err(StoreError::Invalid(
                "the master password cannot be empty".to_owned(),
            ));
        }
        let salt = random::<SALT_LEN>()?;
        let vault = Self::locked(path);
        let open = Unlocked {
            key: derive(password, &salt, params())?,
            salt,
            secrets: secrets
                .into_iter()
                .map(|(id, s)| (id, s.expose().to_owned()))
                .collect(),
        };
        vault.write(&open)?;
        *vault.lock_state() = Some(open);
        Ok(vault)
    }

    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    fn lock_state(&self) -> std::sync::MutexGuard<'_, Option<Unlocked>> {
        self.open
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    #[must_use]
    pub fn is_unlocked(&self) -> bool {
        self.lock_state().is_some()
    }

    /// Reads and decrypts the file; a wrong password is an error and leaves it locked.
    pub fn unlock(&self, password: &str) -> Result<()> {
        let text = std::fs::read_to_string(&self.path).map_err(|source| StoreError::Io {
            path: self.path.clone(),
            source,
        })?;
        let file: VaultFile =
            serde_json::from_str(&text).map_err(|e| corrupt(&self.path, &e.to_string()))?;
        if file.version > FILE_VERSION {
            return Err(corrupt(&self.path, "written by a newer PlatypusDB"));
        }
        let bytes = |s: &str| unhex(s).ok_or_else(|| corrupt(&self.path, "invalid hex"));
        let salt: [u8; SALT_LEN] = bytes(&file.salt)?
            .try_into()
            .map_err(|_| corrupt(&self.path, "invalid salt"))?;
        let nonce: [u8; NONCE_LEN] = bytes(&file.nonce)?
            .try_into()
            .map_err(|_| corrupt(&self.path, "invalid nonce"))?;
        let mut data = bytes(&file.data)?;
        let key = derive(password, &salt, (file.m_cost, file.t_cost, file.p_cost))?;
        let plain = sealing_key(&key)
            .open_in_place(Nonce::assume_unique_for_key(nonce), Aad::empty(), &mut data)
            .map_err(|_| StoreError::Invalid("wrong master password".to_owned()))?;
        let secrets =
            serde_json::from_slice(plain).map_err(|e| corrupt(&self.path, &e.to_string()))?;
        plain.fill(0);
        *self.lock_state() = Some(Unlocked { key, salt, secrets });
        Ok(())
    }

    /// Forgets the key and the secrets until the next unlock.
    pub fn lock(&self) {
        *self.lock_state() = None;
    }

    /// Encrypts the vault with a new password (and a new salt).
    pub fn change_password(&self, password: &str) -> Result<()> {
        if password.is_empty() {
            return Err(StoreError::Invalid(
                "the master password cannot be empty".to_owned(),
            ));
        }
        let mut state = self.lock_state();
        let open = state.as_mut().ok_or_else(locked)?;
        open.salt = random::<SALT_LEN>()?;
        open.key = derive(password, &open.salt, params())?;
        self.write(open)
    }

    /// Every secret, to move them elsewhere when the vault is turned off.
    pub fn entries(&self) -> Result<HashMap<Uuid, Secret>> {
        let state = self.lock_state();
        let open = state.as_ref().ok_or_else(locked)?;
        Ok(open
            .secrets
            .iter()
            .map(|(id, s)| (*id, Secret::new(s.clone())))
            .collect())
    }

    /// Writes atomically, sealed with a fresh nonce.
    fn write(&self, open: &Unlocked) -> Result<()> {
        let io_error = |source| StoreError::Io {
            path: self.path.clone(),
            source,
        };
        let mut data =
            serde_json::to_vec(&open.secrets).map_err(|e| StoreError::Invalid(e.to_string()))?;
        let nonce = random::<NONCE_LEN>()?;
        sealing_key(&open.key)
            .seal_in_place_append_tag(Nonce::assume_unique_for_key(nonce), Aad::empty(), &mut data)
            .map_err(|_| StoreError::Invalid("cannot encrypt the vault".to_owned()))?;
        let (m_cost, t_cost, p_cost) = params();
        let text = serde_json::to_string_pretty(&VaultFile {
            version: FILE_VERSION,
            m_cost,
            t_cost,
            p_cost,
            salt: hex(&open.salt),
            nonce: hex(&nonce),
            data: hex(&data),
        })
        .map_err(|e| StoreError::Invalid(e.to_string()))?;
        let dir = self.path.parent().unwrap_or(Path::new("."));
        std::fs::create_dir_all(dir).map_err(io_error)?;
        let mut tmp = tempfile::NamedTempFile::new_in(dir).map_err(io_error)?;
        tmp.write_all(text.as_bytes()).map_err(io_error)?;
        tmp.persist(&self.path).map_err(|e| io_error(e.error))?;
        Ok(())
    }

    fn update(&self, change: impl FnOnce(&mut HashMap<Uuid, String>) -> bool) -> Result<()> {
        let mut state = self.lock_state();
        let open = state.as_mut().ok_or_else(locked)?;
        if change(&mut open.secrets) {
            self.write(open)?;
        }
        Ok(())
    }
}

impl SecretStore for VaultSecrets {
    fn get(&self, id: Uuid) -> Result<Option<Secret>> {
        let state = self.lock_state();
        let open = state.as_ref().ok_or_else(locked)?;
        Ok(open.secrets.get(&id).map(|s| Secret::new(s.clone())))
    }

    fn set(&self, id: Uuid, secret: &Secret) -> Result<()> {
        self.update(|all| {
            all.insert(id, secret.expose().to_owned()).as_deref() != Some(secret.expose())
        })
    }

    fn delete(&self, id: Uuid) -> Result<()> {
        self.update(|all| all.remove(&id).is_some())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_and_refuses_wrong_passwords() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("secrets.vault");
        let (a, b) = (Uuid::new_v4(), Uuid::new_v4());
        let vault = VaultSecrets::create(&path, "hunter2", HashMap::from([(a, Secret::new("pg"))]))
            .unwrap();
        vault.set(b, &Secret::new("ssh")).unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(!text.contains("pg") && !text.contains("ssh"), "{text}");

        let reopened = VaultSecrets::locked(&path);
        assert!(matches!(reopened.get(a), Err(StoreError::Locked)));
        let err = reopened.unlock("wrong").unwrap_err();
        assert!(err.to_string().contains("wrong master password"), "{err}");
        assert!(!reopened.is_unlocked());
        reopened.unlock("hunter2").unwrap();
        assert_eq!(reopened.get(a).unwrap().unwrap().expose(), "pg");
        assert_eq!(reopened.get(b).unwrap().unwrap().expose(), "ssh");

        reopened.delete(a).unwrap();
        reopened.change_password("correct horse").unwrap();
        reopened.lock();
        assert!(reopened.unlock("hunter2").is_err());
        reopened.unlock("correct horse").unwrap();
        assert_eq!(reopened.get(a).unwrap(), None);
        assert_eq!(reopened.entries().unwrap().len(), 1);
        assert!(VaultSecrets::create(dir.path().join("x"), "", HashMap::new()).is_err());
    }

    #[test]
    fn detects_tampering() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("secrets.vault");
        VaultSecrets::create(
            &path,
            "pw",
            HashMap::from([(Uuid::new_v4(), Secret::new("x"))]),
        )
        .unwrap();
        let mut file: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        let data = file["data"].as_str().unwrap().to_owned();
        let flipped = if data.starts_with('0') { "1" } else { "0" };
        file["data"] = format!("{flipped}{}", &data[1..]).into();
        std::fs::write(&path, file.to_string()).unwrap();
        assert!(VaultSecrets::locked(&path).unlock("pw").is_err());
    }
}
