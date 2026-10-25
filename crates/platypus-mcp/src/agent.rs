//! What agents can do with the saved connections: the tools the MCP server and the CLI
//! expose. Each call reads the connections again, so a change of agent access in the app
//! applies at once; open connections are kept between calls and reopened when their
//! settings change, except DuckDB's, closed after each call.

use std::collections::HashMap;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::{Instant, SystemTime};

use platypus_core::{Connection, DriverKind, Error, ObjectKind, ObjectMeta, ObjectRef};
use platypus_sql::Dialect;
use platypus_store::{
    AgentAccess, ConnectionStore, HistoryStatus, HistoryStore, NewHistoryEntry, SavedConnection,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tokio::sync::OwnedMutexGuard;
use uuid::Uuid;

use crate::access::{self, UnfilteredRule};
use crate::connect;
use crate::error::{AgentError, Result};
use crate::output::QueryResult;
use crate::profile::Profile;
use crate::query::{self, Limits, RunError};
use crate::redact::Redactor;

/// Search hits returned at most.
pub const SEARCH_RESULTS: usize = 200;

/// Characters of an agent's name the history keeps.
const ORIGIN_CHARS: usize = 64;

/// An agent's name as the history keeps it: trimmed, without control characters, at most
/// 64 characters, and `agent` when nothing is left.
#[must_use]
pub fn origin_name(name: &str) -> String {
    let name: String = name
        .trim()
        .chars()
        .filter(|c| !c.is_control())
        .take(ORIGIN_CHARS)
        .collect();
    match name.trim_end() {
        "" => "agent".to_owned(),
        trimmed => trimmed.to_owned(),
    }
}

/// What tells two sets of connection settings apart: a hash of the effective connection,
/// its network profile in place.
fn fingerprint(effective: &SavedConnection) -> u64 {
    let mut hasher = DefaultHasher::new();
    serde_json::to_string(effective)
        .unwrap_or_default()
        .hash(&mut hasher);
    hasher.finish()
}

/// The arguments of `run_query`.
#[derive(Debug, Clone, Deserialize, schemars::JsonSchema)]
pub struct QueryArgs {
    /// The connection's name, as `list_connections` shows it.
    pub connection: String,
    /// One SQL statement.
    pub sql: String,
    /// Rows returned at most (default 100, up to 10000).
    pub max_rows: Option<u64>,
    /// Bytes of output at most (default 32768, up to 1048576).
    pub max_bytes: Option<u64>,
    /// Seconds the statement may run before it is cancelled (default 30, up to 300).
    pub timeout_seconds: Option<f64>,
    /// `text` (an aligned table, the default) or `json`.
    pub format: Option<crate::output::Format>,
}

/// A connection as agents see it: never its host, user, password or SSH settings.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ConnectionInfo {
    pub name: String,
    pub id: Uuid,
    pub driver: platypus_core::DriverKind,
    /// `dev`, `test` or `prod`.
    #[serde(rename = "type")]
    pub connection_type: Option<platypus_store::ConnectionType>,
    /// The database name, or the file of SQLite and DuckDB.
    pub database: String,
    /// What agents may do: `read_only` or `read_write`.
    pub agent_access: AgentAccess,
}

/// An object of a schema as agents see it: the sizes left out, and nothing that is unset.
#[derive(Serialize)]
struct ObjectInfo<'a> {
    name: &'a str,
    kind: ObjectKind,
    #[serde(skip_serializing_if = "Option::is_none")]
    parent: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    comment: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    rows: Option<u64>,
}

impl<'a> From<&'a ObjectMeta> for ObjectInfo<'a> {
    fn from(o: &'a ObjectMeta) -> Self {
        Self {
            name: &o.name,
            kind: o.kind,
            parent: o.parent.as_deref(),
            comment: o.comment.as_deref(),
            rows: o.rows,
        }
    }
}

