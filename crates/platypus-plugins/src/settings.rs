//! The values of plugins' settings pages. A plain value is kept in a [`SettingsStore`] file of
//! its own, `<data>/plugins/settings.toml`, and a `secret` field in the system's keychain (never
//! in a file: without a keychain it cannot be saved). Nothing here is read until a page opens or
//! a module asks.
//!
//! A module reads only the fields its own manifest declares, through the `setting` host
//! function; a page never gets a secret back, only whether it is set.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};

use platypus_core::Secret;
use platypus_store::{KeyringSecrets, SecretStore, SettingsStore, toml};
use serde::Serialize;
use serde_json::{Value, json};
use uuid::Uuid;

use crate::PluginError;
use crate::extensions::{SettingField, SettingKind};

const FILE: &str = "settings.toml";
/// The keychain's service for plugin secrets, apart from the connections' passwords.
const SECRET_SERVICE: &str = "platypusdb-plugins";
/// The longest value saved, in bytes.
const MAX_VALUE: usize = 64 << 10;

/// One change to the file at a time.
static FILE_LOCK: Mutex<()> = Mutex::new(());

/// A field as the page draws it. `value` is what is stored (never a secret), `set` whether
/// anything is: the default applies while it is not.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SettingState {
    pub key: String,
    pub value: Option<Value>,
    pub set: bool,
}

/// Where the values of a data folder's plugins are kept.
#[derive(Debug, Clone)]
pub(crate) struct SettingsFiles {
    file: PathBuf,
    secrets: Arc<dyn SecretStore>,
}

impl SettingsFiles {
    pub(crate) fn new(plugins_dir: &Path) -> Self {
        Self {
            file: plugins_dir.join(FILE),
            secrets: Arc::new(KeyringSecrets::new(SECRET_SERVICE)),
        }
    }

    pub(crate) fn with_secrets(mut self, secrets: Arc<dyn SecretStore>) -> Self {
        self.secrets = secrets;
        self
    }

    fn store(&self) -> Result<SettingsStore, PluginError> {
        SettingsStore::open(&self.file).map_err(store_error)
    }

    fn secret(&self, plugin: &str, key: &str) -> Result<Option<Secret>, PluginError> {
        self.secrets
            .get(secret_id(plugin, key))
            .map_err(store_error)
    }

    /// The fields of a plugin as its page shows them.
    pub(crate) fn states(
        &self,
        plugin: &str,
        fields: &[SettingField],
    ) -> Result<Vec<SettingState>, PluginError> {
        let store = self.store()?;
        fields
            .iter()
            .map(|field| {
                let (value, set) = if field.kind == SettingKind::Secret {
                    (None, self.secret(plugin, &field.key)?.is_some())
                } else {
                    let value = stored(&store, plugin, field);
                    let set = value.is_some();
                    (value, set)
                };
                Ok(SettingState {
                    key: field.key.clone(),
                    value,
                    set,
                })
            })
            .collect()
    }

    /// Saves a value of `field`, or with `null` removes it so that the default applies.
    pub(crate) fn set(
        &self,
        plugin: &str,
        field: &SettingField,
        value: &Value,
    ) -> Result<(), PluginError> {
        if !value.is_null() {
            if let Some(why) = field.refuses(value) {
                return Err(PluginError::Invalid(why));
            }
            if value.as_str().is_some_and(|s| s.len() > MAX_VALUE) {
                return Err(PluginError::Invalid(format!(
                    "the value of `{}` is larger than {} KB",
                    field.key,
                    MAX_VALUE >> 10
                )));
            }
        }
        if field.kind == SettingKind::Secret {
            let id = secret_id(plugin, &field.key);
            return match value.as_str() {
                Some(secret) => self.secrets.set(id, &Secret::new(secret)),
                None => self.secrets.delete(id),
            }
            .map_err(store_error);
        }
        let stored = if value.is_null() {
            None
        } else {
            Some(toml::Value::try_from(value).map_err(|e| PluginError::Invalid(e.to_string()))?)
        };
        self.write(|store| store.set(&file_key(plugin, &field.key), stored))
    }

