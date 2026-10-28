//! A statement's result as agents read it: an aligned text table, or JSON whose shape is
//! stable (documented in the README, under Agents). Both stay within the byte limit and say
//! when rows or values were left out.

use std::fmt::Write as _;
use std::time::Duration;

use serde::Deserialize;
use serde_json::{Value, json};

use crate::query::{Cell, Cut, Limits, MAX_VALUE_CHARS, Outcome, Table};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum Format {
    /// An aligned table, compact for a model's context.
    #[default]
    Text,
    /// `{"columns": [...], "rows": [[...]], ...}`.
    Json,
}

/// What a statement returned, how long it took, and the limits it ran within.
#[derive(Debug, Clone, PartialEq)]
pub struct QueryResult {
    pub outcome: Outcome,
    pub elapsed: Duration,
    pub limits: Limits,
}

impl QueryResult {
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

    /// Why the rendering of the first `shown` rows has fewer rows than the statement
    /// returned, if it does.
    fn cut(table: &Table, shown: usize) -> Option<Cut> {
        if shown < table.rows.len() {
            Some(Cut::MaxBytes)
        } else {
            table.cut
        }
    }

    /// The text table, then a line on how many rows it shows and why some were left out;
    /// the whole of it within the byte limit.
    #[must_use]
    pub fn text(&self) -> String {
        let table = match &self.outcome {
            Outcome::Affected(rows) => {
                return format!("{rows} {} affected.", rows_word(*rows == 1));
            }
            Outcome::Rows(table) => table,
        };
        let cells: Vec<Vec<String>> = table
            .rows
            .iter()
            .map(|row| row.iter().map(|c| one_line(&text_cell(c))).collect())
            .collect();
        let render = |shown: usize| {
            let rows = &cells[..shown];
            let mut widths: Vec<usize> = table.columns.iter().map(|c| width(&c.name)).collect();
            for row in rows {
                for (w, value) in widths.iter_mut().zip(row) {
                    *w = (*w).max(width(value));
                }
            }
            let mut out = String::new();
            let names: Vec<&str> = table.columns.iter().map(|c| c.name.as_str()).collect();
            push_line(&mut out, &names, &widths);
            let rule: Vec<String> = widths.iter().map(|w| "-".repeat(*w)).collect();
            out.push_str(&rule.join("-+-"));
            out.push('\n');
            for row in rows {
                let row: Vec<&str> = row.iter().map(String::as_str).collect();
                push_line(&mut out, &row, &widths);
            }
            let cut = Self::cut(table, shown);
            out.push_str(&footer(
                shown,
                cut,
                table.values_cut,
                self.elapsed,
                &self.limits,
            ));
            out
        };
        fit(cells.len(), self.limits.max_bytes, render, String::len)
    }

    /// The JSON document: `columns`, `rows`, `row_count`, `truncated`, `truncated_by`,
    /// `values_cut` and `elapsed_ms`; or `affected_rows` and `elapsed_ms`. Within the byte
    /// limit as serialized.
    #[must_use]
    pub fn json(&self) -> Value {
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
        let rows: Vec<Value> = table
            .rows
            .iter()
            .map(|row| Value::Array(row.iter().map(cell_json).collect()))
            .collect();
        let render = |shown: usize| {
            let cut = Self::cut(table, shown);
            json!({
                "columns": columns,
                "row_count": shown,
                "rows": rows[..shown],
                "truncated": cut.is_some(),
                "truncated_by": cut.map(|c| match c {
                    Cut::MaxRows => "max_rows",
                    Cut::MaxBytes => "max_bytes",
                }),
                "values_cut": table.values_cut,
                "elapsed_ms": self.elapsed_ms(),
            })
        };
        fit(rows.len(), self.limits.max_bytes, render, |doc| {
            doc.to_string().len()
        })
    }
}

/// The rendering of the first `total` rows, or of as many as fit in `max` bytes; at least
/// the one of none. Renderings grow with the rows, so the most that fit are found by
/// halving.
fn fit<T>(total: usize, max: usize, render: impl Fn(usize) -> T, size: impl Fn(&T) -> usize) -> T {
    let all = render(total);
    if size(&all) <= max {
        return all;
    }
    let (mut fits, mut too_many) = (0, total);
    while too_many - fits > 1 {
        let middle = fits + (too_many - fits) / 2;
        if size(&render(middle)) <= max {
            fits = middle;
        } else {
            too_many = middle;
        }
    }
    render(fits)
}

/// Integers JavaScript (and every parser that reads numbers as doubles) keeps exactly.
const SAFE_INTEGER: i128 = (1 << 53) - 1;

