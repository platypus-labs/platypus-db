//! `server_activity` and `top_queries`: what the database server is doing now (sessions,
//! running statements, locks) and what cost it most time (`pg_stat_statements`,
//! `performance_schema`, Query Store), read with the views of the app's admin tab
//! ([`Connection::server_sessions`](platypus_core::Connection::server_sessions) and the
//! like). Read-only: nothing is cancelled or killed.
//!
//! The statements are other people's: they carry their values. Each one has its credentials
//! masked ([`mask_secrets`](platypus_sql::security::mask_secrets), `'********'` as the
//! history shows them) and the connection's secrets redacted, always; the values of sensitive
//! columns are masked too, unless the user chose in the app to show them on the connection
//! (see [`crate::masking`]). Marks that cannot be read fail the call.

use platypus_core::{Connection, Error, ServerLock, TOP_QUERIES};
use platypus_sql::Dialect;
use platypus_sql::security::mask_secrets;
use platypus_store::{AgentValues, SavedConnection};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::agent::Agent;
use crate::error::{AgentError, Result};
use crate::guard::{Asked, clamp};
use crate::library::SensitiveNames;
use crate::query::{self, Limits};
use crate::redact::Redactor;

/// The arguments of `server_activity` and `top_queries`.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ServerViewArgs {
    /// The connection's name, as `list_connections` shows it.
    pub connection: String,
    /// Entries returned at most: sessions for `server_activity` (50 by default, up to
    /// 200), statements for `top_queries` (20 by default, up to 100).
    pub limit: Option<u64>,
}

/// Characters of a statement kept; a longer one is cut after its values are masked.
pub const STATEMENT_CHARS: usize = 500;

const DEFAULT_SESSIONS: usize = 50;
const MAX_SESSIONS: usize = 200;
const DEFAULT_STATEMENTS: usize = 20;
const MAX_LOCKS: usize = 200;

/// What hides the values and credentials of a statement read from the server.
struct StatementMask {
    redactor: Redactor,
    /// `None` where the user chose to show the values of sensitive columns.
    names: Option<SensitiveNames>,
    dialect: Dialect,
    /// Statements cut at [`STATEMENT_CHARS`].
    cut: usize,
}

impl StatementMask {
    /// `text` with credentials, secrets and sensitive values masked, and cut to length.
    fn apply(&mut self, text: &str) -> String {
        let masked = self.redactor.apply(&mask_secrets(text));
        let masked = match &self.names {
            Some(names) => names.mask(&masked, self.dialect),
            None => masked,
        };
        if masked.chars().count() <= STATEMENT_CHARS {
            return masked;
        }
        self.cut += 1;
        let kept: String = masked.chars().take(STATEMENT_CHARS).collect();
        format!("{kept}…")
    }

    /// `object` with `field` masked, when it is a string.
    fn field(&mut self, object: &mut Value, field: &str) {
        if let Some(Value::String(text)) = object.get_mut(field) {
            *text = self.apply(text);
        }
    }

    fn note(&self) -> Option<String> {
        (self.cut > 0).then(|| {
            format!(
                "{} statements were cut at {STATEMENT_CHARS} characters",
                self.cut
            )
        })
    }
}

/// An error of the server's views as the agent reads it: a missing privilege says so.
fn friendly(error: AgentError, what: &str) -> AgentError {
    let AgentError::Database(message) = &error else {
        return error;
    };
    let lower = message.to_lowercase();
    let privilege = [
        "permission denied",
        "insufficient privilege",
        "view server state",
        "access denied",
        "command denied",
    ];
    if privilege.iter().any(|p| lower.contains(p)) {
        return AgentError::Denied(format!(
            "the connection's database user may not read {what}: the user can grant the \
             privilege in the database ({message})"
        ));
    }
    error
}

/// The limit asked for, within `1..=max`.
fn page(limit: Option<u64>, default: usize, max: usize) -> Result<usize> {
    match limit {
        None => Ok(default),
        Some(0) => Err(AgentError::Invalid(
            "limit must be greater than 0".to_owned(),
        )),
        Some(n) => Ok(usize::try_from(n).unwrap_or(max).min(max)),
    }
}

