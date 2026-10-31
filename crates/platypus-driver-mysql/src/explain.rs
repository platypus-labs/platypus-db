//! Plans as trees: MySQL's `EXPLAIN FORMAT=TREE` text (8.0.16+, and `EXPLAIN ANALYZE`,
//! which has no JSON form before 8.3), MariaDB's `EXPLAIN`/`ANALYZE FORMAT=JSON` and older
//! MySQL's `EXPLAIN FORMAT=JSON`, close enough to MariaDB's to read alike.

use platypus_core::{
    Connection, Error, Explain, ExplainFormat, FullScan, PlanNode, Result, ServerVersion,
    explain_document, plain_plan, step_figures,
};
use serde_json::{Map, Value};

use crate::MySqlConnection;

/// MySQL's first versions with `EXPLAIN FORMAT=TREE` and `EXPLAIN ANALYZE`.
const TREE: ServerVersion = ServerVersion::new(8, 0, 16);
const ANALYZE: ServerVersion = ServerVersion::new(8, 0, 18);

pub async fn explain(conn: &MySqlConnection, sql: &str, analyze: bool) -> Result<Explain> {
    let info = conn.server_info();
    let mariadb = info.product == "MariaDB";
    if analyze && !crate::mysql_has(info, ANALYZE) {
        return Err(Error::Unsupported(format!(
            "EXPLAIN ANALYZE needs MySQL {ANALYZE} or later; this server is {}",
            info.version
        )));
    }
    // Before the tree, MySQL writes its plans in JSON only, much like MariaDB.
    let json = mariadb || !crate::mysql_has(info, TREE);
    let prefix = match (mariadb, analyze) {
        (false, false) if json => "EXPLAIN FORMAT=JSON",
        (false, false) => "EXPLAIN FORMAT=TREE",
        (false, true) => "EXPLAIN ANALYZE",
        (true, false) => "EXPLAIN FORMAT=JSON",
        (true, true) => "ANALYZE FORMAT=JSON",
    };
    let mut plan = explain_document(conn, &format!("{prefix} {sql}"), analyze).await?;
    let (mut json, mut analyze) = (json, analyze);
    // MySQL has no tree for a change (UPDATE, DELETE), analyzed or not: it answers
    // `-> <not executable by iterator executor>`. Its estimated plan in JSON stands in, and
    // the result says it is not analyzed.
    if !json && !has_steps(&plan) {
        (json, analyze) = (true, false);
        plan = explain_document(conn, &format!("EXPLAIN FORMAT=JSON {sql}"), false).await?;
    }
    let (format, root) = if json {
        (ExplainFormat::Json, json_plan(&plan)?)
    } else {
        (ExplainFormat::Text, tree_plan(&plan))
    };
    Ok(Explain::new(format, plan, root, analyze))
}

/// Whether a tree plan has steps (`-> …`), not a message of the server in their place
/// (`-> <not executable by iterator executor>`).
fn has_steps(plan: &str) -> bool {
    plan.lines().any(|l| {
        l.trim_start()
            .strip_prefix("-> ")
            .is_some_and(|step| !step.trim_end().starts_with('<'))
    })
}

/// A plan pasted or opened from a file: MySQL's tree (`EXPLAIN FORMAT=TREE` or
/// `EXPLAIN ANALYZE`, as the client prints it) or MariaDB's JSON.
///
/// # Errors
/// When the text is neither.
pub fn read_plan(document: &str) -> Result<Explain> {
    let plan = plain_plan(document);
    if plan.trim_start().starts_with('{') {
        let root = json_plan(&plan)?;
        return Ok(Explain::read(ExplainFormat::Json, plan, root));
    }
    if !has_steps(&plan) {
        return Err(Error::Other("no plan in the text".to_owned()));
    }
    let root = tree_plan(&plan);
    Ok(Explain::read(ExplainFormat::Text, plan, root))
}

