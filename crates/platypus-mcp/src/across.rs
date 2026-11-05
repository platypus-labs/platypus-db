//! `query_across_databases`: one read-only statement over tables of several connections.
//!
//! Each source table is read from its own connection, which must be open to agents, within
//! that connection's limits and Safety rules, and copied into an in-memory DuckDB of the
//! call (the join of the app's "Query across databases", `platypus-local`). The statement
//! runs only there: it is checked as a read-only DuckDB statement, and the DuckDB session
//! has no access to files, the network or extensions and its settings are locked, so
//! it cannot write to a source nor reach the disk. The database is dropped with the call.
//!
//! Values of sensitive columns are copied as NULL, unless the source connection shows them:
//! after the join their origin would be lost. Dots, all equal, would make a join on such a column
//! match every row with every other; NULL matches nothing, and the answer names the columns.

use std::fmt::Write as _;
use std::sync::Arc;

use arrow_array::{Array, ArrayRef, RecordBatch, StringArray};
use arrow_schema::{DataType, Field, Schema, SchemaRef};
use futures::StreamExt;
use platypus_core::{
    Connection, ConnectionConfig, Driver, DriverKind, Error, ExecOptions, ExecResult, META_DB_TYPE,
};
use platypus_driver_duckdb::{DuckDbDriver, memory_database, memory_path};
use platypus_sql::Dialect;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::access::{self, AgentAccess, UnfilteredRule};
use crate::agent::Agent;
use crate::connect::agent_session_setup;
use crate::error::{AgentError, Result};
use crate::guard::{self, Asked};
use crate::output::{Format, QueryResult};
use crate::query::{self, Limits, Outcome, RunError};

/// The most sources of one call.
pub const MAX_SOURCES: usize = 8;
/// Rows copied from each source (default), and the most that can be asked.
pub const SOURCE_ROWS: u64 = 10_000;
pub const MAX_SOURCE_ROWS: u64 = 100_000;
/// Bytes (of Arrow memory) copied from each source at most.
pub const SOURCE_BYTES: usize = 32 * 1024 * 1024;
/// Rows per Arrow batch read from a source.
const BATCH_ROWS: usize = 4096;

/// A table to copy from a connection into the call's DuckDB.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct AcrossSource {
    /// The connection's name, as `list_connections` shows it.
    pub connection: String,
    /// The table's schema (`main` in SQLite, the database in MySQL).
    pub schema: String,
    /// The table or view.
    pub table: String,
    /// The table's name in the statement: letters, digits and `_`, not starting with a
    /// digit. Default: the table's name.
    pub name: Option<String>,
}

/// The arguments of `query_across_databases`.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct AcrossArgs {
    /// The tables to join, up to 8, each from a connection open to agents.
    pub sources: Vec<AcrossSource>,
    /// One read-only DuckDB statement over the sources, by their `name`.
    pub sql: String,
    /// Rows copied from each source (default 10000, up to 100000), within the
    /// connection's limits. A table with more rows is copied in part, and the answer says so.
    pub source_rows: Option<u64>,
    /// Rows returned at most (default 100, up to 10000).
    pub max_rows: Option<u64>,
    /// Bytes of output at most (default 32768, up to 1048576).
    pub max_bytes: Option<u64>,
    /// Seconds each read and the statement may run before it is cancelled (default 30, up to
    /// 300).
    pub timeout_seconds: Option<f64>,
    /// `text` (an aligned table, the default) or `json`.
    pub format: Option<Format>,
}

/// A source read, ready for DuckDB.
struct Copied {
    name: String,
    schema: SchemaRef,
    batches: Vec<RecordBatch>,
    /// The source has more rows than were copied.
    cut: bool,
    rows: usize,
    /// The columns masked, as `name.column`.
    masked: Vec<String>,
}

/// The tests that fill DuckDB databases wait for each other and for the one that opens a
/// DuckDB file: a database closes when its worker thread exits, which a busy machine delays.
#[cfg(test)]
pub(crate) static DUCKDB_TESTS: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

