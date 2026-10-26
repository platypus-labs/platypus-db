//! Notes and tags on tables and columns (`platypus_store::CatalogStore`).

use platypus_store::{CatalogEntry, CatalogKey, CatalogStore, DisplayColumn, TagCount};
use tauri::State;
use uuid::Uuid;

use super::AppState;
use crate::error::{CommandError, CommandResult};

#[tauri::command]
pub fn catalog_entries(
    state: State<'_, AppState>,
    connection_id: Uuid,
) -> CommandResult<Vec<CatalogEntry>> {
    state.with_catalog(|c| c.entries(connection_id))
}

#[tauri::command]
pub fn save_catalog_entry(
    state: State<'_, AppState>,
    connection_id: Uuid,
    key: CatalogKey,
    note: String,
    tags: Vec<String>,
) -> CommandResult<CatalogEntry> {
    state.with_catalog(|c| c.save(connection_id, &key, &note, &tags))
}

#[tauri::command]
pub fn catalog_tags(
    state: State<'_, AppState>,
    connection_id: Uuid,
) -> CommandResult<Vec<TagCount>> {
    state.with_catalog(|c| c.tags(connection_id))
}

/// After the app renamed an object or a column: its notes follow it.
#[tauri::command]
pub fn rename_catalog_key(
    state: State<'_, AppState>,
    connection_id: Uuid,
    from: CatalogKey,
    to: CatalogKey,
) -> CommandResult<()> {
    state.with_catalog(|c| c.rename(connection_id, &from, &to))
}

/// The statements that set the comment of `table`, or of its `column`, as the database
/// writes them; `None` removes it.
#[tauri::command]
pub async fn comment_sql(
    state: State<'_, AppState>,
    session_id: u64,
    schema: String,
    table: String,
    column: Option<String>,
    comment: Option<String>,
) -> CommandResult<String> {
    let dialect = platypus_sql::Dialect::from(state.session(session_id)?.config.driver);
    let conn = state.live(session_id).await?;
    let old = platypus_schema::load(conn.as_ref(), dialect, &schema, &table).await?;
    let mut new = old.clone();
    let comment = comment.filter(|c| !c.trim().is_empty());
    match &column {
        None => new.comment = comment,
        Some(name) => {
            let c = new
                .columns
                .iter_mut()
                .find(|c| &c.name == name)
                .ok_or_else(|| CommandError::new("not_found", format!("no column {name}")))?;
            c.comment = comment;
        }
    }
    let script = platypus_schema::script(dialect, &schema, Some(&old), &new)
        .map_err(|e| CommandError::new("invalid_design", e))?;
    Ok(script.text())
}

/// The display columns chosen on a connection, one per table.
#[tauri::command]
pub fn catalog_display_columns(
    state: State<'_, AppState>,
    connection_id: Uuid,
) -> CommandResult<Vec<DisplayColumn>> {
    state.with_catalog(|c| c.display_columns(connection_id))
}

/// The column that describes the rows of the table `key`; `None` for the automatic choice.
#[tauri::command]
pub fn set_catalog_display_column(
    state: State<'_, AppState>,
    connection_id: Uuid,
    key: CatalogKey,
    column: Option<String>,
) -> CommandResult<()> {
    state.with_catalog(|c| c.set_display_column(connection_id, &key, column.as_deref()))
}