/// An open connection kept for the next calls.
struct Session {
    /// The [`fingerprint`] of the settings it was opened with, to notice when they change.
    fingerprint: u64,
    driver: DriverKind,
    conn: Box<dyn Connection>,
    redactor: Redactor,
}

type Slot = Arc<tokio::sync::Mutex<Option<Session>>>;

/// The connection of one call, held for the call: calls on the same connection wait for
/// each other.
struct Lease {
    slot: OwnedMutexGuard<Option<Session>>,
}

impl Lease {
    fn session(&self) -> &Session {
        // `Agent::lease` fills the slot before handing it out.
        self.slot
            .as_ref()
            .unwrap_or_else(|| unreachable!("a lease always holds a session"))
    }

    fn conn(&self) -> &dyn Connection {
        &*self.session().conn
    }

    /// The error with the connection's secrets masked.
    fn error(&self, e: &Error) -> AgentError {
        let mut message = self.session().redactor.apply(&e.to_string());
        if let Error::Database(db) = e
            && let Some(position) = db.position
        {
            message = format!("{message} (at character {position})");
        }
        AgentError::Database(message)
    }

    /// Closes the connection: the next call opens a new one.
    fn discard(mut self) {
        *self.slot = None;
    }

    /// The result of a request made within [`timed`].
    fn settle<T>(self, result: Option<Result<T, Error>>) -> Result<T> {
        match result {
            Some(Ok(value)) => Ok(value),
            Some(Err(e)) => Err(self.error(&e)),
            None => {
                self.discard();
                Err(AgentError::Timeout(Limits::DEFAULT_TIMEOUT))
            }
        }
    }
}

impl Drop for Lease {
    /// DuckDB locks its file for the process that opens it: a session kept open would keep
    /// the app out of it.
    fn drop(&mut self) {
        if self
            .slot
            .as_ref()
            .is_some_and(|s| s.driver == DriverKind::DuckDb)
        {
            *self.slot = None;
        }
    }
}

/// `request` within the default timeout; `None` when it ran out.
async fn timed<T>(request: impl Future<Output = Result<T, Error>>) -> Option<Result<T, Error>> {
    tokio::time::timeout(Limits::DEFAULT_TIMEOUT, request)
        .await
        .ok()
}

/// The agent tools over a profile's saved connections.
pub struct Agent {
    profile: Profile,
    sessions: Mutex<HashMap<Uuid, Slot>>,
    history: Mutex<Option<HistoryStore>>,
}

impl std::fmt::Debug for Agent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Agent")
            .field("profile", &self.profile)
            .finish_non_exhaustive()
    }
}

fn locked<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

impl Agent {
    #[must_use]
    pub fn new(profile: Profile) -> Self {
        Self {
            profile,
            sessions: Mutex::default(),
            history: Mutex::default(),
        }
    }

    /// The connections agents may use, in the order the app lists them.
    pub fn list_connections(&self) -> Result<Vec<ConnectionInfo>> {
        let store = self.profile.connections()?;
        Ok(store
            .list()
            .iter()
            .filter(|c| c.effective_agent_access() != AgentAccess::Off)
            .map(|c| ConnectionInfo {
                name: c.name.clone(),
                id: c.id,
                driver: c.driver,
                connection_type: c.connection_type,
                database: c.database.clone(),
                agent_access: c.effective_agent_access(),
            })
            .collect())
    }

    /// The connection named `name` (or with that id), if agents may use it.
    fn find(&self, name: &str) -> Result<(ConnectionStore, SavedConnection)> {
        let store = self.profile.connections()?;
        let visible = || {
            store
                .list()
                .iter()
                .filter(|c| c.effective_agent_access() != AgentAccess::Off)
        };
        let id = Uuid::parse_str(name.trim()).ok();
        let exact: Vec<&SavedConnection> = visible()
            .filter(|c| Some(c.id) == id || c.name == name)
            .collect();
        let found: Vec<&SavedConnection> = if exact.is_empty() {
            visible()
                .filter(|c| c.name.trim().eq_ignore_ascii_case(name.trim()))
                .collect()
        } else {
            exact
        };
        match found.as_slice() {
            [one] => {
                let saved = (*one).clone();
                Ok((store, saved))
            }
            [] => Err(AgentError::NotFound(format!(
                "no connection named `{name}` is available to agents: list_connections shows those that are"
            ))),
            _ => Err(AgentError::Invalid(format!(
                "several connections are named `{name}`: use the id from list_connections"
            ))),
        }
    }

