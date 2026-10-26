//! Schema documentation: the doc model and its Markdown, HTML and Mermaid renderings.

mod html;
mod markdown;
pub mod mermaid;

use serde::{Deserialize, Serialize};

pub use html::html;
pub use markdown::markdown;

/// Schemas with more tables than this get no diagram.
pub const DIAGRAM_LIMIT: usize = 100;

/// A whole document: a title and the schemas it covers.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DocModel {
    pub title: String,
    pub schemas: Vec<DocSchema>,
    pub diagrams: bool,
}

/// One schema and its objects.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DocSchema {
    pub name: String,
    pub objects: Vec<DocObject>,
}

/// A table or view with its columns, keys and indexes.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DocObject {
    pub name: String,
    pub kind: DocKind,
    pub comment: Option<String>,
    pub note: String,
    pub tags: Vec<String>,
    pub columns: Vec<DocColumn>,
    pub primary_key: Vec<String>,
    pub foreign_keys: Vec<DocForeignKey>,
    pub referenced_by: Vec<DocForeignKey>,
    pub indexes: Vec<DocIndex>,
    pub error: Option<String>,
}

/// Whether an object is a table or a view.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum DocKind {
    #[default]
    Table,
    View,
}

/// A column with its catalog comment and the user's note and tags.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DocColumn {
    pub name: String,
    pub data_type: String,
    pub nullable: bool,
    pub default: Option<String>,
    pub primary_key: bool,
    pub comment: Option<String>,
    pub note: String,
    pub tags: Vec<String>,
}

/// A foreign key, either outgoing or (in `referenced_by`) incoming.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DocForeignKey {
    pub name: String,
    pub columns: Vec<String>,
    pub schema: String,
    pub table: String,
    pub ref_columns: Vec<String>,
}

/// An index on an object.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DocIndex {
    pub name: String,
    pub columns: Vec<String>,
    pub unique: bool,
}

/// The anchor of `schema.name` in both formats: lower case letters and digits only.
#[must_use]
pub fn anchor(schema: &str, name: &str) -> String {
    format!("{schema}{name}")
        .chars()
        .filter(char::is_ascii_alphanumeric)
        .collect::<String>()
        .to_lowercase()
}

/// Whether the diagram of `schema` is drawn.
#[must_use]
pub fn draws_diagram(model: &DocModel, schema: &DocSchema) -> bool {
    model.diagrams
        && schema
            .objects
            .iter()
            .filter(|o| o.kind == DocKind::Table)
            .count()
            <= DIAGRAM_LIMIT
}

#[cfg(test)]
mod tests {
    use super::*;

    fn column(name: &str, data_type: &str) -> DocColumn {
        DocColumn {
            name: name.to_owned(),
            data_type: data_type.to_owned(),
            ..DocColumn::default()
        }
    }

    fn model() -> DocModel {
        let orders = DocObject {
            name: "orders".to_owned(),
            comment: Some("All | orders".to_owned()),
            note: "One row\nper order".to_owned(),
            tags: vec!["core".to_owned()],
            columns: vec![
                DocColumn {
                    primary_key: true,
                    ..column("id", "integer")
                },
                DocColumn {
                    nullable: true,
                    ..column("customer_id", "integer")
                },
                DocColumn {
                    note: "<b>pii</b>".to_owned(),
                    ..column("email", "text")
                },
            ],
            primary_key: vec!["id".to_owned()],
            foreign_keys: vec![DocForeignKey {
                name: "orders_customer".to_owned(),
                columns: vec!["customer_id".to_owned()],
                schema: "public".to_owned(),
                table: "customers".to_owned(),
                ref_columns: vec!["id".to_owned()],
            }],
            ..DocObject::default()
        };
        let customers = DocObject {
            name: "customers".to_owned(),
            columns: vec![DocColumn {
                primary_key: true,
                ..column("id", "integer")
            }],
            ..DocObject::default()
        };
        DocModel {
            title: "shop".to_owned(),
            schemas: vec![DocSchema {
                name: "public".to_owned(),
                objects: vec![customers, orders],
            }],
            diagrams: true,
        }
    }

    #[test]
    fn markdown_lists_tables_columns_and_keys() {
        let md = markdown(&model());
        assert!(md.starts_with("# shop\n"), "{md}");
        assert!(md.contains("## public.orders"), "{md}");
        assert!(md.contains(r"All \| orders"), "{md}");
        assert!(md.contains("One row<br>per order"), "{md}");
        assert!(md.contains("| customer_id | integer | yes |"), "{md}");
        assert!(md.contains("[public.customers](#publiccustomers)"), "{md}");
        assert!(md.contains("```mermaid\nerDiagram"), "{md}");
        assert!(md.contains("orders }o--|| customers"), "{md}");
    }

    #[test]
    fn html_escapes_and_embeds_mermaid() {
        let page = html(&model());
        assert!(page.contains("&lt;b&gt;pii&lt;/b&gt;"), "escaped note");
        assert!(!page.contains("<b>pii</b>"));
        assert!(page.contains("<pre class=\"mermaid\">"));
        assert!(page.contains("mermaid.initialize"));
    }

    #[test]
    fn html_without_diagrams_has_no_script() {
        let mut m = model();
        m.diagrams = false;
        assert!(!html(&m).contains("<script>"));
    }

    #[test]
    fn large_schemas_skip_the_diagram() {
        let mut m = model();
        let t = m.schemas[0].objects[0].clone();
        m.schemas[0].objects = (0..=DIAGRAM_LIMIT)
            .map(|i| DocObject {
                name: format!("t{i}"),
                ..t.clone()
            })
            .collect();
        let md = markdown(&m);
        assert!(!md.contains("erDiagram"));
        assert!(md.contains("more than 100 tables"), "{md}");
    }

    #[test]
    fn mermaid_names_are_safe() {
        assert_eq!(mermaid::entity("order items"), "\"order items\"");
        assert_eq!(mermaid::entity("orders"), "orders");
        assert_eq!(mermaid::attribute("weird name!"), "weird_name_");
        assert_eq!(mermaid::type_name("numeric(12,2)"), "numeric_12_2_");
    }

    #[test]
    fn unreadable_tables_are_noted() {
        let mut m = model();
        m.schemas[0].objects[0].error = Some("permission denied".to_owned());
        assert!(markdown(&m).contains("Could not read this table: permission denied"));
    }
}
