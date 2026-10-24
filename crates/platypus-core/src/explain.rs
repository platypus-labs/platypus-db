//! Query plans. Each driver asks its database for a plan and turns the answer into a tree
//! of [`PlanNode`]s, so the UI shows every database the same way.

use serde::Serialize;

use crate::{Connection, Error, ExecOptions, ExecResult, Result, TextRow, query_text};

/// A query plan as the server produced it, and as a tree.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Explain {
    pub format: ExplainFormat,
    /// The plan document: JSON text for [`ExplainFormat::Json`], else plain text.
    pub plan: String,
    pub root: PlanNode,
    /// The statement ran: the nodes have actual rows and times.
    pub analyzed: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ExplainFormat {
    Json,
    Text,
}

/// One step of a plan. Rows and times are per loop, as the databases report them.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct PlanNode {
    /// What the step does: `Seq Scan`, `Hash Join`, `Table scan on t`, `SCAN t`.
    pub operation: String,
    /// What it works on, when the database names it apart: a table, an index, a condition.
    pub detail: Option<String>,
    /// The planner's cost up to and including this step, in the database's own units.
    pub cost: Option<f64>,
    /// Rows the planner expects.
    pub rows: Option<f64>,
    pub actual_rows: Option<f64>,
    /// Milliseconds until the step returned its last row.
    pub actual_ms: Option<f64>,
    pub loops: Option<f64>,
    /// Everything else the server said about the step, as label and text.
    pub properties: Vec<(String, String)>,
    pub children: Vec<PlanNode>,
}

impl PlanNode {
    #[must_use]
    pub fn new(operation: impl Into<String>) -> Self {
        Self {
            operation: operation.into(),
            ..Self::default()
        }
    }
}

/// Runs a plan request and returns the plan document: the first column of its rows, one
/// line each. With `analyze` the request executes the statement, so it runs in a
/// transaction rolled back afterwards (a savepoint inside an open one): what the statement
/// changes is undone where the database can undo it.
pub async fn explain_document(conn: &dyn Connection, sql: &str, analyze: bool) -> Result<String> {
    let rows = explain_rows(conn, sql, analyze).await?;
    let lines: Vec<&str> = rows.iter().filter_map(|r| r.first()?.as_deref()).collect();
    if lines.is_empty() {
        return Err(Error::Other("the database returned no plan".to_owned()));
    }
    Ok(lines.join("\n"))
}

async fn explain_rows(conn: &dyn Connection, sql: &str, analyze: bool) -> Result<Vec<TextRow>> {
    if !analyze {
        return query_text(conn, sql).await;
    }
    let (start, undo) = if conn.transaction().open {
        (
            "SAVEPOINT platypus_explain",
            "ROLLBACK TO SAVEPOINT platypus_explain",
        )
    } else {
        ("BEGIN", "ROLLBACK")
    };
    run(conn, start).await?;
    let rows = query_text(conn, sql).await;
    let undone = run(conn, undo).await;
    let rows = rows?;
    undone?;
    Ok(rows)
}

async fn run(conn: &dyn Connection, sql: &str) -> Result<()> {
    if let ExecResult::Rows(stream) = conn.execute(sql, &ExecOptions::default()).await? {
        drop(stream);
    }
    Ok(())
}
