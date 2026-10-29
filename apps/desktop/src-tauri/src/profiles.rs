//! Saved transfer profiles: an export, an import, a table transfer or a migration with all
//! its choices, named so that it runs again (by hand, or from a scheduled task) without the
//! dialog. They are kept in `transfer-profiles.json` in the data folder; the runs themselves
//! are in `commands::profiles`.
//!
//! The dialogs' own payloads (`ExportOptions`, `ImportPlan`) only deserialize, so a profile
//! keeps them as the JSON the dialog sent and checks, at each save, that they read back.
//! What the dialog does not send (`ImportPlan`'s identity and constraints) is not in a plan
//! the dialog sends either, so a saved plan runs as the dialog's would.
//!
//! The secrets of an export (the secret key of its bucket, the signed URL of its upload)
//! never reach the file: each is kept under an id derived from the profile's id, in the store
//! of the connections' passwords. The file keeps `KEPT_URL` where the URL was.

use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard, PoisonError};

use chrono::{DateTime, Utc};
use platypus_core::Secret;
use platypus_transfer::ExportOptions;
use platypus_transfer::import::ImportPlan;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

use crate::commands::export::TableRef;
use crate::commands::transfer::FileOptions;
use crate::scheduler::plan::Schedule;

/// What a profile does, with the choices of the dialog it was saved from.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ProfileTask {
    /// Writes the rows of a query to `path`.
    ExportQuery {
        connection_id: Uuid,
        #[serde(default)]
        database: Option<String>,
        sql: String,
        path: PathBuf,
        /// An `ExportOptions`, as the export dialog sends it.
        #[serde(default)]
        options: Value,
    },
    /// Writes each of `tables` to a file of `dir`.
    ExportTables {
        connection_id: Uuid,
        #[serde(default)]
        database: Option<String>,
        tables: Vec<TableRef>,
        dir: PathBuf,
        #[serde(default)]
        options: Value,
    },
    /// Imports the file at `path` as `plan` says.
    ImportFile {
        connection_id: Uuid,
        #[serde(default)]
        database: Option<String>,
        path: PathBuf,
        /// An `ImportPlan`, as the import dialog sends it.
        plan: Value,
        #[serde(default)]
        options: FileOptions,
    },
    /// Copies a table to another database as `plan` says.
    TransferTable {
        source_connection_id: Uuid,
        #[serde(default)]
        source_database: Option<String>,
        target_connection_id: Uuid,
        #[serde(default)]
        target_database: Option<String>,
        plan: Value,
    },
    /// Creates `tables` of a schema in another database and copies their rows.
    Migrate {
        source_connection_id: Uuid,
        #[serde(default)]
        source_database: Option<String>,
        target_connection_id: Uuid,
        #[serde(default)]
        target_database: Option<String>,
        source_schema: String,
        target_schema: String,
        /// The tables to copy; `None` is the whole schema, read when the profile runs, so the
        /// tables added later are copied too.
        #[serde(default)]
        tables: Option<Vec<String>>,
        /// The schema's views and routines too.
        #[serde(default)]
        objects: bool,
    },
}

/// A task as saved, with its name.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TransferProfile {
    pub id: Uuid,
    pub name: String,
    pub task: ProfileTask,
}

/// An `ExportOptions` read from the JSON of the dialog; none means the defaults. The bucket's
/// secret key may be missing (it is kept apart): it reads as empty then.
fn export_options(options: &Value) -> Result<ExportOptions, String> {
    if options.is_null() {
        return Ok(ExportOptions::default());
    }
    let mut options = options.clone();
    if let Some(bucket) = options.get_mut("bucket").and_then(Value::as_object_mut) {
        bucket
            .entry("secret_key")
            .or_insert_with(|| Value::String(String::new()));
    }
    serde_json::from_value(options).map_err(|e| format!("the export options: {e}"))
}

/// An `ImportPlan` read from the JSON of the dialog.
pub fn import_plan(plan: &Value) -> Result<ImportPlan, String> {
    serde_json::from_value(plan.clone()).map_err(|e| format!("the import plan: {e}"))
}

/// The bucket of export `options`, as the object that holds its keys.
fn bucket(options: &mut Value) -> Option<&mut serde_json::Map<String, Value>> {
    options.get_mut("bucket")?.as_object_mut()
}

