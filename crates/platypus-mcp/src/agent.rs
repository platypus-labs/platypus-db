//! What agents can do with the saved connections: the tools the MCP server and the CLI
//! expose. Each call reads the connections again, so a change of agent access in the app
//! applies at once; open connections are kept between calls and reopened when their
//! settings change, except DuckDB's, closed after each call.

use std::collections::{HashMap, HashSet};
use std::hash::{DefaultHasher, Hash, Hasher};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant, SystemTime};

use platypus_core::{
    Connection, DriverKind, Error, ObjectKind, ObjectMeta, ObjectRef, TableDetails,
};
use platypus_docs::DocSchema;
use platypus_sql::Dialect;
use platypus_store::{
    AgentAccess, AgentCallStore, ConnectionStore, HistoryStatus, HistoryStore, MAX_PENDING_ALL,
    MAX_PENDING_PROPOSALS, NewAgentCall, NewHistoryEntry, SavedConnection,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tokio::sync::OwnedMutexGuard;
use uuid::Uuid;

use crate::access::{self, UnfilteredRule};
use crate::connect;
use crate::er;
use crate::error::{AgentError, Result};
use crate::estimate;
use crate::guard::{self, Asked, CallWindows};
use crate::link::{AppLink, CLOSED, Verdict, WriteRequest};
use crate::output::QueryResult;
use crate::plan::PlanResult;
use crate::policy::{AgentPolicy, Seal};
use crate::profile::Profile;
use crate::query::{self, Limits, RunError};
use crate::redact::Redactor;

/// Search hits returned at most.
pub const SEARCH_RESULTS: usize = 200;

/// Objects `list_objects` returns by default, and at most.
pub const OBJECT_PAGE: usize = 500;
pub const MAX_OBJECT_PAGE: usize = 2000;

/// The page of `objects` from `offset`, of `limit` objects (default [`OBJECT_PAGE`], at most
/// [`MAX_OBJECT_PAGE`]): the list itself when it holds them all, else
/// `{objects, truncated, total, offset, next_offset}`.
fn object_page(objects: &[ObjectMeta], offset: Option<u64>, limit: Option<u64>) -> Result<Value> {
    let limit = match limit {
        None => OBJECT_PAGE,
        Some(0) => {
            return Err(AgentError::Invalid(
                "limit must be greater than 0".to_owned(),
            ));
        }
        Some(n) => usize::try_from(n)
            .unwrap_or(usize::MAX)
            .min(MAX_OBJECT_PAGE),
    };
    let start = offset.map_or(0, |o| usize::try_from(o).unwrap_or(usize::MAX));
    let page: Vec<ObjectInfo> = objects
        .iter()
        .skip(start)
        .take(limit)
        .map(ObjectInfo::from)
        .collect();
    if start == 0 && page.len() == objects.len() {
        return Ok(json!(page));
    }
    let end = start.saturating_add(page.len());
    let truncated = end < objects.len();
    Ok(json!({
        "objects": page,
        "truncated": truncated,
        "total": objects.len(),
        "offset": start,
        "next_offset": truncated.then_some(end),
    }))
}

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
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
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

/// The arguments of `explain`.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ExplainArgs {
    /// The connection's name, as `list_connections` shows it.
    pub connection: String,
    /// One SQL statement.
    pub sql: String,
    /// Run the statement for real rows and times (`EXPLAIN ANALYZE`); what it changes is
    /// rolled back. Default false: the planner's estimates, without running it.
    pub analyze: Option<bool>,
    /// Bytes of output at most (default 32768, up to 1048576).
    pub max_bytes: Option<u64>,
    /// Seconds the request may take before it is cancelled (default 30, up to 300).
    pub timeout_seconds: Option<f64>,
    /// `text` (an indented tree, the default) or `json`.
    pub format: Option<crate::output::Format>,
}

/// The arguments of `sample_rows`.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct SampleArgs {
    /// The connection's name, as `list_connections` shows it.
    pub connection: String,
    /// The table's schema (a database in MySQL and MongoDB, `main` in SQLite and DuckDB).
    pub schema: String,
    /// The table, view or collection.
    pub table: String,
    /// Rows returned at most (default 20, up to 10000).
    pub max_rows: Option<u64>,
    /// Bytes of output at most (default 32768, up to 1048576).
    pub max_bytes: Option<u64>,
    /// Seconds the query may run before it is cancelled (default 30, up to 300).
    pub timeout_seconds: Option<f64>,
    /// `text` (an aligned table, the default) or `json`.
    pub format: Option<crate::output::Format>,
}

/// The arguments of `propose_sql`.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ProposeArgs {
    /// The connection's name, as `list_connections` shows it.
    pub connection: String,
    /// The SQL to open in the editor: one statement or a script.
    pub sql: String,
    /// The editor tab's title, e.g. what the SQL does.
    pub title: Option<String>,
}

/// Bytes of SQL `propose_sql` takes at most.
pub const MAX_PROPOSAL_BYTES: usize = 1024 * 1024;

/// Characters of a proposal's title kept.
const TITLE_CHARS: usize = 80;

/// Rows `sample_rows` returns by default.
pub const SAMPLE_ROWS: u64 = 20;

