//! The table editor: a table's design, the script that creates or alters it, and running
//! that script (see `platypus-schema`).

use platypus_core::DriverKind;
use platypus_schema::{Finding, SchemaTable, Script, TableDesign};
use platypus_sql::Dialect;
use serde::Serialize;
use tauri::State;

use super::{AppState, one_off};
use crate::error::{CommandError, CommandResult};

/// The design of `schema.table` as it is in the database.
#[tauri::command]
pub async fn table_design(
    state: State<'_, AppState>,
    session_id: u64,
    schema: String,
    table: String,
) -> CommandResult<TableDesign> {
    let dialect = Dialect::from(state.session(session_id)?.config.driver);
    let conn = state.live(session_id).await?;
    Ok(platypus_schema::load(conn.as_ref(), dialect, &schema, &table).await?)
}

#[derive(Debug, Serialize)]
pub struct DesignScript {
    script: Script,
    /// The script as text, for the preview and the editor.
    sql: String,
}

/// The script that turns `original` into `design` in `schema`, or creates `design` when
/// there is no `original`.
#[tauri::command]
pub fn table_script(
    driver: DriverKind,
    schema: String,
    original: Option<TableDesign>,
    design: TableDesign,
) -> CommandResult<DesignScript> {
    let script =
        platypus_schema::script(Dialect::from(driver), &schema, original.as_ref(), &design)
            .map_err(|e| CommandError::new("invalid_design", e))?;
    Ok(DesignScript {
        sql: script.text(),
        script,
    })
}

/// Runs a script from [`table_script`] on the session and commits it (see [`one_off`]).
#[tauri::command]
pub async fn apply_table_script(
    state: State<'_, AppState>,
    session_id: u64,
    script: Script,
) -> CommandResult<()> {
    state.writable(session_id)?;
    let conn = state.live(session_id).await?;
    one_off(&conn, async {
        platypus_schema::apply(conn.as_ref(), &script)
            .await
            .map_err(|e| {
                let first_line = e.statement.lines().next().unwrap_or_default();
                let error = CommandError::from(&e.error).context(first_line);
                if e.applied.is_empty() {
                    return error;
                }
                // What ran before cannot be undone: say what the table now has.
                let applied: Vec<&str> = e.applied.iter().map(String::as_str).collect();
                error.note(&format!(
                    "These statements ran before it and stay applied:
{};",
                    applied.join(
                        ";
"
                    )
                ))
            })
    })
    .await
}

/// The tables of `schema` with their details, those in `tables` or all, for the ER diagram.
#[tauri::command]
pub async fn schema_tables(
    state: State<'_, AppState>,
    session_id: u64,
    schema: String,
    tables: Option<Vec<String>>,
) -> CommandResult<Vec<SchemaTable>> {
    let conn = state.live(session_id).await?;
    Ok(platypus_schema::read_tables(conn.as_ref(), &schema, tables.as_deref()).await?)
}

/// What the linter finds in the tables of `schema`.
#[tauri::command]
pub async fn lint_schema(
    state: State<'_, AppState>,
    session_id: u64,
    schema: String,
) -> CommandResult<Vec<Finding>> {
    let dialect = Dialect::from(state.session(session_id)?.config.driver);
    let conn = state.live(session_id).await?;
    let tables = platypus_schema::read_tables(conn.as_ref(), &schema, None).await?;
    Ok(platypus_schema::lint(dialect, &schema, &tables))
}

/// The script that makes `target_schema` on the target session like `source_schema` on the
/// source session, in the target's dialect.
#[tauri::command]
pub async fn compare_schemas(
    state: State<'_, AppState>,
    source_session: u64,
    source_schema: String,
    target_session: u64,
    target_schema: String,
) -> CommandResult<DesignScript> {
    let source_dialect = Dialect::from(state.session(source_session)?.config.driver);
    let dialect = Dialect::from(state.session(target_session)?.config.driver);
    if source_dialect != dialect {
        return Err(CommandError::new(
            "different_databases",
            "Schemas can be compared only between databases of the same kind.",
        ));
    }
    let source = state.live(source_session).await?;
    let target = state.live(target_session).await?;
    let wanted = platypus_schema::load_designs(source.as_ref(), dialect, &source_schema).await?;
    let current = platypus_schema::load_designs(target.as_ref(), dialect, &target_schema).await?;
    let script =
        platypus_schema::compare(dialect, &target_schema, &source_schema, &wanted, &current)
            .map_err(|e| CommandError::new("invalid_design", e))?;
    Ok(DesignScript {
        sql: script.text(),
        script,
    })
}
