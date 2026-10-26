//! Notes and tags on tables and columns (`platypus_store::CatalogStore`).

use platypus_store::{CatalogEntry, CatalogKey, CatalogStore, DisplayColumn, TagCount};
use tauri::State;
use uuid::Uuid;

use super::AppState;
use crate::error::CommandResult;

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
    use platypus_store::{CatalogKey, CatalogStore};
    use uuid::Uuid;

    #[test]
    fn forget_connection_clears_its_notes() {
        let mut store = Ok(CatalogStore::open_in_memory().unwrap());
        let c = Uuid::new_v4();
        let key = CatalogKey {
            object: "t".to_owned(),
            ..CatalogKey::default()
        };
        store.as_mut().unwrap().save(c, &key, "x", &[]).unwrap();
        super::forget_connection(&mut store, c);
        assert_eq!(store.unwrap().entries(c).unwrap(), Vec::new());
    }
}