    /// Takes the secrets of `fields` out of the keychain. Best effort: a secret left there harms
    /// nothing, as it is read only through a field.
    pub(crate) fn forget_secrets(&self, plugin: &str, fields: &[SettingField]) {
        for field in fields {
            if field.kind == SettingKind::Secret {
                let _ = self.secrets.delete(secret_id(plugin, &field.key));
            }
        }
    }

    /// Forgets everything of a plugin that is gone: its secrets and its section of the file, and
    /// the file with the last one.
    pub(crate) fn forget_plugin(
        &self,
        plugin: &str,
        fields: &[SettingField],
    ) -> Result<(), PluginError> {
        self.forget_secrets(plugin, fields);
        if !self.file.exists() {
            return Ok(());
        }
        self.write(|store| store.set(&plugin.replace('.', "~"), None))?;
        if self.store()?.all().is_empty() {
            std::fs::remove_file(&self.file).map_err(PluginError::io(&self.file))?;
        }
        Ok(())
    }

    /// Forgets what `fields` hold, as they no longer exist.
    pub(crate) fn forget(&self, plugin: &str, fields: &[SettingField]) -> Result<(), PluginError> {
        self.forget_secrets(plugin, fields);
        if !self.file.exists() {
            return Ok(());
        }
        self.write(|store| {
            fields
                .iter()
                .filter(|field| field.kind != SettingKind::Secret)
                .try_for_each(|field| store.set(&file_key(plugin, &field.key), None))
        })
    }

    fn write(
        &self,
        change: impl FnOnce(&mut SettingsStore) -> platypus_store::Result<()>,
    ) -> Result<(), PluginError> {
        let _guard = FILE_LOCK.lock().unwrap_or_else(PoisonError::into_inner);
        let mut store = self.store()?;
        if let Some(parent) = self.file.parent() {
            std::fs::create_dir_all(parent).map_err(PluginError::io(parent))?;
        }
        change(&mut store).map_err(store_error)
    }
}

/// What a sandbox may read through the `setting` host function: its own plugin's fields.
#[derive(Debug, Clone)]
pub(crate) struct SettingsAccess {
    plugin: String,
    fields: Arc<[SettingField]>,
    files: SettingsFiles,
}

impl SettingsAccess {
    pub(crate) fn new(plugin: &str, fields: &[SettingField], files: SettingsFiles) -> Self {
        Self {
            plugin: plugin.to_owned(),
            fields: fields.into(),
            files,
        }
    }

    /// The value of the field `key`: stored, or its default, or `null`. A key the plugin's
    /// manifest does not declare is refused: there is no way to name another plugin's.
    fn value(&self, key: &str) -> Result<Value, PluginError> {
        let field =
            self.fields.iter().find(|f| f.key == key).ok_or_else(|| {
                PluginError::Denied(format!("no setting `{key}` in its manifest"))
            })?;
        if field.kind == SettingKind::Secret {
            let secret = self.files.secret(&self.plugin, key)?;
            return Ok(secret.map_or(Value::Null, |s| Value::String(s.expose().to_owned())));
        }
        let store = self.files.store()?;
        Ok(stored(&store, &self.plugin, field)
            .or_else(|| field.default.clone())
            .unwrap_or(Value::Null))
    }
}

/// The value of the field `key` for a plugin's page in a frame: a secret is for its module only,
/// which has no way to show it, and a page is refused it.
pub(crate) fn plain_value(access: &SettingsAccess, key: &str) -> Result<Value, PluginError> {
    if access
        .fields
        .iter()
        .any(|f| f.key == key && f.kind == SettingKind::Secret)
    {
        return Err(PluginError::Denied(format!(
            "`{key}` is a secret: a page cannot read it"
        )));
    }
    access.value(key)
}

