//! The calls external agents made to the MCP server's tools, every tool and not only the
//! queries: who called which tool, on which connection, with which arguments, how long it
//! took and how it ended. Each start of the server is a session. Beside them, the SQL agents
//! proposed with `propose_sql`, which the app opens in an editor tab.

use std::fmt::Write as _;
use std::path::Path;
use std::time::{Duration, SystemTime};

use platypus_sql::security::{hide_secrets, mask_secrets, secrets_of};
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};

use crate::history::{like_pattern, millis, open_log};
use crate::{HistoryStatus, Result, StoreError};

/// Schema migrations, applied in order and tracked with `PRAGMA user_version`.
const MIGRATIONS: &[&str] = &[
    "CREATE TABLE calls (
        id          INTEGER PRIMARY KEY,
        session     TEXT    NOT NULL,
        agent       TEXT    NOT NULL,
        tool        TEXT    NOT NULL,
        connection  TEXT,
        arguments   TEXT    NOT NULL, -- JSON
        started_at  INTEGER NOT NULL, -- unix epoch milliseconds
        duration_ms INTEGER NOT NULL,
        status      TEXT    NOT NULL,
        error       TEXT
    );
    CREATE INDEX calls_session ON calls (session, id);",
    "CREATE TABLE proposals (
        id            INTEGER PRIMARY KEY,
        agent         TEXT    NOT NULL,
        connection_id TEXT    NOT NULL,
        title         TEXT,
        sql           TEXT    NOT NULL,
        created_at    INTEGER NOT NULL, -- unix epoch milliseconds
        opened        INTEGER NOT NULL DEFAULT 0
    );",
    // Rows to change proposed with `propose_edits`, as JSON; `sql` is empty for such a row.
    "ALTER TABLE proposals ADD COLUMN edits TEXT;",
    // What the report counts. Calls recorded before this stay NULL: the report shows `–` for
    // them, never a made-up 0.
    "ALTER TABLE calls ADD COLUMN rows_returned INTEGER;
     ALTER TABLE calls ADD COLUMN tables_read TEXT; -- JSON array of names
     ALTER TABLE calls ADD COLUMN effect TEXT;      -- none, allowed or refused",
];

/// Proposals older than this are not opened any more: the agent's session is long over.
pub const PROPOSAL_LIFETIME: Duration = Duration::from_hours(24);
/// Proposals of one agent that may wait for the app to open them.
pub const MAX_PENDING_PROPOSALS: u32 = 20;
/// Proposals of all agents that may wait for the app to open them.
pub const MAX_PENDING_ALL: u32 = 100;

/// SQL or rows to change an agent proposed, for the user to review in the editor or the data
/// viewer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AgentProposal {
    pub id: i64,
    pub agent: String,
    pub connection_id: String,
    pub title: Option<String>,
    /// Empty for a proposal of rows (`edits`).
    pub sql: String,
    /// The rows to change, as JSON (`platypus_sql::proposed::Edits`): the file is open to any
    /// program, so the app checks them again before it uses them.
    pub edits: Option<String>,
    /// Unix epoch milliseconds.
    pub created_at: i64,
}

/// Calls kept; older ones are removed when the app starts.
pub const AGENT_CALLS_LIMIT: usize = 20_000;

/// Characters of the arguments kept: a long SQL text is cut.
const ARGUMENT_CHARS: usize = 16 * 1024;

/// Characters of an error message kept.
const ERROR_CHARS: usize = 4 * 1024;

/// One call to a tool, as the app shows it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AgentCall {
    pub id: i64,
    /// The server's run the call belongs to.
    pub session: String,
    /// The client's name, e.g. `claude-code`.
    pub agent: String,
    pub tool: String,
    /// The connection the call named, as the agent wrote it.
    pub connection: Option<String>,
    /// The call's arguments, as JSON text.
    pub arguments: String,
    /// Unix epoch milliseconds.
    pub started_at: i64,
    pub duration_ms: u64,
    pub status: HistoryStatus,
    pub error: Option<String>,
}

/// Whether a call changed data, or was refused when it tried to.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum CallEffect {
    /// Not a change (or a change that failed for another reason).
    #[default]
    None,
    /// A change that ran.
    Allowed,
    /// A change the access or the safety settings refused.
    Refused,
}

impl CallEffect {
    const fn as_str(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Allowed => "allowed",
            Self::Refused => "refused",
        }
    }
}

/// What the report counts about a call: names and numbers only, never a value or a
/// statement.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CallFacts {
    /// Rows the call returned to the agent.
    pub rows: u64,
    /// Tables whose data the call read, as the tool knows them; for a statement, the tables
    /// found after `FROM` and `JOIN`, an estimate.
    pub tables: Vec<String>,
    pub effect: CallEffect,
}

#[derive(Debug, Clone)]
pub struct NewAgentCall {
    pub session: String,
    pub agent: String,
    pub tool: String,
    pub connection: Option<String>,
    pub arguments: String,
    pub started_at: SystemTime,
    pub duration: Duration,
    pub status: HistoryStatus,
    pub error: Option<String>,
    pub facts: CallFacts,
}

/// Which calls a search returns, newest first.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct AgentCallQuery {
    /// Case-insensitive substring of the tool, the agent, the connection or the arguments.
    pub text: Option<String>,
    pub session: Option<String>,
    pub agent: Option<String>,
    pub status: Option<HistoryStatus>,
    /// Page backwards from this call (exclusive).
    pub before_id: Option<i64>,
    #[serde(default)]
    pub limit: usize,
}

