//! `propose_edits`: rows to insert, update or delete in a table, kept for the app to open in
//! the table's data viewer as pending edits that the user saves or discards. Nothing is
//! written here, whatever the connection's agent access, as with `propose_sql`.
//!
//! What is kept is data only: names and values, checked against the table's keys. The app
//! checks them again and writes every statement itself (see [`platypus_sql::proposed`]).

use platypus_core::DriverKind;
use platypus_sql::Dialect;
use platypus_sql::proposed::{self, Edits, MAX_BYTES, MAX_ROWS, Row, Update};
use platypus_store::SavedConnection;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::agent::{Agent, proposal_title, too_many_waiting};
use crate::error::{AgentError, Result};

/// The arguments of `propose_edits`.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ProposeEditsArgs {
    /// The connection's name, as `list_connections` shows it.
    pub connection: String,
    /// The table's schema (a database in MySQL, `main` in SQLite and DuckDB).
    pub schema: String,
    /// The table.
    pub table: String,
    /// The data viewer tab's title, e.g. what the changes do.
    pub title: Option<String>,
    /// Rows to insert: each an object of column to value. A value is a string, number,
    /// boolean or null; columns left out take their default.
    pub inserts: Option<Vec<Map<String, Value>>>,
    /// Rows to change: `key` names a row by exactly the columns of a primary or unique key
    /// of the table, `set` gives the new values of other columns (or the same).
    pub updates: Option<Vec<UpdateArg>>,
    /// Rows to delete: each names a row by exactly the columns of a primary or unique key.
    pub deletes: Option<Vec<Map<String, Value>>>,
}

/// One row to change.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct UpdateArg {
    /// Column to value, the columns of a primary or unique key; no value is null.
    pub key: Map<String, Value>,
    /// Column to new value; not empty.
    pub set: Map<String, Value>,
}

/// A value as text: a string as it is, a number as written, a boolean as `true`/`false`.
fn scalar(what: &str, column: &str, value: &Value) -> Result<Option<String>> {
    match value {
        Value::Null => Ok(None),
        Value::String(s) => Ok(Some(s.clone())),
        Value::Bool(b) => Ok(Some(b.to_string())),
        Value::Number(n) => {
            // A whole number written with a fraction (`5.0`) is written whole.
            Ok(Some(match n.as_f64() {
                Some(f) if n.is_f64() && f.fract() == 0.0 && f.abs() < 1e15 => {
                    format!("{f:.0}")
                }
                _ => n.to_string(),
            }))
        }
        Value::Array(_) | Value::Object(_) => Err(AgentError::Invalid(format!(
            "{what}: the value of `{column}` is an {}: send a string, number, boolean or null \
             (a JSON value goes in as a string of its text)",
            if value.is_array() { "array" } else { "object" }
        ))),
    }
}

fn row(what: &str, cells: &Map<String, Value>) -> Result<Row> {
    cells
        .iter()
        .map(|(column, value)| Ok((column.clone(), scalar(what, column, value)?)))
        .collect()
}

/// The arguments as [`Edits`], without a look at the table.
fn edits_of(args: &ProposeEditsArgs) -> Result<Edits> {
    let rows = |list: &Option<Vec<Map<String, Value>>>, name: &str| -> Result<Vec<Row>> {
        list.iter()
            .flatten()
            .enumerate()
            .map(|(i, cells)| row(&format!("{name}[{i}]"), cells))
            .collect()
    };
    let updates = args
        .updates
        .iter()
        .flatten()
        .enumerate()
        .map(|(i, u)| {
            Ok(Update {
                key: row(&format!("updates[{i}].key"), &u.key)?,
                set: row(&format!("updates[{i}].set"), &u.set)?,
            })
        })
        .collect::<Result<Vec<_>>>()?;
    Ok(Edits {
        schema: args.schema.clone(),
        table: args.table.clone(),
        inserts: rows(&args.inserts, "inserts")?,
        updates,
        deletes: rows(&args.deletes, "deletes")?,
    })
}

