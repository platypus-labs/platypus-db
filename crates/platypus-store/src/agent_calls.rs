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
];

/// Proposals older than this are not opened any more: the agent's session is long over.
pub const PROPOSAL_LIFETIME: Duration = Duration::from_hours(24);
/// Proposals of one agent that may wait for the app to open them.
pub const MAX_PENDING_PROPOSALS: u32 = 20;
/// Proposals of all agents that may wait for the app to open them.
pub const MAX_PENDING_ALL: u32 = 100;

/// SQL an agent proposed, for the user to review and run in the editor.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AgentProposal {
    pub id: i64,
    pub agent: String,
    pub connection_id: String,
    pub title: Option<String>,
    pub sql: String,
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
                                    duration_ms, status, error)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
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
                ],
            )
            .map_err(db_error)?;
        Ok(self.db.last_insert_rowid())
    }

    pub fn search(&self, query: &AgentCallQuery) -> Result<Vec<AgentCall>> {
        let pattern = query
            .text
            .as_deref()
            .map(str::trim)
            .filter(|t| !t.is_empty())
            .map(like_pattern);
        let mut stmt = self
            .db
            .prepare_cached(
                "SELECT id, session, agent, tool, connection, arguments, started_at, duration_ms,
                        status, error
                 FROM calls
                 WHERE (?1 IS NULL OR tool LIKE ?1 ESCAPE '\\' OR agent LIKE ?1 ESCAPE '\\'
                        OR connection LIKE ?1 ESCAPE '\\' OR arguments LIKE ?1 ESCAPE '\\'
                        OR error LIKE ?1 ESCAPE '\\')
                   AND (?2 IS NULL OR session = ?2)
                   AND (?3 IS NULL OR agent = ?3)
                   AND (?4 IS NULL OR status = ?4)
                   AND (?5 IS NULL OR id < ?5)
                 ORDER BY id DESC
                 LIMIT ?6",
            )
            .map_err(db_error)?;
        let rows = stmt
            .query_map(
                params![
                    pattern,
                    query.session,
                    query.agent,
                    query.status.map(HistoryStatus::as_str),
                    query.before_id,
                    i64::try_from(query.limit).unwrap_or(i64::MAX),
                ],
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
        let now = SystemTime::now();
        let added = self
            .db
            .execute(
                "INSERT INTO proposals (agent, connection_id, title, sql, created_at)
                 SELECT ?1, ?2, ?3, ?4, ?5
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
                     RETURNING id, agent, connection_id, title, sql, created_at",
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
                        created_at: r.get(5)?,
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