/// A run of the server: its calls, from the first to the last.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AgentSession {
    pub session: String,
    pub agent: String,
    /// Unix epoch milliseconds of the first and the last call.
    pub first_at: i64,
    pub last_at: i64,
    pub calls: u64,
    pub errors: u64,
}

/// The figures of a group of calls. A figure is `None` when none of the calls was recorded
/// with it (a log from before the report): never a made-up 0.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ReportFigures {
    /// Unix epoch milliseconds of the first and the last call.
    pub first_at: i64,
    pub last_at: i64,
    pub calls: u64,
    /// Calls recorded with the figures below; fewer than `calls` when the log is older.
    pub measured: u64,
    /// Different tables (on different connections) read.
    pub tables_read: Option<u64>,
    pub rows_returned: Option<u64>,
    pub changes_allowed: Option<u64>,
    pub changes_refused: Option<u64>,
    pub errors: u64,
}

/// One run of an agent's server in the report.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SessionReport {
    pub session: String,
    #[serde(flatten)]
    pub figures: ReportFigures,
}

/// One agent in the report, with its newest sessions.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AgentReport {
    pub agent: String,
    #[serde(flatten)]
    pub figures: ReportFigures,
    pub sessions: Vec<SessionReport>,
}

/// Sessions the report lists, newest first.
const REPORT_SESSIONS: i64 = 1000;

/// The calls a query selects; shared by the list, the export and the report. `?5` pages
/// backwards and is `NULL` in the report.
const CALL_FILTER: &str = r"(?1 IS NULL OR tool LIKE ?1 ESCAPE '\' OR agent LIKE ?1 ESCAPE '\'
                        OR connection LIKE ?1 ESCAPE '\' OR arguments LIKE ?1 ESCAPE '\'
                        OR error LIKE ?1 ESCAPE '\')
                   AND (?2 IS NULL OR session = ?2)
                   AND (?3 IS NULL OR agent = ?3)
                   AND (?4 IS NULL OR status = ?4)
                   AND (?5 IS NULL OR id < ?5)";

/// The agents' calls, in a local SQLite database shared by the server and the app.
#[derive(Debug)]
pub struct AgentCallStore {
    db: Connection,
}

#[expect(
    clippy::needless_pass_by_value,
    reason = "used directly as a `map_err` callback"
)]
fn db_error(e: rusqlite::Error) -> StoreError {
    StoreError::History(e.to_string())
}

/// `text` up to `max` characters, with `…` when it was longer.
fn capped(text: &str, max: usize) -> String {
    match text.char_indices().nth(max) {
        Some((end, _)) => format!("{}…", &text[..end]),
        None => text.to_owned(),
    }
}

/// The JSON `arguments` of a call with the passwords of statements that set credentials
/// masked in each of its strings ([`mask_secrets`]), and the secrets found, to hide from the
/// error. Arguments that are not JSON are masked as one text.
fn masked_arguments(arguments: &str) -> (String, Vec<String>) {
    fn walk(value: &mut serde_json::Value, secrets: &mut Vec<String>) {
        match value {
            serde_json::Value::String(s) => {
                let found = secrets_of(s);
                if !found.is_empty() {
                    *s = mask_secrets(s).into_owned();
                    secrets.extend(found);
                }
            }
            serde_json::Value::Array(items) => items.iter_mut().for_each(|v| walk(v, secrets)),
            serde_json::Value::Object(map) => map.values_mut().for_each(|v| walk(v, secrets)),
            _ => {}
        }
    }
    match serde_json::from_str::<serde_json::Value>(arguments) {
        Ok(mut value) => {
            let mut secrets = Vec::new();
            walk(&mut value, &mut secrets);
            if secrets.is_empty() {
                return (arguments.to_owned(), secrets);
            }
            (value.to_string(), secrets)
        }
        Err(_) => (mask_secrets(arguments).into_owned(), secrets_of(arguments)),
    }
}

/// Table names kept per call, and characters of each.
const TABLES_PER_CALL: usize = 50;
const TABLE_NAME_CHARS: usize = 200;

/// The tables of a call as the JSON array the report reads back with `json_each`: each
/// name cut, the list capped, without repeats.
fn tables_json(tables: &[String]) -> String {
    let mut names: Vec<String> = Vec::new();
    for name in tables {
        let name = capped(name, TABLE_NAME_CHARS);
        if names.len() < TABLES_PER_CALL && !names.contains(&name) {
            names.push(name);
        }
    }
    serde_json::to_string(&names).unwrap_or_else(|_| "[]".to_owned())
}

/// The values of [`CALL_FILTER`]'s `?1` to `?5` for `query`, then `limit` as `?6` if given.
fn filter_params(
    query: &AgentCallQuery,
    before_id: Option<i64>,
    limit: Option<i64>,
) -> rusqlite::ParamsFromIter<Vec<rusqlite::types::Value>> {
    use rusqlite::types::Value;
    let text = |s: Option<&str>| s.map_or(Value::Null, |s| Value::Text(s.to_owned()));
    let pattern = query
        .text
        .as_deref()
        .map(str::trim)
        .filter(|t| !t.is_empty())
        .map(like_pattern);
    let mut values = vec![
        text(pattern.as_deref()),
        text(query.session.as_deref()),
        text(query.agent.as_deref()),
        text(query.status.map(HistoryStatus::as_str)),
        before_id.map_or(Value::Null, Value::Integer),
    ];
    values.extend(limit.map(Value::Integer));
    rusqlite::params_from_iter(values)
}

