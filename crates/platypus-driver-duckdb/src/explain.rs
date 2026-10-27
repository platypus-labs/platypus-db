//! `EXPLAIN (FORMAT JSON)` as a plan tree.

use platypus_core::{
    Connection, Error, ExecOptions, Explain, ExplainFormat, PlanNode, Result, query_text,
};
use serde_json::Value;

pub async fn explain(conn: &dyn Connection, sql: &str, analyze: bool) -> Result<Explain> {
    if !analyze {
        let plan = plan_text(conn, &format!("EXPLAIN (FORMAT JSON) {sql}")).await?;
        return Ok(Explain::new(
            ExplainFormat::Json,
            plan.clone(),
            parse(&plan)?,
            false,
        ));
    }
    // The statement runs: what it changes is rolled back. DuckDB has no savepoints to do
    // that inside the user's own transaction.
    if conn.transaction().open {
        return Err(Error::Unsupported(
            "Explain analyze in an open transaction on DuckDB: commit or roll back first"
                .to_owned(),
        ));
    }
    // `BEGIN` and `ROLLBACK` return no rows: not catalog queries.
    let options = ExecOptions::default();
    conn.execute("BEGIN", &options).await?;
    let plan = plan_text(conn, &format!("EXPLAIN (ANALYZE, FORMAT JSON) {sql}")).await;
    let undone = conn.execute("ROLLBACK", &options).await;
    let plan = plan?;
    undone?;
    Ok(Explain::new(
        ExplainFormat::Json,
        plan.clone(),
        parse(&plan)?,
        true,
    ))
}

/// The plan document: the last column of the plan request's row.
async fn plan_text(conn: &dyn Connection, request: &str) -> Result<String> {
    query_text(conn, request)
        .await?
        .into_iter()
        .find_map(|row| row.into_iter().last().flatten())
        .ok_or_else(|| Error::Other("the database returned no plan".to_owned()))
}

fn parse(plan: &str) -> Result<PlanNode> {
    let doc: Value = serde_json::from_str(plan)
        .map_err(|_| Error::Other("unreadable DuckDB plan".to_owned()))?;
    // A list of steps; with analyze, a query whose `children` (`operator` in newer releases)
    // are the steps.
    let tops = match &doc {
        Value::Array(steps) => steps.clone(),
        Value::Object(query) => ["operator", "children"]
            .iter()
            .find_map(|k| query.get(*k).and_then(Value::as_array))
            .cloned()
            .unwrap_or_default(),
        _ => Vec::new(),
    };
    let mut nodes: Vec<PlanNode> = tops.iter().map(node).collect();
    // The steps that run the plan and collect its result are not the query's.
    while let [only] = nodes.as_slice()
        && matches!(
            only.operation.as_str(),
            "RESULT_COLLECTOR" | "EXECUTE" | "EXPLAIN_ANALYZE"
        )
    {
        nodes = nodes.remove(0).children;
    }
    Ok(if nodes.len() == 1 {
        nodes.remove(0)
    } else {
        PlanNode {
            children: nodes,
            ..PlanNode::new("Plan")
        }
    })
}

fn text(value: &Value) -> String {
    match value {
        Value::String(s) => s.clone(),
        Value::Array(items) => items.iter().map(text).collect::<Vec<_>>().join(", "),
        other => other.to_string(),
    }
}

fn node(step: &Value) -> PlanNode {
    let name = ["name", "operator_name", "operator_type", "type"]
        .iter()
        .find_map(|k| step.get(*k).and_then(Value::as_str))
        .unwrap_or("?")
        .trim()
        .to_owned();
    let mut node = PlanNode::new(name);
    match step.get("extra_info") {
        Some(Value::Object(info)) => {
            for (key, value) in info {
                match key.as_str() {
                    "Estimated Cardinality" => node.rows = text(value).trim().parse().ok(),
                    "Table" => node.detail = Some(text(value)),
                    _ => {}
                }
                node.properties.push((key.clone(), text(value)));
            }
        }
        Some(Value::String(info)) if !info.trim().is_empty() => {
            node.properties
                .push((String::new(), info.trim().to_owned()));
        }
        _ => {}
    }
    // The figures of an analyzed plan, under their older and newer names.
    let figure = |keys: [&str; 2]| keys.iter().find_map(|k| step.get(*k)?.as_f64());
    node.actual_rows = figure(["operator_cardinality", "intermediate_rows"]);
    node.actual_ms = figure(["operator_timing", "timing"]).map(|seconds| seconds * 1000.0);
    node.children = step
        .get("children")
        .and_then(Value::as_array)
        .map(|children| children.iter().map(self::node).collect())
        .unwrap_or_default();
    node
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_plans_and_analyzed_plans() {
        let plan = r#"[{"name": "PROJECTION ", "children": [{"name": "RANGE", "children": [],
            "extra_info": {"Function": "RANGE", "Estimated Cardinality": "10"}}],
            "extra_info": {"Projections": "range"}}]"#;
        let root = parse(plan).unwrap();
        assert_eq!(root.operation, "PROJECTION");
        assert_eq!(root.children[0].rows, Some(10.0));
        let analyzed = r#"{"query_name": "x", "children": [{"operator_name": "FILTER",
            "operator_timing": 0.002, "operator_cardinality": 8, "children": [],
            "extra_info": {"Expressions": ["range > 1"]}}]}"#;
        let root = parse(analyzed).unwrap();
        assert_eq!(root.operation, "FILTER");
        assert_eq!((root.actual_rows, root.actual_ms), (Some(8.0), Some(2.0)));
        assert_eq!(root.properties[0].1, "range > 1");
        // Newer releases: `operator`, `type`, `timing` and `intermediate_rows`, under the
        // steps that run the plan.
        let analyzed = r#"{"operator": [{"type": "RESULT_COLLECTOR", "timing": 0.0,
            "intermediate_rows": 0, "children": [{"type": "EXPLAIN_ANALYZE", "children": [
            {"type": "TABLE_SCAN", "timing": 0.004, "intermediate_rows": 1,
            "extra_info": {"Table": "memory.main.t", "Estimated Cardinality": "1"}}]}]}]}"#;
        let root = parse(analyzed).unwrap();
        assert_eq!(root.operation, "TABLE_SCAN");
        assert_eq!(root.detail.as_deref(), Some("memory.main.t"));
        assert_eq!((root.actual_rows, root.actual_ms), (Some(1.0), Some(4.0)));
        assert!(parse("not json").is_err());
    }
}
