//! Upgrades of the stores from older releases: the vault 0.2.0 wrote, store files opened
//! again and again (migrations applied once), and a history whose migration was interrupted.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use platypus_store::{
    ConnectionStore, HistoryOrigin, HistoryQuery, HistoryStore, MemorySecrets, PasswordUpdate,
    SecretStore, VaultSecrets,
};
use uuid::Uuid;

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../apps/desktop/e2e/fixtures/qa3-inst-v020")
        .join(name)
}

const VAULT_PG: &str = "00000000-0000-4000-8000-0000000000a1";

#[test]
fn the_vault_of_0_2_0_unlocks_and_keeps_its_password() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("secrets.vault");
    std::fs::copy(fixture("secrets.vault"), &path).unwrap();
    let vault = VaultSecrets::locked(&path);
    assert!(vault.unlock("wrong").is_err());
    assert!(!vault.is_unlocked());
    vault.unlock("QaMaster-1").unwrap();
    let secret = vault.get(Uuid::parse_str(VAULT_PG).unwrap()).unwrap();
    assert_eq!(
        secret.as_ref().map(platypus_core::Secret::expose),
        Some("platypus")
    );
}

fn history_v1(path: &Path, with_origin: bool) {
    let db = rusqlite::Connection::open(path).unwrap();
    db.execute_batch(
        "CREATE TABLE history (
            id INTEGER PRIMARY KEY, connection_id TEXT NOT NULL, sql TEXT NOT NULL,
            started_at INTEGER NOT NULL, duration_ms INTEGER NOT NULL, rows INTEGER,
            status TEXT NOT NULL, error TEXT);
         CREATE INDEX history_connection ON history (connection_id, id);
         PRAGMA user_version = 1;",
    )
    .unwrap();
    if with_origin {
        db.execute_batch("ALTER TABLE history ADD COLUMN origin TEXT;")
            .unwrap();
    }
    db.execute(
        "INSERT INTO history (connection_id, sql, started_at, duration_ms, status)
         VALUES (?1, 'select 1', 0, 1, 'ok')",
        [Uuid::new_v4().to_string()],
    )
    .unwrap();
}

fn count(h: &HistoryStore) -> usize {
    h.search(&HistoryQuery {
        origin: Some(HistoryOrigin::App),
        limit: 10,
        ..Default::default()
    })
    .unwrap()
    .len()
}

fn user_version(path: &Path) -> i64 {
    rusqlite::Connection::open(path)
        .unwrap()
        .pragma_query_value(None, "user_version", |r| r.get(0))
        .unwrap()
}

#[test]
fn the_history_of_0_1_0_migrates_once() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("history.db");
    history_v1(&path, false);
    drop(HistoryStore::open(&path).unwrap());
    let after_first = user_version(&path);
    assert!(after_first >= 2);
    for _ in 0..3 {
        let h = HistoryStore::open(&path).unwrap();
        assert_eq!(count(&h), 1);
    }
    assert_eq!(user_version(&path), after_first);
}

/// A start killed between a migration and the update of `user_version` leaves the column
/// added at schema 1: the next start must still open the history, not fail on the column
/// that already exists (the migration and its version belong in one transaction).
#[test]
fn a_history_migration_interrupted_before_its_version_still_opens() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("history.db");
    history_v1(&path, true);
    let h = HistoryStore::open(&path).expect("history left at schema 1 with the origin column");
    assert_eq!(count(&h), 1);
}

#[test]
fn connections_of_0_1_0_are_rewritten_once_and_stay_stable() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("connections.toml");
    let id = Uuid::new_v4();
    std::fs::write(
        &path,
        format!(
            "version = 1\n\n[[connections]]\nid = \"{id}\"\nname = \"Legacy\"\ndriver = \"sqlite\"\nhost = \"\"\ndatabase = 'x.db'\nuser = \"\"\nssl_mode = \"prefer\"\nsave_password = false\nread_only = false\nfolder = \"Old\"\n"
        ),
    )
    .unwrap();
    let secrets: Arc<dyn SecretStore> = Arc::new(MemorySecrets::default());
    // Opening alone does not touch the file.
    drop(ConnectionStore::open(&path, secrets.clone()).unwrap());
    assert!(
        std::fs::read_to_string(&path)
            .unwrap()
            .starts_with("version = 1")
    );
    let mut store = ConnectionStore::open(&path, secrets.clone()).unwrap();
    let saved = store.get(id).unwrap().clone();
    store.save(saved, PasswordUpdate::Keep).unwrap();
    let first = std::fs::read_to_string(&path).unwrap();
    assert!(first.contains("version = 8"), "{first}");
    // Saving again after a reopen writes the same file.
    let mut store = ConnectionStore::open(&path, secrets).unwrap();
    let saved = store.get(id).unwrap().clone();
    store.save(saved, PasswordUpdate::Keep).unwrap();
    assert_eq!(std::fs::read_to_string(&path).unwrap(), first);
}

#[test]
fn the_connections_of_0_2_0_open() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("connections.toml");
    std::fs::copy(fixture("connections.toml"), &path).unwrap();
    let store = ConnectionStore::open(&path, Arc::new(MemorySecrets::default())).unwrap();
    assert_eq!(store.projects().len(), 1);
    assert_eq!(store.projects()[0].connections.len(), 2);
}