impl AgentCallStore {
    pub fn open(path: &Path) -> Result<Self> {
        let db = open_log(path)?;
        // The server writes while the app reads.
        db.busy_timeout(Duration::from_secs(2)).map_err(db_error)?;
        Self::init(db)
    }

    pub fn open_in_memory() -> Result<Self> {
        Self::init(Connection::open_in_memory().map_err(db_error)?)
    }

    fn init(db: Connection) -> Result<Self> {
        crate::sqlite::migrate(&db, MIGRATIONS, "agent calls", StoreError::History)?;
        Ok(Self { db })
    }

    /// Adds `call`, the passwords of the statements in its arguments masked, there and in its
    /// error: the log is shown in the app and exported.
    pub fn record(&self, call: &NewAgentCall) -> Result<i64> {
        let (arguments, secrets) = masked_arguments(&call.arguments);
        let error = call.error.as_deref().map(|e| hide_secrets(e, &secrets));
        self.db
            .execute(
                "INSERT INTO calls (session, agent, tool, connection, arguments, started_at,
                                    duration_ms, status, error, rows_returned, tables_read, effect)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
                params![
                    call.session,
                    call.agent,
                    call.tool,
                    call.connection,
                    capped(&arguments, ARGUMENT_CHARS),
                    millis(call.started_at),
                    i64::try_from(call.duration.as_millis()).unwrap_or(i64::MAX),
                    call.status.as_str(),
                    error.as_deref().map(|e| capped(e, ERROR_CHARS)),
                    i64::try_from(call.facts.rows).unwrap_or(i64::MAX),
                    tables_json(&call.facts.tables),
                    call.facts.effect.as_str(),
                ],
            )
            .map_err(db_error)?;
        Ok(self.db.last_insert_rowid())
    }

    /// The summary of the calls `query` selects (its paging is ignored), per agent and per
    /// session, aggregated by SQLite: nothing runs until the Activity page asks.
    pub fn report(&self, query: &AgentCallQuery) -> Result<Vec<AgentReport>> {
        let mut report: Vec<AgentReport> = self
            .groups(query, false)?
            .into_iter()
            .map(|(agent, _, figures)| AgentReport {
                agent,
                figures,
                sessions: Vec::new(),
            })
            .collect();
        for (agent, session, figures) in self.groups(query, true)? {
            if let Some(a) = report.iter_mut().find(|a| a.agent == agent) {
                a.sessions.push(SessionReport { session, figures });
            }
        }
        Ok(report)
    }

    /// The figures of the calls `query` selects, per agent, or per agent and session (newest
    /// first, at most [`REPORT_SESSIONS`]): one pass over the calls, and one over the table
    /// names of those that read any.
    fn groups(
        &self,
        query: &AgentCallQuery,
        by_session: bool,
    ) -> Result<Vec<(String, String, ReportFigures)>> {
        let (shown, group) = if by_session {
            ("agent, session", "agent, session")
        } else {
            ("agent, ''", "agent")
        };
        let mut stmt = self
            .db
            .prepare_cached(&format!(
                "SELECT {shown}, min(started_at), max(started_at), count(*), count(effect),
                        sum(rows_returned), sum(effect = 'allowed'), sum(effect = 'refused'),
                        sum(status = 'error')
                 FROM calls
                 WHERE {CALL_FILTER}
                 GROUP BY {group}
                 ORDER BY max(id) DESC
                 LIMIT {REPORT_SESSIONS}"
            ))
            .map_err(db_error)?;
        let mut groups: Vec<(String, String, ReportFigures)> = stmt
            .query_map(filter_params(query, None, None), |r| {
                let count = |i| r.get::<_, i64>(i).map(|n| u64::try_from(n).unwrap_or(0));
                let measured = count(5)?;
                // Sums of calls recorded without the figures are NULL: `–`, not 0.
                let figure = |i| {
                    Ok::<_, rusqlite::Error>(
                        (measured > 0)
                            .then(|| r.get::<_, Option<i64>>(i))
                            .transpose()?
                            .map(|n| u64::try_from(n.unwrap_or(0)).unwrap_or(0)),
                    )
                };
                Ok((
                    r.get(0)?,
                    r.get(1)?,
                    ReportFigures {
                        first_at: r.get(2)?,
                        last_at: r.get(3)?,
                        calls: count(4)?,
                        measured,
                        tables_read: None,
                        rows_returned: figure(6)?,
                        changes_allowed: figure(7)?,
                        changes_refused: figure(8)?,
                        errors: count(9).unwrap_or(0),
                    },
                ))
            })
            .map_err(db_error)?
            .collect::<Result<_, _>>()
            .map_err(db_error)?;
        let mut stmt = self
            .db
            .prepare_cached(&format!(
                "SELECT {shown}, count(DISTINCT coalesce(connection, '') || char(31) || value)
                 FROM (SELECT agent, session, connection, tables_read FROM calls
                       WHERE {CALL_FILTER} AND tables_read IS NOT NULL AND tables_read <> '[]'),
                      json_each(tables_read)
                 GROUP BY {group}"
            ))
            .map_err(db_error)?;
        let tables = stmt
            .query_map(filter_params(query, None, None), |r| {
                Ok((
                    (r.get::<_, String>(0)?, r.get::<_, String>(1)?),
                    r.get::<_, i64>(2)?,
                ))
            })
            .map_err(db_error)?
            .collect::<Result<std::collections::HashMap<_, _>, _>>()
            .map_err(db_error)?;
        for (agent, session, figures) in &mut groups {
            if figures.measured > 0 {
                let n = tables.get(&(agent.clone(), session.clone())).copied();
                figures.tables_read = Some(n.map_or(0, |n| u64::try_from(n).unwrap_or(0)));
            }
        }
        Ok(groups)
    }

    pub fn search(&self, query: &AgentCallQuery) -> Result<Vec<AgentCall>> {
        let mut stmt = self
            .db
            .prepare_cached(&format!(
                "SELECT id, session, agent, tool, connection, arguments, started_at, duration_ms,
                        status, error
                 FROM calls
                 WHERE {CALL_FILTER}
                 ORDER BY id DESC
                 LIMIT ?6"
            ))
            .map_err(db_error)?;
        let rows = stmt
            .query_map(
                filter_params(
                    query,
                    query.before_id,
                    Some(i64::try_from(query.limit).unwrap_or(i64::MAX)),
                ),
                |r| {
                    let status: String = r.get(8)?;
                    Ok(AgentCall {
                        id: r.get(0)?,
                        session: r.get(1)?,
                        agent: r.get(2)?,
                        tool: r.get(3)?,
                        connection: r.get(4)?,
                        arguments: r.get(5)?,
                        started_at: r.get(6)?,
                        duration_ms: r.get::<_, i64>(7)?.try_into().unwrap_or(0),
                        status: HistoryStatus::parse(&status),
                        error: r.get(9)?,
                    })
                },
            )
            .map_err(db_error)?;
        rows.collect::<Result<_, _>>().map_err(db_error)
    }

    /// The latest `limit` sessions, newest first.
    pub fn sessions(&self, limit: usize) -> Result<Vec<AgentSession>> {
        let mut stmt = self
            .db
            .prepare_cached(
                "SELECT session, max(agent), min(started_at), max(started_at), count(*),
                        sum(status = 'error')
                 FROM calls
                 GROUP BY session
                 ORDER BY max(id) DESC
                 LIMIT ?1",
            )
            .map_err(db_error)?;
        let rows = stmt
            .query_map([i64::try_from(limit).unwrap_or(i64::MAX)], |r| {
                Ok(AgentSession {
                    session: r.get(0)?,
                    agent: r.get(1)?,
                    first_at: r.get(2)?,
                    last_at: r.get(3)?,
                    calls: r.get::<_, i64>(4)?.try_into().unwrap_or(0),
                    errors: r.get::<_, i64>(5)?.try_into().unwrap_or(0),
                })
            })
            .map_err(db_error)?;
        rows.collect::<Result<_, _>>().map_err(db_error)
    }

    /// Keeps SQL `agent` proposes on a connection, for the app to open; returns its id, or
    /// `None` when [`MAX_PENDING_PROPOSALS`] of `agent`'s (or [`MAX_PENDING_ALL`] of all
    /// agents') wait already: an agent cannot fill the profile, nor the app's tab bar.
    pub fn propose(
        &self,
        agent: &str,
        connection_id: &str,
        title: Option<&str>,
        sql: &str,
    ) -> Result<Option<i64>> {
        self.keep(agent, connection_id, title, sql, None)
    }

    /// Keeps rows `agent` proposes to change (`edits`, JSON), under the same limits as
    /// [`propose`](Self::propose).
    pub fn propose_edits(
        &self,
        agent: &str,
        connection_id: &str,
        title: Option<&str>,
        edits: &str,
    ) -> Result<Option<i64>> {
        self.keep(agent, connection_id, title, "", Some(edits))
    }

    fn keep(
        &self,
        agent: &str,
        connection_id: &str,
        title: Option<&str>,
        sql: &str,
        edits: Option<&str>,
    ) -> Result<Option<i64>> {
        let now = SystemTime::now();
        let added = self
            .db
            .execute(
                "INSERT INTO proposals (agent, connection_id, title, sql, edits, created_at)
                 SELECT ?1, ?2, ?3, ?4, ?9, ?5
                 WHERE (SELECT count(*) FROM proposals
                        WHERE agent = ?1 AND opened = 0 AND created_at >= ?6) < ?7
                   AND (SELECT count(*) FROM proposals
                        WHERE opened = 0 AND created_at >= ?6) < ?8",
                params![
                    agent,
                    connection_id,
                    title,
                    sql,
                    millis(now),
                    millis(now - PROPOSAL_LIFETIME),
                    MAX_PENDING_PROPOSALS,
                    MAX_PENDING_ALL,
                    edits,
                ],
            )
            .map_err(db_error)?;
        Ok((added == 1).then(|| self.db.last_insert_rowid()))
    }

    /// The proposals younger than [`PROPOSAL_LIFETIME`], oldest first, removed: each is
    /// handed out once. Older ones are removed unread.
    pub fn take_proposals(&self) -> Result<Vec<AgentProposal>> {
        let since = millis(SystemTime::now() - PROPOSAL_LIFETIME);
        let tx = self.db.unchecked_transaction().map_err(db_error)?;
        let mut proposals = {
            let mut stmt = tx
                .prepare_cached(
                    "DELETE FROM proposals
                     WHERE opened = 0 AND created_at >= ?1
                     RETURNING id, agent, connection_id, title, sql, edits, created_at",
                )
                .map_err(db_error)?;
            let rows = stmt
                .query_map([since], |r| {
                    Ok(AgentProposal {
                        id: r.get(0)?,
                        agent: r.get(1)?,
                        connection_id: r.get(2)?,
                        title: r.get(3)?,
                        sql: r.get(4)?,
                        edits: r.get(5)?,
                        created_at: r.get(6)?,
                    })
                })
                .map_err(db_error)?;
            rows.collect::<Result<Vec<_>, _>>().map_err(db_error)?
        };
        tx.execute("DELETE FROM proposals", []).map_err(db_error)?;
        tx.commit().map_err(db_error)?;
        proposals.sort_by_key(|p| p.id);
        Ok(proposals)
    }

    /// Removes every call, and the proposals not opened yet.
    pub fn clear(&self) -> Result<()> {
        self.db
            .execute_batch("DELETE FROM calls; DELETE FROM proposals;")
            .map_err(db_error)?;
        Ok(())
    }

    /// Keeps the newest `keep` calls; returns how many were removed.
    pub fn prune(&self, keep: usize) -> Result<usize> {
        let cutoff: Option<i64> = self
            .db
            .query_row(
                "SELECT id FROM calls ORDER BY id DESC LIMIT 1 OFFSET ?1",
                [i64::try_from(keep).unwrap_or(i64::MAX)],
                |r| r.get(0),
            )
            .optional()
            .map_err(db_error)?;
        match cutoff {
            Some(id) => self
                .db
                .execute("DELETE FROM calls WHERE id <= ?1", [id])
                .map_err(db_error),
            None => Ok(0),
        }
    }
}

