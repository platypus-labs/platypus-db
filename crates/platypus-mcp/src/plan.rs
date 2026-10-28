//! A query plan as agents read it: an indented tree, one step a line, or JSON. Both stay
//! within the byte limit, like the rows of `run_query`.

use std::fmt::Write as _;
use std::time::Duration;

use platypus_core::{Explain, FullScan, PlanNode};
use serde_json::{Map, Value, json};

use crate::output::{Format, fit};

/// A plan, how long the request took and the bytes it may take.
#[derive(Debug, Clone, PartialEq)]
pub struct PlanResult {
    pub explain: Explain,
    pub elapsed: Duration,
    pub max_bytes: usize,
}

impl PlanResult {
    #[must_use]
    pub fn render(&self, format: Format) -> String {
        match format {
            Format::Text => self.text(),
            Format::Json => self.json().to_string(),
        }
    }

    fn elapsed_ms(&self) -> u64 {
        u64::try_from(self.elapsed.as_millis()).unwrap_or(u64::MAX)
    }

    /// The steps, indented under their parents, then the index suggestions and a line on
    /// the plan: the first lines that fit in the byte limit, saying when some were left out.
    #[must_use]
    pub fn text(&self) -> String {
        let mut lines = Vec::new();
        step_lines(&self.explain.root, 0, &mut lines);
        for scan in &self.explain.suggestions {
            lines.push(suggestion(scan));
        }
        let kind = if self.explain.analyzed {
            "Analyzed plan: the statement ran, and what it changed was rolled back"
        } else {
            "Estimated plan: the statement did not run"
        };
        let render = |shown: usize| {
            let mut out = lines[..shown].join("\n");
            if !out.is_empty() {
                out.push('\n');
            }
            if shown < lines.len() {
                let _ = writeln!(
                    out,
                    "… {} more lines left out: raise max_bytes for the whole plan",
                    lines.len() - shown
                );
            }
            let _ = write!(out, "{kind} ({} ms).", self.elapsed_ms());
            out
        };
        fit(lines.len(), self.max_bytes, render, String::len)
    }

    /// `{analyzed, root, suggestions, elapsed_ms}`, the steps without their unset
    /// figures. Over the byte limit the steps lose their properties; still over it, the
    /// document is `{analyzed, truncated: true, plan}` with the text plan, cut to fit.
    #[must_use]
    pub fn json(&self) -> Value {
        let document = |properties: bool| {
            json!({
                "analyzed": self.explain.analyzed,
                "root": node_json(&self.explain.root, properties),
                "suggestions": self.explain.suggestions,
                "elapsed_ms": self.elapsed_ms(),
            })
        };
        for properties in [true, false] {
            let doc = document(properties);
            if doc.to_string().len() <= self.max_bytes {
                return doc;
            }
        }
        // The wrapper and the escapes of the text take room too: leave them some.
        let text = PlanResult {
            max_bytes: self.max_bytes.saturating_sub(64) * 3 / 4,
            ..self.clone()
        }
        .text();
        json!({ "analyzed": self.explain.analyzed, "truncated": true, "plan": text })
    }
}

/// `n` without a trailing `.0`, and with at most two decimals.
fn figure(n: f64) -> String {
    if n.fract() == 0.0 && n.abs() < 1e15 {
        format!("{n:.0}")
    } else {
        let s = format!("{n:.2}");
        s.trim_end_matches('0').trim_end_matches('.').to_owned()
    }
}

/// The step's line, then its properties, then its children, each level two spaces in.
fn step_lines(node: &PlanNode, depth: usize, lines: &mut Vec<String>) {
    let indent = "  ".repeat(depth);
    let mut line = format!("{indent}-> {}", node.operation);
    if let Some(detail) = &node.detail {
        let _ = write!(line, " on {detail}");
    }
    let figures: Vec<String> = [
        ("cost", node.cost),
        ("rows", node.rows),
        ("actual rows", node.actual_rows),
        ("actual ms", node.actual_ms),
        ("loops", node.loops),
    ]
    .into_iter()
    .filter_map(|(label, value)| value.map(|v| format!("{label}={}", figure(v))))
    .collect();
    if !figures.is_empty() {
        let _ = write!(line, "  ({})", figures.join(", "));
    }
    lines.push(line);
    for (label, value) in &node.properties {
        lines.push(format!("{indent}     {label}: {}", one_line(value)));
    }
    for child in &node.children {
        step_lines(child, depth + 1, lines);
    }
}

