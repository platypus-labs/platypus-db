//! HTML rendering of a [`DocModel`], with the Mermaid script embedded.

use std::fmt::Write as _;

use crate::{DocForeignKey, DocKind, DocModel, DocObject, anchor, draws_diagram, mermaid};

const MERMAID: &str = include_str!("../assets/mermaid.min.js");
const STYLE: &str = "body{font:14px/1.5 system-ui,sans-serif;margin:2rem auto;max-width:1100px;padding:0 1rem;color:#1f2328;background:#fff}\
table{border-collapse:collapse;width:100%;margin:.5rem 0 1rem}th,td{border:1px solid #d0d7de;padding:4px 8px;text-align:left;vertical-align:top}\
th{background:#f6f8fa}code,.tag{font-family:ui-monospace,monospace}.tag{color:#57606a}\
@media (prefers-color-scheme:dark){body{color:#e6edf3;background:#0d1117}th,td{border-color:#30363d}th{background:#161b22}.tag{color:#8b949e}}";

fn esc(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            '\n' => out.push_str("<br>"),
            c => out.push(c),
        }
    }
    out
}

fn tags(t: &[String]) -> String {
    t.iter()
        .map(|t| format!("<span class=\"tag\">#{}</span>", esc(t)))
        .collect::<Vec<_>>()
        .join(" ")
}

fn link(schema: &str, table: &str) -> String {
    format!(
        "<a href=\"#{}\">{}.{}</a>",
        anchor(schema, table),
        esc(schema),
        esc(table)
    )
}

/// The model as one self-contained HTML page.
#[must_use]
pub fn html(model: &DocModel) -> String {
    let mut body = String::new();
    let mut any_diagram = false;
    let _ = writeln!(body, "<h1>{}</h1>", esc(&model.title));
    index(&mut body, model);
    for s in &model.schemas {
        if model.diagrams {
            if draws_diagram(model, s) {
                any_diagram = true;
                let _ = writeln!(
                    body,
                    "<h2>Diagram of {}</h2>\n<pre class=\"mermaid\">{}</pre>",
                    esc(&s.name),
                    esc(&mermaid::diagram(s))
                );
            } else {
                let _ = writeln!(
                    body,
                    "<p><em>The diagram of {} is left out: it has more than 100 tables.</em></p>",
                    esc(&s.name)
                );
            }
        }
        for o in &s.objects {
            object(&mut body, &s.name, o);
        }
    }
    let scripts = if any_diagram {
        format!(
            "<script>{MERMAID}</script>\n<script>mermaid.initialize({{startOnLoad:true,theme:window.matchMedia('(prefers-color-scheme: dark)').matches?'dark':'default'}});</script>\n"
        )
    } else {
        String::new()
    };
    format!(
        "<!doctype html><html><head><meta charset=\"utf-8\"><title>{}</title><style>{STYLE}</style></head><body>\n{body}{scripts}</body></html>\n",
        esc(&model.title)
    )
}

fn index(out: &mut String, model: &DocModel) {
    out.push_str(
        "<table><tr><th>Object</th><th>Kind</th><th>Comment</th><th>Note</th><th>Tags</th></tr>\n",
    );
    for s in &model.schemas {
        for o in &s.objects {
            let kind = if o.kind == DocKind::View {
                "view"
            } else {
                "table"
            };
            let _ = writeln!(
                out,
                "<tr><td>{}</td><td>{kind}</td><td>{}</td><td>{}</td><td>{}</td></tr>",
                link(&s.name, &o.name),
                esc(o.comment.as_deref().unwrap_or("")),
                esc(&o.note),
                tags(&o.tags)
            );
        }
    }
    out.push_str("</table>\n");
}

fn object(out: &mut String, schema: &str, o: &DocObject) {
    let _ = writeln!(
        out,
        "<h2 id=\"{}\">{}.{}</h2>",
        anchor(schema, &o.name),
        esc(schema),
        esc(&o.name)
    );
    if let Some(e) = &o.error {
        let _ = writeln!(out, "<p><em>Could not read this table: {}</em></p>", esc(e));
        return;
    }
    if let Some(c) = &o.comment {
        let _ = writeln!(out, "<p>{}</p>", esc(c));
    }
    if !o.note.is_empty() {
        let _ = writeln!(out, "<p><strong>Note:</strong> {}</p>", esc(&o.note));
    }
    if !o.tags.is_empty() {
        let _ = writeln!(out, "<p><strong>Tags:</strong> {}</p>", tags(&o.tags));
    }
    columns(out, o);
    keys(out, "Foreign keys", &o.foreign_keys);
    keys(out, "Referenced by", &o.referenced_by);
    if !o.indexes.is_empty() {
        out.push_str("<p><strong>Indexes:</strong></p>\n<ul>\n");
        for i in &o.indexes {
            let unique = if i.unique { " unique" } else { "" };
            let _ = writeln!(
                out,
                "<li><code>{}</code> ({}){unique}</li>",
                esc(&i.name),
                esc(&i.columns.join(", "))
            );
        }
        out.push_str("</ul>\n");
    }
}

fn columns(out: &mut String, o: &DocObject) {
    out.push_str("<table><tr><th>Column</th><th>Type</th><th>Null</th><th>Default</th><th>Key</th><th>Comment</th><th>Note</th><th>Tags</th></tr>\n");
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
            "<tr><td>{}</td><td>{}</td><td>{}</td><td>{}</td><td>{key}</td><td>{}</td><td>{}</td><td>{}</td></tr>",
            esc(&c.name),
            esc(&c.data_type),
            if c.nullable { "yes" } else { "no" },
            esc(c.default.as_deref().unwrap_or("")),
            esc(c.comment.as_deref().unwrap_or("")),
            esc(&c.note),
            tags(&c.tags)
        );
    }
    out.push_str("</table>\n");
}

fn keys(out: &mut String, title: &str, list: &[DocForeignKey]) {
    if list.is_empty() {
        return;
    }
    let _ = writeln!(out, "<p><strong>{title}:</strong></p>\n<ul>");
    for f in list {
        let _ = writeln!(
            out,
            "<li><code>{}</code> ({}) &rarr; {} ({})</li>",
            esc(&f.name),
            esc(&f.columns.join(", ")),
            link(&f.schema, &f.table),
            esc(&f.ref_columns.join(", "))
        );
    }
    out.push_str("</ul>\n");
}