/// A CSV field: quoted when it holds a separator, a quote or a line break.
fn csv_field(value: &str) -> String {
    if value.contains([',', '"', '\n', '\r']) {
        format!("\"{}\"", value.replace('"', "\"\""))
    } else {
        value.to_owned()
    }
}

/// The calls as CSV (RFC 4180, with a header): the start in UTC (RFC 3339), then every
/// field of [`AgentCall`].
#[must_use]
pub fn agent_calls_csv(calls: &[AgentCall]) -> String {
    let mut out = String::from(
        "started_at,session,agent,tool,connection,arguments,duration_ms,status,error\r\n",
    );
    for c in calls {
        let fields = [
            rfc3339(c.started_at),
            c.session.clone(),
            c.agent.clone(),
            c.tool.clone(),
            c.connection.clone().unwrap_or_default(),
            c.arguments.clone(),
            c.duration_ms.to_string(),
            c.status.as_str().to_owned(),
            c.error.clone().unwrap_or_default(),
        ];
        let line: Vec<String> = fields.iter().map(|f| csv_field(f)).collect();
        let _ = write!(out, "{}\r\n", line.join(","));
    }
    out
}

/// Unix epoch milliseconds as `YYYY-MM-DDTHH:MM:SS.mmmZ`.
#[must_use]
pub fn rfc3339(millis: i64) -> String {
    let secs = millis.div_euclid(1000);
    let ms = millis.rem_euclid(1000);
    let days = secs.div_euclid(86_400);
    let day_secs = secs.rem_euclid(86_400);
    // Howard Hinnant's days-to-civil.
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}.{ms:03}Z",
        day_secs / 3600,
        day_secs % 3600 / 60,
        day_secs % 60
    )
}

