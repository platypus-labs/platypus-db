//! `EXPLAIN (FORMAT JSON)` as a plan tree; and plans pasted or opened from a file, in JSON or
//! as the text of a plain `EXPLAIN`.

use std::cell::Cell;

use platypus_core::{
    Error, Explain, ExplainFormat, FullScan, PlanNode, Result, explain_document, plain_plan,
    plan_from_text, step_figures,
};
use serde_json::{Map, Value};

use crate::PgConnection;

/// Keys shown as the node's own fields, not as properties.
const SHOWN: &[&str] = &[
    "Node Type",
    "Total Cost",
    "Plan Rows",
    "Actual Rows",
    "Actual Total Time",
    "Actual Loops",
    "Relation Name",
    "CTE Name",
    "Function Name",
    "Alias",
    "Index Name",
    "Plans",
];

pub async fn explain(conn: &PgConnection, sql: &str, analyze: bool) -> Result<Explain> {
    let prefix = if analyze {
        "EXPLAIN (ANALYZE, BUFFERS, FORMAT JSON)"
    } else {
        "EXPLAIN (FORMAT JSON)"
    };
    let plan = explain_document(conn, &format!("{prefix} {sql}"), analyze).await?;
    let root = parse(&plan)?;
    Ok(Explain::new(ExplainFormat::Json, plan, root, analyze))
}

/// CockroachDB's `EXPLAIN [ANALYZE]`: a text tree whose steps start with `•`, their
/// details on the lines below.
pub async fn cockroach(conn: &PgConnection, sql: &str, analyze: bool) -> Result<Explain> {
    let prefix = if analyze {
        "EXPLAIN ANALYZE"
    } else {
        "EXPLAIN"
    };
    let plan = explain_document(conn, &format!("{prefix} {sql}"), analyze).await?;
    let root = cockroach_tree(&plan);
    Ok(Explain::new(ExplainFormat::Text, plan, root, analyze))
}

fn cockroach_tree(plan: &str) -> PlanNode {
    let mut root = plan_from_text(plan, |line| {
        let at = line.find('•')?;
        let operation = line[at + '•'.len_utf8()..].trim().to_owned();
        Some((line[..at].chars().count(), operation))
    });
    cockroach_figures(&mut root);
    root
}

/// A plan pasted or opened from a file: the document of `EXPLAIN (FORMAT JSON)`, the text
/// of a plain `EXPLAIN` as psql or the server's log prints it, or CockroachDB's tree.
///
/// # Errors
/// When the text is none of these.
pub fn read_plan(document: &str) -> Result<Explain> {
    let plan = plain_plan(document);
    let start = plan.trim_start();
    if start.starts_with('[') || start.starts_with('{') {
        // One statement's plan, without the list around it.
        let list = if start.starts_with('{') {
            format!("[{plan}]")
        } else {
            plan.clone()
        };
        let root = parse(&list)?;
        return Ok(Explain::read(ExplainFormat::Json, plan, root));
    }
    let root = if plan.contains('•') {
        cockroach_tree(&plan)
    } else {
        text_plan(&plan)?
    };
    Ok(Explain::read(ExplainFormat::Text, plan, root))
}

/// The text of `EXPLAIN [ANALYZE]`: the first line is the top step, the others `->  step`,
/// nested by indentation, with their properties (`Filter: …`) below them.
fn text_plan(plan: &str) -> Result<PlanNode> {
    let first = Cell::new(true);
    let mut root = plan_from_text(plan, |line| {
        let at = line.find(|c: char| !c.is_whitespace())?;
        let rest = &line[at..];
        let step = match rest.strip_prefix("->") {
            Some(step) => step.trim(),
            None if first.get() => rest.trim(),
            None => return None,
        };
        first.set(false);
        Some((at, step.to_owned()))
    });
    if first.get() {
        return Err(Error::Other("no plan in the text".to_owned()));
    }
    text_step(&mut root);
    // The statement's times end the plan, after the deepest step's properties.
    let mut whole = Vec::new();
    take_statement_times(&mut root, &mut whole);
    root.properties.extend(whole);
    Ok(root)
}

