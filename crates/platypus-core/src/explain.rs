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
    /// Filtered full scans of large tables, where an index on the filtered columns may
    /// help. Set by [`Explain::new`].
    pub suggestions: Vec<FullScan>,
}

/// Rows a filtered full scan must read before an index is worth suggesting.
pub const LARGE_SCAN_ROWS: f64 = 10_000.0;

/// A step that reads a whole table and keeps the rows matching a filter.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct FullScan {
    pub table: String,
    pub filter: String,
    /// Rows read, over all loops: actual with analyze; otherwise the planner's estimate,
    /// which PostgreSQL does not give (`None`).
    pub rows_read: Option<f64>,
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
    /// Set by the driver when the step is a filtered full scan of a table.
    pub full_scan: Option<FullScan>,
    /// Everything else the server said about the step, as label and text.
    pub properties: Vec<(String, String)>,
    pub children: Vec<PlanNode>,
}

impl Explain {
    /// A plan whose steps know their own share of the cost and of the time, with its
    /// index suggestions.
    #[must_use]
    pub fn new(format: ExplainFormat, plan: String, mut root: PlanNode, analyzed: bool) -> Self {
        root.fill_self();
        let mut suggestions = Vec::new();
        root.large_scans(&mut suggestions);
        Self {
            format,
            plan,
            root,
            analyzed,
            suggestions,
        }
    }