/// The statement reading the first `rows` rows of `schema.table` on `driver`.
pub(crate) fn sample_sql(driver: DriverKind, schema: &str, table: &str, rows: u64) -> String {
    if driver == DriverKind::Mongo {
        let collection = platypus_driver_mongo::generate::collection_ref(Some(schema), table);
        return format!("{collection}.find({{}}).limit({rows})");
    }
    if driver == DriverKind::Redis {
        return format!("BROWSE {schema}.{table} LIMIT {rows}");
    }
    if driver == DriverKind::Elasticsearch {
        return platypus_driver_search::command::browse_limited(table, Some(rows));
    }
    if driver == DriverKind::Cassandra {
        let name = platypus_driver_cassandra::cql::qualified(schema, table);
        return format!("SELECT * FROM {name} LIMIT {rows}");
    }
    let dialect = Dialect::from(driver);
    let name = dialect.qualified(Some(schema), table);
    dialect.limited(&format!("SELECT * FROM {name}"), rows)
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
pub(crate) struct Lease {
    slot: OwnedMutexGuard<Option<Session>>,
}

impl Lease {
    fn session(&self) -> &Session {
        // `Agent::lease` fills the slot before handing it out.
        self.slot
            .as_ref()
            .unwrap_or_else(|| unreachable!("a lease always holds a session"))
    }

    pub(crate) fn conn(&self) -> &dyn Connection {
        &*self.session().conn
    }

    /// The connection's secrets, masked wherever they appear.
    pub(crate) fn redactor(&self) -> &Redactor {
        &self.session().redactor
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

    /// The result of a request made within [`query::guarded`]: after a timeout or a
    /// cancellation the connection is closed, the request may still be running on it.
    pub(crate) fn finish<T>(
        self,
        result: Result<T, RunError>,
        timeout: std::time::Duration,
    ) -> Result<T> {
        match result {
            Ok(value) => Ok(value),
            Err(RunError::Database(e)) => Err(self.error(&e)),
            Err(RunError::Timeout) => {
                self.discard();
                Err(AgentError::Timeout(timeout))
            }
            Err(RunError::Cancelled) => {
                self.discard();
                Err(AgentError::Cancelled)
            }
        }
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
    calls: Mutex<Option<AgentCallStore>>,
    /// The open app, which shows its sessions and asks the user before a change.
    link: Option<Arc<dyn AppLink>>,
    /// The connections whose changes the user allowed for a run of an agent's server:
    /// `(session, connection id)`.
    allowed: Mutex<HashSet<(String, Uuid)>>,
    /// The session of calls that bring none (the CLI's).
    session: String,
    /// The policy the user approved in the app, which edits of the files cannot loosen.
    seal: Seal,
    /// The calls of the last minute on each connection, for `calls_per_minute`.
    pub(crate) windows: CallWindows,
}

/// Whether the database refused `message`'s statement for writing in a read-only
/// transaction (PostgreSQL, MySQL and MariaDB) or with `query_only` (SQLite).
fn refused_write(message: &str) -> bool {
    let message = message.to_ascii_lowercase();
    [
        "read-only transaction",
        "read only transaction",
        "readonly database",
    ]
    .iter()
    .any(|refusal| message.contains(refusal))
}

/// Bytes of a plan shown to the user beside a change.
pub(crate) const PLAN_BYTES: usize = 16 * 1024;

impl std::fmt::Debug for Agent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Agent")
            .field("profile", &self.profile)
            .finish_non_exhaustive()
    }
}

pub(crate) fn locked<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

/// The one rule of which connections agents see: those whose agent access is not off.
pub(crate) fn may_use(c: &SavedConnection) -> bool {
    c.effective_agent_access() != AgentAccess::Off
}

impl Agent {
    #[must_use]
    pub fn new(profile: Profile) -> Self {
        let seal = Seal::of(profile.config_dir());
        Self {
            profile,
            sessions: Mutex::default(),
            history: Mutex::default(),
            calls: Mutex::default(),
            link: None,
            allowed: Mutex::default(),
            session: Uuid::new_v4().to_string(),
            seal,
            windows: CallWindows::default(),
        }
    }

    /// The agent, with the approved policy sealed by `seal` instead of the keychain's.
    #[must_use]
    pub fn with_seal(mut self, seal: Seal) -> Self {
        self.seal = seal;
        self
    }

    /// What agents may do now: what the files say, never more than the user approved in the
    /// app (see [`crate::policy`]).
    fn policy(&self, store: &ConnectionStore) -> AgentPolicy {
        let settings = self.profile.settings();
        let current = AgentPolicy::current(store.list(), settings.as_ref());
        self.seal.read().honored(&current)
    }

    /// The saved connections, each with the agent access agents get on it ([`Self::policy`]),
    /// and the store.
    pub(crate) fn connections(&self) -> Result<(ConnectionStore, Vec<SavedConnection>)> {
        let store = self.profile.connections()?;
        let policy = self.policy(&store);
        let connections = store
            .list()
            .iter()
            .map(|c| {
                let mut c = c.clone();
                c.agent_access = policy.access(c.id);
                c.agent_values = policy.values(c.id);
                c.agent_limits = policy.limits(c.id);
                c
            })
            .collect();
        Ok((store, connections))
    }

    /// The agent, reaching the open app through `link`.
    #[must_use]
    pub fn with_link(mut self, link: Arc<dyn AppLink>) -> Self {
        self.link = Some(link);
        self
    }

    /// The open app, if the agent reaches it.
    #[must_use]
    pub fn link(&self) -> Option<&Arc<dyn AppLink>> {
        self.link.as_ref()
    }

    /// Whether agents' changes wait for the user's yes in the app (Settings › Safety), as
    /// approved there.
    /// Whether the user allowed the changes on connection `id` for this server's session.
    pub(crate) fn allowed_for_session(&self, id: Uuid) -> bool {
        locked(&self.allowed).contains(&(self.session.clone(), id))
    }

    pub(crate) fn confirms_changes(&self) -> bool {
        self.profile
            .connections()
            .map_or(true, |store| self.policy(&store).confirm_writes)
    }

    /// What the Safety setting for `UPDATE` or `DELETE` without `WHERE` means for agents, as
    /// approved in the app.
    pub(crate) fn unfiltered_rule(&self, store: &ConnectionStore) -> UnfilteredRule {
        if self.policy(store).unfiltered_allowed {
            UnfilteredRule::Allow
        } else {
            UnfilteredRule::Refuse
        }
    }

    /// Asks the user, in the open app, to let `statement` change `saved` for `origin`, unless
    /// they allowed the changes of `session` there already or need not be asked. A query that
    /// acts outside the transaction is asked about as a change ([`access::asks_first`]).
    async fn allow_change(
        &self,
        (store, saved): (&ConnectionStore, &SavedConnection),
        statement: &str,
        (origin, session): (&str, &str),
    ) -> Result<()> {
        if !self.confirms_changes() || !access::asks_first(statement, saved.driver) {
            return Ok(());
        }
        self.ask((store, saved), statement, (origin, session)).await
    }

    /// Asks the user, in the open app, to let `statement` change `saved` for `origin`,
    /// unless they allowed the changes of `session` there already.
    async fn ask(
        &self,
        (store, saved): (&ConnectionStore, &SavedConnection),
        statement: &str,
        (origin, session): (&str, &str),
    ) -> Result<()> {
        let key = (session.to_owned(), saved.id);
        if locked(&self.allowed).contains(&key) {
            return Ok(());
        }
        let cannot_ask = |why: &str| {
            AgentError::Denied(format!(
                "PlatypusDB asks the user before an agent changes data, and cannot ask: {why}. \
                 Open PlatypusDB, or let agents change data without asking in its Settings \
                 (Agents)"
            ))
        };
        // Nothing to plan for a question nobody can answer.
        let Some(link) = self.link.as_ref().filter(|link| link.reachable()) else {
            return Err(cannot_ask(CLOSED));
        };
        let (estimated_rows, plan) = self.estimate(store, saved, statement).await;
        let request = WriteRequest {
            id: Uuid::new_v4().to_string(),
            agent: origin_name(origin),
            session: session.to_owned(),
            connection: saved.name.clone(),
            statement: statement.trim().to_owned(),
            estimated_rows,
            plan,
        };
        match link.confirm(request).await {
            Ok(Verdict::Once) => Ok(()),
            Ok(Verdict::Session) => {
                locked(&self.allowed).insert(key);
                Ok(())
            }
            Ok(Verdict::Deny { reason }) => Err(AgentError::Denied(match reason {
                Some(reason) if !reason.trim().is_empty() => {
                    format!("the user refused this change: {}", reason.trim())
                }
                _ => "the user refused this change".to_owned(),
            })),
            Err(why) => Err(cannot_ask(&why)),
        }
    }

    /// The rows `statement` is expected to change and its plan, from the database's plan
    /// where it has one; nothing runs.
    async fn estimate(
        &self,
        store: &ConnectionStore,
        saved: &SavedConnection,
        statement: &str,
    ) -> (Option<f64>, Option<String>) {
        let Ok(lease) = self.lease(store, saved).await else {
            return (None, None);
        };
        if !lease.conn().capabilities().explain {
            return (None, None);
        }
        // A plan that timed out leaves the session mid-request: `settle` drops it.
        let explained = timed(lease.conn().explain(statement, false)).await;
        let Ok(explain) = lease.settle(explained) else {
            return (None, None);
        };
        let rows = crate::estimate::expected_rows(&explain.root);
        let plan = PlanResult {
            explain,
            elapsed: std::time::Duration::ZERO,
            max_bytes: PLAN_BYTES,
        }
        .text();
        (rows, Some(plan))
    }

    /// The connections agents may use, in the order the app lists them.
    pub fn list_connections(&self) -> Result<Vec<ConnectionInfo>> {
        let (_, connections) = self.connections()?;
        Ok(connections
            .iter()
            .filter(|c| may_use(c))
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

    pub(crate) const fn profile(&self) -> &Profile {
        &self.profile
    }

    /// The connection named `name` (or with that id), if agents may use it.
    pub(crate) fn find(&self, name: &str) -> Result<(ConnectionStore, SavedConnection)> {
        let (store, connections) = self.connections()?;
        let visible = || connections.iter().filter(|c| may_use(c));
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
    pub(crate) async fn lease(
        &self,
        store: &ConnectionStore,
        saved: &SavedConnection,
    ) -> Result<Lease> {
        let mut slot = self.slot(saved.id).lock_owned().await;
        let fingerprint = fingerprint(&store.effective(saved));
        let reusable = slot.as_ref().is_some_and(|s| {
            s.fingerprint == fingerprint && !s.conn.is_closed() && !s.conn.transaction().open
        });
        if !reusable {
            *slot = None;
            let settings = self.profile.settings();
            let (conn, redactor) = connect::open(store, saved, settings.as_ref()).await?;
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

    /// The tables, views, routines and other objects of a schema, a page at a time (see
    /// [`object_page`]).
    pub async fn list_objects(
        &self,
        connection: &str,
        schema: &str,
        offset: Option<u64>,
        limit: Option<u64>,
    ) -> Result<Value> {
        let (store, saved) = self.find(connection)?;
        let lease = self.lease(&store, &saved).await?;
        let redactor = lease.redactor().clone();
        let conn = lease.conn();
        // No objects may also mean no such schema: say so, rather than an empty list.
        let objects = timed(async {
            let objects = conn.objects(schema).await?;
            let exists =
                !objects.is_empty() || conn.schemas().await?.iter().any(|s| s.name == schema);
            Ok::<_, Error>(exists.then_some(objects))
        })
        .await;
        let objects = lease.settle(objects)?.ok_or_else(|| {
            AgentError::NotFound(format!("`{connection}` has no schema `{schema}`"))
        })?;
        let page = object_page(&objects, offset, limit)?;
        let view = self.catalog_view(saved.id, &saved.database);
        Ok(view.list(schema, page, &redactor))
    }

    /// Columns, keys, foreign keys (both ways), indexes and constraints of a table or view.
    pub async fn describe_table(
        &self,
        connection: &str,
        schema: &str,
        table: &str,
    ) -> Result<Value> {
        let (store, saved) = self.find(connection)?;
        let lease = self.lease(&store, &saved).await?;
        let redactor = lease.redactor().clone();
        let conn = lease.conn();
        // A missing table reads as one without columns: say so instead.
        let details = timed(async {
            let details = conn.table_details(schema, table).await?;
            let exists = !details.columns.is_empty()
                || conn.objects(schema).await?.iter().any(|o| o.name == table);
            Ok::<_, Error>(exists.then_some(details))
        })
        .await;
        let details = lease.settle(details)?.ok_or_else(|| {
            AgentError::NotFound(format!(
                "`{connection}` has no table or view `{schema}.{table}`"
            ))
        })?;
        let mut details = json!(details);
        let view = self.catalog_view(saved.id, &saved.database);
        view.describe(schema, table, &mut details, &redactor);
        Ok(details)
    }

    /// A Mermaid `erDiagram` of a schema's tables and the foreign keys between them: all of
    /// them, or `tables` and those within `depth` foreign-key hops of them. Cut at the byte limit.
    pub async fn er_diagram(
        &self,
        connection: &str,
        schema: &str,
        tables: &[String],
        depth: Option<u64>,
    ) -> Result<String> {
        let (store, saved) = self.find(connection)?;
        if matches!(
            saved.driver,
            DriverKind::Mongo | DriverKind::Redis | DriverKind::Elasticsearch
        ) {
            return Err(AgentError::Invalid(format!(
                "get_er_diagram works on databases with tables and foreign keys: {} has none here",
                saved.driver.name()
            )));
        }
        let lease = self.lease(&store, &saved).await?;
        let read = timed(read_er(lease.conn(), schema, tables, er::hops(depth))).await;
        let read = lease.settle(read).map_err(|e| match e {
            AgentError::Timeout(_) if tables.is_empty() => AgentError::Invalid(format!(
                "reading all the tables of `{schema}` took too long: pass `tables` to draw some"
            )),
            other => other,
        })?;
        match read {
            ErRead::NoSchema => Err(AgentError::NotFound(format!(
                "`{connection}` has no schema `{schema}`"
            ))),
            ErRead::Unknown { name, other } => Err(AgentError::NotFound(if other {
                format!("`{schema}.{name}` is not a table: only tables are drawn")
            } else {
                format!("`{connection}` has no table `{schema}.{name}`")
            })),
            ErRead::Found { doc, total } => Ok(er::render(&doc, total, Limits::DEFAULT_BYTES)),
        }
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
        let (store, saved) = self.find(connection)?;
        let lease = self.lease(&store, &saved).await?;
        let view = self.catalog_view(saved.id, &saved.database);
        let query = view.query(text);
        let redactor = lease.redactor().clone();
        let found = timed(search_all(lease.conn(), text, &view, &query, &redactor)).await;
        let (mut all, catalog_read) = lease.settle(found)?;
        let truncated = all.len() > SEARCH_RESULTS;
        all.truncate(SEARCH_RESULTS);
        let mut answer = json!({ "matches": all, "truncated": truncated });
        if !catalog_read {
            answer["catalog_incomplete"] = json!(
                "comments, notes and tags were not searched: listing the objects took too long"
            );
        }
        Ok(answer)
    }

    /// Runs one statement within the connection's agent access and the limits, and
    /// records it in the query history with `origin`, the agent's name (see
    /// [`origin_name`]).
    pub async fn run_query(&self, args: &QueryArgs, origin: &str) -> Result<QueryResult> {
        self.run_query_until(args, (origin, &self.session), std::future::pending())
            .await
    }

    /// [`run_query`](Self::run_query) for the agent `origin` in session `session` of its
    /// server, cancelling the statement when `stop` completes first: the client cancelled the
    /// request. A change waits for the user's yes in the open app, where Settings ask for it.
    pub async fn run_query_until(
        &self,
        args: &QueryArgs,
        (origin, session): (&str, &str),
        stop: impl Future<Output = ()>,
    ) -> Result<QueryResult> {
        let asked = Limits::new(args.max_rows, args.max_bytes, args.timeout_seconds)?;
        let (store, saved) = self.find(&args.connection)?;
        let requested = Asked {
            max_rows: args.max_rows,
            max_bytes: args.max_bytes,
            timeout_seconds: args.timeout_seconds,
        };
        let (limits, mut notes) = guard::clamp(asked, &requested, &saved.agent_limits);
        let access = saved.effective_agent_access();
        let unfiltered = self.unfiltered_rule(&store);
        let started_at = SystemTime::now();
        let start = Instant::now();
        let checked = estimate::check(&args.sql, &saved, unfiltered);
        // The time the user took to answer is not the statement's.
        let mut waited = Duration::ZERO;
        let mut stop = std::pin::pin!(stop);
        let result = async {
            self.windows.count(&saved)?;
            let statement = checked?;
            let asked = Instant::now();
            // The client's cancellation withdraws the question too.
            tokio::select! {
                allowed = self.allow_change((&store, &saved), statement, (origin, session)) => allowed?,
                () = stop.as_mut() => return Err(AgentError::Cancelled),
            }
            waited = asked.elapsed();
            if let Some(note) = self
                .cost_guard((&store, &saved), statement, &limits, stop.as_mut())
                .await?
            {
                notes.push(note);
            }
            if access == AgentAccess::ReadOnly {
                return self
                    .execute(&store, &saved, statement, Some(saved.driver), &limits, stop)
                    .await;
            }
            // A query may still write through a function: where the database refuses writes
            // in a read-only transaction, it runs in one, and asks when it is refused.
            if !self.watches_queries(&saved, statement, session) {
                return self
                    .execute(&store, &saved, statement, None, &limits, stop)
                    .await;
            }
            let first = self
                .execute(&store, &saved, statement, Some(saved.driver), &limits, stop.as_mut())
                .await;
            match first {
                Err(AgentError::Database(message)) if refused_write(&message) => {
                    let asked = Instant::now();
                    tokio::select! {
                        allowed = self.ask((&store, &saved), statement, (origin, session)) => allowed?,
                        () = stop.as_mut() => return Err(AgentError::Cancelled),
                    }
                    waited += asked.elapsed();
                    self.execute(&store, &saved, statement, None, &limits, stop)
                        .await
                }
                other => other,
            }
        }
        .await;
        let (started_at, start) = (started_at + waited, start + waited);
        let result = result
            .and_then(|outcome| self.mask_outcome(&saved, &args.sql, outcome))
            .map(|outcome| QueryResult {
                outcome,
                elapsed: start.elapsed(),
                limits,
                notes,
            })
            .map_err(|e| self.mask_statement_error(&saved, e));
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

    /// Whether `statement`, which reads, runs in a read-only transaction on `saved` so that
    /// a write it makes through a function is asked about: changes are asked about, the
    /// user did not allow those of `session` nor this statement already, and the database
    /// refuses writes there.
    fn watches_queries(&self, saved: &SavedConnection, statement: &str, session: &str) -> bool {
        query::refuses_writes_when_read_only(saved.driver)
            && self.confirms_changes()
            && !access::asks_first(statement, saved.driver)
            && !locked(&self.allowed).contains(&(session.to_owned(), saved.id))
    }

    async fn execute(
        &self,
        store: &ConnectionStore,
        saved: &SavedConnection,
        statement: &str,
        read_only: Option<DriverKind>,
        limits: &Limits,
        stop: impl Future<Output = ()>,
    ) -> Result<query::Outcome> {
        let lease = self.lease(store, saved).await?;
        let outcome = query::run_until(lease.conn(), statement, read_only, limits, stop).await;
        lease.finish(outcome, limits.timeout)
    }

    /// The plan of one statement, within the connection's agent access and the limits:
    /// the statement is checked as [`run_query`](Self::run_query) checks it. With
    /// `analyze` it runs (and what it changes is rolled back), so on read-only connections
    /// only in a read-only transaction that refuses writes; without, nothing runs, and
    /// the Safety setting on `UPDATE` or `DELETE` without `WHERE` does not apply.
    pub async fn explain_until(
        &self,
        args: &ExplainArgs,
        stop: impl Future<Output = ()>,
    ) -> Result<PlanResult> {
        let limits = Limits::new(None, args.max_bytes, args.timeout_seconds)?;
        let analyze = args.analyze.unwrap_or(false);
        let (store, saved) = self.find(&args.connection)?;
        self.windows.count(&saved)?;
        let (limits, _) = guard::clamp(limits, &Asked::default(), &saved.agent_limits);
        if matches!(
            saved.driver,
            DriverKind::Mongo
                | DriverKind::Redis
                | DriverKind::Elasticsearch
                | DriverKind::Cassandra
        ) {
            return Err(AgentError::Invalid(format!(
                "explain works on SQL databases: {} has no query plans here",
                saved.driver.name()
            )));
        }
        let access = saved.effective_agent_access();
        let unfiltered = if analyze {
            self.unfiltered_rule(&store)
        } else {
            UnfilteredRule::Allow
        };
        let dialect = Dialect::from(saved.driver);
        let statement = access::check(&args.sql, dialect, access, unfiltered, &saved.name)?;
        let read_only = access == AgentAccess::ReadOnly;
        if analyze && read_only && !query::refuses_writes_when_read_only(saved.driver) {
            return Err(AgentError::Denied(format!(
                "`{}` is read-only for agents, and its database has no read-only transaction \
                 that refuses writes, which analyze needs: explain without analyze",
                saved.name
            )));
        }
        let lease = self.lease(&store, &saved).await?;
        if !lease.conn().capabilities().explain {
            return Err(AgentError::Invalid(format!(
                "`{}` has no query plans in PlatypusDB",
                saved.name
            )));
        }
        let start = Instant::now();
        let guard = (analyze && read_only).then_some(saved.driver);
        let work = lease.conn().explain(statement, analyze);
        let explain = query::guarded(lease.conn(), guard, limits.timeout, stop, work).await;
        let explain = lease.finish(explain, limits.timeout)?;
        Ok(PlanResult {
            explain,
            elapsed: start.elapsed(),
            max_bytes: limits.max_bytes,
        })
    }

    /// The first rows of a table, view or collection, run and recorded as
    /// [`run_query`](Self::run_query) runs a `SELECT *` of it.
    pub async fn sample_rows_until(
        &self,
        args: &SampleArgs,
        origin: &str,
        stop: impl Future<Output = ()>,
    ) -> Result<QueryResult> {
        if args.schema.trim().is_empty() || args.table.trim().is_empty() {
            return Err(AgentError::Invalid(
                "schema and table are both needed".to_owned(),
            ));
        }
        let (_, saved) = self.find(&args.connection)?;
        // A default above the connection's limit is lowered without a word: nobody asked for it.
        let max_rows = args.max_rows.unwrap_or_else(|| {
            saved
                .agent_limits
                .max_rows
                .map_or(SAMPLE_ROWS, |max| max.min(SAMPLE_ROWS))
        });
        let limits = Limits::new(Some(max_rows), args.max_bytes, args.timeout_seconds)?;
        let rows = u64::try_from(limits.max_rows).unwrap_or(u64::MAX);
        let query = QueryArgs {
            connection: saved.id.to_string(),
            // One more row than wanted tells whether the table has more.
            sql: sample_sql(saved.driver, &args.schema, &args.table, rows + 1),
            max_rows: Some(rows),
            max_bytes: args.max_bytes,
            timeout_seconds: args.timeout_seconds,
            format: args.format,
        };
        self.run_query_until(&query, (origin, &self.session), stop)
            .await
    }

    /// Keeps `args.sql` for PlatypusDB to open in an editor tab on the connection, where the
    /// user reviews and runs it: nothing runs here. `origin` is the agent's name.
    pub fn propose_sql(&self, args: &ProposeArgs, origin: &str) -> Result<String> {
        let sql = args.sql.trim();
        if sql.is_empty() {
            return Err(AgentError::Invalid("there is no SQL to propose".to_owned()));
        }
        if sql.len() > MAX_PROPOSAL_BYTES {
            return Err(AgentError::Invalid(format!(
                "the SQL is longer than {MAX_PROPOSAL_BYTES} bytes"
            )));
        }
        let (_, saved) = self.find(&args.connection)?;
        let title: Option<String> = args
            .title
            .as_deref()
            .map(|t| {
                t.trim()
                    .chars()
                    .filter(|c| !c.is_control())
                    .take(TITLE_CHARS)
                    .collect()
            })
            .filter(|t: &String| !t.is_empty());
        let kept = self.with_calls(|store| {
            Ok(store.propose(
                &origin_name(origin),
                &saved.id.to_string(),
                title.as_deref(),
                sql,
            )?)
        })?;
        if kept.is_none() {
            return Err(AgentError::Invalid(format!(
                "{MAX_PENDING_PROPOSALS} proposals of yours (or {MAX_PENDING_ALL} of all agents) \
                 wait for the user to open them in PlatypusDB: no more until they are opened. \
                 Put several statements in one proposal instead"
            )));
        }
        Ok(format!(
            "Proposed on `{}`: PlatypusDB opens it in a new editor tab, now if the app is open \
             or when it starts (within 24 hours). Nothing ran: the user reviews and runs it.",
            saved.name
        ))
    }

    /// Adds a tool call to the agents' call log; the log is best effort.
    pub fn record_call(&self, call: &NewAgentCall) {
        let _ = self.with_calls(|c| c.record(call).map_err(AgentError::from));
    }

    /// Runs `f` on the call log, opened on first use and kept open.
    fn with_calls<T>(&self, f: impl FnOnce(&AgentCallStore) -> Result<T>) -> Result<T> {
        let mut calls = locked(&self.calls);
        if calls.is_none() {
            *calls = Some(self.profile.agent_calls()?);
        }
        let store = calls
            .as_ref()
            .unwrap_or_else(|| unreachable!("opened above"));
        f(store)
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
            Err(AgentError::Timeout(_) | AgentError::Cancelled) => {
                (None, HistoryStatus::Cancelled, None)
            }
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

/// What [`read_er`] found.
enum ErRead {
    NoSchema,
    /// A requested table that is not one; `other` when the schema has an object of that name.
    Unknown {
        name: String,
        other: bool,
    },
    /// The doc model of the tables to draw, of `total` that qualified.
    Found {
        doc: DocSchema,
        total: usize,
    },
}

/// The details of the tables `names` that `into` does not hold yet, four at a time.
async fn load_details(
    conn: &dyn Connection,
    schema: &str,
    names: &[String],
    into: &mut HashMap<String, TableDetails>,
) -> Result<(), Error> {
    use futures::StreamExt as _;
    let todo: Vec<&String> = names.iter().filter(|n| !into.contains_key(*n)).collect();
    let reads: Vec<_> = todo.iter().map(|n| conn.table_details(schema, n)).collect();
    let mut stream = futures::stream::iter(reads)
        .buffered(4)
        .zip(futures::stream::iter(todo));
    while let Some((read, name)) = stream.next().await {
        into.insert(name.clone(), read?);
    }
    Ok(())
}

/// The tables of `schema` to draw: `requested` and those `depth` foreign-key hops away, or
/// (none requested) the first [`er::MAX_TABLES`] of the schema.
async fn read_er(
    conn: &dyn Connection,
    schema: &str,
    requested: &[String],
    depth: usize,
) -> Result<ErRead, Error> {
    let objects = conn.objects(schema).await?;
    if objects.is_empty() && !conn.schemas().await?.iter().any(|s| s.name == schema) {
        return Ok(ErRead::NoSchema);
    }
    let names: Vec<String> = objects
        .iter()
        .filter(|o| o.kind == ObjectKind::Table)
        .map(|o| o.name.clone())
        .collect();
    let known: HashSet<&str> = names.iter().map(String::as_str).collect();
    // As the catalog writes them: the exact name, else one that differs only in case.
    let mut chosen = Vec::with_capacity(requested.len());
    for name in requested {
        let found = names
            .iter()
            .find(|n| *n == name)
            .or_else(|| names.iter().find(|n| n.eq_ignore_ascii_case(name)));
        let Some(found) = found else {
            let other = objects.iter().any(|o| o.name.eq_ignore_ascii_case(name));
            return Ok(ErRead::Unknown {
                name: name.clone(),
                other,
            });
        };
        chosen.push(found.clone());
    }
    let requested = chosen;
    let mut details = HashMap::new();
    let (order, total) = if requested.is_empty() {
        let first = names.iter().take(er::MAX_TABLES).cloned().collect();
        (first, names.len())
    } else {
        let mut walk = er::Walk::new(&requested);
        for _ in 0..depth {
            load_details(conn, schema, walk.frontier(), &mut details).await?;
            let edges = walk
                .frontier()
                .iter()
                .filter_map(|t| details.get(t).map(|d| (t, d)))
                .map(|(t, d)| (t.clone(), er::neighbors(schema, d, &known)))
                .collect();
            if !walk.advance(&edges) {
                break;
            }
        }
        (walk.order().to_vec(), walk.order().len())
    };
    load_details(conn, schema, &order, &mut details).await?;
    Ok(ErRead::Found {
        doc: er::doc_schema(schema, &order, &details),
        total,
    })
}

/// The matches of the catalog's tags, notes and comments, and (unless the text is a `#tag`
/// query) those of the database's own search. Listing every schema must not cost the database's
/// own matches: it has [`CATALOG_SEARCH`], and `false` says it ran out.
async fn search_all(
    conn: &dyn Connection,
    text: &str,
    view: &crate::catalog_view::CatalogView,
    query: &crate::catalog_view::SearchQuery,
    redactor: &Redactor,
) -> Result<(Vec<Value>, bool), Error> {
    let mut found = if query.tags.is_empty() {
        search(conn, text).await?
    } else {
        Vec::new()
    };
    let catalog = crate::catalog_view::search(conn, view, query, redactor);
    let catalog_read = match tokio::time::timeout(CATALOG_SEARCH, catalog).await {
        Ok(matches) => {
            found.extend(matches?);
            true
        }
        Err(_) => false,
    };
    Ok((found, catalog_read))
}

/// How long `search_objects` looks through the catalog after the database's own search.
const CATALOG_SEARCH: std::time::Duration = std::time::Duration::from_secs(10);

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

    #[test]
    fn objects_come_a_page_at_a_time() {
        let objects: Vec<ObjectMeta> = (0..1200)
            .map(|i| ObjectMeta::new(format!("t{i}"), ObjectKind::Table, None))
            .collect();
        let small = object_page(&objects[..3], None, None).unwrap();
        assert_eq!(small.as_array().map(Vec::len), Some(3));
        let first = object_page(&objects, None, None).unwrap();
        assert_eq!(first["objects"].as_array().unwrap().len(), OBJECT_PAGE);
        assert_eq!(first["truncated"], true);
        assert_eq!(first["total"], 1200);
        assert_eq!(first["next_offset"], 500);
        let last = object_page(&objects, Some(1000), Some(5000)).unwrap();
        assert_eq!(last["objects"].as_array().unwrap().len(), 200);
        assert_eq!(last["objects"][0]["name"], "t1000");
        assert_eq!(last["truncated"], false);
        assert_eq!(last["next_offset"], Value::Null);
        assert!(object_page(&objects, None, Some(0)).is_err());
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

    /// An app that answers every change with `verdict` and keeps what it was asked.
    #[derive(Debug)]
    struct FakeApp {
        verdict: Verdict,
        asked: Mutex<Vec<WriteRequest>>,
    }

    #[async_trait::async_trait]
    impl AppLink for FakeApp {
        async fn sessions(&self) -> std::result::Result<Vec<crate::OpenSession>, String> {
            Ok(Vec::new())
        }
        async fn confirm(&self, request: WriteRequest) -> std::result::Result<Verdict, String> {
            locked(&self.asked).push(request);
            Ok(self.verdict.clone())
        }
        async fn hello(&self, _: &str, _: &str) -> std::result::Result<(), String> {
            Ok(())
        }
    }

    fn change(sql: &str) -> QueryArgs {
        QueryArgs {
            connection: "Db".to_owned(),
            sql: sql.to_owned(),
            max_rows: None,
            max_bytes: None,
            timeout_seconds: None,
            format: None,
        }
    }

    #[tokio::test]
    async fn changes_wait_for_the_user_unless_settings_say_otherwise() {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("db.sqlite");
        sqlite(&db, 3);
        save_connection(dir.path(), "sqlite", &db);
        let toml = std::fs::read_to_string(dir.path().join("connections.toml")).unwrap();
        std::fs::write(
            dir.path().join("connections.toml"),
            toml + "agent_access = \"read_write\"\n",
        )
        .unwrap();
        let delete = change("DELETE FROM t WHERE id > 0");

        // No app to ask: refused, and nothing changed.
        let alone = Agent::new(Profile::at(dir.path()));
        let refused = alone.run_query(&delete, "test").await.unwrap_err();
        assert!(refused.to_string().contains("cannot ask"), "{refused}");
        assert_eq!(count(&alone).await, "3");

        // Allowed once: it runs, and the next change asks again.
        let app = Arc::new(FakeApp {
            verdict: Verdict::Once,
            asked: Mutex::default(),
        });
        let agent = Agent::new(Profile::at(dir.path())).with_link(app.clone());
        agent.run_query(&delete, "claude").await.unwrap();
        assert_eq!(count(&agent).await, "1", "queries are not asked");
        agent
            .run_query(&change("UPDATE t SET id = 5 WHERE id = 0"), "claude")
            .await
            .unwrap();
        {
            let asked = locked(&app.asked);
            assert_eq!(asked.len(), 2);
            assert_eq!(asked[0].agent, "claude");
            assert_eq!(asked[0].connection, "Db");
            assert_eq!(asked[0].statement, "DELETE FROM t WHERE id > 0");
            assert!(asked[0].plan.is_some());
        }

        // Turned off in the settings: changes run without asking.
        std::fs::write(
            dir.path().join("settings.toml"),
            "[agents]\nconfirm_writes = false\n",
        )
        .unwrap();
        alone
            .run_query(&change("DELETE FROM t"), "test")
            .await
            .unwrap_err();
        alone
            .run_query(&change("DELETE FROM t WHERE id = 5"), "test")
            .await
            .unwrap();
        assert_eq!(count(&alone).await, "0");
    }

    /// On a read-write connection a query that acts outside the transaction is asked about
    /// like a change: no read-only transaction would stop it.
    #[tokio::test]
    async fn queries_that_act_outside_are_asked_about() {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("db.sqlite");
        sqlite(&db, 1);
        save_connection(dir.path(), "sqlite", &db);
        let toml = std::fs::read_to_string(dir.path().join("connections.toml")).unwrap();
        std::fs::write(
            dir.path().join("connections.toml"),
            toml + "agent_access = \"read_write\"\n",
        )
        .unwrap();
        let outside = change("SELECT load_extension('x')");
        let alone = Agent::new(Profile::at(dir.path()));
        let refused = alone.run_query(&outside, "test").await.unwrap_err();
        assert!(refused.to_string().contains("cannot ask"), "{refused}");

        let app = Arc::new(FakeApp {
            verdict: Verdict::Deny { reason: None },
            asked: Mutex::default(),
        });
        let agent = Agent::new(Profile::at(dir.path())).with_link(app.clone());
        let refused = agent.run_query(&outside, "claude").await.unwrap_err();
        assert_eq!(refused.to_string(), "the user refused this change");
        assert_eq!(count(&agent).await, "1");
        let asked = locked(&app.asked);
        assert_eq!(asked.len(), 1, "a plain query is not asked about");
        assert_eq!(asked[0].statement, "SELECT load_extension('x')");
    }

    /// An agent that edits the profile's files to give itself more gets nothing until the
    /// user accepts the edit in the app, which seals it.
    #[tokio::test]
    async fn an_outside_edit_cannot_give_agents_more_than_approved() {
        use platypus_store::{MemorySecrets, SecretStore};

        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("db.sqlite");
        sqlite(&db, 2);
        save_connection(dir.path(), "sqlite", &db);
        let profile = Profile::at(dir.path());
        let secrets: Arc<dyn SecretStore> = Arc::new(MemorySecrets::default());
        let seal = Seal::with(dir.path(), secrets);
        let approve = || {
            let store = profile.connections().unwrap();
            let settings = profile.settings();
            seal.write(&AgentPolicy::current(store.list(), settings.as_ref()))
                .unwrap();
        };
        approve();
        let toml = std::fs::read_to_string(dir.path().join("connections.toml")).unwrap();
        std::fs::write(
            dir.path().join("connections.toml"),
            toml + "agent_access = \"read_write\"\n",
        )
        .unwrap();
        std::fs::write(
            dir.path().join("settings.toml"),
            "[agents]\nconfirm_writes = false\n",
        )
        .unwrap();
        let agent = Agent::new(Profile::at(dir.path())).with_seal(seal.clone());
        let delete = change("DELETE FROM t WHERE id = 1");
        let refused = agent.run_query(&delete, "test").await.unwrap_err();
        assert!(refused.to_string().contains("read-only"), "{refused}");
        assert_eq!(
            agent.list_connections().unwrap()[0].agent_access,
            AgentAccess::ReadOnly
        );
        assert_eq!(count(&agent).await, "2");

        // Accepted in the app: the edit applies, writes without asking included.
        approve();
        agent.run_query(&delete, "test").await.unwrap();
        assert_eq!(count(&agent).await, "1");
    }

    #[tokio::test]
    async fn a_refusal_carries_the_users_reason() {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("db.sqlite");
        sqlite(&db, 2);
        save_connection(dir.path(), "sqlite", &db);
        let toml = std::fs::read_to_string(dir.path().join("connections.toml")).unwrap();
        std::fs::write(
            dir.path().join("connections.toml"),
            toml + "agent_access = \"read_write\"\n",
        )
        .unwrap();
        let app = Arc::new(FakeApp {
            verdict: Verdict::Deny {
                reason: Some(" wrong table ".to_owned()),
            },
            asked: Mutex::default(),
        });
        let agent = Agent::new(Profile::at(dir.path())).with_link(app);
        let refused = agent
            .run_query(&change("DELETE FROM t WHERE id = 1"), "claude")
            .await
            .unwrap_err();
        assert_eq!(
            refused.to_string(),
            "the user refused this change: wrong table"
        );
        assert_eq!(count(&agent).await, "2");
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
        platypus_driver_duckdb::testing::load();
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