/// The figures, operation and target of a step read as text, and of its children.
fn text_step(node: &mut PlanNode) {
    let text = std::mem::take(&mut node.operation);
    let (description, figures) = step_figures(&text);
    let cut = [" using ", " on "]
        .iter()
        .filter_map(|p| description.find(p))
        .min();
    let (operation, detail) = match cut {
        Some(at) => (&description[..at], Some(description[at + 1..].to_owned())),
        None => (description, None),
    };
    node.operation = operation.to_owned();
    node.detail = detail;
    node.cost = figures.cost;
    node.rows = figures.rows;
    node.actual_ms = figures.actual_ms;
    node.actual_rows = figures.actual_rows;
    node.loops = figures.loops;
    node.full_scan = text_full_scan(node);
    for child in &mut node.children {
        text_step(child);
    }
}

/// As [`full_scan`], from the properties of a step read as text.
fn text_full_scan(node: &PlanNode) -> Option<FullScan> {
    if !node.operation.ends_with("Seq Scan") {
        return None;
    }
    let property = |key: &str| {
        node.properties
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.as_str())
    };
    let table = node.detail.as_deref()?.strip_prefix("on ")?;
    Some(FullScan {
        table: table.split_whitespace().next()?.to_owned(),
        filter: property("Filter")?.to_owned(),
        rows_read: node.actual_rows.map(|kept| {
            let removed = property("Rows Removed by Filter").and_then(|r| r.parse().ok());
            (kept + removed.unwrap_or(0.0)) * node.loops.unwrap_or(1.0)
        }),
    })
}

/// Moves `Planning Time`, `Execution Time` and the like from the steps to `out`.
fn take_statement_times(node: &mut PlanNode, out: &mut Vec<(String, String)>) {
    let (whole, own): (Vec<_>, Vec<_>) = std::mem::take(&mut node.properties)
        .into_iter()
        .partition(|(k, _)| {
            ["Planning", "Execution", "JIT", "Trigger"]
                .iter()
                .any(|p| k.starts_with(p))
        });
    node.properties = own;
    out.extend(whole);
    for child in &mut node.children {
        take_statement_times(child, out);
    }
}

/// The table and the row counts of a CockroachDB step, from its detail lines.
fn cockroach_figures(node: &mut PlanNode) {
    let count = |text: &str| {
        text.split_whitespace()
            .next()
            .and_then(|n| n.replace(',', "").parse::<f64>().ok())
    };
    for (key, value) in &node.properties {
        match key.as_str() {
            "table" => node.detail = Some(value.clone()),
            "estimated row count" => node.rows = count(value),
            "actual row count" => node.actual_rows = count(value),
            _ => {}
        }
    }
    for child in &mut node.children {
        cockroach_figures(child);
    }
}

fn parse(plan: &str) -> Result<PlanNode> {
    let unreadable = || Error::Other("unreadable PostgreSQL plan".to_owned());
    let doc: Value = serde_json::from_str(plan).map_err(|_| unreadable())?;
    let top = doc
        .get(0)
        .and_then(Value::as_object)
        .ok_or_else(unreadable)?;
    let mut root = node(
        top.get("Plan")
            .and_then(Value::as_object)
            .ok_or_else(unreadable)?,
    );
    // Planning and execution times, triggers: the statement as a whole.
    root.properties.extend(
        top.iter()
            .filter(|(k, _)| *k != "Plan")
            .map(|(k, v)| (k.clone(), text(v))),
    );
    Ok(root)
}