    fn slot(&self, id: Uuid) -> Slot {
        Arc::clone(locked(&self.sessions).entry(id).or_default())
    }

    /// The open connection of `saved`, reopened when its settings changed, the server
    /// dropped it or a transaction was left open.
    async fn lease(&self, store: &ConnectionStore, saved: &SavedConnection) -> Result<Lease> {
        let mut slot = self.slot(saved.id).lock_owned().await;
        let fingerprint = fingerprint(&store.effective(saved));
        let reusable = slot.as_ref().is_some_and(|s| {
            s.fingerprint == fingerprint && !s.conn.is_closed() && !s.conn.transaction().open
        });
        if !reusable {
            *slot = None;
            let (conn, redactor) = connect::open(store, saved).await?;
            *slot = Some(Session {
                fingerprint,
                driver: saved.driver,
                conn,
                redactor,
            });
        }
        Ok(Lease { slot })
    }

    /// The connection named `connection`, open, for a metadata request.
    async fn open(&self, connection: &str) -> Result<Lease> {
        let (store, saved) = self.find(connection)?;
        self.lease(&store, &saved).await
    }

    /// The schemas of a connection (databases in MySQL, attached databases in SQLite).
    pub async fn list_schemas(&self, connection: &str) -> Result<Value> {
        let lease = self.open(connection).await?;
        let schemas = timed(lease.conn().schemas()).await;
        lease.settle(schemas).map(|s| json!(s))
    }

    /// The tables, views, routines and other objects of a schema.
    pub async fn list_objects(&self, connection: &str, schema: &str) -> Result<Value> {
        let lease = self.open(connection).await?;
        let objects = timed(lease.conn().objects(schema)).await;
        let objects = lease.settle(objects)?;
        Ok(json!(
            objects.iter().map(ObjectInfo::from).collect::<Vec<_>>()
        ))
    }

    /// Columns, keys, foreign keys (both ways), indexes and constraints of a table or view.
    pub async fn describe_table(
        &self,
        connection: &str,
        schema: &str,
        table: &str,
    ) -> Result<Value> {
        let lease = self.open(connection).await?;
        let details = timed(lease.conn().table_details(schema, table)).await;
        lease.settle(details).map(|d| json!(d))
    }

    /// The `CREATE` statement of an object. Without `kind`, the object is looked up in
    /// its schema by name.
    pub async fn object_ddl(
        &self,
        connection: &str,
        schema: &str,
        name: &str,
        kind: Option<&str>,
    ) -> Result<String> {
        let kind = kind
            .map(|k| {
                serde_json::from_value::<ObjectKind>(json!(k.trim().to_ascii_lowercase()))
                    .map_err(|_| AgentError::Invalid(format!("unknown object kind `{k}`")))
            })
            .transpose()?;
        let lease = self.open(connection).await?;
        let ddl = timed(ddl_of(lease.conn(), schema, name, kind)).await;
        lease.settle(ddl)
    }

    /// Objects whose name contains `text` (ignoring case), and with databases that search
    /// their catalog also those with such a column or source.
    pub async fn search_objects(&self, connection: &str, text: &str) -> Result<Value> {
        if text.trim().is_empty() {
            return Err(AgentError::Invalid("the search text is empty".to_owned()));
        }
        let lease = self.open(connection).await?;
        let found = timed(search(lease.conn(), text)).await;
        let mut all = lease.settle(found)?;
        let truncated = all.len() > SEARCH_RESULTS;
        all.truncate(SEARCH_RESULTS);
        Ok(json!({ "matches": all, "truncated": truncated }))
    }