/// The answer to a `setting` call of a guest: `{"ok": value}` or `{"error": message}`. The
/// access is `None` under a sandbox without any settings.
pub(crate) fn answer(access: Option<&SettingsAccess>, request: &[u8]) -> Value {
    let Some(access) = access else {
        return json!({ "error": "not allowed: this plugin has no settings" });
    };
    let key = serde_json::from_slice::<Value>(request)
        .ok()
        .and_then(|r| r.get("key").and_then(Value::as_str).map(str::to_owned));
    let Some(key) = key else {
        return json!({ "error": "the request must be {\"key\": name}" });
    };
    match access.value(&key) {
        Ok(value) => json!({ "ok": value }),
        Err(e) => json!({ "error": e.to_string() }),
    }
}

/// The stored value of `field`, a default not included; one of another type (the file edited by
/// hand) does not count.
fn stored(store: &SettingsStore, plugin: &str, field: &SettingField) -> Option<Value> {
    store
        .get::<Value>(&file_key(plugin, &field.key))
        .filter(|value| field.refuses(value).is_none())
}

/// The dotted key of a value: a section per plugin. An id may hold dots, which the store reads
/// as sections and a setting's key does not, so they are written as a `~`, which no id has.
fn file_key(plugin: &str, key: &str) -> String {
    format!("{}.{key}", plugin.replace('.', "~"))
}

/// The keychain's id of a secret, from its plugin and key: both plain names, so that the pair
/// cannot be read in two ways.
fn secret_id(plugin: &str, key: &str) -> Uuid {
    let digest = ring::digest::digest(&ring::digest::SHA256, format!("{plugin}\0{key}").as_bytes());
    let mut bytes = [0; 16];
    bytes.copy_from_slice(&digest.as_ref()[..16]);
    Uuid::from_bytes(bytes)
}

#[expect(
    clippy::needless_pass_by_value,
    reason = "used directly as a `map_err` callback"
)]
fn store_error(e: platypus_store::StoreError) -> PluginError {
    PluginError::Invalid(e.to_string())
}

#[cfg(test)]
mod tests {
    use platypus_store::MemorySecrets;

    use super::*;
    use crate::manifest::Manifest;

    const MANIFEST: &str = r#"
        id = "a.b"
        name = "A"
        version = "1"
        api = 2
        kinds = ["actions"]

        [[settings]]
        key = "url"
        label = "URL"
        type = "text"
        default = "https://default"

        [[settings]]
        key = "retries"
        label = "Retries"
        type = "number"
        min = 0
        max = 5

        [[settings]]
        key = "on"
        label = "On"
        type = "boolean"

        [[settings]]
        key = "mode"
        label = "Mode"
        type = "choice"
        options = [{ value = "x", label = "X" }]

        [[settings]]
        key = "token"
        label = "Token"
        type = "secret"
    "#;

    fn fields() -> Vec<SettingField> {
        Manifest::parse(MANIFEST).unwrap().settings
    }

    fn files(dir: &Path) -> SettingsFiles {
        SettingsFiles::new(dir).with_secrets(Arc::new(MemorySecrets::default()))
    }