impl Agent {
    /// The connection, with the mask of its statements, for a view of the server. Fails
    /// when the sensitive marks cannot be read.
    fn statement_mask(&self, saved: &SavedConnection, redactor: Redactor) -> Result<StatementMask> {
        let names = if saved.agent_values == AgentValues::Shown {
            None
        } else {
            Some(self.sensitivity()?.names(&[saved.id])?)
        };
        Ok(StatementMask {
            redactor,
            names,
            dialect: Dialect::from(saved.driver),
            cut: 0,
        })
    }

    /// The server's sessions with what they run, and the locks held or waited for: the sessions
    /// that wait or run first, up to `args.limit`. The statements are masked.
    pub async fn server_activity(&self, args: &ServerViewArgs) -> Result<Value> {
        let limit = page(args.limit, DEFAULT_SESSIONS, MAX_SESSIONS)?;
        let (store, saved) = self.find(&args.connection)?;
        self.windows.count(&saved)?;
        let lease = self.lease(&store, &saved).await?;
        if !lease.conn().capabilities().server_sessions {
            return Err(unavailable(&saved, "server sessions and locks"));
        }
        let mut mask = self.statement_mask(&saved, lease.redactor())?;
        let (limits, _) = clamp(
            Limits::new(None, None, None)?,
            &Asked::default(),
            &saved.agent_limits,
        );
        let work = sessions_and_locks(lease.conn());
        let read = query::guarded(
            lease.conn(),
            None,
            limits.timeout,
            std::future::pending(),
            work,
        )
        .await;
        let (mut sessions, locks) = lease
            .finish(read, limits.timeout)
            .map_err(|e| friendly(e, "the server's sessions"))?;
        // Those that wait first, then those that run.
        sessions.sort_by_key(|s| (s.blocked_by.is_empty(), !s.is_running()));
        let total = sessions.len();
        let shown: Vec<Value> = sessions
            .iter()
            .take(limit)
            .map(|s| {
                let mut session = json!(s);
                mask.field(&mut session, "query");
                session
            })
            .collect();
        let mut notes = Vec::new();
        if total > limit {
            notes.push(format!(
                "{} more sessions left out: raise limit (up to {MAX_SESSIONS})",
                total - limit
            ));
        }
        notes.extend(mask.note());
        let mut answer = json!({ "sessions": shown, "total_sessions": total });
        match locks {
            Ok(locks) => {
                if locks.len() > MAX_LOCKS {
                    notes.push(format!("{} more locks left out", locks.len() - MAX_LOCKS));
                }
                answer["locks"] = locks.iter().take(MAX_LOCKS).map(|l| json!(l)).collect();
            }
            Err(why) => {
                answer["locks"] = json!([]);
                notes.push(format!(
                    "the locks could not be read: {}",
                    mask.redactor.apply(&why)
                ));
            }
        }
        answer["notes"] = json!(notes);
        Ok(answer)
    }

    /// The statements that took the server most time, the costliest first, up to
    /// `args.limit`. The statements and their samples are masked.
    pub async fn top_queries(&self, args: &ServerViewArgs) -> Result<Value> {
        let limit = page(args.limit, DEFAULT_STATEMENTS, TOP_QUERIES)?;
        let (store, saved) = self.find(&args.connection)?;
        self.windows.count(&saved)?;
        let lease = self.lease(&store, &saved).await?;
        if !lease.conn().capabilities().server_statistics {
            return Err(unavailable(&saved, "query statistics"));
        }
        let mut mask = self.statement_mask(&saved, lease.redactor())?;
        let (limits, _) = clamp(
            Limits::new(None, None, None)?,
            &Asked::default(),
            &saved.agent_limits,
        );
        let work = lease.conn().top_queries();
        let read = query::guarded(
            lease.conn(),
            None,
            limits.timeout,
            std::future::pending(),
            work,
        )
        .await;
        let queries = lease
            .finish(read, limits.timeout)
            .map_err(|e| friendly(e, "the server's query statistics"))?;
        let total = queries.len();
        let shown: Vec<Value> = queries
            .iter()
            .take(limit)
            .map(|q| {
                let mut query = json!(q);
                mask.field(&mut query, "query");
                mask.field(&mut query, "sample");
                query
            })
            .collect();
        let mut notes = Vec::new();
        if total > limit {
            notes.push(format!(
                "{} more statements left out: raise limit (up to {TOP_QUERIES})",
                total - limit
            ));
        }
        notes.extend(mask.note());
        Ok(json!({ "queries": shown, "total_queries": total, "notes": notes }))
    }
}