/// Reads the `-> step  (cost=… rows=…) (actual time=…..… rows=… loops=…)` lines, nested
/// by indentation.
fn tree_plan(plan: &str) -> PlanNode {
    // Open steps with their indentation, innermost last.
    let mut open: Vec<(usize, PlanNode)> = Vec::new();
    let mut roots = Vec::new();
    let close = |open: &mut Vec<(usize, PlanNode)>, roots: &mut Vec<PlanNode>| {
        if let Some((_, node)) = open.pop() {
            match open.last_mut() {
                Some((_, parent)) => parent.children.push(node),
                None => roots.push(node),
            }
        }
    };
    for line in plan.lines() {
        let trimmed = line.trim_start();
        let indent = line.len() - trimmed.len();
        let Some(step) = trimmed.strip_prefix("-> ") else {
            // A description that goes on to the next line.
            if let Some((_, node)) = open.last_mut().filter(|_| !trimmed.is_empty()) {
                node.operation.push(' ');
                node.operation.push_str(trimmed);
            }
            continue;
        };
        while open.last().is_some_and(|(i, _)| *i >= indent) {
            close(&mut open, &mut roots);
        }
        open.push((indent, tree_step(step)));
    }
    while !open.is_empty() {
        close(&mut open, &mut roots);
    }
    let mut root = single_root(roots);
    mark_scans(&mut root);
    root
}

/// A `Filter` right over a `Table scan on t` is a filtered full scan of `t`.
fn mark_scans(node: &mut PlanNode) {
    if node.operation == "Filter"
        && let Some(filter) = node.detail.clone()
        && let [scan] = &mut node.children[..]
        && let Some(table) = scan.operation.strip_prefix("Table scan on ")
        // `<temporary>` and the like are the query's own.
        && !table.starts_with('<')
    {
        scan.full_scan = Some(FullScan {
            table: table.split_whitespace().next().unwrap_or(table).to_owned(),
            filter,
            rows_read: rows_read(scan.actual_rows, scan.loops, scan.rows),
        });
    }
    for child in &mut node.children {
        mark_scans(child);
    }
}

/// Rows a scan read over all its loops, or the planner's estimate without analyze.
fn rows_read(actual: Option<f64>, loops: Option<f64>, estimate: Option<f64>) -> Option<f64> {
    actual.map(|rows| rows * loops.unwrap_or(1.0)).or(estimate)
}

fn tree_step(step: &str) -> PlanNode {
    let (description, figures) = step_figures(step);
    let (operation, detail) = match description.split_once(": ") {
        Some((operation, detail)) => (operation, Some(detail.to_owned())),
        None => (description, None),
    };
    PlanNode {
        operation: operation.to_owned(),
        detail,
        ..figures
    }
}

/// Reads MariaDB's JSON plan: objects nest as steps, lists of steps (`nested_loop`,
/// `subqueries`) become a step holding them, and other values are properties.
fn json_plan(plan: &str) -> Result<PlanNode> {
    let unreadable = || Error::Other("unreadable MariaDB plan".to_owned());
    let doc: Value = serde_json::from_str(plan).map_err(|_| unreadable())?;
    let Value::Object(map) = doc else {
        return Err(unreadable());
    };
    Ok(single_root(children(&map)))
}

/// Keys shown as the node's own fields, not as properties.
const SHOWN: &[&str] = &[
    "access_type",
    "table_name",
    "cost",
    "query_cost",
    "cost_info",
    "rows",
    "rows_examined_per_scan",
    "r_rows",
    "r_loops",
    "r_total_time_ms",
];

/// A number, as MariaDB writes it or as MySQL does in a string (`"51.93"`).
fn number(v: &Value) -> Option<f64> {
    v.as_f64().or_else(|| v.as_str()?.parse().ok())
}

/// The rows a step reads: MariaDB's `rows`, MySQL's `rows_examined_per_scan`.
fn step_rows(map: &Map<String, Value>) -> Option<f64> {
    map.get("rows")
        .or_else(|| map.get("rows_examined_per_scan"))
        .and_then(number)
}

