//! Mermaid `erDiagram` text for a schema.
//!
//! One name Mermaid's lexer cannot read fails the whole diagram, so every name is made to fit:
//! entity names and relationship labels are quoted, attribute names and types are plain words.

use std::fmt::Write as _;

use crate::{DocForeignKey, DocKind, DocSchema};

/// `s` in double quotes, without what a quoted name cannot hold: `"` becomes `'`, and `%`
/// (comments and directives), `\` and control characters are dropped.
fn quoted(s: &str) -> String {
    let inner: String = s
        .chars()
        .filter(|&c| c != '%' && c != '\\' && !c.is_control())
        .map(|c| if c == '"' { '\'' } else { c })
        .collect();
    format!("\"{inner}\"")
}

/// An entity name, always quoted: a quoted name is never taken for a keyword (`one`, `to`,
/// `u`…) and may start with a digit or hold spaces. `"_"` when nothing of it is left.
#[must_use]
pub fn entity(name: &str) -> String {
    let name = quoted(name);
    if name.len() == 2 {
        "\"_\"".to_owned()
    } else {
        name
    }
}

/// A plain word: ASCII letters, digits and `_`, not starting with a digit and not one of the
/// key markers `PK`, `FK` and `UK`, which get a leading `_`.
fn word(s: &str) -> String {
    let word: String = s
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect();
    let marker = ["pk", "fk", "uk"]
        .iter()
        .any(|k| word.eq_ignore_ascii_case(k));
    if word.is_empty() || word.starts_with(|c: char| c.is_ascii_digit()) || marker {
        format!("_{word}")
    } else {
        word
    }
}

/// An attribute name: Mermaid takes only plain words there.
#[must_use]
pub fn attribute(name: &str) -> String {
    word(name)
}

/// A column type as a plain word; `_` for a column without one (SQLite allows it).
#[must_use]
pub fn type_name(t: &str) -> String {
    word(t)
}

