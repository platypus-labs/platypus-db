//! Editing table data in the data viewer. The UI keeps the pending changes; these commands
//! turn them into UPDATE, INSERT and DELETE statements for a preview, or run them all or
//! none on the session of the result being edited.

use arrow_array::RecordBatch;
use platypus_sql::dml::{self, ApplyError, Column, Statement, Value, ValueKind};
use platypus_sql::{Dialect, pg_literal};
use serde::{Deserialize, Serialize};
use tauri::State;

use super::AppState;
use super::query::stop_jobs;
use crate::display;
use crate::error::{CommandError, CommandResult};
use crate::jobs::Job;

#[derive(Debug, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum CellInput {
    Value { text: String },
    Null,
    Default,
}

impl CellInput {
    fn as_value(&self) -> Value<'_> {
        match self {
            Self::Value { text } => Value::Text(text),
            Self::Null => Value::Null,
            Self::Default => Value::Default,
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct ColumnValue {
    /// Index of the result column.
    column: usize,
    value: CellInput,
}

#[derive(Debug, Deserialize)]
pub struct RowUpdate {
    /// Index of the result row.
    row: usize,
    values: Vec<ColumnValue>,
}

/// How the database fills a column by itself (see [`platypus_sql::generated`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Generated {
    /// Numbered when left out; takes the values given.
    Numbered,
    /// Numbered; takes a value only with an override.
    Identity,
    /// Computed, never written.
    Computed,
}

#[derive(Debug, Deserialize)]
pub struct TableRef {
    schema: String,
    name: String,
}

/// The pending changes to a result whose rows are a table's rows.
#[derive(Debug, Deserialize)]
pub struct Changes {
    table: TableRef,
    /// Result columns that identify a row: the primary key, a non-null unique key or a
    /// virtual key the user set.
    key: Vec<usize>,
    /// The key is the table's primary key or a unique key, not a virtual key the user set:
    /// deleted rows can go in batches, checked by their total row count.
    #[serde(default)]
    unique_key: bool,
    /// Result columns that are the table's own (not expressions), by which the undo script
    /// inserts deleted rows back.
    table_columns: Vec<usize>,
    /// Default expressions by result column, for dialects without a `DEFAULT` keyword.
    defaults: Vec<Option<String>>,
    /// How the database fills each result column by itself, if it does: the undo script
    /// leaves computed columns out and puts identity values back with an override.
    #[serde(default)]
    generated: Vec<Option<Generated>>,
    updates: Vec<RowUpdate>,
    /// New rows, as the values the user set; other columns take their default.
    inserts: Vec<Vec<ColumnValue>>,
    /// Result rows to delete.
    deletes: Vec<usize>,
}

fn pairs(values: &[ColumnValue]) -> Vec<(usize, Value<'_>)> {
    values
        .iter()
        .map(|v| (v.column, v.value.as_value()))
        .collect()
}

fn borrowed(key: &[(usize, Option<String>)]) -> Vec<(usize, Option<&str>)> {
    key.iter().map(|(c, v)| (*c, v.as_deref())).collect()
}

/// A loaded value, as the statements that put it back write it.
fn loaded(text: &[Option<String>], column: usize) -> Value<'_> {
    text.get(column)
        .and_then(Option::as_deref)
        .map_or(Value::Null, Value::Text)
}

/// The key a statement's values give a row; `None` when one is left to the database.
fn key_in<'a>(
    key: &[usize],
    values: &[(usize, Value<'a>)],
) -> Option<Vec<(usize, Option<&'a str>)>> {
    key.iter()
        .map(|c| match values.iter().find(|(v, _)| v == c)?.1 {
            Value::Text(t) => Some((*c, Some(t))),
            Value::Null => Some((*c, None)),
            Value::Default => None,
        })
        .collect()
}

/// The full text of the buffered result rows `offset..offset + limit`.
pub(super) fn rows_text(
    job: &Job,
    offset: usize,
    limit: usize,
) -> CommandResult<Vec<Vec<Option<String>>>> {
    columns_text(job, offset, limit, None)
}

/// Like [`rows_text`], with only `columns` of each row (in that order) when given.
fn columns_text(
    job: &Job,
    offset: usize,
    limit: usize,
    columns: Option<&[usize]>,
) -> CommandResult<Vec<Vec<Option<String>>>> {
    let (_, batches) = job
        .window(offset, limit)
        .ok_or_else(|| CommandError::new("not_ready", "query has no result set yet"))?;
    let mut rows = Vec::with_capacity(limit.min(batches.iter().map(RecordBatch::num_rows).sum()));
    for batch in &batches {
        display::columns_text(batch, columns, &mut rows)?;
    }
    Ok(rows)
}

fn row_text(job: &Job, row: usize) -> CommandResult<Vec<Option<String>>> {
    rows_text(job, row, 1)?
        .pop()
        .ok_or_else(|| CommandError::not_found("row", row + 1))
}

/// What saving runs, and the script that puts the loaded rows back afterwards.
struct Script {
    statements: Vec<Statement>,
    /// Statements, and `--` comments for what no statement can undo.
    undo: Vec<String>,
}

impl Script {
    fn undo_text(&self) -> String {
        let mut text = String::new();
        for line in &self.undo {
            text.push_str(line);
            text.push_str(if line.starts_with("--") { "\n" } else { ";\n" });
        }
        text
    }
}

/// The table the statements of a save write to.
struct Target<'a> {
    dialect: Dialect,
    /// Quoted and qualified.
    table: String,
    columns: Vec<Column<'a>>,
}

