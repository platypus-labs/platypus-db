//! Query plans for the explain tab: asked of a session, or read from a pasted document.

use std::time::Instant;

use platypus_core::{Explain, plain_plan};
use platypus_driver_sqlite::meta::is_plan_step;
use serde::{Deserialize, Serialize};
use tauri::State;

use super::{AppState, plugins};
use crate::error::{CommandError, CommandResult};

/// The plan of one statement; with `analyze` it runs, and its changes are rolled back.
#[tauri::command]
pub async fn explain(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    session_id: u64,
    sql: String,
    analyze: bool,
) -> CommandResult<Explain> {
    // Analyze runs the statement: it goes through the hooks as any other.
    let mut after = None;
    if analyze {
        state.may_run(session_id, &sql)?;
        if plugins::query_hooked() {
            after = plugins::before_query(&app, &state, session_id, &sql).await?;
        }
    }
    let conn = state.live(session_id).await?;
    // `EXPLAIN SELECT 1;` is a valid request, but not with a semicolon then more text.
    let sql = sql.trim().trim_end_matches(';');
    let started = Instant::now();
    let plan = conn.explain(sql, analyze).await;
    if let Some(after) = after {
        after.fire(plugins::status_of(&plan), started.elapsed(), None);
    }
    Ok(plan?)
}

/// Whose plan a pasted document is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PlanSource {
    Postgres,
    Mysql,
    Sqlite,
    Sqlserver,
}

/// Whose plan `document` looks like: JSON is PostgreSQL's unless it is MariaDB's
/// (`query_block`); steps in SQLite's capitals are SQLite's, others drawn with `|--` SQL
/// Server's; a first step `-> …` is MySQL's tree; anything else PostgreSQL's text.
fn guess(document: &str) -> PlanSource {
    let plan = plain_plan(document);
    let start = plan.trim_start();
    if start.starts_with('{') || start.starts_with('[') {
        return if plan.contains("\"query_block\"") {
            PlanSource::Mysql
        } else {
            PlanSource::Postgres
        };
    }
    // Steps drawn `|--Step`: SQLite's in capitals, SQL Server's otherwise; undrawn, a plan
    // of SQLite's steps only (as the app writes it).
    let drawn: Vec<&str> = plan
        .lines()
        .filter_map(|l| {
            let at = l.find("|--").or_else(|| l.find("`--"))?;
            Some(l[at + 3..].trim())
        })
        .collect();
    if !drawn.is_empty() {
        return if drawn.iter().any(|s| is_plan_step(s)) {
            PlanSource::Sqlite
        } else {
            PlanSource::Sqlserver
        };
    }
    let mut lines = plan
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .peekable();
    if lines.peek().is_some() && lines.all(is_plan_step) {
        return PlanSource::Sqlite;
    }
    if start.starts_with("->") {
        PlanSource::Mysql
    } else {
        PlanSource::Postgres
    }
}

/// A plan read from a document, and whose it was taken to be.
#[derive(Debug, Serialize)]
pub struct ReadPlan {
    pub explain: Explain,
    pub source: PlanSource,
}

/// Reads a plan pasted or opened from a file, of `source` or of the database it looks like.
#[tauri::command]
pub fn read_plan(document: String, source: Option<PlanSource>) -> CommandResult<ReadPlan> {
    let source = source.unwrap_or_else(|| guess(&document));
    let read = match source {
        PlanSource::Postgres => platypus_driver_pg::read_plan,
        PlanSource::Mysql => platypus_driver_mysql::read_plan,
        PlanSource::Sqlite => platypus_driver_sqlite::meta::read_plan,
        PlanSource::Sqlserver => platypus_driver_mssql::read_plan,
    };
    let explain = read(&document).map_err(|e| CommandError::new("plan", e.to_string()))?;
    Ok(ReadPlan { explain, source })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn guesses_whose_plan_it_is() {
        assert_eq!(
            guess(r#"[{"Plan": {"Node Type": "Seq Scan"}}]"#),
            PlanSource::Postgres
        );
        assert_eq!(
            guess(r#"{"query_block": {"select_id": 1}}"#),
            PlanSource::Mysql
        );
        assert_eq!(
            guess("-> Table scan on t  (cost=1 rows=1)"),
            PlanSource::Mysql
        );
        assert_eq!(
            guess(" Seq Scan on t  (cost=0.00..1.00 rows=1 width=4)\n   ->  Hash"),
            PlanSource::Postgres
        );
        assert_eq!(
            guess("QUERY PLAN\n|--SCAN a\n`--SEARCH b"),
            PlanSource::Sqlite
        );
        assert_eq!(
            guess("  |--Clustered Index Scan(OBJECT:([t]))"),
            PlanSource::Sqlserver
        );
        assert_eq!(guess("• scan\n  table: t@t_pkey"), PlanSource::Postgres);
        // SQL Server repeats the statement, which may start like a step of SQLite's.
        assert_eq!(
            guess("MERGE INTO t USING s ON t.id = s.id\n  |--Clustered Index Merge(OBJECT:([t]))"),
            PlanSource::Sqlserver
        );
        assert_eq!(
            guess("SCAN a\nSEARCH b USING INDEX i (x=?)"),
            PlanSource::Sqlite
        );
    }

    #[test]
    fn reads_with_the_source_asked() {
        let explain = read_plan("SCAN t".to_owned(), Some(PlanSource::Sqlite)).unwrap();
        assert_eq!(explain.explain.root.children[0].operation, "SCAN t");
        assert_eq!(explain.source, PlanSource::Sqlite);
        assert!(read_plan("SCAN t".to_owned(), Some(PlanSource::Mysql)).is_err());
    }
}