/// A number as JSON: a JSON number when every parser reads it back as it is, else a string.
/// Integers beyond 2^53 would come back as another number (9007199254740993 as
/// 9007199254740992), so they are strings, like decimals already are.
fn number_json(n: &str) -> Value {
    let lossless = match n.parse::<i128>() {
        Ok(i) => (-SAFE_INTEGER..=SAFE_INTEGER).contains(&i),
        // Floats print as the shortest text that reads back as the same double.
        Err(_) => n.parse::<f64>().is_ok_and(f64::is_finite),
    };
    match serde_json::from_str(n) {
        Ok(value) if lossless => value,
        _ => Value::String(n.to_owned()),
    }
}

fn cell_json(cell: &Cell) -> Value {
    match cell {
        Cell::Null => Value::Null,
        Cell::Bool(b) => Value::Bool(*b),
        Cell::Number(n) => number_json(n),
        Cell::Text(s) => Value::String(s.clone()),
    }
}

const fn rows_word(one: bool) -> &'static str {
    if one { "row" } else { "rows" }
}

fn width(s: &str) -> usize {
    s.chars().count()
}

/// A value as the text table shows it. SQL `NULL` is `NULL`; a text that would read the
/// same (`NULL` in any case), or that is quoted already, shows as a SQL string literal,
/// `'NULL'`, so the two cannot be confused.
fn text_cell(cell: &Cell) -> std::borrow::Cow<'_, str> {
    match cell {
        Cell::Text(s)
            if s.eq_ignore_ascii_case("null")
                || (s.len() >= 2 && s.starts_with('\'') && s.ends_with('\'')) =>
        {
            format!("'{}'", s.replace('\'', "''")).into()
        }
        other => other.text().into(),
    }
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
    use crate::query::Column;

    fn table(rows: usize, cut: Option<Cut>, limits: Limits) -> QueryResult {
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
            limits,
        }
    }

    #[test]
    fn text_tables_align_and_say_what_was_left_out() {
        let limits = Limits::default();
        assert_eq!(
            table(2, None, limits).text(),
            "id | name\n---+-----\n1  | n\\n1\n2  | NULL\n(2 rows, 7 ms)"
        );
        let more = table(1, Some(Cut::MaxRows), limits).text();
        assert!(more.ends_with("(1 row shown, 7 ms: the result has more. Raise max_rows (up to 10000) or narrow the query)"), "{more}");
        let affected = QueryResult {
            outcome: Outcome::Affected(1),
            elapsed: Duration::ZERO,
            limits,
        };
        assert_eq!(affected.text(), "1 row affected.");
    }

    #[test]
    fn text_tables_tell_null_from_the_text_null() {
        assert_eq!(text_cell(&Cell::Null), "NULL");
        assert_eq!(text_cell(&Cell::Text("NULL".to_owned())), "'NULL'");
        assert_eq!(text_cell(&Cell::Text("null".to_owned())), "'null'");
        assert_eq!(text_cell(&Cell::Text("'NULL'".to_owned())), "'''NULL'''");
        assert_eq!(text_cell(&Cell::Text("it's".to_owned())), "it's");
        assert_eq!(text_cell(&Cell::Text("'".to_owned())), "'");
    }

    #[test]
    fn text_tables_stay_within_the_byte_limit() {
        // The footer counts too.
        let limits = Limits {
            max_bytes: 200,
            ..Limits::default()
        };
        let out = table(50, None, limits).text();
        assert!(out.len() <= 200, "{}: {out}", out.len());
        assert!(out.contains("output cut at 200 bytes"), "{out}");
        assert!(out.contains("1  | n\\n1"), "{out}");
    }

    #[test]
    fn json_is_typed_and_cut_by_bytes() {
        let doc = table(2, None, Limits::default()).json();
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
        let small = table(
            500,
            None,
            Limits {
                max_bytes: 1000,
                ..Limits::default()
            },
        )
        .json();
        assert!(small.to_string().len() <= 1000, "{small}");
        assert_eq!(small["truncated_by"], "max_bytes");
        assert!(small["row_count"].as_u64().unwrap() > 10);
        let affected = QueryResult {
            outcome: Outcome::Affected(3),
            elapsed: Duration::ZERO,
            limits: Limits::default(),
        };
        assert_eq!(
            affected.json(),
            json!({"affected_rows": 3, "elapsed_ms": 0})
        );
    }

    #[test]
    fn keeps_numbers_that_json_parsers_would_round_as_strings() {
        let number = |n: &str| number_json(n);
        assert_eq!(number("9007199254740991"), json!(9_007_199_254_740_991_i64));
        assert_eq!(
            number("-9007199254740991"),
            json!(-9_007_199_254_740_991_i64)
        );
        assert_eq!(number("9007199254740993"), json!("9007199254740993"));
        assert_eq!(
            number("-9223372036854775808"),
            json!("-9223372036854775808")
        );
        assert_eq!(number("2.5"), json!(2.5));
        assert_eq!(number("1e300"), json!(1e300));
        assert_eq!(number("not a number"), json!("not a number"));
    }
}
