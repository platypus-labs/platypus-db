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