impl ProfileTask {
    /// The connections the task uses, the first being where its data is read or written.
    pub fn connections(&self) -> Vec<Uuid> {
        match self {
            Self::ExportQuery { connection_id, .. }
            | Self::ExportTables { connection_id, .. }
            | Self::ImportFile { connection_id, .. } => vec![*connection_id],
            Self::TransferTable {
                source_connection_id,
                target_connection_id,
                ..
            }
            | Self::Migrate {
                source_connection_id,
                target_connection_id,
                ..
            } => vec![*source_connection_id, *target_connection_id],
        }
    }

    /// The first connection and its database: what a scheduled task keeps as its own.
    pub fn first_connection(&self) -> (Uuid, Option<String>) {
        match self {
            Self::ExportQuery {
                connection_id,
                database,
                ..
            }
            | Self::ExportTables {
                connection_id,
                database,
                ..
            }
            | Self::ImportFile {
                connection_id,
                database,
                ..
            } => (*connection_id, database.clone()),
            Self::TransferTable {
                source_connection_id,
                source_database,
                ..
            }
            | Self::Migrate {
                source_connection_id,
                source_database,
                ..
            } => (*source_connection_id, source_database.clone()),
        }
    }

    /// The options of an export.
    fn export_options_mut(&mut self) -> Option<&mut Value> {
        match self {
            Self::ExportQuery { options, .. } | Self::ExportTables { options, .. } => Some(options),
            _ => None,
        }
    }

    /// The options of an export, with the defaults for what the dialog left out.
    pub fn export_options(&self) -> Result<Option<ExportOptions>, String> {
        match self {
            Self::ExportQuery { options, .. } | Self::ExportTables { options, .. } => {
                export_options(options).map(Some)
            }
            _ => Ok(None),
        }
    }

    /// Whether the export also sends its files to a bucket.
    fn has_bucket(&mut self) -> bool {
        self.export_options_mut()
            .is_some_and(|o| bucket(o).is_some())
    }

    /// The signed URL of the export, when it has one.
    fn upload_url_mut(&mut self) -> Option<&mut Value> {
        let url = self.export_options_mut()?.get_mut("upload_url")?;
        url.as_str().is_some_and(|u| !u.is_empty()).then_some(url)
    }

    /// Whether the export also sends its file to a signed URL (kept apart or not).
    fn has_url(&mut self) -> bool {
        self.upload_url_mut().is_some()
    }

    /// Takes the signed URL out of the task, leaving `KEPT_URL` in its place. `None` when
    /// there is none, or when it is `KEPT_URL` already (the page saves the profile again as
    /// it opened it).
    fn take_url(&mut self) -> Option<String> {
        let url = self.upload_url_mut()?;
        let text = url.as_str().filter(|u| *u != KEPT_URL)?.to_owned();
        *url = Value::String(KEPT_URL.to_owned());
        Some(text)
    }

    /// Puts the signed URL back, for a run.
    fn put_url(&mut self, url: &str) {
        if let Some(value) = self.upload_url_mut() {
            *value = Value::String(url.to_owned());
        }
    }

    /// Takes the bucket's secret key out of the task.
    fn take_key(&mut self) -> Option<String> {
        let key = bucket(self.export_options_mut()?)?.remove("secret_key")?;
        key.as_str().filter(|k| !k.is_empty()).map(str::to_owned)
    }

    /// Puts the bucket's secret key back, for a run.
    fn put_key(&mut self, key: &str) {
        if let Some(bucket) = self.export_options_mut().and_then(bucket) {
            bucket.insert("secret_key".to_owned(), Value::String(key.to_owned()));
        }
    }

    /// What is wrong with the task, if anything: it would not run, or not do its work.
    fn problem(&self) -> Option<String> {
        fn empty(path: &Path) -> bool {
            path.as_os_str().is_empty()
        }
        let nothing = |what: &str| Some(format!("choose {what}"));
        match self {
            Self::ExportQuery { sql, path, .. } => {
                if sql.trim().is_empty() {
                    return Some("write the query to export".to_owned());
                }
                if empty(path) {
                    return nothing("the file to write");
                }
            }
            Self::ExportTables { tables, dir, .. } => {
                if tables.is_empty() {
                    return nothing("the tables to export");
                }
                if empty(dir) {
                    return nothing("the folder to write to");
                }
            }
            Self::ImportFile { path, .. } if empty(path) => return nothing("the file to import"),
            Self::Migrate {
                source_schema,
                target_schema,
                tables,
                ..
            } => {
                if tables.as_ref().is_some_and(Vec::is_empty) {
                    return nothing("the tables to migrate");
                }
                if source_schema.trim().is_empty() || target_schema.trim().is_empty() {
                    return nothing("the schemas to migrate from and to");
                }
            }
            _ => {}
        }
        match self {
            Self::ExportQuery { options, .. } | Self::ExportTables { options, .. } => {
                export_options(options).err()
            }
            Self::ImportFile { plan, .. } | Self::TransferTable { plan, .. } => {
                import_plan(plan).err()
            }
            Self::Migrate { .. } => None,
        }
    }

