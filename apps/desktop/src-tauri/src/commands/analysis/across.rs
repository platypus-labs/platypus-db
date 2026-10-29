//! "Query across databases": tables of different live connections (a PostgreSQL table and a
//! MySQL one) copied into one in-memory DuckDB, so a single query can join them. Like "SQL on
//! results" the database sits behind a temporary connection that is never saved; here the
//! rows come from `SELECT * FROM <table> [WHERE …]` on each source's session, not from a
//! result's job.

use std::collections::HashSet;

use arrow_array::RecordBatch;
use futures::StreamExt;
use platypus_core::{ExecOptions, ExecResult};
use platypus_sql::Dialect;
use platypus_store::SavedConnection;
use serde::{Deserialize, Serialize};
use tauri::State;
use uuid::Uuid;

use super::sql::{Source, contents, fill_new, refill};
use crate::commands::AppState;
use crate::error::{CommandError, CommandResult};
use crate::jobs::{Ending, MEMORY_LIMIT_BYTES};

/// Rows per Arrow batch; the whole table is read, so bigger than a grid's page.
const BATCH_SIZE: usize = 4096;

/// A table to copy: where it lives and the DuckDB table that holds its rows.
#[derive(Debug, Deserialize)]
pub struct AcrossSource {
    /// A session of the source's connection, opened by the UI.
    session_id: u64,
    /// `None` for the session's current schema.
    schema: Option<String>,
    table: String,
    /// The condition of a `WHERE`, without the keyword.
    filter: Option<String>,
    /// The table's name in the DuckDB.
    name: String,
    /// How errors name this source, e.g. `pg · public.orders`.
    label: String,
}

/// A table of the DuckDB, as the new tab's first comment lists it.
#[derive(Debug, Serialize)]
pub struct AcrossTable {
    name: String,
    columns: Vec<String>,
    rows: usize,
    /// The memory limit stopped the reading: the table lacks rows.
    cut: bool,
}

#[derive(Debug, Serialize)]
pub struct OpenedAcross {
    connection: SavedConnection,
    tables: Vec<AcrossTable>,
}

/// The statement that reads a source: the whole table, or the rows `filter` keeps. A filter
/// that is more than a condition (a second statement, a write) is refused, as it would run on
/// a live connection.
fn select_sql(
    dialect: Dialect,
    schema: Option<&str>,
    table: &str,
    filter: Option<&str>,
) -> Result<String, String> {
    let from = dialect.qualified(schema.filter(|s| !s.is_empty()), table);
    let mut sql = format!("SELECT * FROM {from}");
    let filter = filter.map(str::trim).filter(|f| !f.is_empty());
    if let Some(filter) = filter {
        sql.push_str(" WHERE ");
        sql.push_str(filter);
        let one = platypus_sql::split(&sql, dialect).len() <= 1;
        if !one || !platypus_sql::is_read_only(&sql, dialect) {
            return Err("the filter must be a single condition".to_owned());
        }
    }
    Ok(sql)
}

/// Checks the DuckDB table names: at least one source, every name filled and distinct
/// ignoring case (DuckDB identifiers are case-insensitive).
fn check_names(sources: &[AcrossSource]) -> CommandResult<()> {
    if sources.is_empty() {
        return Err(CommandError::new("no_rows", "Choose at least one table."));
    }
    let mut seen = HashSet::new();
    for s in sources {
        let name = s.name.trim();
        if name.is_empty() {
            return Err(CommandError::new(
                "invalid_name",
                format!("{}: the table needs a name.", s.label),
            ));
        }
        if !seen.insert(name.to_lowercase()) {
            return Err(CommandError::new(
                "duplicate_name",
                format!("{}: another table is already called {name}.", s.label),
            ));
        }
    }
    Ok(())
}

/// Reads one source, taking from `budget` the bytes it buffers; the source ends with
/// `Ending::MemoryLimit` once the budget is spent. A failure names the source.
async fn read_source(
    state: &AppState,
    s: &AcrossSource,
    budget: &mut usize,
) -> CommandResult<Source> {
    read(state, s, budget)
        .await
        .map_err(|e| e.context(&s.label))
}

async fn read(state: &AppState, s: &AcrossSource, budget: &mut usize) -> CommandResult<Source> {
    // Another database is where a restricted connection's rows do not go.
    state.exportable(s.session_id)?;
    let dialect = state.dialect(s.session_id)?;
    let sql = select_sql(dialect, s.schema.as_deref(), &s.table, s.filter.as_deref())
        .map_err(|m| CommandError::new("bad_filter", m))?;
    let conn = state.live(s.session_id).await?;
    let options = ExecOptions {
        batch_size: BATCH_SIZE,
        paged: false,
    };
    let ExecResult::Rows(mut stream) = conn.execute(&sql, &options).await? else {
        return Err(CommandError::new("no_rows", "The table returned no rows."));
    };
    let schema = stream.schema();
    let mut batches: Vec<RecordBatch> = Vec::new();
    let mut ending = Ending::Complete;
    while let Some(batch) = stream.next().await {
        let batch = batch?;
        *budget = budget.saturating_sub(batch.get_array_memory_size());
        batches.push(batch);
        if *budget == 0 {
            // Dropping the stream cancels the rest of the statement on the server.
            ending = Ending::MemoryLimit;
            break;
        }
    }
    Ok(Source {
        table: s.name.trim().to_owned(),
        schema,
        batches,
        ending,
    })
}

