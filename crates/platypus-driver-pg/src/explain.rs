//! `EXPLAIN (FORMAT JSON)` as a plan tree.

use platypus_core::{Error, Explain, ExplainFormat, PlanNode, Result, explain_document};
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
    Ok(Explain {
        format: ExplainFormat::Json,
        plan,
        root,
        analyzed: analyze,
    })
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
    PlanNode {
        operation: s("Node Type").unwrap_or("?").to_owned(),
        detail: (!detail.is_empty()).then(|| detail.join(" ")),
        cost: num("Total Cost"),
        rows: num("Plan Rows"),
        actual_rows: num("Actual Rows"),
        actual_ms: num("Actual Total Time"),
        loops: num("Actual Loops"),
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
    }
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