    /// Runs one statement within the connection's agent access and the limits, and
    /// records it in the query history with `origin`, the agent's name (see
    /// [`origin_name`]).
    pub async fn run_query(&self, args: &QueryArgs, origin: &str) -> Result<QueryResult> {
        let limits = Limits::new(args.max_rows, args.max_bytes, args.timeout_seconds)?;
        let (store, saved) = self.find(&args.connection)?;
        let dialect = Dialect::from(saved.driver);
        let access = saved.effective_agent_access();
        let unfiltered = UnfilteredRule::from_settings(self.profile.settings().as_ref());
        let started_at = SystemTime::now();
        let start = Instant::now();
        let result = match access::check(&args.sql, dialect, access, unfiltered, &saved.name) {
            Ok(statement) => {
                let read_only = (access == AgentAccess::ReadOnly).then_some(saved.driver);
                self.execute(&store, &saved, statement, read_only, &limits)
                    .await
            }
            Err(e) => Err(e),
        };
        let result = result.map(|outcome| QueryResult {
            outcome,
            elapsed: start.elapsed(),
            limits,
        });
        self.record(
            &saved,
            args.sql.trim(),
            started_at,
            start,
            &result,
            &origin_name(origin),
        );
        result
    }

    async fn execute(
        &self,
        store: &ConnectionStore,
        saved: &SavedConnection,
        statement: &str,
        read_only: Option<DriverKind>,
        limits: &Limits,
    ) -> Result<query::Outcome> {
        let lease = self.lease(store, saved).await?;
        match query::run(lease.conn(), statement, read_only, limits).await {
            Ok(outcome) => Ok(outcome),
            Err(RunError::Database(e)) => Err(lease.error(&e)),
            Err(RunError::Timeout) => {
                lease.discard();
                Err(AgentError::Timeout(limits.timeout))
            }
        }
    }

    /// Adds the statement to the query history; history is best effort.
    fn record(
        &self,
        saved: &SavedConnection,
        sql: &str,
        started_at: SystemTime,
        start: Instant,
        result: &Result<QueryResult>,
        origin: &str,
    ) {
        let (rows, status, error) = match result {
            Ok(r) => (
                match &r.outcome {
                    query::Outcome::Affected(n) => Some(*n),
                    query::Outcome::Rows(t) => u64::try_from(t.rows.len()).ok(),
                },
                HistoryStatus::Ok,
                None,
            ),
            Err(AgentError::Timeout(_)) => (None, HistoryStatus::Cancelled, None),
            Err(e) => (None, HistoryStatus::Error, Some(e.to_string())),
        };
        let entry = NewHistoryEntry {
            connection_id: saved.id,
            sql: sql.to_owned(),
            started_at,
            duration: start.elapsed(),
            rows,
            status,
            error,
            origin: Some(origin.to_owned()),
        };
        let mut history = locked(&self.history);
        if history.is_none() {
            *history = self.profile.history().ok();
        }
        if let Some(h) = history.as_ref() {
            let _ = h.record(&entry);
        }
    }
}

/// The `CREATE` statement of `schema.name`, of `kind` or of the object found by its name.
async fn ddl_of(
    conn: &dyn Connection,
    schema: &str,
    name: &str,
    kind: Option<ObjectKind>,
) -> Result<String, Error> {
    if !conn.capabilities().object_ddl {
        return Err(Error::Unsupported(
            "the DDL of objects with this database".to_owned(),
        ));
    }
    let (kind, parent) = if let Some(kind) = kind {
        (kind, None)
    } else {
        let objects = conn.objects(schema).await?;
        let object = objects
            .into_iter()
            .find(|o| o.name == name)
            .ok_or_else(|| Error::Other(format!("no object `{name}` in schema `{schema}`")))?;
        (object.kind, object.parent)
    };
    conn.object_ddl(&ObjectRef {
        schema,
        name,
        kind,
        parent: parent.as_deref(),
    })
    .await
}