#[cfg(test)]
mod tests {
    use std::time::UNIX_EPOCH;

    use super::*;

    fn call(session: &str, tool: &str, status: HistoryStatus) -> NewAgentCall {
        NewAgentCall {
            session: session.to_owned(),
            agent: "claude-code".to_owned(),
            tool: tool.to_owned(),
            connection: Some("App db".to_owned()),
            arguments: r#"{"connection":"App db","sql":"SELECT 1"}"#.to_owned(),
            started_at: UNIX_EPOCH + Duration::from_millis(1_700_000_000_123),
            duration: Duration::from_millis(12),
            status,
            error: (status == HistoryStatus::Error).then(|| "no such table: t".to_owned()),
            facts: CallFacts::default(),
        }
    }

    #[test]
    fn records_filters_and_groups_calls() {
        let s = AgentCallStore::open_in_memory().unwrap();
        s.record(&call("one", "list_connections", HistoryStatus::Ok))
            .unwrap();
        s.record(&call("one", "run_query", HistoryStatus::Error))
            .unwrap();
        s.record(&NewAgentCall {
            agent: "codex".to_owned(),
            ..call("two", "explain", HistoryStatus::Ok)
        })
        .unwrap();
        let search = |query: AgentCallQuery| {
            s.search(&AgentCallQuery { limit: 10, ..query })
                .unwrap()
                .into_iter()
                .map(|c| c.tool)
                .collect::<Vec<_>>()
        };
        assert_eq!(
            search(AgentCallQuery::default()),
            ["explain", "run_query", "list_connections"]
        );
        let text = |t: &str| AgentCallQuery {
            text: Some(t.to_owned()),
            ..Default::default()
        };
        assert_eq!(search(text("RUN_")), ["run_query"]);
        assert_eq!(search(text("no such")), ["run_query"]);
        assert_eq!(search(text("select 1")).len(), 3);
        assert_eq!(search(text("100%")), Vec::<String>::new());
        let session = AgentCallQuery {
            session: Some("one".to_owned()),
            status: Some(HistoryStatus::Ok),
            ..Default::default()
        };
        assert_eq!(search(session), ["list_connections"]);
        let agent = AgentCallQuery {
            agent: Some("codex".to_owned()),
            ..Default::default()
        };
        assert_eq!(search(agent), ["explain"]);

        let sessions = s.sessions(10).unwrap();
        assert_eq!(sessions.len(), 2);
        assert_eq!(sessions[0].session, "two");
        assert_eq!(
            (
                sessions[1].agent.as_str(),
                sessions[1].calls,
                sessions[1].errors
            ),
            ("claude-code", 2, 1)
        );

        assert_eq!(s.prune(1).unwrap(), 2);
        assert_eq!(search(AgentCallQuery::default()), ["explain"]);
        s.clear().unwrap();
        assert_eq!(search(AgentCallQuery::default()), Vec::<String>::new());
    }