    /// The file or folder a run started at `at` of a scheduled `schedule` writes, when the
    /// schedule has a folder: an export's files go into it named as the task's other files,
    /// `name-20261029-070000.csv`; an export of tables writes its files into a folder of
    /// that name with the extension `tables`. `None` for the tasks that write no file, and
    /// for a schedule without a folder: the task writes where its profile says.
    pub fn scheduled_output(
        &self,
        schedule: &Schedule,
        at: DateTime<Utc>,
    ) -> Result<Option<PathBuf>, String> {
        Ok(self
            .scheduled_extension()?
            .and_then(|extension| schedule.file_at(at, &extension)))
    }

    /// The extension of what a scheduled run of the task writes: the file of a query, the
    /// folder of tables; none for the tasks that write no file.
    pub fn scheduled_extension(&self) -> Result<Option<String>, String> {
        match self {
            Self::ExportQuery { .. } => {
                let options = self.export_options()?.unwrap_or_default();
                Ok(Some(format!(
                    "{}{}",
                    options.format.extension(),
                    options.compression.suffix()
                )))
            }
            Self::ExportTables { .. } => Ok(Some(TABLES_EXTENSION.to_owned())),
            _ => Ok(None),
        }
    }

    /// Where an export writes, as the profile says: a file, or the folder of its files.
    pub fn own_output(&self) -> Option<&Path> {
        match self {
            Self::ExportQuery { path, .. } => Some(path),
            Self::ExportTables { dir, .. } => Some(dir),
            _ => None,
        }
    }
}

/// The extension of the folder a scheduled export of tables writes into.
pub const TABLES_EXTENSION: &str = "tables";

/// Where the secret keys of the profiles are kept: the store of the passwords in the app.
pub trait KeyStore {
    fn get(&self, id: Uuid) -> Result<Option<Secret>, String>;
    /// Sets the key, or removes it with `None`.
    fn set(&self, id: Uuid, key: Option<&Secret>) -> Result<(), String>;
}

/// What the file keeps in place of the signed URL of an upload; the page shows it, and a
/// profile saved with it keeps the URL it has.
pub const KEPT_URL: &str = "(saved apart)";

/// The id a profile's bucket key is kept under.
fn key_id(profile: Uuid) -> Uuid {
    Uuid::from_u128(profile.as_u128() ^ 0x6275_636b_6574_2d6b_6579_0000_0000_0000)
}