impl Target<'_> {
    /// The deletes of the rows with `keys`: in batches when the key is unique (see
    /// [`dml::deletes`]), else one per row, each checked on its own.
    fn deletes(
        &self,
        changes: &Changes,
        keys: &[Vec<(usize, Option<&str>)>],
    ) -> dml::Result<Vec<Statement>> {
        if changes.unique_key {
            return dml::deletes(self.dialect, &self.table, &self.columns, keys);
        }
        keys.iter()
            .map(|k| dml::delete(self.dialect, &self.table, &self.columns, k).map(Statement::one))
            .collect()
    }
}

fn invalid(e: dml::InvalidValue) -> CommandError {
    CommandError::new("invalid_value", e.to_string())
}

/// The key of a loaded row, from its full text.
fn key_of(
    changes: &Changes,
    text: &[Option<String>],
) -> CommandResult<Vec<(usize, Option<String>)>> {
    changes
        .key
        .iter()
        .map(|&c| {
            let value = text
                .get(c)
                .ok_or_else(|| CommandError::not_found("column", c + 1))?;
            Ok((c, value.clone()))
        })
        .collect()
}

/// The deletes, in batches when the key is unique (see [`dml::deletes`]), and the inserts
/// that put the rows back, in batches too.
fn delete_statements(
    target: &Target<'_>,
    job: &Job,
    changes: &Changes,
) -> CommandResult<(Vec<Statement>, Vec<String>)> {
    let Target {
        dialect,
        ref table,
        ref columns,
    } = *target;
    let texts = changes
        .deletes
        .iter()
        .map(|&row| row_text(job, row))
        .collect::<CommandResult<Vec<_>>>()?;
    let keys = texts
        .iter()
        .map(|text| key_of(changes, text))
        .collect::<CommandResult<Vec<_>>>()?;
    let keys: Vec<_> = keys.iter().map(|k| borrowed(k)).collect();
    let statements = target.deletes(changes, &keys);
    let generated = |c: usize| changes.generated.get(c).copied().flatten();
    let restored: Vec<usize> = changes
        .table_columns
        .iter()
        .copied()
        .filter(|&c| generated(c) != Some(Generated::Computed))
        .collect();
    let identity: Vec<usize> = restored
        .iter()
        .copied()
        .filter(|&c| generated(c) == Some(Generated::Identity))
        .collect();
    let rows: Vec<Vec<_>> = texts
        .iter()
        .map(|text| restored.iter().map(|&c| (c, loaded(text, c))).collect())
        .collect();
    let undo = dml::restore_inserts(dialect, table, columns, &rows, &identity).map_err(invalid)?;
    Ok((statements.map_err(invalid)?, undo))
}

/// The updates, one per row, and the updates that put the loaded values back.
fn update_statements(
    target: &Target<'_>,
    job: &Job,
    changes: &Changes,
) -> CommandResult<(Vec<Statement>, Vec<String>)> {
    let Target {
        dialect,
        ref table,
        ref columns,
    } = *target;
    let mut statements = Vec::with_capacity(changes.updates.len());
    let mut undo = Vec::with_capacity(changes.updates.len());
    for update in &changes.updates {
        let text = row_text(job, update.row)?;
        let key = key_of(changes, &text)?;
        let set = pairs(&update.values);
        let sql = dml::update(dialect, table, columns, &set, &borrowed(&key)).map_err(invalid)?;
        statements.push(Statement::one(sql));
        // The row is found again by its key as saved.
        let saved_key: Option<Vec<(usize, Option<&str>)>> = key
            .iter()
            .map(|(c, v)| match set.iter().find(|(s, _)| s == c) {
                Some((_, Value::Text(t))) => Some((*c, Some(*t))),
                Some((_, Value::Null)) => Some((*c, None)),
                Some((_, Value::Default)) => None,
                None => Some((*c, v.as_deref())),
            })
            .collect();
        let back: Vec<_> = set.iter().map(|&(c, _)| (c, loaded(&text, c))).collect();
        undo.push(match saved_key {
            Some(k) => dml::update(dialect, table, columns, &back, &k).map_err(invalid)?,
            None => format!(
                "-- Row {}: its key was set to its default, restore the row by hand",
                update.row + 1
            ),
        });
    }
    Ok((statements, undo))
}

