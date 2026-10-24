//! Searching a database: its metadata (names, columns, sources) and the text of its rows.

use futures::TryStreamExt;
use platypus_core::{ExecOptions, ExecResult, MetadataMatch};
use platypus_sql::Dialect;
use serde::Serialize;
use tauri::State;

use super::AppState;
use crate::error::CommandResult;

/// Matching rows counted per table at most: enough to tell where to look.
const ROW_LIMIT: u64 = 1000;

#[tauri::command]
pub async fn search_metadata(
    state: State<'_, AppState>,
    session_id: u64,
    needle: String,
) -> CommandResult<Vec<MetadataMatch>> {
    let conn = state.live(session_id).await?;
    Ok(conn.search_metadata(&needle).await?)
}

#[derive(Debug, Serialize)]
pub struct TableHits {
    /// At most `limit`.
    rows: u64,
    limit: u64,
    /// The `WHERE` condition that selects them, for the data viewer.
    condition: String,
}

/// How many rows of a table hold `needle` in any column, read as text.
#[tauri::command]
pub async fn search_table_data(
    state: State<'_, AppState>,
    session_id: u64,
    schema: String,
    table: String,
    needle: String,
) -> CommandResult<TableHits> {
    let dialect = Dialect::from(state.session(session_id)?.config.driver);
    let conn = state.live(session_id).await?;
    let qualified = dialect.qualified(Some(&schema), &table);
    // The column names, from an empty result: cheaper than the table's full details.
    let ExecResult::Rows(empty) = conn
        .execute(
            &format!("SELECT * FROM {qualified} WHERE 1 = 0"),
            &ExecOptions::default(),
        )
        .await?
    else {
        return Err(platypus_core::Error::Other(format!("{qualified} returned no columns")).into());
    };
    let fields = empty.schema();
    empty.try_collect::<Vec<_>>().await?;
    let columns: Vec<&str> = fields.fields().iter().map(|f| f.name().as_str()).collect();
    if columns.is_empty() {
        return Ok(TableHits {
            rows: 0,
            limit: ROW_LIMIT,
            condition: String::new(),
        });
    }
    let condition = dialect.any_column_contains(&columns, &needle);
    let sql = format!(
        "SELECT COUNT(*) FROM (SELECT 1 FROM {qualified} WHERE {condition} {}) hits",
        dialect.limit_clause(ROW_LIMIT)
    );
    let rows = platypus_core::query_text(conn.as_ref(), &sql)
        .await?
        .first()
        .and_then(|r| r.first().cloned().flatten())
        .and_then(|n| n.parse().ok())
        .unwrap_or(0);
    Ok(TableHits {
        rows,
        limit: ROW_LIMIT,
        condition,
    })
}