fn one_line(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn suggestion(scan: &FullScan) -> String {
    let rows = scan
        .rows_read
        .map(|r| format!(", {} rows read", figure(r)))
        .unwrap_or_default();
    format!(
        "Suggestion: full scan of {} filtered by {}{rows}: an index on the filtered columns may help",
        scan.table,
        one_line(&scan.filter)
    )
}

/// A step as JSON, without the figures it has not got (and without its properties
/// unless `properties`).
fn node_json(node: &PlanNode, properties: bool) -> Value {
    let mut out = Map::new();
    out.insert("operation".to_owned(), json!(node.operation));
    if let Some(detail) = &node.detail {
        out.insert("detail".to_owned(), json!(detail));
    }
    for (key, value) in [
        ("cost", node.cost),
        ("rows", node.rows),
        ("actual_rows", node.actual_rows),
        ("actual_ms", node.actual_ms),
        ("loops", node.loops),
        ("self_cost", node.self_cost),
        ("self_ms", node.self_ms),
    ] {
        if let Some(v) = value {
            out.insert(key.to_owned(), json!(v));
        }
    }
    if properties && !node.properties.is_empty() {
        let props: Map<String, Value> = node
            .properties
            .iter()
            .map(|(k, v)| (k.clone(), json!(v)))
            .collect();
        out.insert("properties".to_owned(), Value::Object(props));
    }
    if !node.children.is_empty() {
        let children = node
            .children
            .iter()
            .map(|c| node_json(c, properties))
            .collect();
        out.insert("children".to_owned(), Value::Array(children));
    }
    Value::Object(out)
}

#[cfg(test)]
mod tests {
    use platypus_core::ExplainFormat;

    use super::*;

    fn plan(max_bytes: usize) -> PlanResult {
        let scan = PlanNode {
            detail: Some("orders".to_owned()),
            cost: Some(1500.0),
            rows: Some(20_000.0),
            properties: vec![("Filter".to_owned(), "(status =\n 'open')".to_owned())],
            full_scan: Some(FullScan {
                table: "orders".to_owned(),
                filter: "status = 'open'".to_owned(),
                rows_read: Some(20_000.0),
            }),
            ..PlanNode::new("Seq Scan")
        };
        let root = PlanNode {
            cost: Some(1600.25),
            rows: Some(10.0),
            children: vec![scan],
            ..PlanNode::new("Sort")
        };
        PlanResult {
            explain: Explain::new(ExplainFormat::Text, String::new(), root, false),
            elapsed: Duration::from_millis(4),
            max_bytes,
        }
    }

    #[test]
    fn text_plans_are_indented_trees() {
        let text = plan(32 * 1024).text();
        assert!(
            text.starts_with("-> Sort  (cost=1600.25, rows=10)\n"),
            "{text}"
        );
        assert!(text.contains("\n  -> Seq Scan on orders  (cost=1500, rows=20000)\n"));
        assert!(text.contains("\n       Filter: (status = 'open')\n"));
        assert!(text.contains(
            "Suggestion: full scan of orders filtered by status = 'open', 20000 rows read"
        ));
        assert!(text.ends_with("Estimated plan: the statement did not run (4 ms)."));
    }

    #[test]
    fn text_plans_keep_to_the_byte_limit() {
        let text = plan(150).text();
        assert!(text.len() <= 150, "{text}");
        assert!(text.starts_with("-> Sort"), "{text}");
        assert!(text.contains("more lines left out"), "{text}");
    }

    #[test]
    fn json_plans_drop_what_does_not_fit() {
        let whole = plan(32 * 1024).json();
        assert_eq!(whole["root"]["operation"], "Sort");
        assert_eq!(
            whole["root"]["children"][0]["properties"]["Filter"],
            "(status =\n 'open')"
        );
        assert_eq!(whole["root"]["children"][0]["self_cost"], 1500.0);
        assert!(whole["root"].get("actual_rows").is_none());
        assert_eq!(whole["suggestions"][0]["table"], "orders");

        let size = whole.to_string().len();
        let lean = plan(size - 1).json();
        assert!(lean["root"]["children"][0].get("properties").is_none());
        assert!(lean.to_string().len() < size);

        let tiny = plan(200).json();
        assert_eq!(tiny["truncated"], true);
        assert!(tiny.to_string().len() <= 200, "{tiny}");
    }

    #[test]
    fn figures_are_short() {
        assert_eq!(figure(3.0), "3");
        assert_eq!(figure(0.126), "0.13");
        assert_eq!(figure(2.5), "2.5");
    }
}