/// Why the app cannot edit the rows of this connection from its data viewer, if it cannot.
fn not_editable(saved: &SavedConnection) -> Option<String> {
    let name = &saved.name;
    match saved.driver {
        DriverKind::Mongo
        | DriverKind::Redis
        | DriverKind::Elasticsearch
        | DriverKind::Cassandra => Some(format!(
            "`{name}` is {}: its rows are not edited by key from the data viewer, so there \
                 is nothing to open the changes in",
            saved.driver.name()
        )),
        _ if saved.restrictions.no_data_editing => Some(format!(
            "`{name}` does not allow editing its data from PlatypusDB (restriction no data editing)"
        )),
        _ if saved.read_only => Some(format!(
            "`{name}` is read-only: the data viewer would not let the user save the changes"
        )),
        _ => None,
    }
}

impl Agent {
    /// Keeps the rows to change for PlatypusDB to open in the table's data viewer, where the
    /// user reviews, saves or discards them: nothing is written here. `origin` is the agent's
    /// name.
    pub async fn propose_edits(&self, args: &ProposeEditsArgs, origin: &str) -> Result<String> {
        let (store, saved) = self.find(&args.connection)?;
        if let Some(why) = not_editable(&saved) {
            return Err(AgentError::Denied(why));
        }
        let edits = edits_of(args)?;
        if edits.rows() == 0 {
            return Err(AgentError::Invalid(
                "there are no changes to propose: give inserts, updates or deletes".to_owned(),
            ));
        }
        if edits.rows() > MAX_ROWS {
            return Err(AgentError::Invalid(format!(
                "{} rows are too many for one proposal (at most {MAX_ROWS}): send fewer",
                edits.rows()
            )));
        }
        let json = serde_json::to_string(&edits).map_err(|e| AgentError::Store(e.to_string()))?;
        if json.len() > MAX_BYTES {
            return Err(AgentError::Invalid(format!(
                "the proposal is longer than {MAX_BYTES} bytes: send fewer rows"
            )));
        }
        self.windows.count(&saved)?;
        let details = self
            .read_table(&store, &saved, &args.schema, &args.table)
            .await?;
        // What the app will check again when it opens the proposal, and the statement it
        // will write from it.
        proposed::viewer(Dialect::from(saved.driver), &edits, &details)
            .map_err(AgentError::Invalid)?;
        let title = proposal_title(args.title.as_deref());
        let origin = crate::agent::origin_name(origin);
        let kept = self.with_calls(|calls| {
            Ok(calls.propose_edits(&origin, &saved.id.to_string(), title.as_deref(), &json)?)
        })?;
        if kept.is_none() {
            return Err(too_many_waiting(
                "Put all the rows of a table in one proposal instead",
            ));
        }
        Ok(format!(
            "Proposed on `{}`, table `{}.{}`: {} rows to insert, {} to update, {} to delete. \
             PlatypusDB opens the table's data viewer with them as pending edits, now if the \
             app is open or when it starts (within 24 hours). Nothing changed: the user reviews \
             them in the data viewer and saves or discards them.",
            saved.name,
            args.schema,
            args.table,
            edits.inserts.len(),
            edits.updates.len(),
            edits.deletes.len()
        ))
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn args(value: &Value) -> ProposeEditsArgs {
        serde_json::from_value(value.clone()).unwrap()
    }

    #[test]
    fn values_become_text() {
        let a = args(&json!({
            "connection": "c", "schema": "main", "table": "t",
            "inserts": [{"a": "x", "b": 5, "c": 1.5, "d": true, "e": null, "f": 5.0}],
        }));
        let edits = edits_of(&a).unwrap();
        let cells: Vec<(&str, Option<&str>)> = edits.inserts[0]
            .iter()
            .map(|(c, v)| (c.as_str(), v.as_deref()))
            .collect();
        assert_eq!(
            cells,
            [
                ("a", Some("x")),
                ("b", Some("5")),
                ("c", Some("1.5")),
                ("d", Some("true")),
                ("e", None),
                ("f", Some("5")),
            ]
        );
    }

    #[test]
    fn arrays_and_objects_are_refused_by_name() {
        let a = args(&json!({
            "connection": "c", "schema": "main", "table": "t",
            "updates": [{"key": {"id": 1}, "set": {"tags": ["a"]}}],
        }));
        let message = edits_of(&a).unwrap_err().to_string();
        assert!(message.contains("updates[0].set"), "{message}");
        assert!(message.contains("`tags` is an array"), "{message}");
        let a = args(&json!({
            "connection": "c", "schema": "main", "table": "t",
            "deletes": [{"id": {"x": 1}}],
        }));
        assert!(edits_of(&a).unwrap_err().to_string().contains("object"));
    }
}
