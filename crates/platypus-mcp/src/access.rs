//! What agents may run: the connection's agent access, then the statement itself, checked
//! with the same analysis the app runs before a statement (`platypus_sql::analyze`).

use platypus_core::{TxEffect, classify};
use platypus_sql::{Dialect, StatementKind};
pub use platypus_store::AgentAccess;
use platypus_store::SettingsStore;

use crate::error::{AgentError, Result};

/// What the user's Safety setting for `UPDATE` or `DELETE` without `WHERE` (and `TRUNCATE`)
/// means for agents, who cannot answer the app's confirmation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnfilteredRule {
    /// "Run without asking".
    Allow,
    /// "Ask before running" or "Never run".
    Refuse,
}

impl UnfilteredRule {
    /// From `safety.unfiltered` in the app's settings: only `off` lets such statements run.
    #[must_use]
    pub fn from_settings(settings: Option<&SettingsStore>) -> Self {
        match settings
            .and_then(|s| s.get::<String>("safety.unfiltered"))
            .as_deref()
        {
            Some("off") => Self::Allow,
            _ => Self::Refuse,
        }
    }
}

/// The single statement of `sql`, if agents may run it on `connection` with `access`.
pub fn check<'a>(
    sql: &'a str,
    dialect: Dialect,
    access: AgentAccess,
    unfiltered: UnfilteredRule,
    connection: &str,
) -> Result<&'a str> {
    let statements = platypus_sql::split(sql, dialect);
    let statement = match statements.as_slice() {
        [] => return Err(AgentError::Invalid("there is no SQL to run".to_owned())),
        [one] => one.text(sql),
        many => {
            return Err(AgentError::Invalid(format!(
                "run_query runs one statement at a time and this SQL has {}: send them in separate calls",
                many.len()
            )));
        }
    };
    let analysis = platypus_sql::analyze(statement, dialect);
    match (access, analysis.kind) {
        (AgentAccess::Off, _) => Err(AgentError::NotFound(format!(
            "`{connection}` is not available to agents"
        ))),
        (_, StatementKind::Command) => Err(AgentError::Denied(
            "client commands such as `@set` belong to PlatypusDB's editor: send plain SQL"
                .to_owned(),
        )),
        (AgentAccess::ReadOnly, StatementKind::Query) => Ok(statement),
        (AgentAccess::ReadOnly, kind) => Err(AgentError::Denied(format!(
            "`{connection}` is read-only for agents: only statements that change nothing may run, \
             and this one {}. The user can allow agents to write in PlatypusDB's connection \
             settings (Agent access), except on production and read-only connections",
            match kind {
                StatementKind::Dml => "changes data",
                StatementKind::Ddl => "changes the schema or permissions",
                _ => "changes the session or its transaction",
            }
        ))),
        (AgentAccess::ReadWrite, _)
            if matches!(classify(statement), TxEffect::Begin | TxEffect::End) =>
        {
            Err(AgentError::Denied(
                "each statement an agent runs commits on its own: BEGIN, COMMIT and ROLLBACK \
                 are not available"
                    .to_owned(),
            ))
        }
        (AgentAccess::ReadWrite, _) => match (&analysis.unfiltered, unfiltered) {
            (Some(table), UnfilteredRule::Refuse) => Err(AgentError::Denied(format!(
                "this statement changes every row of `{}`: add a WHERE. PlatypusDB's Safety \
                 settings ask before UPDATE or DELETE without WHERE and TRUNCATE, and agents \
                 cannot answer",
                table.schema.as_ref().map_or_else(
                    || table.name.clone(),
                    |schema| format!("{schema}.{}", table.name)
                )
            ))),
            _ => Ok(statement),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PG: Dialect = Dialect::Postgres;

    fn denied(r: Result<&str>) -> String {
        match r {
            Err(AgentError::Denied(m)) => m,
            other => panic!("expected a refusal, got {other:?}"),
        }
    }

    #[test]
    fn read_only_runs_queries_only() {
        let ro = |sql| check(sql, PG, AgentAccess::ReadOnly, UnfilteredRule::Allow, "db");
        assert_eq!(ro("  select * from t;  ").unwrap(), "select * from t");
        assert_eq!(ro("explain select 1").unwrap(), "explain select 1");
        assert!(denied(ro("update t set a = 1 where id = 2")).contains("changes data"));
        assert!(denied(ro("drop table t")).contains("schema"));
        assert!(denied(ro("set search_path = x")).contains("session"));
        assert!(
            denied(ro("with d as (delete from t returning *) select * from d")).contains("data")
        );
        assert!(denied(ro("select * into copy from t")).contains("data"));
        assert!(denied(ro("begin")).contains("session"));
        assert!(denied(ro("@set x = 1")).contains("client commands"));
    }

    #[test]
    fn one_statement_per_call() {
        let rw = |sql| check(sql, PG, AgentAccess::ReadWrite, UnfilteredRule::Allow, "db");
        assert!(
            matches!(rw("select 1; select 2"), Err(AgentError::Invalid(m)) if m.contains("has 2"))
        );
        assert!(matches!(rw(" -- nothing\n"), Err(AgentError::Invalid(_))));
    }

    #[test]
    fn read_write_keeps_the_safety_checks() {
        let rw = |sql, rule| check(sql, PG, AgentAccess::ReadWrite, rule, "db");
        assert!(rw("update t set a = 1 where id = 2", UnfilteredRule::Refuse).is_ok());
        assert!(rw("create table x (id int)", UnfilteredRule::Refuse).is_ok());
        let every = denied(rw("delete from app.t", UnfilteredRule::Refuse));
        assert!(every.contains("every row of `app.t`"), "{every}");
        assert!(denied(rw("truncate t", UnfilteredRule::Refuse)).contains("`t`"));
        assert!(rw("delete from t", UnfilteredRule::Allow).is_ok());
        assert!(denied(rw("commit", UnfilteredRule::Allow)).contains("commits on its own"));
        assert!(denied(rw("start transaction", UnfilteredRule::Allow)).contains("BEGIN"));
    }

    #[test]
    fn off_hides_the_connection() {
        let off = check(
            "select 1",
            PG,
            AgentAccess::Off,
            UnfilteredRule::Allow,
            "db",
        );
        assert!(matches!(off, Err(AgentError::NotFound(_))));
    }

    #[test]
    fn reads_the_unfiltered_setting() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.toml");
        assert_eq!(UnfilteredRule::from_settings(None), UnfilteredRule::Refuse);
        let mut settings = SettingsStore::open(&path).unwrap();
        assert_eq!(
            UnfilteredRule::from_settings(Some(&settings)),
            UnfilteredRule::Refuse
        );
        settings
            .set("safety.unfiltered", Some("off".into()))
            .unwrap();
        assert_eq!(
            UnfilteredRule::from_settings(Some(&settings)),
            UnfilteredRule::Allow
        );
    }
}
