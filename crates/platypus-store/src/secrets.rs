use std::collections::HashMap;
use std::fmt;
use std::sync::Mutex;

use platypus_core::Secret;
use uuid::Uuid;

use crate::{Result, StoreError};

/// Where connection passwords are kept, keyed by connection id.
pub trait SecretStore: Send + Sync + fmt::Debug {
    fn get(&self, id: Uuid) -> Result<Option<Secret>>;
    fn set(&self, id: Uuid, secret: &Secret) -> Result<()>;
    /// Removing a missing secret is not an error.
    fn delete(&self, id: Uuid) -> Result<()>;
}

/// The OS keychain: Windows Credential Manager, macOS Keychain, Secret Service on Linux.
#[derive(Debug, Clone)]
pub struct KeyringSecrets {
    service: String,
}

impl KeyringSecrets {
    #[must_use]
    pub fn new(service: impl Into<String>) -> Self {
        Self {
            service: service.into(),
        }
    }

    /// `None` when the system has no keychain, e.g. Linux without a Secret Service: it holds
    /// no password then, and saving one is an error that says what to do instead.
    fn entry(&self, id: Uuid) -> Result<Option<keyring::Entry>> {
        match keyring::Entry::new(&self.service, &id.to_string()) {
            Ok(entry) => Ok(Some(entry)),
            Err(keyring::Error::NoDefaultStore) => Ok(None),
            Err(e) => Err(keychain_error(e)),
        }
    }
}

impl Default for KeyringSecrets {
    fn default() -> Self {
        Self::new("platypusdb")
    }
}

const NO_KEYCHAIN: &str = "the system has no keychain to save the password in (on Linux, a \
     Secret Service such as GNOME Keyring or KWallet): set a master password to keep \
     passwords in the app, or leave the password unsaved";

#[expect(
    clippy::needless_pass_by_value,
    reason = "used directly as a `map_err` callback"
)]
fn keychain_error(e: keyring::Error) -> StoreError {
    StoreError::Keychain(e.to_string())
}

impl SecretStore for KeyringSecrets {
    fn get(&self, id: Uuid) -> Result<Option<Secret>> {
        let Some(entry) = self.entry(id)? else {
            return Ok(None);
        };
        match entry.get_password() {
            Ok(password) => Ok(Some(Secret::new(password))),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(e) => Err(keychain_error(e)),
        }
    }

    fn set(&self, id: Uuid, secret: &Secret) -> Result<()> {
        let Some(entry) = self.entry(id)? else {
            return Err(StoreError::Keychain(NO_KEYCHAIN.to_owned()));
        };
        entry.set_password(secret.expose()).map_err(keychain_error)
    }

    fn delete(&self, id: Uuid) -> Result<()> {
        let Some(entry) = self.entry(id)? else {
            return Ok(());
        };
        match entry.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(e) => Err(keychain_error(e)),
        }
    }
}

/// In-memory secrets for tests and for sessions that should not persist passwords.
#[derive(Debug, Default)]
pub struct MemorySecrets(Mutex<HashMap<Uuid, Secret>>);

impl SecretStore for MemorySecrets {
    fn get(&self, id: Uuid) -> Result<Option<Secret>> {
        Ok(self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .get(&id)
            .cloned())
    }

    fn set(&self, id: Uuid, secret: &Secret) -> Result<()> {
        self.0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .insert(id, secret.clone());
        Ok(())
    }

    fn delete(&self, id: Uuid) -> Result<()> {
        self.0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .remove(&id);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "touches the real OS keychain"]
    fn keyring_round_trip() {
        let secrets = KeyringSecrets::new("platypusdb-test");
        let id = Uuid::new_v4();
        assert_eq!(secrets.get(id).unwrap(), None);
        secrets.set(id, &Secret::new("s3cret")).unwrap();
        assert_eq!(secrets.get(id).unwrap(), Some(Secret::new("s3cret")));
        secrets.delete(id).unwrap();
        assert_eq!(secrets.get(id).unwrap(), None);
        secrets.delete(id).unwrap();
    }

    /// Where the system has no keychain (this machine, when `store_status` fails): nothing is
    /// found and nothing to delete, and saving says what to do instead.
    #[test]
    fn without_a_keychain_nothing_is_stored() {
        if keyring::Entry::store_status().is_ok() {
            return;
        }
        let secrets = KeyringSecrets::new("platypusdb-test");
        let id = Uuid::new_v4();
        assert_eq!(secrets.get(id).unwrap(), None);
        secrets.delete(id).unwrap();
        let error = secrets.set(id, &Secret::new("s3cret")).unwrap_err();
        assert!(error.to_string().contains("master password"), "{error}");
    }
}