fn node(map: &Map<String, Value>) -> PlanNode {
    let num = |k: &str| map.get(k).and_then(Value::as_f64);
    let s = |k: &str| map.get(k).and_then(Value::as_str);
    let mut detail = Vec::new();
    if let Some(index) = s("Index Name") {
        detail.push(format!("using {index}"));
    }
    if let Some(target) = s("Relation Name").or(s("CTE Name")).or(s("Function Name")) {
        detail.push(format!("on {target}"));
        detail.extend(s("Alias").filter(|a| *a != target).map(str::to_owned));
    }
    let operation = s("Node Type").unwrap_or("?");
    PlanNode {
        operation: operation.to_owned(),
        detail: (!detail.is_empty()).then(|| detail.join(" ")),
        cost: num("Total Cost"),
        rows: num("Plan Rows"),
        actual_rows: num("Actual Rows"),
        actual_ms: num("Actual Total Time"),
        loops: num("Actual Loops"),
        full_scan: full_scan(operation, map),
        properties: map
            .iter()
            .filter(|(k, _)| !SHOWN.contains(&k.as_str()))
            .map(|(k, v)| (k.clone(), text(v)))
            .collect(),
        children: map
            .get("Plans")
            .and_then(Value::as_array)
            .map(|plans| {
                plans
                    .iter()
                    .filter_map(Value::as_object)
                    .map(node)
                    .collect()
            })
            .unwrap_or_default(),
        ..PlanNode::default()
    }
}

/// A `Seq Scan` (or `Parallel Seq Scan`) with a filter. With analyze it read the rows it
/// returned plus those the filter removed, per loop. Without analyze PostgreSQL only
/// estimates the rows kept, which says nothing of the rows read.
fn full_scan(operation: &str, map: &Map<String, Value>) -> Option<FullScan> {
    let num = |k: &str| map.get(k).and_then(Value::as_f64);
    let s = |k: &str| map.get(k).and_then(Value::as_str);
    if !operation.ends_with("Seq Scan") {
        return None;
    }
    Some(FullScan {
        table: s("Relation Name")?.to_owned(),
        filter: s("Filter")?.to_owned(),
        rows_read: num("Actual Rows").map(|kept| {
            (kept + num("Rows Removed by Filter").unwrap_or(0.0))
                * num("Actual Loops").unwrap_or(1.0)
        }),
    })
}

