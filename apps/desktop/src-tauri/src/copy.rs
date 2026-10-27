//! Clipboard formats for grid selections. Values arrive as the full text the grid shows;
//! the column kinds decide what is a number in JSON and how SQL literals are written.

use std::fmt::Write as _;

use platypus_sql::Dialect;
use platypus_sql::dml::{Column, InvalidValue, ValueKind};
use serde::Deserialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CopyFormat {
    /// Tab-separated, as spreadsheets paste it.
    Tsv,
    Csv,
    /// An array of objects, one per row.
    Json,
    Markdown,
    Html,
    /// One `INSERT` per row.
    Insert,
    /// `(a, b, c)` of the distinct values, for `WHERE x IN …`.
    InList,
}

/// The selection to copy: column names and kinds, then the rows' values (`None` = NULL).
#[derive(Debug)]
pub struct Selection<'a> {
    pub names: Vec<&'a str>,
    pub kinds: Vec<ValueKind>,
    pub rows: Vec<Vec<Option<String>>>,
}

pub struct Target<'a> {
    pub dialect: Dialect,
    /// Quoted table name for `INSERT`.
    pub table: &'a str,
    /// Column names as the first line (TSV, CSV).
    pub header: bool,
}

/// Quotes a delimited field when it holds the delimiter, a quote or a line break.
fn delimited(out: &mut String, value: &str, delimiter: char) {
    if value.contains([delimiter, '"', '\n', '\r']) {
        out.push('"');
        out.push_str(&value.replace('"', "\"\""));
        out.push('"');
    } else {
        out.push_str(value);
    }
}

fn delimited_rows(s: &Selection<'_>, header: bool, delimiter: char) -> String {
    let mut out = String::new();
    let mut line = |cells: &mut dyn Iterator<Item = &str>| {
        for (i, cell) in cells.enumerate() {
            if i > 0 {
                out.push(delimiter);
            }
            delimited(&mut out, cell, delimiter);
        }
        out.push('\n');
    };
    if header {
        line(&mut s.names.iter().copied());
    }
    for row in &s.rows {
        line(&mut row.iter().map(|v| v.as_deref().unwrap_or("")));
    }
    out.pop();
    out
}

fn json_string(out: &mut String, s: &str) {
    // Serializing a `&str` cannot fail.
    out.push_str(&serde_json::to_string(s).unwrap_or_default());
}

/// A JSON value: numbers and booleans bare when the column has that type and the text is one.
fn json_value(out: &mut String, kind: ValueKind, value: Option<&str>) {
    match (kind, value) {
        (_, None) => out.push_str("null"),
        (ValueKind::Integer | ValueKind::Decimal | ValueKind::Float | ValueKind::Real, Some(v))
            if v.parse::<f64>().is_ok_and(f64::is_finite) =>
        {
            out.push_str(v);
        }
        (ValueKind::Bool, Some(v @ ("true" | "false"))) => out.push_str(v),
        (_, Some(v)) => json_string(out, v),
    }
}

fn json(s: &Selection<'_>) -> String {
    let mut out = String::from("[");
    for (r, row) in s.rows.iter().enumerate() {
        out.push_str(if r == 0 { "\n  {" } else { ",\n  {" });
        for (c, value) in row.iter().enumerate() {
            if c > 0 {
                out.push_str(", ");
            }
            json_string(&mut out, s.names[c]);
            out.push_str(": ");
            json_value(&mut out, s.kinds[c], value.as_deref());
        }
        out.push('}');
    }
    out.push_str(if s.rows.is_empty() { "]" } else { "\n]" });
    out
}

fn markdown_cell(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('|', "\\|")
        .replace("\r\n", "<br>")
        .replace('\n', "<br>")
}

fn markdown(s: &Selection<'_>) -> String {
    let numeric = |k: ValueKind| {
        matches!(
            k,
            ValueKind::Integer | ValueKind::Decimal | ValueKind::Float | ValueKind::Real
        )
    };
    let mut out = String::from("|");
    for name in &s.names {
        let _ = write!(out, " {} |", markdown_cell(name));
    }
    out.push_str("\n|");
    for &kind in &s.kinds {
        out.push_str(if numeric(kind) { " ---: |" } else { " --- |" });
    }
    for row in &s.rows {
        out.push_str("\n|");
        for value in row {
            let _ = write!(out, " {} |", markdown_cell(value.as_deref().unwrap_or("")));
        }
    }
    out
}

fn html_escape(out: &mut String, value: &str) {
    for c in value.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            c => out.push(c),
        }
    }
}

fn html(s: &Selection<'_>) -> String {
    let mut out = String::from("<table>\n  <thead>\n    <tr>");
    for name in &s.names {
        out.push_str("<th>");
        html_escape(&mut out, name);
        out.push_str("</th>");
    }
    out.push_str("</tr>\n  </thead>\n  <tbody>");
    for row in &s.rows {
        out.push_str("\n    <tr>");
        for value in row {
            out.push_str("<td>");
            html_escape(&mut out, value.as_deref().unwrap_or(""));
            out.push_str("</td>");
        }
        out.push_str("</tr>");
    }
    out.push_str("\n  </tbody>\n</table>");
    out
}

fn columns<'a>(s: &'a Selection<'a>) -> Vec<Column<'a>> {
    s.names
        .iter()
        .zip(&s.kinds)
        .map(|(&name, &kind)| Column {
            name,
            kind,
            default: None,
        })
        .collect()
}

