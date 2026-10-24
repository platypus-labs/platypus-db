//! A statement's result as agents read it: an aligned text table, or JSON whose shape is
//! stable (documented in `docs/EXTENDING.md`). Both stay within the byte limit and say
//! when rows or values were left out.

use std::fmt::Write as _;
use std::time::Duration;

use serde::Deserialize;
use serde_json::{Value, json};

use crate::query::{Cell, Cut, Limits, MAX_VALUE_CHARS, Outcome};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum Format {
    /// An aligned table, compact for a model's context.
    #[default]
    Text,
    /// `{"columns": [...], "rows": [[...]], ...}`.
    Json,
}

/// What ran, how long it took and what it returned.
#[derive(Debug, Clone, PartialEq)]
pub struct QueryResult {
    pub outcome: Outcome,
    pub elapsed: Duration,
}

impl QueryResult {
    #[must_use]
    pub fn render(&self, format: Format, limits: &Limits) -> String {
        match format {
            Format::Text => self.text(limits),
            Format::Json => self.json(limits).to_string(),
        }
    }

    fn elapsed_ms(&self) -> u64 {
        u64::try_from(self.elapsed.as_millis()).unwrap_or(u64::MAX)
    }

    /// The text table, then a line on how many rows it shows and why some were left out.
    #[must_use]
    pub fn text(&self, limits: &Limits) -> String {
        let table = match &self.outcome {
            Outcome::Affected(rows) => {
                return format!("{rows} {} affected.", rows_word(*rows == 1));
            }
            Outcome::Rows(table) => table,
        };
        let cells: Vec<Vec<String>> = table
            .rows
            .iter()
            .map(|row| row.iter().map(|c| one_line(c.text())).collect())
            .collect();
        // Per column: the width in characters, and the most bytes a value has beyond its
        // characters, so `width + extra` bounds the bytes of any padded value.
        let mut widths: Vec<(usize, usize)> = table
            .columns
            .iter()
            .map(|c| (width(&c.name), c.name.len() - width(&c.name)))
            .collect();
        let line = |widths: &[(usize, usize)]| {
            widths.iter().map(|(w, extra)| w + extra).sum::<usize>()
                + 3 * widths.len().saturating_sub(1)
                + 1
        };
        // Every line is at most as long as the widest: grow the table while it fits.
        let mut shown = 0;
        for row in &cells {
            let mut grown = widths.clone();
            for ((w, extra), value) in grown.iter_mut().zip(row) {
                let chars = width(value);
                *w = (*w).max(chars);
                *extra = (*extra).max(value.len() - chars);
            }
            if (shown + 3) * line(&grown) > limits.max_bytes {
                break;
            }
            widths = grown;
            shown += 1;
        }
        let widths: Vec<usize> = widths.into_iter().map(|(w, _)| w).collect();
        let cut = if shown < cells.len() {
            Some(Cut::MaxBytes)
        } else {
            table.cut
        };
        let mut out = String::new();
        let names: Vec<&str> = table.columns.iter().map(|c| c.name.as_str()).collect();
        push_line(&mut out, &names, &widths);
        let rule: Vec<String> = widths.iter().map(|w| "-".repeat(*w)).collect();
        out.push_str(&rule.join("-+-"));
        out.push('\n');
        for row in &cells[..shown] {
            let row: Vec<&str> = row.iter().map(String::as_str).collect();
            push_line(&mut out, &row, &widths);
        }
        out.push_str(&footer(shown, cut, table.values_cut, self.elapsed, limits));
        out
    }

    /// The JSON document: `columns`, `rows`, `row_count`, `truncated`, `truncated_by`,
    /// `values_cut` and `elapsed_ms`; or `affected_rows` and `elapsed_ms`.
    #[must_use]
    pub fn json(&self, limits: &Limits) -> Value {
        let table = match &self.outcome {
            Outcome::Affected(rows) => {
                return json!({ "affected_rows": rows, "elapsed_ms": self.elapsed_ms() });
            }
            Outcome::Rows(table) => table,
        };
        let columns: Vec<Value> = table
            .columns
            .iter()
            .map(|c| json!({ "name": c.name, "type": c.db_type }))
            .collect();
        let mut size = Value::Array(columns.clone()).to_string().len() + 160;
        let mut rows = Vec::with_capacity(table.rows.len());
        for row in &table.rows {
            let row = Value::Array(row.iter().map(cell_json).collect());
            size += row.to_string().len() + 1;
            if size > limits.max_bytes {
                break;
            }
            rows.push(row);
        }
        let cut = if rows.len() < table.rows.len() {
            Some(Cut::MaxBytes)
        } else {
            table.cut
        };
        json!({
            "columns": columns,
            "row_count": rows.len(),
            "rows": rows,
            "truncated": cut.is_some(),
            "truncated_by": cut.map(|c| match c {
                Cut::MaxRows => "max_rows",
                Cut::MaxBytes => "max_bytes",
            }),
            "values_cut": table.values_cut,
            "elapsed_ms": self.elapsed_ms(),
        })
    }
}