    /// A plan read from a document pasted or opened from a file: analyzed when its steps
    /// have actual rows or times.
    #[must_use]
    pub fn read(format: ExplainFormat, plan: String, root: PlanNode) -> Self {
        let analyzed = root.has_actuals();
        Self::new(format, plan, root, analyzed)
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

    fn has_actuals(&self) -> bool {
        self.actual_rows.is_some()
            || self.actual_ms.is_some()
            || self.children.iter().any(Self::has_actuals)
    }

    /// Milliseconds over all loops, children included.
    fn total_ms(&self) -> Option<f64> {
        Some(self.actual_ms? * self.loops.unwrap_or(1.0))
    }

    fn large_scans(&self, out: &mut Vec<FullScan>) {
        if let Some(scan) = &self.full_scan
            && scan.rows_read.is_some_and(|r| r >= LARGE_SCAN_ROWS)
        {
            out.push(scan.clone());
        }
        for child in &self.children {
            child.large_scans(out);
        }
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
        self.self_cost = own(self.cost, |c| c.cost);
        self.self_ms = own(self.total_ms(), Self::total_ms);
    }
}

/// The tree of a plan the database writes as indented text: `step` tells, for each line,
/// whether it starts a step, at which column (deeper columns are children of the step above)
/// and with which operation. Other lines are properties of the step above them (`key: value`),
/// or of the plan when no step came yet. Several top-level steps hang under one `Plan`.
#[must_use]
pub fn plan_from_text(text: &str, step: impl Fn(&str) -> Option<(usize, String)>) -> PlanNode {
    // The open steps, deepest last, with their columns.
    let mut open: Vec<(usize, PlanNode)> = Vec::new();
    let mut tops: Vec<PlanNode> = Vec::new();
    let mut loose: Vec<(String, String)> = Vec::new();
    let close = |open: &mut Vec<(usize, PlanNode)>, tops: &mut Vec<PlanNode>| {
        if let Some((_, node)) = open.pop() {
            match open.last_mut() {
                Some((_, parent)) => parent.children.push(node),
                None => tops.push(node),
            }
        }
    };
    for line in text.lines() {
        if let Some((column, operation)) = step(line) {
            while open.last().is_some_and(|(c, _)| *c >= column) {
                close(&mut open, &mut tops);
            }
            open.push((column, PlanNode::new(operation)));
            continue;
        }
        let property = line.trim().trim_start_matches(['│', '|']).trim();
        if property.is_empty() {
            continue;
        }
        let pair = match property.split_once(": ") {
            Some((key, value)) => (key.trim().to_owned(), value.trim().to_owned()),
            None => (String::new(), property.to_owned()),
        };
        match open.last_mut() {
            Some((_, node)) => node.properties.push(pair),
            None => loose.push(pair),
        }
    }
    while !open.is_empty() {
        close(&mut open, &mut tops);
    }
    let mut root = if tops.len() == 1 {
        tops.remove(0)
    } else {
        PlanNode {
            children: tops,
            ..PlanNode::new("Plan")
        }
    };
    loose.append(&mut root.properties);
    root.properties = loose;
    root
}

/// A step written as `description  (cost=… rows=…) (actual time=…..… rows=… loops=…)`, as
/// PostgreSQL's text plans and MySQL's tree write them: the description, and a node with
/// the figures (no operation yet). `never executed` is no loop.
#[must_use]
pub fn step_figures(step: &str) -> (&str, PlanNode) {
    let cut = ["(cost=", "(actual ", "(never executed)"]
        .iter()
        .filter_map(|p| step.find(p))
        .min()
        .unwrap_or(step.len());
    let (description, figures) = step.split_at(cut);
    let mut node = PlanNode::default();
    for group in figures.split(')') {
        let group = group.trim().trim_start_matches('(');
        if group == "never executed" {
            node.loops = Some(0.0);
        }
        let actual = group.starts_with("actual");
        for (key, value) in group.split_whitespace().filter_map(|t| t.split_once('=')) {
            // `cost=first..last`, `time=first..last`: the cost and time to the last row.
            let value = value.rsplit("..").next().and_then(|v| v.parse().ok());
            match (actual, key) {
                (false, "cost") => node.cost = value,
                (false, "rows") => node.rows = value,
                (true, "time") => node.actual_ms = value,
                (true, "rows") => node.actual_rows = value,
                (true, "loops") => node.loops = value,
                _ => {}
            }
        }
    }
    (description.trim(), node)
}

/// A plan as a client printed it, without the client's frame: psql's `QUERY PLAN` header,
/// its rule, its `(N rows)` footer and the `+` that ends its wrapped lines; the MySQL
/// client's table borders, its `EXPLAIN` header, the `EXPLAIN: ` of its vertical output and
/// the `\n` its batch mode writes for line breaks. The plan's own indentation stays.
#[must_use]
pub fn plain_plan(text: &str) -> String {
    let rule = |l: &str| !l.is_empty() && l.chars().all(|c| matches!(c, '-' | '+' | '='));
    let footer = |l: &str| {
        l.strip_prefix('(')
            .and_then(|l| l.strip_suffix(" rows)").or_else(|| l.strip_suffix(" row)")))
            .is_some_and(|n| n.chars().all(|c| c.is_ascii_digit()))
    };
    // A header cell: `QUERY PLAN`, `| EXPLAIN    |`, or `EXPLAIN` alone in batch mode.
    let header = |l: &str| {
        let cell = l
            .strip_prefix('|')
            .and_then(|c| c.strip_suffix('|'))
            .unwrap_or(l);
        matches!(cell.trim(), "QUERY PLAN" | "EXPLAIN")
    };
    // psql writes a long value over several lines, each but the last ending in `+` (right
    // after the text when it fills the column): most lines end so.
    let filled = text.lines().filter(|l| !l.trim().is_empty());
    let wrapped = filled
        .clone()
        .filter(|l| l.trim_end().ends_with('+'))
        .count()
        * 2
        > filled.count();
    let mut lines = Vec::new();
    for line in text.lines() {
        let trimmed = line.trim();
        if header(trimmed) || rule(trimmed) || footer(trimmed) {
            continue;
        }
        let line = line.trim_end();
        let line = match line.strip_suffix('+') {
            Some(start) if wrapped => start.trim_end(),
            _ => line,
        };
        // The MySQL client's table: `| -> Filter ...`, its cell going on over the lines
        // below, closed by ` |`.
        let line = line.strip_suffix(" |").map_or(line, str::trim_end);
        let line = line.strip_prefix("| ").unwrap_or(line);
        let line = line.trim_start_matches("EXPLAIN: ");
        // `mysql -e` writes the plan on one line, its breaks as `\n`.
        if line.trim_start().starts_with(['-', '{']) && line.contains("\\n") {
            lines.extend(line.split("\\n"));
        } else {
            lines.push(line);
        }
    }
    // psql and the MySQL client put one space before every line.
    let indent = lines
        .iter()
        .filter(|l| !l.trim().is_empty())
        .map(|l| l.len() - l.trim_start().len())
        .min()
        .unwrap_or(0);
    lines
        .iter()
        .map(|l| l.get(indent..).unwrap_or_else(|| l.trim_start()))
        .collect::<Vec<_>>()
        .join("\n")
        .trim_matches('\n')
        .to_owned()
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

    #[test]
    fn reads_step_figures() {
        let (description, node) = step_figures(
            "Seq Scan on orders o  (cost=0.00..1.06 rows=6 width=12) (actual time=0.008..0.009 rows=5 loops=2)",
        );
        assert_eq!(description, "Seq Scan on orders o");
        assert_eq!((node.cost, node.rows), (Some(1.06), Some(6.0)));
        assert_eq!(
            (node.actual_ms, node.actual_rows, node.loops),
            (Some(0.009), Some(5.0), Some(2.0))
        );
        let (description, node) =
            step_figures("Hash  (cost=1.04..1.04 rows=4 width=36) (never executed)");
        assert_eq!((description, node.loops), ("Hash", Some(0.0)));
        let (description, node) = step_figures("Hash Join");
        assert_eq!((description, node.cost), ("Hash Join", None));
    }

    #[test]
    fn drops_the_frame_of_clients() {
        let psql = "                         QUERY PLAN\n\
                    ------------------------------------------------------------\n \
                    Seq Scan on t  (cost=0.00..35.50 rows=2550 width=4)\n   \
                    Filter: (a > 1)\n\
                    (2 rows)\n";
        assert_eq!(
            plain_plan(psql),
            "Seq Scan on t  (cost=0.00..35.50 rows=2550 width=4)\n  Filter: (a > 1)"
        );
        let json = " [                 +\n   {               +\n     \"Plan\": {\"a\": \"x\"}+\n   }               +\n ]";
        assert_eq!(
            plain_plan(json),
            "[\n  {\n    \"Plan\": {\"a\": \"x\"}\n  }\n]"
        );
        // A step that ends in `+` is no wrapped line.
        assert_eq!(
            plain_plan("Result\n  Output: (1 +"),
            "Result\n  Output: (1 +"
        );
        let mysql = "+------+\n| EXPLAIN |\n+------+\n| -> Filter: (t.a = 1)  (cost=0.35 rows=1)\n    -> Table scan on t  (cost=0.35 rows=1)\n |\n+------+";
        assert_eq!(
            plain_plan(mysql),
            "-> Filter: (t.a = 1)  (cost=0.35 rows=1)\n    -> Table scan on t  (cost=0.35 rows=1)"
        );
        assert_eq!(
            plain_plan("EXPLAIN: -> Rows fetched before execution"),
            "-> Rows fetched before execution"
        );
        assert_eq!(
            plain_plan("EXPLAIN\n-> Filter: (a = 1)\\n    -> Table scan on t\\n"),
            "-> Filter: (a = 1)\n    -> Table scan on t"
        );
        assert_eq!(
            plain_plan("| EXPLAIN        |\n| -> Filter  |"),
            "-> Filter"
        );
        // SQL Server's steps keep their bars.
        assert_eq!(
            plain_plan("  |--Clustered Index Scan(t)"),
            "|--Clustered Index Scan(t)"
        );
    }

    #[test]
    fn a_read_plan_is_analyzed_when_its_steps_ran() {
        let estimated = Explain::read(
            ExplainFormat::Text,
            String::new(),
            step(1.0, None, 1.0, vec![]),
        );
        assert!(!estimated.analyzed);
        let analyzed = Explain::read(
            ExplainFormat::Text,
            String::new(),
            step(2.0, None, 1.0, vec![step(1.0, Some(0.5), 1.0, vec![])]),
        );
        assert!(analyzed.analyzed);
    }

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

    #[test]
    fn reads_plans_written_as_indented_text() {
        let indent = |line: &str| {
            let text = line.trim_start();
            (!text.is_empty()).then(|| (line.len() - text.len(), text.to_owned()))
        };
        let clickhouse =
            "Expression (Project names)\n  Filter (WHERE)\n    ReadFromSystemNumbers\n  Sort";
        let root = plan_from_text(clickhouse, indent);
        assert_eq!(root.operation, "Expression (Project names)");
        let ops: Vec<_> = root.children.iter().map(|c| c.operation.as_str()).collect();
        assert_eq!(ops, ["Filter (WHERE)", "Sort"]);
        assert_eq!(
            root.children[0].children[0].operation,
            "ReadFromSystemNumbers"
        );

        let bullet = |line: &str| {
            let at = line.find('•')?;
            Some((
                line[..at].chars().count(),
                line[at + '•'.len_utf8()..].trim().to_owned(),
            ))
        };
        let cockroach = "distribution: local\n\n• filter\n│ filter: x > 1\n│\n└── • scan\n      table: t@t_pkey";
        let root = plan_from_text(cockroach, bullet);
        assert_eq!(root.operation, "filter");
        assert_eq!(
            root.properties,
            [
                ("distribution".to_owned(), "local".to_owned()),
                ("filter".to_owned(), "x > 1".to_owned())
            ]
        );
        assert_eq!(root.children[0].operation, "scan");
        assert_eq!(root.children[0].properties[0].1, "t@t_pkey");
        assert_eq!(plan_from_text("a\nb", indent).children.len(), 2, "two tops");
    }

    #[test]
    fn suggests_indexes_for_large_filtered_scans() {
        let scan = |table: &str, rows: f64| PlanNode {
            full_scan: Some(FullScan {
                table: table.to_owned(),
                filter: "(total > 10)".to_owned(),
                rows_read: Some(rows),
            }),
            ..PlanNode::new("Seq Scan")
        };
        let join = PlanNode {
            children: vec![scan("small", 50.0), scan("orders", LARGE_SCAN_ROWS)],
            ..PlanNode::new("Hash Join")
        };
        let plan = Explain::new(ExplainFormat::Json, String::new(), join, false);
        let tables: Vec<_> = plan.suggestions.iter().map(|s| s.table.as_str()).collect();
        assert_eq!(tables, ["orders"]);
    }
}