    fn field<'a>(fields: &'a [SettingField], key: &str) -> &'a SettingField {
        fields.iter().find(|f| f.key == key).unwrap()
    }

    #[test]
    fn a_secret_goes_to_the_keychain_and_never_to_a_file_or_the_page() {
        let dir = tempfile::tempdir().unwrap();
        let (files, fields) = (files(dir.path()), fields());
        files
            .set("a.b", field(&fields, "token"), &json!("s3cret"))
            .unwrap();
        files
            .set("a.b", field(&fields, "url"), &json!("https://x"))
            .unwrap();
        let on_disk = std::fs::read_to_string(dir.path().join(FILE)).unwrap();
        assert!(on_disk.contains("https://x"), "{on_disk}");
        assert!(!on_disk.contains("s3cret"), "{on_disk}");

        let states = files.states("a.b", &fields).unwrap();
        let token = states.iter().find(|s| s.key == "token").unwrap();
        assert!(token.set);
        assert_eq!(token.value, None, "a page never reads a secret back");

        let access = SettingsAccess::new("a.b", &fields, files.clone());
        assert_eq!(access.value("token").unwrap(), json!("s3cret"));

        files
            .set("a.b", field(&fields, "token"), &Value::Null)
            .unwrap();
        assert_eq!(access.value("token").unwrap(), Value::Null);
    }

    #[test]
    fn a_module_reads_values_and_defaults_of_its_own_fields_only() {
        let dir = tempfile::tempdir().unwrap();
        let (files, fields) = (files(dir.path()), fields());
        let access = SettingsAccess::new("a.b", &fields, files.clone());
        assert_eq!(access.value("url").unwrap(), json!("https://default"));
        assert_eq!(access.value("retries").unwrap(), Value::Null);
        files
            .set("a.b", field(&fields, "retries"), &json!(3))
            .unwrap();
        assert_eq!(access.value("retries").unwrap(), json!(3));

        // Another plugin's value, under the same key, is out of reach.
        let other = SettingsAccess::new("other", &fields, files.clone());
        assert_eq!(other.value("retries").unwrap(), Value::Null);
        let answer_of =
            |key: &str| answer(Some(&other), json!({ "key": key }).to_string().as_bytes());
        assert_eq!(answer_of("url")["ok"], "https://default");
        assert!(
            answer_of("a.b.retries")["error"]
                .as_str()
                .unwrap()
                .contains("not allowed"),
        );
        assert!(answer(Some(&other), b"nonsense")["error"].is_string());
        assert!(
            answer(None, b"{\"key\":\"url\"}")["error"]
                .as_str()
                .unwrap()
                .contains("not allowed")
        );
    }

    #[test]
    fn refuses_values_that_do_not_fit() {
        let dir = tempfile::tempdir().unwrap();
        let (files, fields) = (files(dir.path()), fields());
        for (key, value, why) in [
            ("url", json!(1), "not a valid text"),
            ("retries", json!("x"), "not a valid number"),
            ("retries", json!(9), "out of range"),
            ("retries", json!(-1), "out of range"),
            ("on", json!("yes"), "not a valid boolean"),
            ("mode", json!("z"), "not a valid choice"),
            ("token", json!(5), "not a valid secret"),
        ] {
            let e = files
                .set("a.b", field(&fields, key), &value)
                .unwrap_err()
                .to_string();
            assert!(e.contains(why), "{key}: {e}");
        }
        let long = "x".repeat(MAX_VALUE + 1);
        let e = files
            .set("a.b", field(&fields, "url"), &json!(long))
            .unwrap_err()
            .to_string();
        assert!(e.contains("larger"), "{e}");
    }

    #[test]
    fn a_value_edited_by_hand_into_another_type_does_not_count() {
        let dir = tempfile::tempdir().unwrap();
        let (files, fields) = (files(dir.path()), fields());
        std::fs::write(dir.path().join(FILE), "[\"a~b\"]\nurl = 5\nretries = 99\n").unwrap();
        let access = SettingsAccess::new("a.b", &fields, files.clone());
        assert_eq!(access.value("url").unwrap(), json!("https://default"));
        assert_eq!(access.value("retries").unwrap(), Value::Null);
        let states = files.states("a.b", &fields).unwrap();
        assert!(states.iter().all(|s| !s.set));
    }

    #[test]
    fn forgets_a_plugins_values() {
        let dir = tempfile::tempdir().unwrap();
        let (files, fields) = (files(dir.path()), fields());
        files
            .set("a.b", field(&fields, "url"), &json!("https://x"))
            .unwrap();
        files
            .set("a.b", field(&fields, "token"), &json!("s"))
            .unwrap();
        files
            .set("a", field(&fields, "url"), &json!("https://kept"))
            .unwrap();
        files.forget("a.b", &fields).unwrap();
        let states = files.states("a.b", &fields).unwrap();
        assert!(states.iter().all(|s| !s.set), "{states:?}");
        let kept = files.states("a", &fields).unwrap();
        assert!(kept.iter().find(|s| s.key == "url").unwrap().set);
    }

    #[test]
    fn secrets_of_two_plugins_or_keys_have_different_ids() {
        assert_ne!(secret_id("a", "b"), secret_id("a", "c"));
        assert_ne!(secret_id("a", "b"), secret_id("b", "a"));
        assert_eq!(secret_id("a", "b"), secret_id("a", "b"));
    }
}
