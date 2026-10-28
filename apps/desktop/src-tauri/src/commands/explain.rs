//! Query plans for the explain tab: asked of a session, or read from a pasted document.

use platypus_core::{Explain, plain_plan};
use serde::{Deserialize, Serialize};
use tauri::State;

use super::AppState;
use crate::error::{CommandError, CommandResult};

/// The plan of one statement; with `analyze` it runs, and its changes are rolled back.
#[tauri::command]
pub async fn explain(
    state: State<'_, AppState>,
    session_id: u64,
    sql: String,
    analyze: bool,
) -> CommandResult<Explain> {
    // Analyze runs the statement.
    if analyze {
        state.may_run(session_id, &sql)?;
    }
    let conn = state.live(session_id).await?;
    // `EXPLAIN SELECT 1;` is a valid request, but not with a semicolon then more text.
    let sql = sql.trim().trim_end_matches(';');
    Ok(conn.explain(sql, analyze).await?)
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

/// The first words of SQLite's steps, which no other database writes in capitals.
const SQLITE_STEPS: &[&str] = &[
    "SCAN ",
    "SEARCH ",
    "USE TEMP B-TREE",
    "CO-ROUTINE",
    "MATERIALIZE",
    "COMPOUND",
    "CORRELATED ",
    "SCALAR SUBQUERY",
    "LIST SUBQUERY",
    "MULTI-INDEX",
    "BLOOM FILTER",
    "LEFT-MOST",
    "MERGE ",
    "UNION ",
];

/// Whose plan `document` looks like: JSON is PostgreSQL's unless it is MariaDB's
/// (`query_block`); a tree of steps in capitals is SQLite's, one drawn with `|--` otherwise
/// SQL Server's; a first step `-> …` is MySQL's tree; anything else PostgreSQL's text.
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
    let steps = plan
        .lines()
        .map(|l| l.trim_start_matches(|c: char| c.is_whitespace() || matches!(c, '|' | '`' | '-')));
    if steps
        .clone()
        .any(|s| SQLITE_STEPS.iter().any(|w| s.starts_with(w)))
    {
        return PlanSource::Sqlite;
    }
    if plan.contains("|--") {
        return PlanSource::Sqlserver;
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
    }

    #[test]
    fn reads_with_the_source_asked() {
        let explain = read_plan("SCAN t".to_owned(), Some(PlanSource::Sqlite)).unwrap();
        assert_eq!(explain.explain.root.children[0].operation, "SCAN t");
        assert_eq!(explain.source, PlanSource::Sqlite);
        assert!(read_plan("SCAN t".to_owned(), Some(PlanSource::Mysql)).is_err());
    }
}
