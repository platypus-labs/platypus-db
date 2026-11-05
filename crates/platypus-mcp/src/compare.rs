//! The logic of `compare_schemas`: the differences between two schemas and the script that
//! makes the second like the first, as the app's schema compare writes them
//! (`platypus_schema::compare` and `summary`), within a byte limit. Nothing runs.

use std::pin::Pin;
use std::time::Duration;

use platypus_core::{Connection, Error, ObjectMeta};
use platypus_schema::{SchemaChange, Script, TableDesign};
use platypus_sql::Dialect;
use platypus_store::{ConnectionStore, SavedConnection};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::agent::Agent;
use crate::error::{AgentError, Result};
use crate::guard::{self, has_plans};
use crate::query::{self, Limits};

/// The arguments of `compare_schemas`.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct CompareArgs {
    /// The connection with the schema that is the model, as `list_connections` shows it.
    pub connection: String,
    /// The model schema (a database in MySQL, `main` in SQLite and DuckDB).
    pub schema: String,
    /// The connection with the schema to change; left out, the same connection.
    pub other_connection: Option<String>,
    /// The schema the script would change to make it like `schema`.
    pub other_schema: String,
    /// Bytes of output at most (default 32768, up to 1048576).
    pub max_bytes: Option<u64>,
    /// Seconds each schema may take to read before it is cancelled (default 30, up to 300).
    pub timeout_seconds: Option<f64>,
}

/// What the answer says about the script, always: it reaches the user only through the
/// proposal tool.
const SCRIPT_NOTE: &str = "Nothing ran. The script makes the other schema like the first; it reaches \
the user only through propose_sql on the other connection, where they review and run it by hand.";

/// The objects of `schema`, or `None` when the connection has no such schema (some databases
/// answer a schema that does not exist with an error, others with no objects).
pub(crate) async fn objects_of(
    conn: &dyn Connection,
    schema: &str,
) -> Result<Option<Vec<ObjectMeta>>, Error> {
    let known = conn.schemas().await?.iter().any(|s| s.name == schema);
    match conn.objects(schema).await {
        Ok(objects) if known || !objects.is_empty() => Ok(Some(objects)),
        Err(e) if known => Err(e),
        _ => Ok(None),
    }
}

/// The two sides of a comparison, for the answer.
pub(crate) struct Sides<'a> {
    pub source: (&'a str, &'a str),
    pub target: (&'a str, &'a str),
    pub driver: &'a str,
}

/// The answer as JSON text within `max_bytes`. The differences are cut first, with a note;
/// a script that does not fit is left out whole and the answer says so, since a part of a
/// migration is dangerous to run.
pub(crate) fn render(
    sides: &Sides<'_>,
    changes: &[SchemaChange],
    script: &Script,
    dialect: Dialect,
    max_bytes: usize,
) -> String {
    let sql = script.text(dialect);
    let build = |shown: usize, with_script: bool| -> String {
        let mut answer = json!({
            "source": { "connection": sides.source.0, "schema": sides.source.1 },
            "target": { "connection": sides.target.0, "schema": sides.target.1 },
            "driver": sides.driver,
            "identical": changes.is_empty() && script.statements.is_empty(),
            "differences": &changes[..shown],
            "differences_total": changes.len(),
            "warnings": script.warnings,
            "note": SCRIPT_NOTE,
        });
        if shown < changes.len() {
            answer["differences_truncated"] = json!(format!(
                "only the first {shown} of {} differences are listed: the answer is cut at max_bytes",
                changes.len()
            ));
        }
        if with_script {
            answer["script"] = json!(sql);
            answer["script_complete"] = json!(true);
        } else {
            answer["script"] = Value::Null;
            answer["script_complete"] = json!(false);
            answer["script_omitted"] = json!(format!(
                "the migration script ({} bytes) does not fit in max_bytes and is NOT included: \
                 a part of it would be unsafe to run. Raise max_bytes (up to {}) or compare a \
                 schema with fewer differences",
                sql.len(),
                Limits::MAX_BYTES
            ));
        }
        answer.to_string()
    };
    for with_script in [true, false] {
        // The largest count of differences that fits: the text only grows with each one.
        let (mut lo, mut hi) = (0, changes.len());
        if build(0, with_script).len() > max_bytes {
            continue;
        }
        while lo < hi {
            let mid = (lo + hi).div_ceil(2);
            if build(mid, with_script).len() <= max_bytes {
                lo = mid;
            } else {
                hi = mid - 1;
            }
        }
        return build(lo, with_script);
    }
    // Not even the frame fits: the smallest honest answer.
    json!({
        "script": Value::Null,
        "script_complete": false,
        "script_omitted": "max_bytes is too small for any answer: raise it",
    })
    .to_string()
}