fn cell_json(cell: &Cell) -> Value {
    match cell {
        Cell::Null => Value::Null,
        Cell::Bool(b) => Value::Bool(*b),
        Cell::Number(n) => serde_json::from_str(n).unwrap_or_else(|_| Value::String(n.clone())),
        Cell::Text(s) => Value::String(s.clone()),
    }
}

const fn rows_word(one: bool) -> &'static str {
    if one { "row" } else { "rows" }
}

fn width(s: &str) -> usize {
    s.chars().count()
}

/// Keeps each row on one line.
fn one_line(s: &str) -> String {
    if s.contains(['\n', '\r', '\t']) {
        s.replace('\r', "\\r")
            .replace('\n', "\\n")
            .replace('\t', "\\t")
    } else {
        s.to_owned()
    }
}

fn push_line(out: &mut String, values: &[&str], widths: &[usize]) {
    let start = out.len();
    for (i, (value, w)) in values.iter().zip(widths).enumerate() {
        if i > 0 {
            out.push_str(" | ");
        }
        let _ = write!(out, "{value:<w$}");
    }
    out.truncate(start + out[start..].trim_end().len());
    out.push('\n');
}

fn footer(
    shown: usize,
    cut: Option<Cut>,
    values_cut: bool,
    elapsed: Duration,
    limits: &Limits,
) -> String {
    let rows = format!("{shown} {}", rows_word(shown == 1));
    let ms = elapsed.as_millis();
    let mut out = match cut {
        None => format!("({rows}, {ms} ms)"),
        Some(Cut::MaxRows) => format!(
            "({rows} shown, {ms} ms: the result has more. Raise max_rows (up to {}) or narrow the query)",
            Limits::MAX_ROWS
        ),
        Some(Cut::MaxBytes) => format!(
            "({rows} shown, {ms} ms: output cut at {} bytes. Raise max_bytes (up to {}) or select fewer columns)",
            limits.max_bytes,
            Limits::MAX_BYTES
        ),
    };
    if values_cut {
        let _ = write!(
            out,
            "\nValues longer than {MAX_VALUE_CHARS} characters were cut."
        );
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::query::{Column, Table};

    fn table(rows: usize, cut: Option<Cut>) -> QueryResult {
        QueryResult {
            outcome: Outcome::Rows(Table {
                columns: vec![
                    Column {
                        name: "id".into(),
                        db_type: "integer".into(),
                    },
                    Column {
                        name: "name".into(),
                        db_type: "text".into(),
                    },
                ],
                rows: (1..=rows)
                    .map(|i| {
                        vec![
                            Cell::Number(i.to_string()),
                            if i == 2 {
                                Cell::Null
                            } else {
                                Cell::Text(format!("n\n{i}"))
                            },
                        ]
                    })
                    .collect(),
                cut,
                values_cut: false,
            }),
            elapsed: Duration::from_millis(7),
        }
    }

    #[test]
    fn text_tables_align_and_say_what_was_left_out() {
        let limits = Limits::default();
        assert_eq!(
            table(2, None).text(&limits),
            "id | name\n---+-----\n1  | n\\n1\n2  | NULL\n(2 rows, 7 ms)"
        );
        let more = table(1, Some(Cut::MaxRows)).text(&limits);
        assert!(more.ends_with("(1 row shown, 7 ms: the result has more. Raise max_rows (up to 10000) or narrow the query)"), "{more}");
        let affected = QueryResult {
            outcome: Outcome::Affected(1),
            elapsed: Duration::ZERO,
        };
        assert_eq!(affected.text(&limits), "1 row affected.");
    }

    #[test]
    fn text_tables_stay_within_the_byte_limit() {
        let limits = Limits {
            max_bytes: 60,
            ..Limits::default()
        };
        let out = table(50, None).text(&limits);
        let body = out.split("\n(").next().unwrap();
        assert!(body.len() <= 60, "{out}");
        assert!(out.contains("output cut at 60 bytes"), "{out}");
    }

    #[test]
    fn json_is_typed_and_cut_by_bytes() {
        let doc = table(2, None).json(&Limits::default());
        assert_eq!(
            doc,
            json!({
                "columns": [{"name": "id", "type": "integer"}, {"name": "name", "type": "text"}],
                "rows": [[1, "n\n1"], [2, null]],
                "row_count": 2,
                "truncated": false,
                "truncated_by": null,
                "values_cut": false,
                "elapsed_ms": 7,
            })
        );
        let small = table(500, None).json(&Limits {
            max_bytes: 1000,
            ..Limits::default()
        });
        assert!(small.to_string().len() <= 1000, "{small}");
        assert_eq!(small["truncated_by"], "max_bytes");
        assert!(small["row_count"].as_u64().unwrap() > 10);
        let affected = QueryResult {
            outcome: Outcome::Affected(3),
            elapsed: Duration::ZERO,
        };
        assert_eq!(
            affected.json(&Limits::default()),
            json!({"affected_rows": 3, "elapsed_ms": 0})
        );
    }
}
