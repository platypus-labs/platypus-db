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
    /// The step's own cost, without its children's: where the planner expects the work.
    /// Set by [`Explain::new`].
    pub self_cost: Option<f64>,
    /// With analyze, the milliseconds spent in the step itself over all its loops. Set by
    /// [`Explain::new`].
    pub self_ms: Option<f64>,
    /// Everything else the server said about the step, as label and text.
    pub properties: Vec<(String, String)>,
    pub children: Vec<PlanNode>,
}

impl Explain {
    /// A plan whose steps know their own share of the cost and of the time.
    #[must_use]
    pub fn new(format: ExplainFormat, plan: String, mut root: PlanNode, analyzed: bool) -> Self {
        root.fill_self();
        Self {
            format,
            plan,
            root,
            analyzed,
        }
    }
}

impl PlanNode {
    #[must_use]
    pub fn new(operation: impl Into<String>) -> Self {
        Self {
            operation: operation.into(),
            ..Self::default()
        }
    }

    /// Milliseconds over all loops, children included.
    fn total_ms(&self) -> Option<f64> {
        Some(self.actual_ms? * self.loops.unwrap_or(1.0))
    }

    fn fill_self(&mut self) {
        for child in &mut self.children {
            child.fill_self();
        }
        // Costs and times include the children's; a parallel or cached child can make the
        // difference negative.
        let own = |total: Option<f64>, of: fn(&Self) -> Option<f64>| {
            total.map(|t| (t - self.children.iter().filter_map(of).sum::<f64>()).max(0.0))
        };
        let (cost, ms) = (
            own(self.cost, |c| c.cost),
            own(self.total_ms(), Self::total_ms),
        );
        self.self_cost = cost;
        self.self_ms = ms;
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

#[cfg(test)]
mod tests {
    use super::*;

    fn step(cost: f64, ms: Option<f64>, loops: f64, children: Vec<PlanNode>) -> PlanNode {
        PlanNode {
            cost: Some(cost),
            actual_ms: ms,
            loops: ms.map(|_| loops),
            children,
            ..PlanNode::new("step")
        }
    }

    #[test]
    fn steps_get_their_own_cost_and_time() {
        // A join of a scan run once and a lookup run 10 times, 0.2 ms each.
        let scan = step(10.0, Some(2.0), 1.0, vec![]);
        let lookup = step(5.0, Some(0.2), 10.0, vec![]);
        let join = step(40.0, Some(5.0), 1.0, vec![scan, lookup]);
        let root = Explain::new(ExplainFormat::Json, String::new(), join, true).root;
        assert_eq!(root.self_cost, Some(25.0));
        assert_eq!(root.self_ms, Some(1.0));
        assert_eq!(root.children[1].self_ms, Some(2.0));

        // Children reported as slower than their parent (parallel workers) count as zero.
        let parallel = step(1.0, Some(1.0), 1.0, vec![step(2.0, Some(3.0), 1.0, vec![])]);
        let root = Explain::new(ExplainFormat::Json, String::new(), parallel, true).root;
        assert_eq!((root.self_cost, root.self_ms), (Some(0.0), Some(0.0)));

        // Without analyze there is no time.
        let plain = Explain::new(
            ExplainFormat::Text,
            String::new(),
            step(3.0, None, 1.0, vec![]),
            false,
        );
        assert_eq!(
            (plain.root.self_cost, plain.root.self_ms),
            (Some(3.0), None)
        );
    }
}