/// Drops the notes of a deleted connection; a catalog that cannot be opened has none.
pub(super) fn forget_connection(catalog: &mut Result<CatalogStore, String>, id: Uuid) {
    if let Ok(c) = catalog {
        let _ = c.remove_connection(id);
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use platypus_core::DriverKind;
    use platypus_store::{
        CatalogKey, CatalogStore, ConnectionStore, MemorySecrets, PasswordUpdate, SavedConnection,
    };
    use uuid::Uuid;

    use crate::commands::AppState;

    fn key() -> CatalogKey {
        CatalogKey {
            object: "t".to_owned(),
            ..CatalogKey::default()
        }
    }

    fn state(
        connections: Result<ConnectionStore, String>,
        catalog: Result<CatalogStore, String>,
    ) -> AppState {
        AppState::new(
            (connections, None),
            Err("no vault".to_owned()),
            Err("no history".to_owned()),
            catalog,
            Err("no settings".to_owned()),
            Err("no scripts".to_owned()),
        )
    }

    #[test]
    fn forget_connection_clears_its_notes() {
        let mut store = Ok(CatalogStore::open_in_memory().unwrap());
        let c = Uuid::new_v4();
        store.as_mut().unwrap().save(c, &key(), "x", &[]).unwrap();
        super::forget_connection(&mut store, c);
        assert_eq!(store.unwrap().entries(c).unwrap(), Vec::new());
    }

    #[test]
    fn forget_connection_keeps_the_notes_of_the_others() {
        let mut store = Ok(CatalogStore::open_in_memory().unwrap());
        let (a, b) = (Uuid::new_v4(), Uuid::new_v4());
        for c in [a, b] {
            store.as_mut().unwrap().save(c, &key(), "x", &[]).unwrap();
        }
        super::forget_connection(&mut store, a);
        let store = store.unwrap();
        assert_eq!(store.entries(a).unwrap(), Vec::new());
        assert_eq!(store.entries(b).unwrap().len(), 1);
    }

    #[test]
    fn forget_connection_without_a_catalog_does_nothing() {
        let mut store = Err("cannot open".to_owned());
        super::forget_connection(&mut store, Uuid::new_v4());
        assert_eq!(store.unwrap_err(), "cannot open");
    }

    #[test]
    fn a_catalog_that_cannot_be_opened_reports_a_catalog_error() {
        let state = state(
            Err("no connections".to_owned()),
            Err("disk full".to_owned()),
        );
        let err = state
            .with_catalog(|c| c.entries(Uuid::new_v4()))
            .unwrap_err();
        let json = serde_json::to_value(&err).unwrap();
        assert_eq!(json["kind"], "catalog");
        assert_eq!(json["message"], "disk full");
        // Deleting a connection and pruning still go through.
        state.forget_catalog(Uuid::new_v4());
        state.prune_catalog();
    }

    #[test]
    fn deleting_a_connection_forgets_its_notes_through_the_state() {
        let state = state(
            Err("no connections".to_owned()),
            Ok(CatalogStore::open_in_memory().unwrap()),
        );
        let (a, b) = (Uuid::new_v4(), Uuid::new_v4());
        for c in [a, b] {
            state.with_catalog(|s| s.save(c, &key(), "x", &[])).unwrap();
        }
        state.forget_catalog(a);
        assert_eq!(state.with_catalog(|s| s.entries(a)).unwrap(), Vec::new());
        assert_eq!(state.with_catalog(|s| s.entries(b)).unwrap().len(), 1);
    }

    #[test]
    fn prune_catalog_keeps_the_saved_connections_only() {
        let dir = tempfile::tempdir().unwrap();
        let mut connections = ConnectionStore::open(
            dir.path().join("connections.toml"),
            Arc::new(MemorySecrets::default()),
        )
        .unwrap();
        let saved = SavedConnection::new("kept", DriverKind::Postgres);
        let kept = saved.id;
        connections.save(saved, PasswordUpdate::Keep).unwrap();
        let state = state(Ok(connections), Ok(CatalogStore::open_in_memory().unwrap()));
        let gone = Uuid::new_v4();
        for c in [kept, gone] {
            state.with_catalog(|s| s.save(c, &key(), "x", &[])).unwrap();
        }
        state.prune_catalog();
        assert_eq!(state.with_catalog(|s| s.entries(kept)).unwrap().len(), 1);
        assert_eq!(state.with_catalog(|s| s.entries(gone)).unwrap(), Vec::new());
    }
}
