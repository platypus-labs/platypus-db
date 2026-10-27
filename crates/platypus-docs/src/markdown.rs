//! Markdown rendering of a [`DocModel`].

use std::fmt::Write as _;

use crate::{DocKind, DocModel, DocObject, anchor, draws_diagram, mermaid};

/// `s` with `|` escaped, as a table cell needs even inside a code span.
fn pipes(s: &str) -> String {
    s.replace('|', r"\|")
}

/// Text on one line, safe in a table cell: `<`, `>` and `&` as entities, so that no raw HTML
/// from a name, comment or note gets through, `|` escaped and line breaks as `<br>`.
fn cell(s: &str) -> String {
    let text = s
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;");
    pipes(&text).replace("\r\n", "<br>").replace('\n', "<br>")
}

/// `s` as a code span on one line: its fence is longer than any run of backticks inside it.
fn code(s: &str) -> String {
    let s = s.replace("\r\n", " ").replace('\n', " ");
    let longest = s.split(|c| c != '`').map(str::len).max().unwrap_or(0);
    let fence = "`".repeat(longest + 1);
    let pad = if s.starts_with('`') || s.ends_with('`') {
        " "
    } else {
        ""
    };
    format!("{fence}{pad}{s}{pad}{fence}")
}

fn tags(t: &[String]) -> String {
    t.iter()
        .map(|t| code(&format!("#{t}")))
        .collect::<Vec<_>>()
        .join(" ")
}

/// The model as one Markdown document.
#[must_use]
pub fn markdown(model: &DocModel) -> String {
    let mut out = String::new();
    let _ = writeln!(out, "# {}\n", cell(&model.title));
    out.push_str("| Object | Kind | Comment | Note | Tags |\n|---|---|---|---|---|\n");
    for s in &model.schemas {
        for o in &s.objects {
            let kind = if o.kind == DocKind::View {
                "view"
            } else {
                "table"
            };
            let _ = writeln!(
                out,
                "| [{}.{}](#{}) | {kind} | {} | {} | {} |",
                cell(&s.name),
                cell(&o.name),
                anchor(&s.name, &o.name),
                cell(o.comment.as_deref().unwrap_or("")),
                cell(&o.note),
                pipes(&tags(&o.tags))
            );
        }
    }
    for s in &model.schemas {
        if model.diagrams {
            if draws_diagram(model, s) {
                let _ = write!(
                    out,
                    "\n## Diagram of {}\n\n```mermaid\n{}```\n",
                    cell(&s.name),
                    mermaid::diagram(s)
                );
            } else {
                let _ = writeln!(
                    out,
                    "\n> The diagram of {} is left out: it has more than 100 tables.",
                    cell(&s.name)
                );
            }
        }
        for o in &s.objects {
            object(&mut out, &s.name, o);
        }
    }
    out
}

fn object(out: &mut String, schema: &str, o: &DocObject) {
    // An explicit anchor: renderers derive a heading's own id each in their own way.
    let _ = writeln!(
        out,
        "\n<a id=\"{}\"></a>\n\n## {}.{}\n",
        anchor(schema, &o.name),
        cell(schema),
        cell(&o.name)
    );
    if let Some(e) = &o.error {
        let _ = writeln!(out, "> Could not read this table: {}", cell(e));
        return;
    }
    if let Some(c) = &o.comment {
        let _ = writeln!(out, "{}\n", cell(c));
    }
    if !o.note.is_empty() {
        let _ = writeln!(out, "**Note:** {}\n", cell(&o.note));
    }
    if !o.tags.is_empty() {
        let _ = writeln!(out, "**Tags:** {}\n", tags(&o.tags));
    }
    columns(out, o);
    keys(out, o);
    if !o.indexes.is_empty() {
        out.push_str("\n**Indexes:**\n\n");
        for i in &o.indexes {
            let unique = if i.unique { " unique" } else { "" };
            let _ = writeln!(
                out,
                "- {} ({}){unique}",
                code(&i.name),
                cell(&i.columns.join(", "))
            );
        }
    }
}