/// The catalog search of the database, else a search of the object names.
async fn search(conn: &dyn Connection, text: &str) -> Result<Vec<Value>, Error> {
    if conn.capabilities().metadata_search {
        let matches = conn.search_metadata(text).await?;
        return Ok(matches.iter().map(|m| json!(m)).collect());
    }
    search_names(conn, text).await
}

/// Objects whose name contains `text`, for databases without a catalog search.
async fn search_names(conn: &dyn Connection, text: &str) -> Result<Vec<Value>, Error> {
    let needle = text.to_lowercase();
    let mut found = Vec::new();
    for schema in conn.schemas().await? {
        if schema.system {
            continue;
        }
        for object in conn.objects(&schema.name).await? {
            if object.name.to_lowercase().contains(&needle) {
                found.push(json!({
                    "schema": schema.name,
                    "name": object.name,
                    "kind": object.kind,
                    "parent": object.parent,
                    "matched": "name",
                    "column": null,
                }));
            }
        }
    }
    Ok(found)
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;

    #[test]
    fn origin_names_are_trimmed_and_capped() {
        assert_eq!(origin_name("  claude-code \n"), "claude-code");
        assert_eq!(origin_name(" \t "), "agent");
        assert_eq!(origin_name("a\u{7}b"), "ab");
        assert_eq!(origin_name(&"é".repeat(100)).chars().count(), ORIGIN_CHARS);
    }

    fn sqlite(path: &Path, rows: u32) {
        let db = rusqlite::Connection::open(path).unwrap();
        db.execute_batch("CREATE TABLE t (id INTEGER)").unwrap();
        for id in 0..rows {
            db.execute("INSERT INTO t VALUES (?1)", [id]).unwrap();
        }
    }

    fn save_connection(dir: &Path, driver: &str, database: &Path) {
        std::fs::write(
            dir.join("connections.toml"),
            format!(
                "version = 4\n\n[[connections]]\nid = \"00000000-0000-4000-8000-000000000001\"\n\
                 name = \"Db\"\ndriver = \"{driver}\"\ndatabase = '{}'\n",
                database.display()
            ),
        )
        .unwrap();
    }

    async fn count(agent: &Agent) -> String {
        let args = QueryArgs {
            connection: "Db".to_owned(),
            sql: "SELECT count(*) AS n FROM t".to_owned(),
            max_rows: None,
            max_bytes: None,
            timeout_seconds: None,
            format: None,
        };
        let result = agent.run_query(&args, "test").await.unwrap();
        result.json()["rows"][0][0].to_string()
    }

    #[tokio::test]
    async fn a_changed_connection_is_opened_again() {
        let dir = tempfile::tempdir().unwrap();
        let (one, two) = (dir.path().join("one.db"), dir.path().join("two.db"));
        sqlite(&one, 1);
        sqlite(&two, 2);
        let agent = Agent::new(Profile::at(dir.path()));
        save_connection(dir.path(), "sqlite", &one);
        assert_eq!(count(&agent).await, "1");
        save_connection(dir.path(), "sqlite", &two);
        assert_eq!(count(&agent).await, "2");
    }

    #[tokio::test]
    async fn duckdb_files_are_free_between_calls() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("data.duckdb");
        duckdb::Connection::open(&path)
            .unwrap()
            .execute_batch("CREATE TABLE t (id INTEGER); INSERT INTO t VALUES (1), (2), (3);")
            .unwrap();
        save_connection(dir.path(), "duckdb", &path);
        let agent = Agent::new(Profile::at(dir.path()));
        assert_eq!(count(&agent).await, "3");
        // Another instance, as the app would open, gets the file.
        duckdb::Connection::open(&path)
            .unwrap()
            .execute_batch("INSERT INTO t VALUES (4)")
            .unwrap();
        assert_eq!(count(&agent).await, "4");
    }
}