    #[test]
    fn masks_passwords_before_keeping_them() {
        let s = AgentCallStore::open_in_memory().unwrap();
        s.record(&NewAgentCall {
            arguments:
                r#"{"connection":"App db","sql":"CREATE ROLE x LOGIN PASSWORD 'Sup3r\"S3cret'"}"#
                    .to_owned(),
            error: Some("near 'Sup3r\"S3cret': syntax error".to_owned()),
            ..call("one", "run_query", HistoryStatus::Error)
        })
        .unwrap();
        s.record(&NewAgentCall {
            arguments: "ALTER USER a IDENTIFIED BY 'hunter2' {".to_owned(),
            ..call("one", "run_query", HistoryStatus::Ok)
        })
        .unwrap();
        let calls = s
            .search(&AgentCallQuery {
                limit: 10,
                ..Default::default()
            })
            .unwrap();
        assert_eq!(
            calls[0].arguments,
            "ALTER USER a IDENTIFIED BY '********' {"
        );
        let args: serde_json::Value = serde_json::from_str(&calls[1].arguments).unwrap();
        assert_eq!(args["sql"], "CREATE ROLE x LOGIN PASSWORD '********'");
        assert_eq!(args["connection"], "App db");
        let error = calls[1].error.as_deref().unwrap();
        assert!(!error.contains("S3cret"), "{error}");
        // Arguments with nothing to mask are kept as sent.
        s.record(&call("one", "run_query", HistoryStatus::Ok))
            .unwrap();
        let last = s
            .search(&AgentCallQuery {
                limit: 1,
                ..Default::default()
            })
            .unwrap();
        assert_eq!(
            last[0].arguments,
            r#"{"connection":"App db","sql":"SELECT 1"}"#
        );
    }

    #[test]
    fn long_arguments_are_cut() {
        let s = AgentCallStore::open_in_memory().unwrap();
        s.record(&NewAgentCall {
            arguments: "x".repeat(ARGUMENT_CHARS + 10),
            ..call("one", "run_query", HistoryStatus::Ok)
        })
        .unwrap();
        let calls = s
            .search(&AgentCallQuery {
                limit: 1,
                ..Default::default()
            })
            .unwrap();
        assert_eq!(calls[0].arguments.chars().count(), ARGUMENT_CHARS + 1);
    }

    #[test]
    fn exports_csv() {
        let s = AgentCallStore::open_in_memory().unwrap();
        s.record(&call("one", "run_query", HistoryStatus::Error))
            .unwrap();
        let calls = s
            .search(&AgentCallQuery {
                limit: 10,
                ..Default::default()
            })
            .unwrap();
        let csv = agent_calls_csv(&calls);
        let mut lines = csv.split("\r\n");
        assert_eq!(
            lines.next(),
            Some("started_at,session,agent,tool,connection,arguments,duration_ms,status,error")
        );
        assert_eq!(
            lines.next(),
            Some(
                r#"2023-11-14T22:13:20.123Z,one,claude-code,run_query,App db,"{""connection"":""App db"",""sql"":""SELECT 1""}",12,error,no such table: t"#
            )
        );
        assert_eq!(rfc3339(0), "1970-01-01T00:00:00.000Z");
        assert_eq!(rfc3339(951_782_400_000), "2000-02-29T00:00:00.000Z");
    }

    #[test]
    fn proposals_are_handed_out_once() {
        let s = AgentCallStore::open_in_memory().unwrap();
        s.propose(
            "codex",
            "c1",
            Some("Fix"),
            "UPDATE t SET x = 1 WHERE id = 2",
        )
        .unwrap();
        s.propose("codex", "c1", None, "SELECT 1").unwrap();
        s.db.execute(
            "INSERT INTO proposals (agent, connection_id, sql, created_at) VALUES ('old', 'c1', 'SELECT 0', 0)",
            [],
        )
        .unwrap();
        let taken = s.take_proposals().unwrap();
        let sql: Vec<&str> = taken.iter().map(|p| p.sql.as_str()).collect();
        assert_eq!(sql, ["UPDATE t SET x = 1 WHERE id = 2", "SELECT 1"]);
        assert_eq!(taken[0].title.as_deref(), Some("Fix"));
        assert_eq!(s.take_proposals().unwrap(), Vec::new());
        let left: i64 =
            s.db.query_row("SELECT count(*) FROM proposals", [], |r| r.get(0))
                .unwrap();
        assert_eq!(left, 0, "opened and expired proposals are removed");
    }

