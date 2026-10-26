//! Markdown rendering of a [`DocModel`].

use std::fmt::Write as _;

use crate::{DocKind, DocModel, DocObject, anchor, draws_diagram, mermaid};

/// Text safe in a table cell: `|` escaped, line breaks as `<br>`.
fn cell(s: &str) -> String {
    s.replace('|', r"\|")
        .replace("\r\n", "<br>")
        .replace('\n', "<br>")
}

fn tags(t: &[String]) -> String {
    t.iter()
        .map(|t| format!("`#{t}`"))
        .collect::<Vec<_>>()
        .join(" ")
}

/// The model as one Markdown document.
#[must_use]
pub fn markdown(model: &DocModel) -> String {
    let mut out = String::new();
    let _ = writeln!(out, "# {}\n", model.title);
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
                s.name,
                o.name,
                anchor(&s.name, &o.name),
                cell(o.comment.as_deref().unwrap_or("")),
                cell(&o.note),
                tags(&o.tags)
            );
        }
    }
    for s in &model.schemas {
        if model.diagrams {
            if draws_diagram(model, s) {
                let _ = write!(
                    out,
                    "\n## Diagram of {}\n\n```mermaid\n{}```\n",
                    s.name,
                    mermaid::diagram(s)
                );
            } else {
                let _ = writeln!(
                    out,
                    "\n> The diagram of {} is left out: it has more than 100 tables.",
                    s.name
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
    let _ = writeln!(out, "\n## {schema}.{}\n", o.name);
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
            let _ = writeln!(out, "- `{}` ({}){unique}", i.name, i.columns.join(", "));
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
            tags(&c.tags)
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
                "- `{}` ({}) → [{}.{}](#{}) ({})",
                f.name,
                f.columns.join(", "),
                f.schema,
                f.table,
                anchor(&f.schema, &f.table),
                f.ref_columns.join(", ")
            );
        }
    }
}