/// The inserts, in batches (see [`dml::inserts`]), and the deletes that remove the rows.
fn insert_statements(
    target: &Target<'_>,
    changes: &Changes,
) -> CommandResult<(Vec<Statement>, Vec<String>)> {
    let Target {
        dialect,
        ref table,
        ref columns,
    } = *target;
    let rows: Vec<_> = changes.inserts.iter().map(|v| pairs(v)).collect();
    let statements = dml::inserts(dialect, table, columns, &rows).map_err(invalid)?;
    let mut undo = Vec::new();
    let mut keys = Vec::with_capacity(rows.len());
    for (n, set) in rows.iter().enumerate() {
        match key_in(&changes.key, set) {
            Some(k) => keys.push(k),
            None => undo.push(format!(
                "-- New row {}: its key was generated by the database, delete it by hand",
                n + 1
            )),
        }
    }
    let deletes = target.deletes(changes, &keys).map_err(invalid)?;
    undo.extend(deletes.into_iter().map(|s| s.sql));
    Ok((statements, undo))
}

/// The statements for `changes`: deletes first, which frees unique values for the updates
/// and inserts that follow. The undo script does the opposite, in the opposite order.
fn script(state: &AppState, job: &Job, changes: &Changes) -> CommandResult<Script> {
    let dialect = state.dialect(job.session_id)?;
    let (schema, _) = job
        .window(0, 0)
        .ok_or_else(|| CommandError::new("not_ready", "query has no result set yet"))?;
    let target = Target {
        dialect,
        table: dialect.qualified(Some(&changes.table.schema), &changes.table.name),
        columns: schema
            .fields()
            .iter()
            .enumerate()
            .map(|(i, f)| Column {
                name: f.name(),
                kind: ValueKind::of(f.data_type()),
                default: changes.defaults.get(i).and_then(Option::as_deref),
            })
            .collect(),
    };
    let (mut statements, undo_deletes) = delete_statements(&target, job, changes)?;
    let (updates, undo_updates) = update_statements(&target, job, changes)?;
    let (inserts, mut undo) = insert_statements(&target, changes)?;
    statements.extend(updates);
    statements.extend(inserts);
    undo.extend(undo_updates);
    undo.extend(undo_deletes);
    Ok(Script { statements, undo })
}

/// The Redis commands for `changes` to a key type's virtual table (see
/// [`platypus_driver_redis::edit`]), which the session runs as one `MULTI`/`EXEC`.
fn redis_script(
    job: &Job,
    changes: &Changes,
) -> CommandResult<platypus_driver_redis::edit::Script> {
    use platypus_driver_redis::{command::KeyType, edit, meta};
    let invalid = |m: String| CommandError::new("invalid_value", m);
    let kind = KeyType::from_table(&changes.table.name)
        .ok_or_else(|| CommandError::not_found("table", &changes.table.name))?;
    let db = meta::database(&changes.table.schema).map_err(|e| invalid(e.to_string()))?;
    let (schema, _) = job
        .window(0, 0)
        .ok_or_else(|| CommandError::new("not_ready", "query has no result set yet"))?;
    let columns: Vec<String> = schema.fields().iter().map(|f| f.name().clone()).collect();
    let values = |values: &[ColumnValue]| -> Vec<(usize, Option<String>)> {
        values
            .iter()
            .map(|v| match &v.value {
                CellInput::Value { text } => (v.column, Some(text.clone())),
                CellInput::Null | CellInput::Default => (v.column, None),
            })
            .collect()
    };
    let edits = edit::Edits {
        db,
        kind,
        columns: &columns,
        updates: changes
            .updates
            .iter()
            .map(|u| {
                Ok(edit::Update {
                    loaded: row_text(job, u.row)?,
                    set: values(&u.values),
                })
            })
            .collect::<CommandResult<_>>()?,
        inserts: changes.inserts.iter().map(|v| values(v)).collect(),
        deletes: changes
            .deletes
            .iter()
            .map(|&row| row_text(job, row))
            .collect::<CommandResult<_>>()?,
    };
    edit::script(&edits).map_err(invalid)
}