/// The label of a relationship, quoted: the foreign key's name, or its columns when the key
/// has no name.
#[must_use]
pub fn label(fk: &DocForeignKey) -> String {
    if fk.name.is_empty() {
        quoted(&fk.columns.join(", "))
    } else {
        quoted(&fk.name)
    }
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
                label(fk)
            );
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{DocColumn, DocObject};

    /// An attribute word of Mermaid's `erDiagram` lexer that is not read as a key marker.
    fn is_word(w: &str) -> bool {
        let mut chars = w.chars();
        chars
            .next()
            .is_some_and(|c| c.is_ascii_alphabetic() || c == '_' || c == '*')
            && chars.all(|c| c.is_ascii_alphanumeric() || "-_[]()".contains(c))
            && !["pk", "fk", "uk"].iter().any(|k| w.eq_ignore_ascii_case(k))
    }

    /// The quoted token `s` starts with, and the rest; the token holds at least `min` characters.
    fn quoted_token(s: &str, min: usize) -> Option<(&str, &str)> {
        let inner = s.strip_prefix('"')?;
        let end = inner.find('"')?;
        let token = &inner[..end];
        let valid = token.chars().count() >= min
            && !token.contains(|c: char| c == '%' || c == '\\' || c.is_control());
        valid.then_some((token, &inner[end + 1..]))
    }

    /// Checks every line of `diagram` against the grammar the bundled Mermaid reads.
    fn assert_parses(diagram: &str) {
        let mut lines = diagram.lines();
        assert_eq!(lines.next(), Some("erDiagram"));
        let mut in_block = false;
        for line in lines {
            let line = line.trim();
            if in_block {
                if line == "}" {
                    in_block = false;
                    continue;
                }
                let words: Vec<&str> = line.split(' ').collect();
                assert!(matches!(words.len(), 2 | 3), "attribute: {line}");
                assert!(is_word(words[0]) && is_word(words[1]), "attribute: {line}");
                assert!(words.len() == 2 || words[2] == "PK", "attribute: {line}");
            } else if let Some(name) = line.strip_suffix(" {") {
                let (_, rest) = quoted_token(name, 1).unwrap_or_else(|| panic!("entity: {line}"));
                assert_eq!(rest, "", "entity: {line}");
                in_block = true;
            } else {
                let (_, rest) = quoted_token(line, 1).unwrap_or_else(|| panic!("from: {line}"));
                let rest = rest.strip_prefix(" }o--|| ").expect(line);
                let (_, rest) = quoted_token(rest, 1).unwrap_or_else(|| panic!("to: {line}"));
                let rest = rest.strip_prefix(" : ").expect(line);
                let (_, rest) = quoted_token(rest, 0).unwrap_or_else(|| panic!("label: {line}"));
                assert_eq!(rest, "", "relationship: {line}");
            }
        }
        assert!(!in_block, "unclosed entity");
    }

    fn table(name: &str, columns: &[(&str, &str)], foreign_keys: Vec<DocForeignKey>) -> DocObject {
        DocObject {
            name: name.to_owned(),
            columns: columns
                .iter()
                .map(|(name, data_type)| DocColumn {
                    name: (*name).to_owned(),
                    data_type: (*data_type).to_owned(),
                    ..DocColumn::default()
                })
                .collect(),
            foreign_keys,
            ..DocObject::default()
        }
    }

    fn fk(name: &str, column: &str, table: &str) -> DocForeignKey {
        DocForeignKey {
            name: name.to_owned(),
            columns: vec![column.to_owned()],
            schema: "main".to_owned(),
            table: table.to_owned(),
            ref_columns: vec!["id".to_owned()],
        }
    }

    #[test]
    fn plain_names() {
        assert_eq!(entity("orders"), "\"orders\"");
        assert_eq!(entity("order items"), "\"order items\"");
        assert_eq!(attribute("weird name!"), "weird_name_");
        assert_eq!(type_name("numeric(12,2)"), "numeric_12_2_");
    }

    #[test]
    fn a_column_without_a_type_gets_a_word() {
        assert_eq!(type_name(""), "_");
        assert_eq!(attribute(""), "_");
        let schema = DocSchema {
            name: "main".to_owned(),
            objects: vec![table("t", &[("a", "")], vec![])],
        };
        let text = diagram(&schema);
        assert!(text.contains("\n        _ a\n"), "{text}");
        assert_parses(&text);
    }

    #[test]
    fn names_starting_with_a_digit() {
        assert_eq!(entity("2024_orders"), "\"2024_orders\"");
        assert_eq!(attribute("2fa"), "_2fa");
        assert_eq!(type_name("2d_point"), "_2d_point");
    }

    #[test]
    fn quoted_names_drop_percent_backslash_and_control_characters() {
        assert_eq!(entity("100%\\done\t\"x\""), "\"100done'x'\"");
        assert_eq!(entity("%%{init}%%"), "\"{init}\"");
        assert_eq!(entity(""), "\"_\"");
        assert_eq!(entity("%"), "\"_\"");
    }

    #[test]
    fn keywords_are_quoted_or_prefixed() {
        for name in ["one", "many", "to", "u", "erDiagram", "accTitle"] {
            assert_eq!(entity(name), format!("\"{name}\""));
        }
        assert_eq!(attribute("pk"), "_pk");
        assert_eq!(attribute("FK"), "_FK");
        assert_eq!(type_name("uk"), "_uk");
        assert_eq!(attribute("pkey"), "pkey");
    }

    #[test]
    fn a_foreign_key_without_a_name_is_labelled_with_its_columns() {
        assert_eq!(
            label(&fk("", "customer_id", "customers")),
            "\"customer_id\""
        );
        assert_eq!(
            label(&fk("orders_customer", "c", "customers")),
            "\"orders_customer\""
        );
        let mut nameless = fk("", "", "customers");
        nameless.columns.clear();
        assert_eq!(label(&nameless), "\"\"");
    }

    #[test]
    fn awkward_names_still_make_a_diagram_mermaid_reads() {
        let schema = DocSchema {
            name: "main".to_owned(),
            objects: vec![
                table("one", &[("id", "integer"), ("pk", "")], vec![]),
                table(
                    "2024 orders%",
                    &[
                        ("2fa", "2d_point"),
                        ("to", "numeric(12,2)"),
                        ("a\\b", "\"x\""),
                    ],
                    vec![fk("", "one_id", "one"), fk("50%\\off", "x", "2024 orders%")],
                ),
                table("", &[], vec![fk("", "", "u")]),
            ],
        };
        assert_parses(&diagram(&schema));
    }
}