impl Agent {
    /// The differences between `schema` of `connection` and `other_schema` of
    /// `other_connection` (the same by default), and the script that makes the second like the
    /// first, in the dialect they share. Each schema is read within the connection's agent
    /// limits; nothing runs.
    pub async fn compare_schemas_until(
        &self,
        args: &CompareArgs,
        stop: impl Future<Output = ()>,
    ) -> Result<String> {
        let (store, source) = self.find(&args.connection)?;
        let other = args.other_connection.as_deref().unwrap_or(&args.connection);
        let (other_store, target) = self.find(other)?;
        if source.id == target.id && args.schema == args.other_schema {
            return Err(AgentError::Invalid(
                "that is one schema twice: name another schema or another connection".to_owned(),
            ));
        }
        let dialect = Dialect::from(source.driver);
        // The databases with plans are the ones with tables, keys and indexes to compare.
        if !has_plans(source.driver) || !has_plans(target.driver) {
            return Err(AgentError::Invalid(
                "compare_schemas works on databases with tables and foreign keys".to_owned(),
            ));
        }
        if dialect != Dialect::from(target.driver) {
            return Err(AgentError::Invalid(
                "Schemas can be compared only between databases of the same kind.".to_owned(),
            ));
        }
        let a_limits =
            guard::byte_limits(args.max_bytes, args.timeout_seconds, &source.agent_limits)?;
        let b_limits =
            guard::byte_limits(args.max_bytes, args.timeout_seconds, &target.agent_limits)?;
        self.windows.count(&source)?;
        if target.id != source.id {
            self.windows.count(&target)?;
        }
        let mut stop = std::pin::pin!(stop);
        let wanted = self
            .read_designs(
                (&store, &source),
                &args.schema,
                a_limits.timeout,
                stop.as_mut(),
            )
            .await?;
        let current = self
            .read_designs(
                (&other_store, &target),
                &args.other_schema,
                b_limits.timeout,
                stop.as_mut(),
            )
            .await?;
        let script =
            platypus_schema::compare(dialect, &args.other_schema, &args.schema, &wanted, &current)
                .map_err(AgentError::Invalid)?;
        let changes = platypus_schema::summary(&current, &wanted);
        let sides = Sides {
            source: (&source.name, &args.schema),
            target: (&target.name, &args.other_schema),
            driver: source.driver.name(),
        };
        Ok(render(
            &sides,
            &changes,
            &script,
            dialect,
            a_limits.max_bytes.min(b_limits.max_bytes),
        ))
    }

    /// The designs of the tables of `schema` on `saved`, read within `timeout`.
    async fn read_designs<S: Future<Output = ()>>(
        &self,
        (store, saved): (&ConnectionStore, &SavedConnection),
        schema: &str,
        timeout: Duration,
        stop: Pin<&mut S>,
    ) -> Result<Vec<TableDesign>> {
        let lease = self.lease(store, saved).await?;
        let dialect = Dialect::from(saved.driver);
        let conn = lease.conn();
        let work = async {
            if objects_of(conn, schema).await?.is_none() {
                return Ok(None);
            }
            platypus_schema::load_designs(conn, dialect, schema)
                .await
                .map(Some)
        };
        let read = query::guarded(conn, None, timeout, stop, work).await;
        lease.finish(read, timeout)?.ok_or_else(|| {
            AgentError::NotFound(format!("`{}` has no schema `{schema}`", saved.name))
        })
    }
}

#[cfg(test)]
mod tests {
    use platypus_schema::ChangeKind;

    use super::*;

    fn change(table: &str) -> SchemaChange {
        SchemaChange {
            table: table.to_owned(),
            kind: ChangeKind::TableAdded,
            item: None,
            from: None,
            to: None,
        }
    }

    fn script(statements: &[&str]) -> Script {
        Script {
            statements: statements.iter().map(|s| (*s).to_owned()).collect(),
            ..Script::default()
        }
    }

    fn sides() -> Sides<'static> {
        Sides {
            source: ("dev", "main"),
            target: ("prod", "main"),
            driver: "SQLite",
        }
    }

    fn answer(changes: &[SchemaChange], script: &Script, max: usize) -> Value {
        serde_json::from_str(&render(&sides(), changes, script, Dialect::Sqlite, max)).unwrap()
    }

    #[test]
    fn a_small_answer_has_everything() {
        let changes = [change("a"), change("b")];
        let out = answer(&changes, &script(&["CREATE TABLE a (id INT)"]), 100_000);
        assert_eq!(out["identical"], false);
        assert_eq!(out["differences"].as_array().unwrap().len(), 2);
        assert_eq!(out["script_complete"], true);
        assert!(out["script"].as_str().unwrap().contains("CREATE TABLE a"));
        assert!(out["note"].as_str().unwrap().contains("propose_sql"));
        assert!(out.get("differences_truncated").is_none());
    }

    #[test]
    fn no_differences_is_identical() {
        let out = answer(&[], &Script::default(), 100_000);
        assert_eq!(out["identical"], true);
    }

    #[test]
    fn differences_are_cut_before_the_script() {
        let changes: Vec<_> = (0..200).map(|i| change(&format!("table_{i}"))).collect();
        let sql = script(&["CREATE TABLE t (id INT)"]);
        let max = 2000;
        let text = render(&sides(), &changes, &sql, Dialect::Sqlite, max);
        assert!(text.len() <= max);
        let out: Value = serde_json::from_str(&text).unwrap();
        assert_eq!(out["script_complete"], true);
        assert!(out["differences_truncated"].is_string());
        assert_eq!(out["differences_total"], 200);
    }

    #[test]
    fn a_script_that_does_not_fit_is_left_out_whole() {
        let long = "x".repeat(5000);
        let sql = script(&[&format!("CREATE TABLE {long} (id INT)")]);
        let text = render(&sides(), &[change("a")], &sql, Dialect::Sqlite, 2000);
        assert!(text.len() <= 2000);
        assert!(!text.contains("CREATE TABLE"));
        let out: Value = serde_json::from_str(&text).unwrap();
        assert_eq!(out["script"], Value::Null);
        assert_eq!(out["script_complete"], false);
        assert!(out["script_omitted"].as_str().unwrap().contains("NOT"));
    }

    #[test]
    fn a_tiny_limit_still_answers_honestly() {
        let text = render(
            &sides(),
            &[change("a")],
            &script(&["S"]),
            Dialect::Sqlite,
            10,
        );
        let out: Value = serde_json::from_str(&text).unwrap();
        assert_eq!(out["script_complete"], false);
    }
}