/// Whether the result's session is a Redis one, whose edits are commands, not SQL.
fn is_redis(state: &AppState, job: &Job) -> CommandResult<bool> {
    Ok(state.session(job.session_id)?.config.driver == platypus_core::DriverKind::Redis)
}

/// The SQL that saving `changes` would run.
#[tauri::command]
pub fn preview_changes(
    state: State<'_, AppState>,
    job_id: u64,
    changes: Changes,
) -> CommandResult<Vec<String>> {
    let job = state.job(job_id)?;
    if is_redis(&state, &job)? {
        return Ok(redis_script(&job, &changes)?.commands);
    }
    let script = script(&state, &job, &changes)?;
    Ok(script.statements.into_iter().map(|s| s.sql).collect())
}

#[derive(Debug, Serialize)]
pub struct Saved {
    statements: usize,
    /// SQL that puts the rows back as they were loaded.
    undo: String,
}

/// Runs the statements for `changes` on the result's session, all or none (see
/// [`dml::apply`]). Fetching of the session's unfinished results stops first: they hold the
/// session. The UI reloads the data afterwards.
#[tauri::command]
pub async fn save_changes(
    state: State<'_, AppState>,
    job_id: u64,
    changes: Changes,
) -> CommandResult<Saved> {
    let job = state.job(job_id)?;
    state.editable(job.session_id)?;
    if is_redis(&state, &job)? {
        return save_redis(&state, &job, &changes).await;
    }
    let script = script(&state, &job, &changes)?;
    // The driver's dialect brackets the statements: the generic ODBC driver translates no
    // SAVEPOINT for SQL Server, as the native one does.
    let dialect = Dialect::from(state.session(job.session_id)?.config.driver);
    let conn = state.live(job.session_id).await?;
    stop_jobs(&state, job.session_id);
    dml::apply(conn.as_ref(), dialect, &script.statements)
        .await
        .map_err(|e| match e {
            ApplyError::Failed { index, error } => {
                CommandError::from(&error).context(format!("Statement {}", index + 1))
            }
            e @ ApplyError::RowCount { .. } => CommandError::new("row_count", e.to_string()),
            e @ ApplyError::SessionRolledBack(_) => CommandError::new("rolled_back", e.to_string()),
        })?;
    Ok(Saved {
        statements: script.statements.len(),
        undo: script.undo_text(),
    })
}

/// [`save_changes`] on Redis: the commands in one `MULTI`/`EXEC` on the result's session.
async fn save_redis(state: &AppState, job: &Job, changes: &Changes) -> CommandResult<Saved> {
    let script = redis_script(job, changes)?;
    let conn = state.live(job.session_id).await?;
    stop_jobs(state, job.session_id);
    let commands: Vec<&str> = script.commands.iter().map(String::as_str).collect();
    let ran = conn.execute_all(&commands, &[]).await;
    if let Some(error) = ran.error {
        return Err(CommandError::from(&error).context(format!("Command {}", ran.rows.len() + 1)));
    }
    Ok(Saved {
        statements: commands.len(),
        undo: script.undo.join(
            "
",
        ) + "
",
    })
}

/// The full values of loaded rows `offset..offset + limit`, as text (`None` for NULL): the
/// grid shows them cut to one line, editing and copying need them whole. With `columns`, only
/// those columns of each row, in that order.
#[tauri::command]
pub fn row_values(
    state: State<'_, AppState>,
    job_id: u64,
    offset: usize,
    limit: usize,
    columns: Option<Vec<usize>>,
) -> CommandResult<Vec<Vec<Option<String>>>> {
    columns_text(&*state.job(job_id)?, offset, limit, columns.as_deref())
}

/// The elements of a PostgreSQL array of the type `type_name` (its delimiter is `;` for
/// boxes), or, `composite`, the fields of a composite value, for the data editor's list;
/// `None` when the text is not one, to edit it as text.
#[tauri::command]
pub fn split_literal(composite: bool, type_name: &str, text: &str) -> Option<Vec<Option<String>>> {
    if composite {
        pg_literal::split_composite(text)
    } else {
        pg_literal::split_array(text, pg_literal::array_delimiter(type_name))
    }
}

/// The array (or composite) literal of the data editor's list.
#[tauri::command]
pub fn join_literal(composite: bool, type_name: &str, items: Vec<Option<String>>) -> String {
    if composite {
        pg_literal::join_composite(&items)
    } else {
        pg_literal::join_array(&items, pg_literal::array_delimiter(type_name))
    }
}
