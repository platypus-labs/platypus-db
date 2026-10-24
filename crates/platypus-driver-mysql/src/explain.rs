//! Plans as trees: MySQL's `EXPLAIN FORMAT=TREE` text (8.0.16+, and `EXPLAIN ANALYZE`,
//! which has no JSON form before 8.3), MariaDB's `EXPLAIN`/`ANALYZE FORMAT=JSON`.

use platypus_core::{
    Connection, Error, Explain, ExplainFormat, PlanNode, Result, explain_document,
};
use serde_json::{Map, Value};

use crate::MySqlConnection;

pub async fn explain(conn: &MySqlConnection, sql: &str, analyze: bool) -> Result<Explain> {
    let mariadb = conn.server_info().product == "MariaDB";
    let prefix = match (mariadb, analyze) {
        (false, false) => "EXPLAIN FORMAT=TREE",
        (false, true) => "EXPLAIN ANALYZE",
        (true, false) => "EXPLAIN FORMAT=JSON",
        (true, true) => "ANALYZE FORMAT=JSON",
    };
    let plan = explain_document(conn, &format!("{prefix} {sql}"), analyze).await?;
    let (format, root) = if mariadb {
        (ExplainFormat::Json, json_plan(&plan)?)
    } else {
        (ExplainFormat::Text, tree_plan(&plan))
    };
    Ok(Explain {
        format,
        plan,
        root,
        analyzed: analyze,
    })
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
    single_root(roots)
}

fn tree_step(step: &str) -> PlanNode {
    let cut = ["(cost=", "(actual ", "(never executed)"]
        .iter()
        .filter_map(|p| step.find(p))
        .min()
        .unwrap_or(step.len());
    let (description, figures) = step.split_at(cut);
    let description = description.trim();
    let mut node = match description.split_once(": ") {
        Some((operation, detail)) => PlanNode {
            detail: Some(detail.to_owned()),
            ..PlanNode::new(operation)
        },
        None => PlanNode::new(description),
    };
    for group in figures.split(')') {
        let group = group.trim().trim_start_matches('(');
        if group == "never executed" {
            node.loops = Some(0.0);
        }
        let actual = group.starts_with("actual");
        for (key, value) in group.split_whitespace().filter_map(|t| t.split_once('=')) {
            // `time=first..last`: the time to the last row.
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
    node
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
    "rows",
    "r_rows",
    "r_loops",
    "r_total_time_ms",
];

fn json_step(key: &str, map: &Map<String, Value>) -> PlanNode {
    let num = |k: &str| map.get(k).and_then(Value::as_f64);
    let s = |k: &str| map.get(k).and_then(Value::as_str);
    let loops = num("r_loops");
    PlanNode {
        // A table's access type (`ALL`, `ref`, `range`) is what its step does.
        operation: s("access_type").map_or_else(|| key.replace('_', " "), str::to_owned),
        detail: s("table_name").map(str::to_owned),
        cost: num("cost").or(num("query_cost")),
        rows: num("rows"),
        actual_rows: num("r_rows"),
        // MariaDB reports the time of all loops.
        actual_ms: num("r_total_time_ms").map(|ms| ms / loops.filter(|l| *l > 0.0).unwrap_or(1.0)),
        loops,
        properties: map
            .iter()
            .filter(|(k, v)| !SHOWN.contains(&k.as_str()) && !is_step(v))
            .map(|(k, v)| (k.clone(), text(v)))
            .collect(),
        children: children(map),
    }
}

fn children(map: &Map<String, Value>) -> Vec<PlanNode> {
    map.iter()
        .filter_map(|(key, value)| match value {
            Value::Object(inner) => Some(json_step(key, inner)),
            Value::Array(items) if is_step(value) => Some(PlanNode {
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

/// Objects and lists of objects are steps; anything else is a property.
fn is_step(v: &Value) -> bool {
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
                "r_rows": 3, "r_total_time_ms": 0.04, "attached_condition": "a.id > 1"}},
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
            (Some(3.0), Some(2.0), Some(0.02))
        );
        assert!(
            a.properties
                .contains(&("attached_condition".into(), "a.id > 1".into()))
        );
        assert!(
            b.properties
                .contains(&("possible_keys".into(), "PRIMARY".into()))
        );
    }
}