    #[test]
    fn edit_proposals_share_the_limits_and_are_handed_out_once() {
        let s = AgentCallStore::open_in_memory().unwrap();
        let json = r#"{"schema":"main","table":"t"}"#;
        assert!(
            s.propose_edits("codex", "c1", Some("Fix"), json)
                .unwrap()
                .is_some()
        );
        s.propose("codex", "c1", None, "SELECT 1").unwrap();
        let taken = s.take_proposals().unwrap();
        assert_eq!(taken.len(), 2);
        assert_eq!(taken[0].sql, "");
        assert_eq!(taken[0].edits.as_deref(), Some(json));
        assert_eq!(taken[1].edits, None);
        assert_eq!(s.take_proposals().unwrap(), Vec::new());
        // The pending limit counts both kinds.
        for i in 0..MAX_PENDING_PROPOSALS {
            let kept = if i % 2 == 0 {
                s.propose_edits("flood", "c1", None, "{}")
            } else {
                s.propose("flood", "c1", None, "SELECT 1")
            };
            assert!(kept.unwrap().is_some());
        }
        assert_eq!(s.propose_edits("flood", "c1", None, "{}").unwrap(), None);
    }

    #[test]
    fn a_file_from_before_edit_proposals_still_opens() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("agent-calls.db");
        {
            let old = Connection::open(&path).unwrap();
            old.execute_batch(MIGRATIONS[0]).unwrap();
            old.execute_batch(MIGRATIONS[1]).unwrap();
            old.pragma_update(None, "user_version", 2).unwrap();
            old.execute(
                "INSERT INTO proposals (agent, connection_id, sql, created_at) VALUES ('codex', 'c1', 'SELECT 1', ?1)",
                [millis(SystemTime::now())],
            )
            .unwrap();
        }
        let s = AgentCallStore::open(&path).unwrap();
        let taken = s.take_proposals().unwrap();
        assert_eq!(taken.len(), 1);
        assert_eq!(taken[0].sql, "SELECT 1");
        assert_eq!(taken[0].edits, None);
        assert!(
            s.propose_edits("codex", "c1", None, "{}")
                .unwrap()
                .is_some()
        );
        let version: i64 =
            s.db.pragma_query_value(None, "user_version", |r| r.get(0))
                .unwrap();
        assert_eq!(version, 4);
    }

    #[test]
    fn waiting_proposals_are_bounded() {
        let s = AgentCallStore::open_in_memory().unwrap();
        for i in 0..MAX_PENDING_PROPOSALS {
            assert!(
                s.propose("flood", "c1", None, &format!("SELECT {i}"))
                    .unwrap()
                    .is_some()
            );
        }
        assert_eq!(
            s.propose("flood", "c1", None, "SELECT 'more'").unwrap(),
            None
        );
        // Another agent still may, until all agents together reach their bound.
        assert!(
            s.propose("codex", "c1", None, "SELECT 1")
                .unwrap()
                .is_some()
        );
        // Expired proposals do not count; opened ones are gone.
        s.db.execute(
            "UPDATE proposals SET created_at = 0 WHERE agent = 'flood'",
            [],
        )
        .unwrap();
        assert!(
            s.propose("flood", "c1", None, "SELECT 2")
                .unwrap()
                .is_some()
        );
        assert_eq!(s.take_proposals().unwrap().len(), 2);
        for i in 0..MAX_PENDING_ALL {
            let agent = format!("agent{}", i % 10);
            assert!(s.propose(&agent, "c1", None, "SELECT 1").unwrap().is_some());
        }
        assert_eq!(s.propose("late", "c1", None, "SELECT 1").unwrap(), None);
    }

    fn with_facts(
        agent: &str,
        session: &str,
        status: HistoryStatus,
        facts: CallFacts,
    ) -> NewAgentCall {
        NewAgentCall {
            agent: agent.to_owned(),
            facts,
            ..call(session, "run_query", status)
        }
    }

    fn facts(rows: u64, tables: &[&str], effect: CallEffect) -> CallFacts {
        CallFacts {
            rows,
            tables: tables.iter().map(|t| (*t).to_owned()).collect(),
            effect,
        }
    }

    #[test]
    fn reports_figures_per_agent_and_session() {
        let s = AgentCallStore::open_in_memory().unwrap();
        let ok = HistoryStatus::Ok;
        for c in [
            with_facts(
                "claude-code",
                "s1",
                ok,
                facts(5, &["main.a", "main.b"], CallEffect::None),
            ),
            with_facts(
                "claude-code",
                "s1",
                ok,
                facts(3, &["main.a"], CallEffect::None),
            ),
            with_facts("claude-code", "s1", ok, facts(0, &[], CallEffect::Allowed)),
            with_facts(
                "claude-code",
                "s2",
                HistoryStatus::Error,
                facts(0, &[], CallEffect::Refused),
            ),
            with_facts("codex", "s3", ok, facts(7, &["main.c"], CallEffect::None)),
        ] {
            s.record(&c).unwrap();
        }
        let report = s.report(&AgentCallQuery::default()).unwrap();
        assert_eq!(report.len(), 2);
        let claude = report.iter().find(|a| a.agent == "claude-code").unwrap();
        let f = &claude.figures;
        assert_eq!(
            (f.calls, f.measured, f.rows_returned, f.tables_read),
            (4, 4, Some(8), Some(2))
        );
        assert_eq!(
            (f.changes_allowed, f.changes_refused, f.errors),
            (Some(1), Some(1), 1)
        );
        // Tables count once per agent, and again in each session.
        let s1 = claude.sessions.iter().find(|x| x.session == "s1").unwrap();
        assert_eq!(s1.figures.tables_read, Some(2));
        assert_eq!(s1.figures.rows_returned, Some(8));
        let s2 = claude.sessions.iter().find(|x| x.session == "s2").unwrap();
        assert_eq!((s2.figures.tables_read, s2.figures.errors), (Some(0), 1));
        // Newest session first.
        assert_eq!(claude.sessions[0].session, "s2");

        // The report follows the list's filters.
        let errors = s
            .report(&AgentCallQuery {
                status: Some(HistoryStatus::Error),
                ..Default::default()
            })
            .unwrap();
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].figures.calls, 1);
        let one = s
            .report(&AgentCallQuery {
                session: Some("s3".to_owned()),
                ..Default::default()
            })
            .unwrap();
        assert_eq!(one[0].agent, "codex");
        assert_eq!(one[0].sessions.len(), 1);
    }

    #[test]
    fn same_table_name_on_two_connections_counts_twice() {
        let s = AgentCallStore::open_in_memory().unwrap();
        for connection in ["A", "B"] {
            s.record(&NewAgentCall {
                connection: Some(connection.to_owned()),
                facts: facts(1, &["main.t"], CallEffect::None),
                ..call("s", "sample_rows", HistoryStatus::Ok)
            })
            .unwrap();
        }
        let report = s.report(&AgentCallQuery::default()).unwrap();
        assert_eq!(report[0].figures.tables_read, Some(2));
    }

    #[test]
    fn an_empty_log_has_no_report() {
        let s = AgentCallStore::open_in_memory().unwrap();
        assert_eq!(s.report(&AgentCallQuery::default()).unwrap(), Vec::new());
    }

    #[test]
    fn calls_from_before_the_report_show_no_figures() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("agent-calls.db");
        {
            let old = Connection::open(&path).unwrap();
            for m in &MIGRATIONS[..3] {
                old.execute_batch(m).unwrap();
            }
            old.pragma_update(None, "user_version", 3).unwrap();
            old.execute(
                "INSERT INTO calls (session, agent, tool, arguments, started_at, duration_ms, status)
                 VALUES ('old', 'claude-code', 'run_query', '{}', 1000, 1, 'error')",
                [],
            )
            .unwrap();
        }
        let s = AgentCallStore::open(&path).unwrap();
        let report = s.report(&AgentCallQuery::default()).unwrap();
        let f = &report[0].figures;
        // Calls and errors are known; the rest was never recorded.
        assert_eq!((f.calls, f.measured, f.errors), (1, 0, 1));
        assert_eq!(
            (
                f.tables_read,
                f.rows_returned,
                f.changes_allowed,
                f.changes_refused
            ),
            (None, None, None, None)
        );
        // A new call joins the old ones: the figures cover the calls that have them.
        s.record(&with_facts(
            "claude-code",
            "old",
            HistoryStatus::Ok,
            facts(4, &["main.a"], CallEffect::None),
        ))
        .unwrap();
        let f = &s.report(&AgentCallQuery::default()).unwrap()[0].figures;
        assert_eq!((f.calls, f.measured, f.rows_returned), (2, 1, Some(4)));
    }

    #[test]
    fn the_report_of_100000_calls_is_fast() {
        let s = AgentCallStore::open_in_memory().unwrap();
        s.db.execute_batch("BEGIN").unwrap();
        {
            let mut insert =
                s.db.prepare(
                    "INSERT INTO calls (session, agent, tool, connection, arguments, started_at,
                                        duration_ms, status, rows_returned, tables_read, effect)
                     VALUES (?1, ?2, 'run_query', 'c', '{}', ?3, 1, ?4, 2, ?5, 'none')",
                )
                .unwrap();
            for i in 0..100_000_i64 {
                insert
                    .execute(params![
                        format!("s{}", i / 50),
                        format!("agent{}", i % 4),
                        i,
                        if i % 97 == 0 { "error" } else { "ok" },
                        format!(r#"["main.t{}","main.t{}"]"#, i % 40, i % 7),
                    ])
                    .unwrap();
            }
        }
        s.db.execute_batch("COMMIT").unwrap();
        let start = std::time::Instant::now();
        let report = s.report(&AgentCallQuery::default()).unwrap();
        let took = start.elapsed();
        eprintln!("report of 100000 calls: {took:?}");
        assert_eq!(report.len(), 4);
        assert_eq!(report.iter().map(|a| a.figures.calls).sum::<u64>(), 100_000);
        assert!(took < Duration::from_secs(5), "{took:?}");
        let plan: Vec<String> =
            s.db.prepare(&format!(
                "EXPLAIN QUERY PLAN SELECT agent, session, count(*) FROM calls
                 WHERE {CALL_FILTER} GROUP BY agent, session"
            ))
            .unwrap()
            .query_map(
                rusqlite::params![
                    None::<String>,
                    None::<String>,
                    None::<String>,
                    None::<String>,
                    None::<i64>
                ],
                |r| r.get::<_, String>(3),
            )
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();
        eprintln!("plan: {plan:?}");
    }

    #[test]
    fn two_handles_share_the_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("agent-calls.db");
        let server = AgentCallStore::open(&path).unwrap();
        let app = AgentCallStore::open(&path).unwrap();
        server
            .record(&call("one", "list_schemas", HistoryStatus::Cancelled))
            .unwrap();
        let calls = app
            .search(&AgentCallQuery {
                limit: 10,
                ..Default::default()
            })
            .unwrap();
        assert_eq!(calls[0].status, HistoryStatus::Cancelled);
    }
}