/// The id a profile's signed URL is kept under.
fn url_id(profile: Uuid) -> Uuid {
    Uuid::from_u128(profile.as_u128() ^ 0x7369_676e_6564_2d75_726c_0000_0000_0000)
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct Saved {
    profiles: Vec<TransferProfile>,
}

/// The saved profiles.
#[derive(Debug)]
pub struct Profiles {
    /// `None` without a data folder: the profiles then live until the app closes.
    path: Option<PathBuf>,
    saved: Mutex<Saved>,
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

impl Profiles {
    /// The profiles saved at `path`; none when it does not exist. One that cannot be read is
    /// moved aside to `transfer-profiles.json.unreadable` rather than written over.
    pub fn open(path: Option<PathBuf>) -> Self {
        let saved = path
            .as_ref()
            .and_then(|p| Some((p, std::fs::read_to_string(p).ok()?)))
            .and_then(|(p, text)| match serde_json::from_str(&text) {
                Ok(saved) => Some(saved),
                Err(e) => {
                    let aside = p.with_extension("json.unreadable");
                    let _ = std::fs::rename(p, &aside);
                    crate::applog::write(
                        "WARN",
                        &format!(
                            "transfer-profiles.json cannot be read ({e}): moved to {}",
                            aside.display()
                        ),
                    );
                    None
                }
            })
            .unwrap_or_default();
        Self {
            path,
            saved: Mutex::new(saved),
        }
    }

    fn write(&self, saved: &Saved) -> Result<(), String> {
        let Some(path) = &self.path else {
            return Ok(());
        };
        let text = serde_json::to_string_pretty(saved).map_err(|e| e.to_string())?;
        // Written aside, then moved: a crash halfway leaves the old file whole.
        let aside = path.with_extension("json.tmp");
        std::fs::write(&aside, text)
            .and_then(|()| std::fs::rename(&aside, path))
            .map_err(|e| format!("{}: {e}", path.display()))
    }

    /// The profiles, without their secret keys.
    pub fn list(&self) -> Vec<TransferProfile> {
        lock(&self.saved).profiles.clone()
    }

    /// The profile `id`, without its secret key.
    pub fn get(&self, id: Uuid) -> Option<TransferProfile> {
        lock(&self.saved)
            .profiles
            .iter()
            .find(|p| p.id == id)
            .cloned()
    }

    /// The ids the secrets of the profiles are kept under, for a store that moves them with
    /// the connections' passwords. Ids with nothing behind them are harmless.
    pub fn secret_ids(&self) -> Vec<Uuid> {
        lock(&self.saved)
            .profiles
            .iter()
            .flat_map(|p| [key_id(p.id), url_id(p.id)])
            .collect()
    }

    /// The profile `id` ready to run: its bucket's secret key and signed URL are back in it.
    pub fn restored(&self, id: Uuid, keys: &dyn KeyStore) -> Result<TransferProfile, String> {
        let mut profile = self.get(id).ok_or("the profile is gone")?;
        if profile.task.has_bucket() {
            let key = keys.get(key_id(id))?.ok_or(
                "the secret key of the bucket is not saved: open the profile and enter it again",
            )?;
            profile.task.put_key(key.expose());
        }
        if profile.task.has_url() {
            let url = keys
                .get(url_id(id))?
                .ok_or("the signed URL is not saved: open the profile and enter it again")?;
            profile.task.put_url(url.expose());
        }
        Ok(profile)
    }

    /// Saves `profile`, new or changed, and returns it as kept: without the secret key of
    /// its bucket, which goes to `keys`. A profile saved again without a key keeps the one
    /// it has; one without a bucket loses it. `known` says whether a connection is saved.
    pub fn put(
        &self,
        mut profile: TransferProfile,
        known: impl Fn(Uuid) -> bool,
        keys: &dyn KeyStore,
    ) -> Result<TransferProfile, String> {
        profile.name = profile.name.trim().into();
        if profile.name.is_empty() {
            return Err("the profile needs a name".to_owned());
        }
        if let Some(problem) = profile.task.problem() {
            return Err(problem);
        }
        if profile.task.connections().into_iter().any(|c| !known(c)) {
            return Err("the connection of the profile is gone".to_owned());
        }
        let mut saved = lock(&self.saved);
        let lower = profile.name.to_lowercase();
        if let Some(other) = saved
            .profiles
            .iter()
            .find(|p| p.id != profile.id && p.name.to_lowercase() == lower)
        {
            return Err(format!(
                "{} is a profile already: choose another name",
                other.name
            ));
        }
        // Each secret is new (`Some`), kept as it is, or, without its option, removed. A
        // missing one is refused before anything is stored.
        let wanted = [
            (
                key_id(profile.id),
                profile.task.take_key(),
                profile.task.has_bucket(),
                "enter the secret key of the bucket",
            ),
            (
                url_id(profile.id),
                profile.task.take_url(),
                profile.task.has_url(),
                "enter the signed URL",
            ),
        ];
        for (id, new, used, problem) in &wanted {
            if *used && new.is_none() && keys.get(*id)?.is_none() {
                return Err((*problem).to_owned());
            }
        }
        for (id, new, used, _) in wanted {
            match new {
                Some(secret) => keys.set(id, Some(&Secret::new(secret)))?,
                None if !used => keys.set(id, None)?,
                None => {}
            }
        }
        match saved.profiles.iter_mut().find(|p| p.id == profile.id) {
            Some(old) => *old = profile.clone(),
            None => saved.profiles.push(profile.clone()),
        }
        self.write(&saved)?;
        Ok(profile)
    }

    /// Deletes the profile `id` and its secrets.
    pub fn delete(&self, id: Uuid, keys: &dyn KeyStore) -> Result<(), String> {
        let mut saved = lock(&self.saved);
        saved.profiles.retain(|p| p.id != id);
        keys.set(key_id(id), None)?;
        keys.set(url_id(id), None)?;
        self.write(&saved)
    }

    /// Checks that a scheduled task of profile `profile_id` can run, and gives it the
    /// profile's first connection and database as its own, so that what reads a task's
    /// connection (its list, its runs) keeps working.
    pub fn follow(&self, schedule: &mut Schedule, profile_id: Uuid) -> Result<(), String> {
        let profile = self
            .get(profile_id)
            .ok_or("the profile of the task is gone: choose another")?;
        (schedule.connection_id, schedule.database) = profile.task.first_connection();
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use platypus_store::{MemorySecrets, SecretStore};

    use super::*;
    use crate::scheduler::plan::{Repeat, Task};

    /// The keys in memory, as the connections' passwords are in the tests of the store.
    #[derive(Default)]
    struct Keys(MemorySecrets);

    impl KeyStore for Keys {
        fn get(&self, id: Uuid) -> Result<Option<Secret>, String> {
            self.0.get(id).map_err(|e| e.to_string())
        }

        fn set(&self, id: Uuid, key: Option<&Secret>) -> Result<(), String> {
            match key {
                Some(key) => self.0.set(id, key),
                None => self.0.delete(id),
            }
            .map_err(|e| e.to_string())
        }
    }

    fn everything(_: Uuid) -> bool {
        true
    }

    fn query(options: Value) -> ProfileTask {
        ProfileTask::ExportQuery {
            connection_id: Uuid::new_v4(),
            database: Some("sales".to_owned()),
            sql: "SELECT * FROM orders".to_owned(),
            path: PathBuf::from("/out/orders.csv"),
            options,
        }
    }

    fn profile(name: &str, task: ProfileTask) -> TransferProfile {
        TransferProfile {
            id: Uuid::new_v4(),
            name: name.to_owned(),
            task,
        }
    }

    fn with_bucket(key: &str) -> Value {
        serde_json::json!({
            "format": "parquet",
            "bucket": {
                "endpoint": "https://s3.eu-west-1.amazonaws.com",
                "region": "eu-west-1",
                "bucket": "backups",
                "access_key": "AKIA",
                "secret_key": key,
            }
        })
    }

    fn plan() -> Value {
        serde_json::json!({
            "source_table": "orders",
            "schema": "public",
            "table": "orders",
            "create": true,
            "mode": "upsert",
            "keys": ["id"],
            "columns": [
                { "source": "id", "target": "id", "data_type": "bigint", "not_null": true },
                { "source": "total", "target": "total" }
            ],
            "primary_key": ["id"]
        })
    }

    fn all_kinds() -> Vec<ProfileTask> {
        let (a, b) = (Uuid::new_v4(), Uuid::new_v4());
        vec![
            query(Value::Null),
            ProfileTask::ExportTables {
                connection_id: a,
                database: None,
                tables: vec![TableRef {
                    schema: Some("public".to_owned()),
                    name: "orders".to_owned(),
                }],
                dir: PathBuf::from("/out"),
                options: serde_json::json!({ "format": "json", "compression": "gzip" }),
            },
            ProfileTask::ImportFile {
                connection_id: a,
                database: None,
                path: PathBuf::from("/in/orders.csv"),
                plan: plan(),
                options: FileOptions {
                    encoding: Some("windows-1252".to_owned()),
                    skip_bad_lines: true,
                },
            },
            ProfileTask::TransferTable {
                source_connection_id: a,
                source_database: Some("one".to_owned()),
                target_connection_id: b,
                target_database: None,
                plan: plan(),
            },
            ProfileTask::Migrate {
                source_connection_id: a,
                source_database: None,
                target_connection_id: b,
                target_database: Some("two".to_owned()),
                source_schema: "public".to_owned(),
                target_schema: "dbo".to_owned(),
                tables: Some(vec!["orders".to_owned()]),
                objects: true,
            },
        ]
    }

    #[test]
    fn every_kind_survives_the_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("transfer-profiles.json");
        let profiles = Profiles::open(Some(path.clone()));
        let keys = Keys::default();
        let mut saved = Vec::new();
        for (i, task) in all_kinds().into_iter().enumerate() {
            let p = profile(&format!("Profile {i}"), task);
            saved.push(profiles.put(p, everything, &keys).unwrap());
        }
        let reopened = Profiles::open(Some(path.clone()));
        assert_eq!(reopened.list(), saved);
        let text = std::fs::read_to_string(&path).unwrap();
        for kind in [
            "export_query",
            "export_tables",
            "import_file",
            "transfer_table",
            "migrate",
        ] {
            assert!(text.contains(&format!("\"kind\": \"{kind}\"")), "{kind}");
        }
        // A saved plan reads back to the plan the dialog sent, and runs as it would.
        let ProfileTask::ImportFile { plan: kept, .. } = &reopened.list()[2].task else {
            panic!("an import");
        };
        let read = import_plan(kept).unwrap();
        assert_eq!(read, import_plan(&plan()).unwrap());
        assert_eq!((read.identity, read.constraints.len()), (None, 0));
    }

    #[test]
    fn the_secret_key_stays_out_of_the_file_and_comes_back_for_a_run() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("transfer-profiles.json");
        let profiles = Profiles::open(Some(path.clone()));
        let keys = Keys::default();
        let p = profile("To the cloud", query(with_bucket("s3cr3t-value")));
        let id = p.id;
        let returned = profiles.put(p, everything, &keys).unwrap();

        let text = std::fs::read_to_string(&path).unwrap();
        assert!(!text.contains("s3cr3t-value"), "{text}");
        assert!(!text.contains("secret_key"), "{text}");
        assert!(text.contains("AKIA"), "the other keys stay");
        assert!(!format!("{returned:?}").contains("s3cr3t-value"));
        assert!(!format!("{:?}", profiles.get(id)).contains("s3cr3t-value"));

        let run = Profiles::open(Some(path.clone()))
            .restored(id, &keys)
            .unwrap();
        let options = run.task.export_options().unwrap().unwrap();
        assert_eq!(options.bucket.unwrap().secret_key, "s3cr3t-value");

        // Saved again as the page has it, without a key: the key stays.
        profiles.put(returned.clone(), everything, &keys).unwrap();
        assert!(profiles.restored(id, &keys).is_ok());
        // A new key replaces it.
        let mut again = returned.clone();
        again.task.put_key("second");
        profiles.put(again, everything, &keys).unwrap();
        let options = profiles.restored(id, &keys).unwrap();
        assert_eq!(
            options
                .task
                .export_options()
                .unwrap()
                .unwrap()
                .bucket
                .unwrap()
                .secret_key,
            "second"
        );
        // Without its bucket the profile drops the key; with it back and no key, it is told.
        let mut plain = returned.clone();
        let ProfileTask::ExportQuery { options, .. } = &mut plain.task else {
            panic!("a query");
        };
        *options = Value::Null;
        profiles.put(plain, everything, &keys).unwrap();
        assert_eq!(keys.get(key_id(id)).unwrap(), None);
        assert!(
            profiles
                .put(returned.clone(), everything, &keys)
                .unwrap_err()
                .contains("secret key"),
            "a bucket needs its key"
        );
        profiles
            .put(profile("Other", query(with_bucket("k"))), everything, &keys)
            .unwrap();
        let other = profiles.list().pop().unwrap().id;
        keys.set(key_id(other), None).unwrap();
        assert!(
            profiles
                .restored(other, &keys)
                .unwrap_err()
                .contains("secret key")
        );

        profiles.delete(id, &keys).unwrap();
        assert!(profiles.get(id).is_none());
        assert_eq!(keys.get(key_id(id)).unwrap(), None);
    }

    fn with_url(url: &str) -> Value {
        serde_json::json!({ "format": "csv", "upload_url": url })
    }

    fn url_of(profiles: &Profiles, id: Uuid) -> Value {
        let ProfileTask::ExportQuery { options, .. } = profiles.get(id).unwrap().task else {
            panic!("a query");
        };
        options["upload_url"].clone()
    }

    #[test]
    fn the_signed_url_stays_out_of_the_file_and_comes_back_for_a_run() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("transfer-profiles.json");
        let profiles = Profiles::open(Some(path.clone()));
        let keys = Keys::default();
        let signed = "https://b.s3.amazonaws.com/f.csv?X-Amz-Signature=abc123";
        let p = profile("Signed", query(with_url(signed)));
        let id = p.id;
        let returned = profiles.put(p, everything, &keys).unwrap();

        let text = std::fs::read_to_string(&path).unwrap();
        assert!(
            !text.contains("abc123") && !text.contains("amazonaws"),
            "{text}"
        );
        assert!(text.contains(KEPT_URL), "{text}");
        assert!(!format!("{returned:?}").contains("abc123"));
        assert_eq!(url_of(&profiles, id), KEPT_URL);

        let run = Profiles::open(Some(path)).restored(id, &keys).unwrap();
        assert_eq!(
            run.task
                .export_options()
                .unwrap()
                .unwrap()
                .upload_url
                .as_deref(),
            Some(signed)
        );

        // Saved again as the page has it, with the placeholder: the URL stays.
        profiles.put(returned.clone(), everything, &keys).unwrap();
        let run = profiles.restored(id, &keys).unwrap();
        assert_eq!(
            run.task
                .export_options()
                .unwrap()
                .unwrap()
                .upload_url
                .as_deref(),
            Some(signed)
        );
        // A new URL replaces it.
        let mut again = returned.clone();
        again.task.put_url("https://b.s3.amazonaws.com/g.csv?sig=2");
        profiles.put(again, everything, &keys).unwrap();
        let run = profiles.restored(id, &keys).unwrap();
        assert_eq!(
            run.task
                .export_options()
                .unwrap()
                .unwrap()
                .upload_url
                .as_deref(),
            Some("https://b.s3.amazonaws.com/g.csv?sig=2")
        );
        // The ids to move with the other secrets are the profile's.
        assert!(profiles.secret_ids().contains(&url_id(id)));
        assert!(keys.get(url_id(id)).unwrap().is_some());

        // Without the URL the profile drops it; with the placeholder and no URL, it is told.
        let mut plain = returned.clone();
        let ProfileTask::ExportQuery { options, .. } = &mut plain.task else {
            panic!("a query");
        };
        *options = Value::Null;
        profiles.put(plain, everything, &keys).unwrap();
        assert_eq!(keys.get(url_id(id)).unwrap(), None);
        assert!(
            profiles
                .put(returned, everything, &keys)
                .unwrap_err()
                .contains("signed URL")
        );

        // Deleted, the URL goes too.
        let mut back = profile("Back", query(with_url(signed)));
        back.id = id;
        profiles.put(back, everything, &keys).unwrap();
        assert!(keys.get(url_id(id)).unwrap().is_some());
        profiles.delete(id, &keys).unwrap();
        assert_eq!(keys.get(url_id(id)).unwrap(), None);
        assert_eq!(keys.get(key_id(id)).unwrap(), None);
    }

    #[test]
    fn a_migration_of_a_whole_schema_keeps_no_table_list() {
        let (a, b) = (Uuid::new_v4(), Uuid::new_v4());
        let migrate = |tables| ProfileTask::Migrate {
            source_connection_id: a,
            source_database: None,
            target_connection_id: b,
            target_database: None,
            source_schema: "public".to_owned(),
            target_schema: "dbo".to_owned(),
            tables,
            objects: true,
        };
        let profiles = Profiles::open(None);
        let keys = Keys::default();
        let whole = profiles
            .put(profile("Whole", migrate(None)), everything, &keys)
            .unwrap();
        let some = profiles
            .put(
                profile("Some", migrate(Some(vec!["t".to_owned()]))),
                everything,
                &keys,
            )
            .unwrap();
        assert_eq!(
            serde_json::to_value(&whole.task).unwrap()["tables"],
            Value::Null
        );
        assert_eq!(
            serde_json::to_value(&some.task).unwrap()["tables"],
            serde_json::json!(["t"])
        );
        // A file saved before tables could be left out reads as it was, and a missing list is the whole schema.
        let read: ProfileTask = serde_json::from_value(serde_json::json!({
            "kind": "migrate", "source_connection_id": a, "target_connection_id": b,
            "source_schema": "s", "target_schema": "t"
        }))
        .unwrap();
        assert!(matches!(read, ProfileTask::Migrate { tables: None, .. }));
        // An empty selection is no migration.
        let empty = profile("Empty", migrate(Some(Vec::new())));
        assert!(
            profiles
                .put(empty, everything, &keys)
                .unwrap_err()
                .contains("tables")
        );
    }

    #[test]
    fn names_are_there_and_different() {
        let profiles = Profiles::open(None);
        let keys = Keys::default();
        let first = profiles
            .put(profile("Orders", query(Value::Null)), everything, &keys)
            .unwrap();
        assert!(
            profiles
                .put(profile("  ", query(Value::Null)), everything, &keys)
                .unwrap_err()
                .contains("name")
        );
        let clash = profile(" ORDERS ", query(Value::Null));
        assert!(
            profiles
                .put(clash, everything, &keys)
                .unwrap_err()
                .contains("Orders")
        );
        // The same profile saved again, even renamed in case, is no clash.
        let mut same = first.clone();
        same.name = "orders".to_owned();
        assert_eq!(
            profiles.put(same, everything, &keys).unwrap().name,
            "orders"
        );
        assert_eq!(profiles.list().len(), 1);
    }

    #[test]
    fn a_profile_that_cannot_work_is_not_saved() {
        let profiles = Profiles::open(None);
        let keys = Keys::default();
        let mut cases = all_kinds();
        let mut blank_sql = query(Value::Null);
        if let ProfileTask::ExportQuery { sql, .. } = &mut blank_sql {
            *sql = " ".to_owned();
        }
        let mut no_path = query(Value::Null);
        if let ProfileTask::ExportQuery { path, .. } = &mut no_path {
            *path = PathBuf::new();
        }
        let bad_options = query(serde_json::json!({ "format": "docx" }));
        let bad_plan = ProfileTask::TransferTable {
            source_connection_id: Uuid::new_v4(),
            source_database: None,
            target_connection_id: Uuid::new_v4(),
            target_database: None,
            plan: serde_json::json!({ "table": 3 }),
        };
        cases.extend([blank_sql, no_path, bad_options, bad_plan]);
        // The five good ones save; the four bad ones do not.
        let outcomes: Vec<bool> = cases
            .into_iter()
            .enumerate()
            .map(|(i, task)| {
                profiles
                    .put(profile(&format!("P{i}"), task), everything, &keys)
                    .is_ok()
            })
            .collect();
        assert_eq!(
            outcomes,
            [true; 5].into_iter().chain([false; 4]).collect::<Vec<_>>()
        );

        let gone = |_: Uuid| false;
        assert!(
            profiles
                .put(profile("Gone", query(Value::Null)), gone, &keys)
                .unwrap_err()
                .contains("connection")
        );
    }

    fn schedule(folder: Option<&str>) -> Schedule {
        Schedule {
            id: Uuid::new_v4(),
            name: "Daily orders".to_owned(),
            enabled: true,
            connection_id: Uuid::nil(),
            database: None,
            task: Task::Profile {
                profile_id: Uuid::nil(),
            },
            repeat: Repeat::Day,
            time: "07:00".to_owned(),
            timezone: String::new(),
            folder: folder.map(PathBuf::from),
            keep: 2,
            retries: 0,
            retry_minutes: 5,
        }
    }

    #[test]
    fn a_task_of_a_missing_profile_is_refused_and_one_of_a_profile_follows_it() {
        let profiles = Profiles::open(None);
        let keys = Keys::default();
        let mut s = schedule(None);
        assert!(
            profiles
                .follow(&mut s, Uuid::new_v4())
                .unwrap_err()
                .contains("gone")
        );
        let task = all_kinds().remove(3);
        let (source, database) = task.first_connection();
        let p = profiles
            .put(profile("Copy", task), everything, &keys)
            .unwrap();
        profiles.follow(&mut s, p.id).unwrap();
        assert_eq!((s.connection_id, s.database), (source, database));
    }

    #[test]
    fn an_export_goes_into_the_folder_of_its_task() {
        let at: DateTime<Utc> = "2026-11-10T06:00:00Z".parse().unwrap();
        let kinds = all_kinds();
        let s = schedule(Some("/scheduled"));
        let name = |t: &ProfileTask, s: &Schedule| {
            t.scheduled_output(s, at).unwrap().map(|f| {
                (
                    f.parent().unwrap().to_owned(),
                    f.extension().unwrap().to_owned(),
                )
            })
        };
        let (folder, extension) = name(&kinds[0], &s).unwrap();
        assert_eq!(folder, Path::new("/scheduled"));
        assert_eq!(extension, "csv");
        let file = kinds[0].scheduled_output(&s, at).unwrap().unwrap();
        assert!(
            file.file_name()
                .unwrap()
                .to_str()
                .unwrap()
                .starts_with("Daily_orders-2026111"),
            "{file:?}"
        );
        // The compression counts in the extension the old files are found by.
        assert_eq!(
            kinds[0].scheduled_extension().unwrap().as_deref(),
            Some("csv")
        );
        let mut gzipped = kinds[0].clone();
        if let ProfileTask::ExportQuery { options, .. } = &mut gzipped {
            *options = serde_json::json!({ "format": "json", "compression": "gzip" });
        }
        assert_eq!(
            gzipped.scheduled_extension().unwrap().as_deref(),
            Some("json.gz")
        );
        let (_, extension) = name(&kinds[1], &s).unwrap();
        assert_eq!(extension, TABLES_EXTENSION);
        // Imports and copies write no file; without a folder the profile's own path stays.
        assert_eq!(name(&kinds[2], &s), None);
        assert_eq!(name(&kinds[4], &s), None);
        assert_eq!(name(&kinds[0], &schedule(None)), None);
    }

    #[test]
    fn an_unreadable_file_is_kept_aside() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("transfer-profiles.json");
        std::fs::write(&path, "{ not json").unwrap();
        let profiles = Profiles::open(Some(path.clone()));
        assert_eq!(profiles.list().len(), 0);
        assert_eq!(
            std::fs::read_to_string(dir.path().join("transfer-profiles.json.unreadable")).unwrap(),
            "{ not json"
        );
        let keys = Keys::default();
        profiles
            .put(profile("New", query(Value::Null)), everything, &keys)
            .unwrap();
        assert!(path.exists());
    }
}