fn insert(s: &Selection<'_>, t: &Target<'_>) -> Result<String, InvalidValue> {
    let columns = columns(s);
    let names = s
        .names
        .iter()
        .map(|n| t.dialect.quote_ident(n))
        .collect::<Vec<_>>()
        .join(", ");
    let mut out = String::new();
    for row in &s.rows {
        let values = columns
            .iter()
            .zip(row)
            .map(|(c, v)| c.literal(t.dialect, v.as_deref()))
            .collect::<Result<Vec<_>, _>>()?;
        let _ = writeln!(
            out,
            "INSERT INTO {} ({names}) VALUES ({});",
            t.table,
            values.join(", ")
        );
    }
    out.pop();
    Ok(out)
}

/// The distinct rows as SQL literals, in order: one value per row, or a tuple per row when
/// several columns are selected.
fn in_list(s: &Selection<'_>, t: &Target<'_>) -> Result<String, InvalidValue> {
    let columns = columns(s);
    let mut seen = std::collections::HashSet::new();
    let mut items = Vec::new();
    for row in &s.rows {
        let literals = columns
            .iter()
            .zip(row)
            .map(|(c, v)| c.literal(t.dialect, v.as_deref()))
            .collect::<Result<Vec<_>, _>>()?;
        let item = if literals.len() == 1 {
            literals.into_iter().next().unwrap_or_default()
        } else {
            format!("({})", literals.join(", "))
        };
        if seen.insert(item.clone()) {
            items.push(item);
        }
    }
    Ok(format!("({})", items.join(", ")))
}

/// The selection in `format`.
pub fn render(
    format: CopyFormat,
    s: &Selection<'_>,
    t: &Target<'_>,
) -> Result<String, InvalidValue> {
    Ok(match format {
        CopyFormat::Tsv => delimited_rows(s, t.header, '\t'),
        CopyFormat::Csv => delimited_rows(s, t.header, ','),
        CopyFormat::Json => json(s),
        CopyFormat::Markdown => markdown(s),
        CopyFormat::Html => html(s),
        CopyFormat::Insert => insert(s, t)?,
        CopyFormat::InList => in_list(s, t)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn selection() -> Selection<'static> {
        Selection {
            names: vec!["id", "name", "ok"],
            kinds: vec![ValueKind::Integer, ValueKind::Text, ValueKind::Bool],
            rows: vec![
                vec![
                    Some("1".into()),
                    Some("a,\"b\"".into()),
                    Some("true".into()),
                ],
                vec![Some("2".into()), None, Some("false".into())],
                vec![Some("1".into()), Some("x|y\nz".into()), None],
            ],
        }
    }

    fn target(header: bool) -> Target<'static> {
        Target {
            dialect: Dialect::Postgres,
            table: "demo.t",
            header,
        }
    }

    fn copy(format: CopyFormat, header: bool) -> String {
        render(format, &selection(), &target(header)).unwrap()
    }

    #[test]
    fn delimited_formats() {
        assert_eq!(
            copy(CopyFormat::Tsv, true),
            "id\tname\tok\n1\t\"a,\"\"b\"\"\"\ttrue\n2\t\tfalse\n1\t\"x|y\nz\"\t"
        );
        assert_eq!(
            copy(CopyFormat::Csv, false),
            "1,\"a,\"\"b\"\"\",true\n2,,false\n1,\"x|y\nz\","
        );
    }

    #[test]
    fn json_keeps_types() {
        assert_eq!(
            copy(CopyFormat::Json, false),
            "[\n  {\"id\": 1, \"name\": \"a,\\\"b\\\"\", \"ok\": true},\n  \
             {\"id\": 2, \"name\": null, \"ok\": false},\n  \
             {\"id\": 1, \"name\": \"x|y\\nz\", \"ok\": null}\n]"
        );
        let empty = Selection {
            rows: Vec::new(),
            ..selection()
        };
        assert_eq!(
            render(CopyFormat::Json, &empty, &target(false)).unwrap(),
            "[]"
        );
    }

    #[test]
    fn markdown_and_html() {
        assert_eq!(
            copy(CopyFormat::Markdown, false),
            "| id | name | ok |\n| ---: | --- | --- |\n| 1 | a,\"b\" | true |\n| 2 |  | false |\n\
             | 1 | x\\|y<br>z |  |"
        );
        assert!(copy(CopyFormat::Html, false).contains("<td>a,&quot;b&quot;</td>"));
    }

    #[test]
    fn sql_formats() {
        assert_eq!(
            copy(CopyFormat::Insert, false).lines().next(),
            Some("INSERT INTO demo.t (id, name, ok) VALUES (1, 'a,\"b\"', TRUE);")
        );
        let ids = Selection {
            names: vec!["id"],
            kinds: vec![ValueKind::Integer],
            rows: vec![
                vec![Some("1".into())],
                vec![Some("2".into())],
                vec![Some("1".into())],
            ],
        };
        assert_eq!(
            render(CopyFormat::InList, &ids, &target(false)).unwrap(),
            "(1, 2)"
        );
        assert_eq!(
            copy(CopyFormat::InList, false),
            "((1, 'a,\"b\"', TRUE), (2, NULL, FALSE), (1, 'x|y\nz', NULL))"
        );
    }
}