/// The cost of a step: MariaDB's `cost` or `query_cost`, MySQL's in `cost_info` (the
/// query's, else the step's with those before it).
fn step_cost(map: &Map<String, Value>) -> Option<f64> {
    let num = |m: &Map<String, Value>, k: &str| m.get(k).and_then(number);
    num(map, "cost")
        .or_else(|| num(map, "query_cost"))
        .or_else(|| {
            let info = map.get("cost_info")?.as_object()?;
            num(info, "query_cost").or_else(|| num(info, "prefix_cost"))
        })
}

fn json_step(key: &str, map: &Map<String, Value>) -> PlanNode {
    let num = |k: &str| map.get(k).and_then(Value::as_f64);
    let s = |k: &str| map.get(k).and_then(Value::as_str);
    let loops = num("r_loops");
    PlanNode {
        // A table's access type (`ALL`, `ref`, `range`) is what its step does.
        operation: s("access_type").map_or_else(|| key.replace('_', " "), str::to_owned),
        detail: s("table_name").map(str::to_owned),
        cost: step_cost(map),
        rows: step_rows(map),
        actual_rows: num("r_rows"),
        // MariaDB reports the time of all loops; a table's as reading plus checking rows.
        actual_ms: num("r_total_time_ms")
            .or_else(|| Some(num("r_table_time_ms")? + num("r_other_time_ms").unwrap_or(0.0)))
            .map(|ms| ms / loops.filter(|l| *l > 0.0).unwrap_or(1.0)),
        loops,
        properties: map
            .iter()
            .filter(|(k, v)| !SHOWN.contains(&k.as_str()) && !is_step(k, v))
            .map(|(k, v)| (k.clone(), text(v)))
            .collect(),
        children: children(map),
        full_scan: full_scan(map),
        ..PlanNode::default()
    }
}

/// A table read in full (`ALL`) with a condition on its rows.
fn full_scan(map: &Map<String, Value>) -> Option<FullScan> {
    let s = |k: &str| map.get(k).and_then(Value::as_str);
    let num = |k: &str| map.get(k).and_then(Value::as_f64);
    if s("access_type")? != "ALL" {
        return None;
    }
    Some(FullScan {
        table: s("table_name")?.to_owned(),
        filter: s("attached_condition")?.to_owned(),
        rows_read: rows_read(num("r_rows"), num("r_loops"), step_rows(map)),
    })
}

fn children(map: &Map<String, Value>) -> Vec<PlanNode> {
    map.iter()
        .filter(|(key, value)| is_step(key, value))
        .filter_map(|(key, value)| match value {
            Value::Object(inner) => Some(json_step(key, inner)),
            Value::Array(items) => Some(PlanNode {
                children: items
                    .iter()
                    .filter_map(Value::as_object)
                    .flat_map(|item| match single_entry(item) {
                        Some((k, Value::Object(inner))) => vec![json_step(k, inner)],
                        _ => children(item),
                    })
                    .collect(),
                ..PlanNode::new(key.replace('_', " "))
            }),
            _ => None,
        })
        .collect()
}

fn single_entry(map: &Map<String, Value>) -> Option<(&String, &Value)> {
    let mut entries = map.iter();
    let first = entries.next()?;
    entries.next().is_none().then_some(first)
}

/// Objects and lists of objects are steps, except ANALYZE's statistics (`r_engine_stats`)
/// and MySQL's costs (`cost_info`); anything else is a property.
fn is_step(key: &str, v: &Value) -> bool {
    if key.starts_with("r_") || key == "cost_info" {
        return false;
    }
    match v {
        Value::Object(_) => true,
        Value::Array(items) => items.iter().any(Value::is_object),
        _ => false,
    }
}

fn text(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Array(items) => items.iter().map(text).collect::<Vec<_>>().join(", "),
        other => other.to_string(),
    }
}