fn columns(out: &mut String, o: &DocObject) {
    out.push_str(
        "| Column | Type | Null | Default | Key | Comment | Note | Tags |\n|---|---|---|---|---|---|---|---|\n",
    );
    for c in &o.columns {
        let key = if c.primary_key {
            "PK"
        } else if o.foreign_keys.iter().any(|f| f.columns.contains(&c.name)) {
            "FK"
        } else {
            ""
        };
        let _ = writeln!(
            out,
            "| {} | {} | {} | {} | {key} | {} | {} | {} |",
            cell(&c.name),
            cell(&c.data_type),
            if c.nullable { "yes" } else { "no" },
            cell(c.default.as_deref().unwrap_or("")),
            cell(c.comment.as_deref().unwrap_or("")),
            cell(&c.note),
            pipes(&tags(&c.tags))
        );
    }
}

fn keys(out: &mut String, o: &DocObject) {
    for (title, list) in [
        ("Foreign keys", &o.foreign_keys),
        ("Referenced by", &o.referenced_by),
    ] {
        if list.is_empty() {
            continue;
        }
        let _ = writeln!(out, "\n**{title}:**\n");
        for f in list {
            let _ = writeln!(
                out,
                "- {} ({}) → [{}.{}](#{}) ({})",
                code(&f.name),
                cell(&f.columns.join(", ")),
                cell(&f.schema),
                cell(&f.table),
                anchor(&f.schema, &f.table),
                cell(&f.ref_columns.join(", "))
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{DocColumn, DocIndex, DocSchema};

    fn model(object: DocObject) -> DocModel {
        DocModel {
            title: "<b>shop</b> & co".to_owned(),
            schemas: vec![DocSchema {
                name: "public".to_owned(),
                objects: vec![object],
            }],
            diagrams: false,
        }
    }

    #[test]
    fn raw_html_is_escaped() {
        let md = markdown(&model(DocObject {
            name: "a<b>".to_owned(),
            comment: Some("<img src=x onerror=alert(1)>".to_owned()),
            note: "<script>alert(1)</script>\nnext".to_owned(),
            columns: vec![DocColumn {
                name: "c".to_owned(),
                data_type: "text".to_owned(),
                default: Some("'<x>'".to_owned()),
                note: "a & b".to_owned(),
                ..DocColumn::default()
            }],
            ..DocObject::default()
        }));
        assert!(
            md.starts_with("# &lt;b&gt;shop&lt;/b&gt; &amp; co\n"),
            "{md}"
        );
        assert!(!md.contains("<script>") && !md.contains("<img"), "{md}");
        assert!(md.contains("## public.a&lt;b&gt;\n"), "{md}");
        assert!(md.contains("&lt;img src=x onerror=alert(1)&gt;"), "{md}");
        assert!(
            md.contains("**Note:** &lt;script&gt;alert(1)&lt;/script&gt;<br>next"),
            "{md}"
        );
        assert!(md.contains("| c | text | no | '&lt;x&gt;' |  | "), "{md}");
        assert!(md.contains("| a &amp; b |"), "{md}");
    }

    #[test]
    fn code_spans_hold_backticks_and_pipes() {
        assert_eq!(code("idx"), "`idx`");
        assert_eq!(code("a`b"), "``a`b``");
        assert_eq!(code("`a``"), "``` `a`` ```");
        let md = markdown(&model(DocObject {
            name: "t".to_owned(),
            tags: vec!["a|b".to_owned(), "c`d".to_owned()],
            indexes: vec![DocIndex {
                name: "i`x".to_owned(),
                columns: vec!["c".to_owned()],
                unique: true,
            }],
            ..DocObject::default()
        }));
        assert!(md.contains(r"| `#a\|b` ``#c`d`` |"), "{md}");
        assert!(md.contains("**Tags:** `#a|b` ``#c`d``\n"), "{md}");
        assert!(md.contains("- ``i`x`` (c) unique\n"), "{md}");
    }
}