/// Reads every source in turn; the first failure ends it, so nothing is half done.
async fn read_all(state: &AppState, sources: &[AcrossSource]) -> CommandResult<Vec<Source>> {
    check_names(sources)?;
    let mut budget = MEMORY_LIMIT_BYTES;
    let mut all = Vec::with_capacity(sources.len());
    for s in sources {
        all.push(read_source(state, s, &mut budget).await?);
    }
    Ok(all)
}

/// What the new tab lists of each table.
fn describe(sources: &[Source]) -> Vec<AcrossTable> {
    sources
        .iter()
        .map(|s| AcrossTable {
            name: s.table.clone(),
            columns: s.schema.fields().iter().map(|f| f.name().clone()).collect(),
            rows: s.batches.iter().map(RecordBatch::num_rows).sum(),
            cut: s.ending == Ending::MemoryLimit,
        })
        .collect()
}

/// Copies the tables into a new in-memory DuckDB and returns the temporary connection to
/// it, called `name`, with the tables as loaded. A source that fails fails the whole call,
/// naming it, and registers nothing.
#[tauri::command]
pub async fn open_across_db(
    state: State<'_, AppState>,
    name: String,
    sources: Vec<AcrossSource>,
) -> CommandResult<OpenedAcross> {
    let read = read_all(&state, &sources).await?;
    let tables = describe(&read);
    let connection = fill_new(&state, &name, read).await?;
    Ok(OpenedAcross { connection, tables })
}

/// Reads the sources again into the database of temporary connection `connection_id`,
/// replacing its tables. A failing source leaves the database as it was.
#[tauri::command]
pub async fn reload_across_db(
    state: State<'_, AppState>,
    connection_id: Uuid,
    sources: Vec<AcrossSource>,
) -> CommandResult<Vec<AcrossTable>> {
    // Fails before reading anything when the database is gone.
    contents(&state, connection_id)?;
    let read = read_all(&state, &sources).await?;
    let tables = describe(&read);
    refill(&state, connection_id, read).await?;
    Ok(tables)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::analysis::error_kind;

    fn source(name: &str) -> AcrossSource {
        AcrossSource {
            session_id: 1,
            schema: None,
            table: "t".to_owned(),
            filter: None,
            name: name.to_owned(),
            label: format!("db · {name}"),
        }
    }

    #[test]
    fn reads_the_table_qualified_for_its_dialect() {
        let sql = |d, s: Option<&str>, f| select_sql(d, s, "Order Items", f).unwrap();
        assert_eq!(
            sql(Dialect::Postgres, Some("public"), None),
            "SELECT * FROM public.\"Order Items\""
        );
        assert_eq!(
            sql(Dialect::MySql, Some("shop"), None),
            "SELECT * FROM shop.`Order Items`"
        );
        assert_eq!(
            sql(Dialect::SqlServer, Some("dbo"), None),
            "SELECT * FROM dbo.[Order Items]"
        );
        assert_eq!(
            sql(Dialect::Postgres, Some(""), Some("  ")),
            "SELECT * FROM \"Order Items\""
        );
    }

    #[test]
    fn a_filter_is_a_where_condition() {
        let sql = select_sql(
            Dialect::Sqlite,
            Some("main"),
            "t",
            Some(" n > 1 AND s = 'a;b' "),
        );
        assert_eq!(
            sql.unwrap(),
            "SELECT * FROM main.t WHERE n > 1 AND s = 'a;b'"
        );
        let two = select_sql(Dialect::Sqlite, None, "t", Some("1=1; DELETE FROM t"));
        assert!(two.is_err());
        let write = select_sql(Dialect::Postgres, None, "t", Some("true; DROP TABLE t"));
        assert!(write.is_err());
    }

    #[test]
    fn names_must_be_filled_and_distinct_ignoring_case() {
        assert!(check_names(&[source("a"), source("b")]).is_ok());
        let Err(e) = check_names(&[]) else {
            panic!("no sources")
        };
        assert_eq!(error_kind(&e), "no_rows");
        let Err(e) = check_names(&[source("a"), source("  ")]) else {
            panic!("blank name")
        };
        assert_eq!(error_kind(&e), "invalid_name");
        let Err(e) = check_names(&[source("Orders"), source("orders")]) else {
            panic!("duplicate name")
        };
        assert_eq!(error_kind(&e), "duplicate_name");
        assert!(e.message().contains("db · orders"));
    }

    #[test]
    fn a_source_whose_session_is_gone_names_itself() {
        let state = AppState::for_tests();
        let Err(e) = tauri::async_runtime::block_on(read_all(&state, &[source("a")])) else {
            panic!("there is no session 1")
        };
        assert!(e.message().starts_with("db · a: "), "{}", e.message());
    }
}