/// The error of a connection whose database has no such view.
fn unavailable(saved: &SavedConnection, what: &str) -> AgentError {
    AgentError::Invalid(format!(
        "`{}` ({}) has no {what} in PlatypusDB: they are available for PostgreSQL, MySQL,          MariaDB and SQL Server",
        saved.name,
        saved.driver.name()
    ))
}

/// The sessions, and the locks (or why they could not be read: the sessions are of use alone).
async fn sessions_and_locks(
    conn: &dyn Connection,
) -> std::result::Result<
    (
        Vec<platypus_core::ServerSession>,
        std::result::Result<Vec<ServerLock>, String>,
    ),
    Error,
> {
    let sessions = conn.server_sessions().await?;
    let locks = conn.server_locks().await.map_err(|e| e.to_string());
    Ok((sessions, locks))
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use platypus_sql::sensitive::NameRules;

    use super::*;

    fn mask(names: Option<SensitiveNames>) -> StatementMask {
        let mut redactor = Redactor::default();
        redactor.add("s3cr3t-token");
        StatementMask {
            redactor,
            names,
            dialect: Dialect::Postgres,
            cut: 0,
        }
    }

    fn sensitive() -> SensitiveNames {
        SensitiveNames::new(NameRules::new("*email*"), HashSet::new())
    }

    #[test]
    fn a_password_in_a_statement_never_shows() {
        // Shown lifts the masking of sensitive columns, never that of credentials.
        for names in [Some(sensitive()), None] {
            let text = mask(names).apply("CREATE ROLE ada LOGIN PASSWORD 'hunter2'");
            assert!(
                text.contains("'********'") && !text.contains("hunter2"),
                "{text}"
            );
        }
    }

    #[test]
    fn the_connections_secrets_are_redacted() {
        let text = mask(None).apply("SELECT 's3cr3t-token'");
        assert!(!text.contains("s3cr3t-token"), "{text}");
    }

    #[test]
    fn sensitive_values_mask_unless_shown() {
        let sql = "UPDATE users SET name = 'x' WHERE email = 'ada@example.com'";
        assert!(
            !mask(Some(sensitive()))
                .apply(sql)
                .contains("ada@example.com")
        );
        assert!(mask(None).apply(sql).contains("ada@example.com"));
    }

    #[test]
    fn a_long_statement_is_cut_with_a_count() {
        let mut m = mask(None);
        let long = format!("SELECT '{}'", "a".repeat(STATEMENT_CHARS * 2));
        let text = m.apply(&long);
        assert_eq!(text.chars().count(), STATEMENT_CHARS + 1);
        assert!(text.ends_with('…'));
        assert!(m.note().is_some_and(|n| n.starts_with("1 statements")));
        assert!(mask(None).note().is_none());
    }

    #[test]
    fn a_missing_privilege_is_a_clear_denial() {
        let denied = friendly(
            AgentError::Database("permission denied for view pg_stat_activity".to_owned()),
            "the sessions",
        );
        assert!(
            matches!(&denied, AgentError::Denied(m) if m.contains("may not read the sessions"))
        );
        let other = friendly(AgentError::Database("syntax error".to_owned()), "x");
        assert!(matches!(other, AgentError::Database(_)));
    }

    #[test]
    fn the_limit_is_within_bounds() {
        assert_eq!(page(None, 50, 200).unwrap(), 50);
        assert_eq!(page(Some(9999), 50, 200).unwrap(), 200);
        assert_eq!(page(Some(3), 50, 200).unwrap(), 3);
        assert!(page(Some(0), 50, 200).is_err());
    }
}