fn single_root(mut roots: Vec<PlanNode>) -> PlanNode {
    if roots.len() == 1 {
        roots.remove(0)
    } else {
        PlanNode {
            children: roots,
            ..PlanNode::new("plan")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_pasted_plans() {
        let client = "+----------+\n| EXPLAIN |\n+----------+\n| -> Filter: (t.a = 1)  (cost=1.25 rows=1) (actual time=0.03..0.04 rows=1 loops=1)\n    -> Table scan on t  (cost=1.25 rows=10) (actual time=0.02..0.03 rows=10 loops=1)\n |\n+----------+";
        let explain = read_plan(client).unwrap();
        assert!(explain.analyzed);
        assert_eq!(explain.root.operation, "Filter");
        assert_eq!(explain.root.children[0].operation, "Table scan on t");
        assert_eq!(explain.root.children[0].actual_rows, Some(10.0));
        let mariadb = r#"{"query_block": {"select_id": 1, "table": {"table_name": "t", "access_type": "ALL", "rows": 10}}}"#;
        let explain = read_plan(mariadb).unwrap();
        assert_eq!(explain.format, ExplainFormat::Json);
        assert!(read_plan("SELECT 1").is_err());
    }

    #[test]
    fn reads_a_mysql_json_plan() {
        // MySQL 5.7's EXPLAIN FORMAT=JSON: costs in `cost_info`, as strings.
        let plan = r#"{"query_block": {"select_id": 1, "cost_info": {"query_cost": "51.93"},
            "nested_loop": [
              {"table": {"table_name": "c", "access_type": "ALL", "rows_examined_per_scan": 2,
                "rows_produced_per_join": 2, "filtered": "100.00",
                "cost_info": {"read_cost": "1.00", "eval_cost": "0.40", "prefix_cost": "1.40"},
                "used_columns": ["id", "email"], "attached_condition": "(`c`.`id` > 1)"}},
              {"table": {"table_name": "o", "access_type": "ref", "key": "orders_customer_id_idx",
                "rows_examined_per_scan": 21,
                "cost_info": {"read_cost": "42.11", "prefix_cost": "51.93"}}}]}}"#;
        let root = read_plan(plan).unwrap().root;
        assert_eq!(root.operation, "query block");
        assert_eq!(root.cost, Some(51.93));
        let [loop_] = &root.children[..] else {
            panic!(
                "only the nested loop, no cost info step: {:?}",
                root.children
            )
        };
        let [c, o] = &loop_.children[..] else {
            panic!("{:?}", loop_.children)
        };
        assert_eq!(
            (c.operation.as_str(), c.rows, c.cost),
            ("ALL", Some(2.0), Some(1.4))
        );
        assert_eq!((o.detail.as_deref(), o.rows), (Some("o"), Some(21.0)));
        let scan = c.full_scan.as_ref().unwrap();
        assert_eq!((scan.table.as_str(), scan.rows_read), ("c", Some(2.0)));
    }

    #[test]
    fn reads_an_analyzed_tree() {
        let plan =
            "-> Nested loop inner join  (cost=1.05 rows=2) (actual time=0.05..0.08 rows=2 loops=1)
    -> Filter: (a.id > 1)  (cost=0.45 rows=2) (actual time=0.02..0.03 rows=2 loops=1)
        -> Table scan on a  (cost=0.45 rows=3) (actual time=0.01..0.02 rows=3 loops=1)
    -> Single-row index lookup on b using PRIMARY (id=a.id)  (cost=0.3 rows=1) (never executed)
";
        let root = tree_plan(plan);
        assert_eq!(root.operation, "Nested loop inner join");
        assert_eq!((root.cost, root.rows), (Some(1.05), Some(2.0)));
        assert_eq!(
            (root.actual_ms, root.actual_rows, root.loops),
            (Some(0.08), Some(2.0), Some(1.0))
        );
        let [filter, lookup] = &root.children[..] else {
            panic!("two children: {:?}", root.children)
        };
        assert_eq!(filter.operation, "Filter");
        assert_eq!(filter.detail.as_deref(), Some("(a.id > 1)"));
        assert_eq!(filter.children[0].operation, "Table scan on a");
        assert_eq!(filter.children[0].rows, Some(3.0));
        let scan = filter.children[0].full_scan.as_ref().unwrap();
        assert_eq!(
            (scan.table.as_str(), scan.filter.as_str(), scan.rows_read),
            ("a", "(a.id > 1)", Some(3.0))
        );
        assert_eq!(
            lookup.operation,
            "Single-row index lookup on b using PRIMARY (id=a.id)"
        );
        assert_eq!((lookup.loops, lookup.actual_rows), (Some(0.0), None));
    }

    #[test]
    fn reads_a_plain_tree_with_several_top_steps() {
        let root = tree_plan(
            "-> Rows fetched before execution  (cost=0..0 rows=1)\n-> Select #2 (subquery in projection; run only once)\n    -> Table scan on t\n",
        );
        assert_eq!(root.operation, "plan");
        assert_eq!(root.children[0].cost, Some(0.0));
        assert_eq!(root.children[1].children[0].operation, "Table scan on t");
    }

    #[test]
    fn reads_a_mariadb_analyze() {
        let plan = r#"{"query_block": {"select_id": 1, "cost": 0.03, "r_loops": 1,
            "r_total_time_ms": 0.1, "nested_loop": [
              {"table": {"table_name": "a", "access_type": "ALL", "rows": 3, "r_loops": 2,
                "r_rows": 3, "r_table_time_ms": 0.75, "r_other_time_ms": 0.25,
                "r_engine_stats": {"pages_accessed": 2}, "attached_condition": "a.id > 1"}},
              {"table": {"table_name": "b", "access_type": "eq_ref",
                "possible_keys": ["PRIMARY"], "rows": 1}}]}}"#;
        let root = json_plan(plan).unwrap();
        assert_eq!(root.operation, "query block");
        assert_eq!(root.cost, Some(0.03));
        assert!(root.properties.contains(&("select_id".into(), "1".into())));
        let join = &root.children[0];
        assert_eq!(join.operation, "nested loop");
        let [a, b] = &join.children[..] else {
            panic!("two tables: {:?}", join.children)
        };
        assert_eq!(
            (a.operation.as_str(), a.detail.as_deref()),
            ("ALL", Some("a"))
        );
        assert_eq!(
            (a.actual_rows, a.loops, a.actual_ms),
            (Some(3.0), Some(2.0), Some(0.5))
        );
        // Statistics are a property, not a step.
        assert_eq!(a.children, Vec::new());
        assert!(a.properties.iter().any(|(k, _)| k == "r_engine_stats"));
        assert_eq!(
            a.full_scan
                .as_ref()
                .map(|s| (s.table.as_str(), s.rows_read)),
            Some(("a", Some(6.0)))
        );
        assert_eq!(b.full_scan, None);
        assert!(
            a.properties
                .contains(&("attached_condition".into(), "a.id > 1".into()))
        );
        assert!(
            b.properties
                .contains(&("possible_keys".into(), "PRIMARY".into()))
        );
    }

    #[test]
    fn a_message_of_the_server_is_not_a_plan() {
        assert!(!has_steps("<not executable by iterator executor>\n"));
        // As MySQL 8.4 writes it for an UPDATE, with EXPLAIN ANALYZE and FORMAT=TREE alike.
        assert!(!has_steps("-> <not executable by iterator executor>\n"));
        // The JSON plan that stands in reads as a step on the table changed.
        let update = r#"{"query_block": {"select_id": 1, "table": {"update": true,
            "table_name": "t", "access_type": "range", "key": "PRIMARY",
            "rows_examined_per_scan": 1, "attached_condition": "(`p`.`t`.`id` < 3)"}}}"#;
        let root = json_plan(update).unwrap();
        let text = format!("{root:?}");
        assert!(text.contains("\"t\""), "{text}");
        assert!(!text.contains("not executable"), "{text}");
        assert!(has_steps("-> Table scan on t  (cost=1 rows=1)\n"));
        assert!(has_steps("  -> Filter: (t.a > 1)\n"));
    }
}