/// Whether `name` can stand unquoted in the statement.
fn is_simple(name: &str) -> bool {
    let mut chars = name.chars();
    chars
        .next()
        .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// The names of the sources in DuckDB: given or the table's, simple and distinct ignoring case.
fn names_of(sources: &[AcrossSource]) -> Result<Vec<String>> {
    if sources.is_empty() || sources.len() > MAX_SOURCES {
        return Err(AgentError::Invalid(format!(
            "give from 1 to {MAX_SOURCES} sources"
        )));
    }
    let mut names: Vec<String> = Vec::new();
    for s in sources {
        if s.schema.trim().is_empty() || s.table.trim().is_empty() {
            return Err(AgentError::Invalid(
                "each source needs a schema and a table".to_owned(),
            ));
        }
        let name = s.name.as_deref().unwrap_or(&s.table).trim().to_owned();
        if !is_simple(&name) {
            return Err(AgentError::Invalid(format!(
                "`{name}` cannot name a table in the statement: give `name` for {}.{} \
                 (letters, digits and `_`, not starting with a digit)",
                s.schema, s.table
            )));
        }
        if names.iter().any(|n| n.eq_ignore_ascii_case(&name)) {
            return Err(AgentError::Invalid(format!(
                "two sources are called `{name}`: give each its own `name`"
            )));
        }
        names.push(name);
    }
    Ok(names)
}

/// The statement that reads a source: all its rows.
fn select_sql(dialect: Dialect, source: &AcrossSource) -> String {
    format!(
        "SELECT * FROM {}",
        dialect.qualified(Some(source.schema.trim()), source.table.trim())
    )
}

/// `batch` with the columns `masked` all NULL, as text.
fn mask_batch(batch: &RecordBatch, schema: &SchemaRef, masked: &[bool]) -> Result<RecordBatch> {
    let columns: Vec<ArrayRef> = batch
        .columns()
        .iter()
        .zip(masked)
        .map(|(column, masked)| {
            if *masked {
                Arc::new(StringArray::new_null(column.len())) as ArrayRef
            } else {
                Arc::clone(column)
            }
        })
        .collect();
    RecordBatch::try_new(Arc::clone(schema), columns)
        .map_err(|e| AgentError::Database(e.to_string()))
}

/// `schema` with the fields `masked` as text.
fn masked_schema(schema: &Schema, masked: &[bool]) -> SchemaRef {
    let fields: Vec<Field> = schema
        .fields()
        .iter()
        .zip(masked)
        .map(|(f, masked)| {
            if *masked {
                Field::new(f.name(), DataType::Utf8, true)
            } else {
                f.as_ref().clone()
            }
        })
        .collect();
    Arc::new(Schema::new(fields))
}

/// Reads up to `max_rows` rows and [`SOURCE_BYTES`] bytes of `sql` as Arrow batches; whether
/// the statement had more.
async fn read_arrow(
    conn: &dyn Connection,
    sql: &str,
    max_rows: usize,
) -> std::result::Result<(SchemaRef, Vec<RecordBatch>, bool), Error> {
    let options = ExecOptions {
        batch_size: BATCH_ROWS,
        paged: true,
    };
    let ExecResult::Rows(mut stream) = conn.execute(sql, &options).await? else {
        return Err(Error::Config("the table returned no rows".to_owned()));
    };
    let schema = stream.schema();
    let (mut batches, mut rows, mut bytes) = (Vec::new(), 0usize, 0usize);
    while let Some(batch) = stream.next().await {
        let batch = batch?;
        let room = max_rows - rows;
        let take = batch.num_rows().min(room);
        let cut = batch.num_rows() > room;
        let batch = if cut { batch.slice(0, take) } else { batch };
        rows += batch.num_rows();
        bytes += batch.get_array_memory_size();
        batches.push(batch);
        // Dropping the stream cancels the rest of the statement.
        if cut || (rows == max_rows) || bytes > SOURCE_BYTES {
            let more = cut || stream.next().await.is_some() || bytes > SOURCE_BYTES;
            return Ok((schema, batches, more));
        }
    }
    Ok((schema, batches, false))
}

impl Agent {
    /// Reads `source` as `name` from its connection, masked as its connection's rules say.
    async fn read_source(
        &self,
        source: &AcrossSource,
        name: &str,
        (asked, requested): (Limits, &Asked),
        stop: &mut std::pin::Pin<&mut impl Future<Output = ()>>,
    ) -> Result<(Copied, Limits)> {
        let (store, saved) = self.find(&source.connection)?;
        if matches!(
            saved.driver,
            DriverKind::Mongo | DriverKind::Redis | DriverKind::Elasticsearch
        ) {
            return Err(AgentError::Invalid(format!(
                "`{}` is not a SQL database: its tables cannot be joined",
                saved.name
            )));
        }
        self.windows.count(&saved)?;
        let (limits, _) = guard::clamp(asked, requested, &saved.agent_limits);
        let dialect = Dialect::from(saved.driver);
        let sql = select_sql(dialect, source);
        access::check(
            &sql,
            dialect,
            AgentAccess::ReadOnly,
            UnfilteredRule::Refuse,
            &saved.name,
        )?;
        let sensitivity =
            self.table_sensitivity(&saved, source.schema.trim(), source.table.trim())?;
        self.cost_guard((&store, &saved), &sql, &limits, stop.as_mut())
            .await?;
        let lease = self.lease(&store, &saved).await?;
        let outcome = query::guarded(
            lease.conn(),
            Some(saved.driver),
            limits.timeout,
            stop.as_mut(),
            read_arrow(lease.conn(), &sql, limits.max_rows),
        )
        .await;
        let (schema, batches, cut) = lease
            .finish(outcome, limits.timeout)
            .map_err(|e| with_source(&saved.name, &source.table, e))?;
        let masked: Vec<bool> = schema
            .fields()
            .iter()
            .map(|f| {
                let db_type = f
                    .metadata()
                    .get(META_DB_TYPE)
                    .cloned()
                    .unwrap_or_else(|| f.data_type().to_string());
                sensitivity.is_sensitive(f.name(), &db_type)
            })
            .collect();
        let (schema, batches) = if masked.contains(&true) {
            let masked_schema = masked_schema(&schema, &masked);
            let batches = batches
                .iter()
                .map(|b| mask_batch(b, &masked_schema, &masked))
                .collect::<Result<Vec<_>>>()?;
            (masked_schema, batches)
        } else {
            (schema, batches)
        };
        let masked_names = schema
            .fields()
            .iter()
            .zip(&masked)
            .filter(|(_, masked)| **masked)
            .map(|(f, _)| format!("{name}.{}", f.name()))
            .collect();
        let rows = batches.iter().map(RecordBatch::num_rows).sum();
        Ok((
            Copied {
                name: name.to_owned(),
                schema,
                batches,
                cut,
                rows,
                masked: masked_names,
            },
            limits,
        ))
    }

    /// Runs `args.sql` over the sources, copied into a DuckDB of this call, and renders the
    /// result as `args.format` says.
    pub async fn query_across_until(
        &self,
        args: &AcrossArgs,
        stop: impl Future<Output = ()>,
    ) -> Result<String> {
        let start = std::time::Instant::now();
        let names = names_of(&args.sources)?;
        let rows = args.source_rows.unwrap_or(SOURCE_ROWS).min(MAX_SOURCE_ROWS);
        let asked = Limits::new(Some(rows), None, args.timeout_seconds)?;
        let requested = Asked {
            max_rows: Some(rows),
            max_bytes: None,
            timeout_seconds: args.timeout_seconds,
        };
        let mut stop = std::pin::pin!(stop);
        let mut copied = Vec::new();
        let mut rules = Vec::new();
        for (source, name) in args.sources.iter().zip(&names) {
            let (one, limits) = self
                .read_source(source, name, (asked, &requested), &mut stop)
                .await?;
            copied.push(one);
            rules.push(limits);
        }
        // The result keeps to the strictest of the sources' rules.
        let result_asked = Limits::new(args.max_rows, args.max_bytes, args.timeout_seconds)?;
        let result_requested = Asked {
            max_rows: args.max_rows,
            max_bytes: args.max_bytes,
            timeout_seconds: args.timeout_seconds,
        };
        let mut limits = result_asked;
        let mut notes = Vec::new();
        for (_, saved) in args
            .sources
            .iter()
            .map(|s| self.find(&s.connection))
            .collect::<Result<Vec<_>>>()?
        {
            let (tightest, more) = guard::clamp(limits, &result_requested, &saved.agent_limits);
            limits = tightest;
            for note in more {
                if !notes.contains(&note) {
                    notes.push(note);
                }
            }
        }
        let statement = access::check(
            &args.sql,
            Dialect::DuckDb,
            AgentAccess::ReadOnly,
            UnfilteredRule::Refuse,
            "the call's DuckDB",
        )?;
        let outcome = run_in_duckdb(&copied, statement, &limits, &mut stop).await?;
        notes.extend(copy_notes(&copied));
        let mut result = QueryResult {
            outcome,
            elapsed: start.elapsed(),
            limits,
            notes,
            cursor: None,
        };
        if let Outcome::Rows(table) = &mut result.outcome {
            table.masked = copied.iter().flat_map(|c| c.masked.clone()).collect();
        }
        Ok(result.render(args.format.unwrap_or_default()))
    }
}

/// What the answer says about how the sources were copied.
fn copy_notes(copied: &[Copied]) -> Vec<String> {
    let mut notes = Vec::new();
    for c in copied {
        if c.cut {
            notes.push(format!(
                "`{}` was copied in part ({} rows): the source has more, so joins may miss rows. \
                 Raise source_rows or choose a smaller table.",
                c.name, c.rows
            ));
        }
    }
    if copied.iter().any(|c| !c.masked.is_empty()) {
        let mut note = String::from(
            "Values of sensitive columns were copied as NULL before the join, so a join or a \
             filter on one matches nothing.",
        );
        let _ = write!(note, " Masked: {}.", masked_list(copied));
        notes.push(note);
    }
    notes
}

fn masked_list(copied: &[Copied]) -> String {
    copied
        .iter()
        .flat_map(|c| c.masked.iter().map(String::as_str))
        .collect::<Vec<_>>()
        .join(", ")
}

/// An error of reading `table` of `connection`, saying which.
fn with_source(connection: &str, table: &str, e: AgentError) -> AgentError {
    match e {
        AgentError::Database(m) => AgentError::Database(format!("{connection} · {table}: {m}")),
        other => other,
    }
}

/// What agents read when DuckDB is not installed.
fn duckdb_error(e: &Error) -> AgentError {
    match e {
        Error::DriverMissing { .. } => AgentError::Connect {
            name: "DuckDB".to_owned(),
            message: "the DuckDB library is not installed. PlatypusDB downloads it the first \
                      time it is needed in the app (opening a DuckDB connection, or Settings > \
                      Drivers); agents cannot download it: ask the user to do that once, then \
                      try again"
                .to_owned(),
        },
        other => AgentError::Database(other.to_string()),
    }
}

/// Fills a DuckDB of this call with `copied`, and runs `statement` in a session that reads
/// and writes no file and cannot change its settings.
async fn run_in_duckdb(
    copied: &[Copied],
    statement: &str,
    limits: &Limits,
    stop: &mut std::pin::Pin<&mut impl Future<Output = ()>>,
) -> Result<Outcome> {
    let name = format!("platypus-across-{}", Uuid::new_v4());
    let (path, tables) = (
        name.clone(),
        copied
            .iter()
            .map(|c| (c.name.clone(), c.schema.clone(), c.batches.clone()))
            .collect::<Vec<_>>(),
    );
    // Kept until the statement ends: the database goes with it.
    let _database = tokio::task::spawn_blocking(move || {
        let (database, conn) = memory_database(&path).map_err(|e| duckdb_error(&e))?;
        for (table, schema, batches) in &tables {
            platypus_local::duck::load(&conn, table, schema, batches)
                .map_err(|e| AgentError::Database(e.to_string()))?;
        }
        Ok::<_, AgentError>(database)
    })
    .await
    .map_err(|e| AgentError::Database(e.to_string()))??;
    let mut config = ConnectionConfig::new(DriverKind::DuckDb);
    config.database = memory_path(&name);
    let conn = DuckDbDriver
        .connect(&config)
        .await
        .map_err(|e| duckdb_error(&e))?;
    for sql in agent_session_setup(&config) {
        conn.execute(&sql, &ExecOptions::default())
            .await
            .map_err(|e| duckdb_error(&e))?;
    }
    query::run_until(&*conn, statement, None, limits, stop.as_mut())
        .await
        .map_err(|e| match e {
            RunError::Database(e) => AgentError::Database(e.to_string()),
            RunError::Timeout => AgentError::Timeout(limits.timeout),
            RunError::Cancelled => AgentError::Cancelled,
        })
}

#[cfg(test)]
mod tests {
    use arrow_array::Int64Array;

    use super::*;

    fn source(table: &str, name: Option<&str>) -> AcrossSource {
        AcrossSource {
            connection: "c".to_owned(),
            schema: "main".to_owned(),
            table: table.to_owned(),
            name: name.map(str::to_owned),
        }
    }

    /// Two SQLite databases, a hidden one and a profile that saves them.
    fn shop(dir: &std::path::Path) -> Agent {
        let make = |file: &str, sql: &str| {
            rusqlite::Connection::open(dir.join(file))
                .unwrap()
                .execute_batch(sql)
                .unwrap();
        };
        make(
            "shop.db",
            "CREATE TABLE orders (id INTEGER, customer_id INTEGER, total INTEGER);
             INSERT INTO orders VALUES (1, 1, 10), (2, 1, 5), (3, 2, 7);",
        );
        make(
            "crm.db",
            "CREATE TABLE customers (id INTEGER, name TEXT, email TEXT);
             INSERT INTO customers VALUES (1, 'Ann', 'ann@example.com'), (2, 'Bob', NULL);",
        );
        make("hidden.db", "CREATE TABLE secrets (id INTEGER);");
        let connection = |n: u32, name: &str, file: &str, extra: &str| {
            format!(
                "[[connections]]\nid = \"00000000-0000-4000-8000-00000000000{n}\"\nname = \"{name}\"\n\
                 driver = \"sqlite\"\ndatabase = '{}'\n{extra}\n",
                dir.join(file).display()
            )
        };
        std::fs::write(
            dir.join("connections.toml"),
            format!(
                "version = 4\n\n{}{}{}",
                connection(1, "Shop", "shop.db", ""),
                connection(2, "Crm", "crm.db", ""),
                connection(3, "Hidden", "hidden.db", "agent_access = \"off\""),
            ),
        )
        .unwrap();
        Agent::new(crate::Profile::at(dir))
    }

    fn across(sources: &[(&str, &str)], sql: &str) -> AcrossArgs {
        AcrossArgs {
            sources: sources
                .iter()
                .map(|(connection, table)| AcrossSource {
                    connection: (*connection).to_owned(),
                    schema: "main".to_owned(),
                    table: (*table).to_owned(),
                    name: None,
                })
                .collect(),
            sql: sql.to_owned(),
            source_rows: None,
            max_rows: None,
            max_bytes: None,
            timeout_seconds: None,
            format: Some(Format::Json),
        }
    }

    async fn run(agent: &Agent, args: &AcrossArgs) -> Result<serde_json::Value> {
        agent
            .query_across_until(args, std::future::pending())
            .await
            .map(|text| serde_json::from_str(&text).unwrap())
    }

    #[tokio::test]
    async fn joins_tables_of_two_connections() {
        platypus_driver_duckdb::testing::load();
        let _serial = crate::across::DUCKDB_TESTS.lock().await;
        let dir = tempfile::tempdir().unwrap();
        let agent = shop(dir.path());
        let args = across(
            &[("Shop", "orders"), ("Crm", "customers")],
            "SELECT c.name, CAST(sum(o.total) AS BIGINT) AS total FROM orders o JOIN customers c \
             ON c.id = o.customer_id GROUP BY c.name ORDER BY c.name",
        );
        let doc = run(&agent, &args).await.unwrap();
        assert_eq!(
            doc["rows"],
            serde_json::json!([["Ann", 15], ["Bob", 7]]),
            "{doc}"
        );
        // The same name can be asked again: each call has its own database.
        assert!(run(&agent, &args).await.is_ok());
    }

    #[tokio::test]
    async fn sensitive_columns_are_masked_before_they_enter_duckdb() {
        platypus_driver_duckdb::testing::load();
        let _serial = crate::across::DUCKDB_TESTS.lock().await;
        let dir = tempfile::tempdir().unwrap();
        let agent = shop(dir.path());
        let args = across(
            &[("Crm", "customers")],
            "SELECT id, email, upper(email) AS shout, length(email) AS n FROM customers ORDER BY id",
        );
        let doc = run(&agent, &args).await.unwrap();
        assert_eq!(
            doc["rows"][0],
            serde_json::json!([1, null, null, null]),
            "{doc}"
        );
        assert_eq!(
            doc["rows"][1],
            serde_json::json!([2, null, null, null]),
            "{doc}"
        );
        assert!(!doc.to_string().contains("ann@example.com"), "{doc}");
        assert_eq!(
            doc["masked_columns"],
            serde_json::json!(["customers.email"]),
            "{doc}"
        );
        let notes = doc["notes"].to_string();
        assert!(notes.contains("copied as NULL"), "{notes}");

        // Dots, all equal, would match every row with every other.
        let args = across(
            &[("Crm", "customers")],
            "SELECT count(*) AS n FROM customers a JOIN customers b ON a.email = b.email",
        );
        let doc = run(&agent, &args).await.unwrap();
        assert_eq!(doc["rows"][0][0], 0, "{doc}");
    }

    #[tokio::test]
    async fn a_source_off_to_agents_is_unknown() {
        platypus_driver_duckdb::testing::load();
        let _serial = crate::across::DUCKDB_TESTS.lock().await;
        let dir = tempfile::tempdir().unwrap();
        let agent = shop(dir.path());
        let args = across(&[("Shop", "orders"), ("Hidden", "secrets")], "SELECT 1");
        let Err(e) = run(&agent, &args).await else {
            panic!("a hidden connection joined")
        };
        assert!(
            e.to_string().contains("no connection named `Hidden`"),
            "{e}"
        );
        let args = across(&[("Shop", "orders"), ("Nowhere", "t")], "SELECT 1");
        assert!(run(&agent, &args).await.is_err());
    }

    #[tokio::test]
    async fn statements_that_write_or_reach_outside_are_refused() {
        platypus_driver_duckdb::testing::load();
        let _serial = crate::across::DUCKDB_TESTS.lock().await;
        let dir = tempfile::tempdir().unwrap();
        let agent = shop(dir.path());
        let out = dir.path().join("out.csv");
        let data = dir.path().join("data.csv");
        std::fs::write(&data, "a\n1\n").unwrap();
        let (out_sql, data_sql) = (
            out.display().to_string().replace('\\', "/"),
            data.display().to_string().replace('\\', "/"),
        );
        for sql in [
            "INSERT INTO orders VALUES (9, 9, 9)".to_owned(),
            "UPDATE orders SET total = 0".to_owned(),
            "DELETE FROM orders WHERE id = 1".to_owned(),
            "DROP TABLE orders".to_owned(),
            "CREATE TABLE z AS SELECT 1".to_owned(),
            format!("COPY orders TO '{out_sql}'"),
            format!("COPY (SELECT * FROM orders) TO '{out_sql}' (FORMAT csv)"),
            format!("ATTACH '{out_sql}.db' AS other"),
            format!("SELECT * FROM read_csv('{data_sql}')"),
            format!("SELECT * FROM '{data_sql}'"),
            format!("SELECT * FROM read_text('{data_sql}')"),
            "SELECT * FROM read_csv('https://example.com/x.csv')".to_owned(),
            "SET enable_external_access = true".to_owned(),
            "INSTALL httpfs".to_owned(),
            "SELECT 1; SELECT 2".to_owned(),
        ] {
            let args = across(&[("Shop", "orders")], &sql);
            assert!(run(&agent, &args).await.is_err(), "{sql}");
        }
        assert!(!out.exists() && !dir.path().join("out.csv.db").exists());
        // Nothing reached the source either.
        let args = across(&[("Shop", "orders")], "SELECT count(*) FROM orders");
        assert_eq!(run(&agent, &args).await.unwrap()["rows"][0][0], 3);
    }

    #[tokio::test]
    async fn a_source_copied_in_part_says_so() {
        platypus_driver_duckdb::testing::load();
        let _serial = crate::across::DUCKDB_TESTS.lock().await;
        let dir = tempfile::tempdir().unwrap();
        let agent = shop(dir.path());
        let mut args = across(&[("Shop", "orders")], "SELECT count(*) AS n FROM orders");
        args.source_rows = Some(2);
        let doc = run(&agent, &args).await.unwrap();
        assert_eq!(doc["rows"][0][0], 2, "{doc}");
        assert!(doc["notes"].to_string().contains("copied in part"), "{doc}");
        args.source_rows = Some(3);
        let doc = run(&agent, &args).await.unwrap();
        assert!(doc.get("notes").is_none(), "{doc}");
    }

    #[test]
    fn names_are_simple_and_distinct() {
        let ok = names_of(&[source("orders", None), source("Customers", Some("c"))]).unwrap();
        assert_eq!(ok, ["orders", "c"]);
        assert!(names_of(&[]).is_err());
        assert!(names_of(&[source("order items", None)]).is_err());
        assert!(names_of(&[source("order items", Some("items"))]).is_ok());
        assert!(names_of(&[source("a", None), source("b", Some("A"))]).is_err());
        assert!(names_of(&[source("1a", None)]).is_err());
        let many: Vec<_> = (0..=MAX_SOURCES)
            .map(|i| source("t", Some(&format!("t{i}"))))
            .collect();
        assert!(names_of(&many).is_err());
    }

    #[test]
    fn a_source_is_read_whole_with_quoted_names() {
        let s = AcrossSource {
            schema: "public".to_owned(),
            ..source("Order Items", None)
        };
        assert_eq!(
            select_sql(Dialect::Postgres, &s),
            "SELECT * FROM public.\"Order Items\""
        );
    }

    #[test]
    fn masking_writes_the_mask_and_keeps_null() {
        let schema = Arc::new(Schema::new(vec![
            Field::new("id", DataType::Int64, false),
            Field::new("email", DataType::Utf8, true),
        ]));
        let batch = RecordBatch::try_new(
            Arc::clone(&schema),
            vec![
                Arc::new(Int64Array::from(vec![1, 2])),
                Arc::new(StringArray::from(vec![Some("a@b.c"), None])),
            ],
        )
        .unwrap();
        let masked = [false, true];
        let out_schema = masked_schema(&schema, &masked);
        let out = mask_batch(&batch, &out_schema, &masked).unwrap();
        let email = out
            .column(1)
            .as_any()
            .downcast_ref::<StringArray>()
            .unwrap();
        assert!(email.is_null(0));
        assert!(email.is_null(1));
        assert_eq!(out.column(0).len(), 2);
    }

    #[test]
    fn a_masked_number_becomes_text() {
        let schema = Arc::new(Schema::new(vec![Field::new("ssn", DataType::Int64, true)]));
        let batch = RecordBatch::try_new(
            Arc::clone(&schema),
            vec![Arc::new(Int64Array::from(vec![Some(123), None]))],
        )
        .unwrap();
        let out_schema = masked_schema(&schema, &[true]);
        assert_eq!(out_schema.field(0).data_type(), &DataType::Utf8);
        let out = mask_batch(&batch, &out_schema, &[true]).unwrap();
        assert_eq!(out.num_rows(), 2);
    }

    #[test]
    fn the_statement_check_refuses_writes_and_file_access() {
        let check = |sql: &str| {
            access::check(
                sql,
                Dialect::DuckDb,
                AgentAccess::ReadOnly,
                UnfilteredRule::Refuse,
                "x",
            )
            .map(|_| ())
        };
        assert!(check("SELECT * FROM a JOIN b ON a.id = b.id").is_ok());
        for refused in [
            "INSERT INTO a VALUES (1)",
            "DELETE FROM a WHERE id = 1",
            "CREATE TABLE z AS SELECT 1",
            "COPY (SELECT 1) TO 'out.csv'",
            "COPY a TO 'out.parquet'",
            "ATTACH 'other.db' AS o",
            "SELECT * FROM read_csv('/etc/passwd')",
            "SELECT * FROM 'data.csv'",
            "SELECT * FROM read_parquet('x.parquet')",
            "SELECT getenv('HOME')",
            "SET enable_external_access = true",
            "INSTALL httpfs",
            "PRAGMA database_list",
        ] {
            assert!(check(refused).is_err(), "{refused}");
        }
    }
}