/// A value as shown: strings unquoted, lists (sort keys, output columns) comma-separated.
fn text(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Array(items) => items.iter().map(text).collect::<Vec<_>>().join(", "),
        other => other.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TEXT: &str = " Hash Join  (cost=1.09..2.19 rows=4 width=40) (actual time=0.040..0.045 rows=4 loops=1)
   Hash Cond: (o.customer_id = c.id)
   ->  Seq Scan on orders o  (cost=0.00..1.06 rows=6 width=12) (actual time=0.008..0.009 rows=5 loops=1)
         Filter: (status = 'paid'::text)
         Rows Removed by Filter: 20000
   ->  Hash  (cost=1.04..1.04 rows=4 width=36) (never executed)
         ->  Index Scan using customers_pkey on customers c  (cost=0.15..8.17 rows=1 width=36)
 Planning Time: 0.120 ms
 Execution Time: 0.081 ms";

    #[test]
    fn reads_the_text_of_a_plan() {
        let explain = read_plan(TEXT).unwrap();
        assert_eq!(explain.format, ExplainFormat::Text);
        assert!(explain.analyzed);
        let root = &explain.root;
        assert_eq!(
            (root.operation.as_str(), root.detail.as_deref()),
            ("Hash Join", None)
        );
        assert_eq!((root.cost, root.actual_ms), (Some(2.19), Some(0.045)));
        assert_eq!(
            root.properties,
            [
                ("Hash Cond".to_owned(), "(o.customer_id = c.id)".to_owned()),
                ("Planning Time".to_owned(), "0.120 ms".to_owned()),
                ("Execution Time".to_owned(), "0.081 ms".to_owned()),
            ]
        );
        let [scan, hash] = &root.children[..] else {
            panic!("{:?}", root.children)
        };
        assert_eq!(
            (scan.operation.as_str(), scan.detail.as_deref()),
            ("Seq Scan", Some("on orders o"))
        );
        let full = scan.full_scan.as_ref().unwrap();
        assert_eq!(
            (full.table.as_str(), full.filter.as_str(), full.rows_read),
            ("orders", "(status = 'paid'::text)", Some(20005.0))
        );
        assert_eq!(explain.suggestions.len(), 1);
        assert_eq!(hash.loops, Some(0.0));
        let index = &hash.children[0];
        assert_eq!(
            (index.operation.as_str(), index.detail.as_deref()),
            ("Index Scan", Some("using customers_pkey on customers c"))
        );
    }

    #[test]
    fn reads_pasted_json_and_cockroach_plans() {
        let json = r#"{"Plan": {"Node Type": "Seq Scan", "Relation Name": "t", "Total Cost": 3}}"#;
        let explain = read_plan(json).unwrap();
        assert_eq!(
            (explain.format, explain.analyzed),
            (ExplainFormat::Json, false)
        );
        assert_eq!(explain.root.cost, Some(3.0));
        let cockroach = "distribution: local\n• scan\n  estimated row count: 5\n  table: t@t_pkey";
        let explain = read_plan(cockroach).unwrap();
        assert_eq!(explain.root.operation, "scan");
        assert_eq!(explain.root.detail.as_deref(), Some("t@t_pkey"));
        // A plan without costs is still a plan.
        let bare = read_plan("Seq Scan on t\n  Filter: (a > 1)").unwrap();
        assert_eq!(bare.root.operation, "Seq Scan");
        assert!(read_plan("").is_err());
        assert!(read_plan("{ not json").is_err());
    }

    #[test]
    fn reads_an_analyzed_plan() {
        let plan = r#"[{"Plan": {"Node Type": "Hash Join", "Join Type": "Inner",
            "Total Cost": 35.5, "Plan Rows": 10, "Actual Rows": 3, "Actual Total Time": 0.25,
            "Actual Loops": 1, "Hash Cond": "(o.customer_id = c.id)", "Plans": [
              {"Node Type": "Seq Scan", "Relation Name": "orders", "Alias": "o",
               "Total Cost": 20, "Plan Rows": 1000, "Filter": "(total > 10)"},
              {"Node Type": "Index Scan", "Index Name": "customers_pkey",
               "Relation Name": "customers", "Alias": "customers", "Output": ["id", "name"]}]},
            "Planning Time": 0.1, "Execution Time": 0.3}]"#;
        let root = parse(plan).unwrap();
        assert_eq!(root.operation, "Hash Join");
        assert_eq!(root.detail, None);
        assert_eq!((root.cost, root.rows), (Some(35.5), Some(10.0)));
        assert_eq!((root.actual_rows, root.loops), (Some(3.0), Some(1.0)));
        assert!(
            root.properties
                .contains(&("Join Type".into(), "Inner".into()))
        );
        assert!(
            root.properties
                .contains(&("Execution Time".into(), "0.3".into()))
        );
        let [scan, index] = &root.children[..] else {
            panic!("two children: {:?}", root.children)
        };
        assert_eq!(scan.detail.as_deref(), Some("on orders o"));
        assert_eq!(scan.actual_rows, None);
        let full = scan.full_scan.as_ref().unwrap();
        assert_eq!((full.table.as_str(), full.rows_read), ("orders", None));
        assert_eq!(index.full_scan, None);
        assert!(
            scan.properties
                .contains(&("Filter".into(), "(total > 10)".into()))
        );
        assert_eq!(
            index.detail.as_deref(),
            Some("using customers_pkey on customers")
        );
        assert!(
            index
                .properties
                .contains(&("Output".into(), "id, name".into()))
        );
    }

    #[test]
    fn refuses_what_is_not_a_plan() {
        assert!(parse("[]").is_err());
        assert!(parse("QUERY PLAN").is_err());
    }
}
