//! Mermaid `erDiagram` text for a schema.

use std::fmt::Write as _;

use crate::{DocKind, DocSchema};

/// An entity name: bare when it is a plain identifier, quoted otherwise.
#[must_use]
pub fn entity(name: &str) -> String {
    if !name.is_empty() && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
        name.to_owned()
    } else {
        format!("\"{}\"", name.replace('"', "'"))
    }
}

fn word(s: &str) -> String {
    s.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

/// An attribute name: Mermaid takes only plain words there.
#[must_use]
pub fn attribute(name: &str) -> String {
    word(name)
}

/// A column type as a plain word.
#[must_use]
pub fn type_name(t: &str) -> String {
    word(t)
}

/// The diagram of `schema`'s tables and the foreign keys between them.
#[must_use]
pub fn diagram(schema: &DocSchema) -> String {
    let mut out = String::from("erDiagram\n");
    for o in schema.objects.iter().filter(|o| o.kind == DocKind::Table) {
        let _ = writeln!(out, "    {} {{", entity(&o.name));
        for c in &o.columns {
            let key = if c.primary_key { " PK" } else { "" };
            let _ = writeln!(
                out,
                "        {} {}{key}",
                type_name(&c.data_type),
                attribute(&c.name)
            );
        }
        out.push_str("    }\n");
    }
    for o in schema.objects.iter().filter(|o| o.kind == DocKind::Table) {
        for fk in o.foreign_keys.iter().filter(|f| f.schema == schema.name) {
            let _ = writeln!(
                out,
                "    {} }}o--|| {} : {}",
                entity(&o.name),
                entity(&fk.table),
                attribute(&fk.name)
            );
        }
    }
    out
}
