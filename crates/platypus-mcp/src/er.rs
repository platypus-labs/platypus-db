//! The logic of `get_er_diagram`: which tables a diagram shows (the requested ones and those
//! a few foreign-key hops away), the doc model of them, and the Mermaid text within a byte limit.

use std::collections::{HashMap, HashSet};
use std::fmt::Write as _;

use platypus_core::TableDetails;
use platypus_docs::{DocColumn, DocForeignKey, DocKind, DocObject, DocSchema, mermaid};

/// Hops followed from the requested tables when `depth` is left out.
pub const DEFAULT_DEPTH: usize = 1;
/// The most hops a call may ask for.
pub const MAX_DEPTH: usize = 3;
/// Tables read from a schema when no `tables` narrow it: the rest is reported as left out.
pub const MAX_TABLES: usize = 300;

/// The hops to follow: `requested`, else the default, up to [`MAX_DEPTH`].
pub fn hops(requested: Option<u64>) -> usize {
    requested.map_or(DEFAULT_DEPTH, |d| {
        usize::try_from(d).unwrap_or(usize::MAX).min(MAX_DEPTH)
    })
}

/// The tables a foreign key links `details` to, in the same schema: those it references and
/// those that reference it. Only names in `tables` count.
pub fn neighbors(schema: &str, details: &TableDetails, tables: &HashSet<&str>) -> Vec<String> {
    let outgoing = details
        .foreign_keys
        .iter()
        .filter(|f| f.ref_schema == schema)
        .map(|f| &f.ref_table);
    let incoming = details
        .referenced_by
        .iter()
        .filter(|r| r.schema == schema)
        .map(|r| &r.table);
    outgoing
        .chain(incoming)
        .filter(|t| tables.contains(t.as_str()))
        .cloned()
        .collect()
}

/// A walk over foreign keys: the requested tables first, then hop by hop the tables linked to
/// the last hop that were not reached yet.
pub struct Walk {
    seen: HashSet<String>,
    order: Vec<String>,
    frontier: Vec<String>,
}

impl Walk {
    /// Starts at `start`, without duplicates.
    pub fn new(start: &[String]) -> Self {
        let mut seen = HashSet::new();
        let order: Vec<String> = start
            .iter()
            .filter(|s| seen.insert((*s).clone()))
            .cloned()
            .collect();
        Self {
            seen,
            frontier: order.clone(),
            order,
        }
    }

    /// The tables reached by the last hop (the start before any hop).
    pub fn frontier(&self) -> &[String] {
        &self.frontier
    }

    /// Every table reached so far, in the order found.
    pub fn order(&self) -> &[String] {
        &self.order
    }

    /// One hop outward through `edges`; false when it reached nothing new.
    pub fn advance(&mut self, edges: &HashMap<String, Vec<String>>) -> bool {
        let mut next = Vec::new();
        for table in &self.frontier {
            for n in edges.get(table).into_iter().flatten() {
                if self.seen.insert(n.clone()) {
                    self.order.push(n.clone());
                    next.push(n.clone());
                }
            }
        }
        self.frontier = next;
        !self.frontier.is_empty()
    }
}

/// `start` and the tables within `depth` foreign-key hops of it, for tests with all the edges known.
#[cfg(test)]
fn within_hops(
    start: &[String],
    depth: usize,
    edges: &HashMap<String, Vec<String>>,
) -> Vec<String> {
    let mut walk = Walk::new(start);
    for _ in 0..depth {
        if !walk.advance(edges) {
            break;
        }
    }
    walk.order
}

/// The doc model of `chosen` (in that order): their columns and the foreign keys between
/// them. A key to a table that is not shown is left out, so no entity appears out of nowhere.
pub fn doc_schema(
    schema: &str,
    chosen: &[String],
    details: &HashMap<String, TableDetails>,
) -> DocSchema {
    let shown: HashSet<&str> = chosen.iter().map(String::as_str).collect();
    let objects = chosen
        .iter()
        .filter_map(|name| details.get(name).map(|d| (name, d)))
        .map(|(name, d)| DocObject {
            name: name.clone(),
            kind: DocKind::Table,
            columns: d
                .columns
                .iter()
                .map(|c| DocColumn {
                    name: c.name.clone(),
                    data_type: c.data_type.clone(),
                    primary_key: c.primary_key,
                    ..DocColumn::default()
                })
                .collect(),
            foreign_keys: d
                .foreign_keys
                .iter()
                .filter(|f| f.ref_schema == schema && shown.contains(f.ref_table.as_str()))
                .map(|f| DocForeignKey {
                    name: f.name.clone(),
                    columns: f.columns.clone(),
                    schema: f.ref_schema.clone(),
                    table: f.ref_table.clone(),
                    ref_columns: f.ref_columns.clone(),
                })
                .collect(),
            ..DocObject::default()
        })
        .collect();
    DocSchema {
        name: schema.to_owned(),
        objects,
    }
}

/// The first `tables` tables of `schema`, each with its first `columns` columns and the keys
/// between those tables.
fn first_tables(schema: &DocSchema, tables: usize, columns: usize) -> DocSchema {
    let keep: HashSet<&str> = schema
        .objects
        .iter()
        .take(tables)
        .map(|o| o.name.as_str())
        .collect();
    let objects = schema
        .objects
        .iter()
        .take(tables)
        .map(|o| DocObject {
            columns: o.columns.iter().take(columns).cloned().collect(),
            foreign_keys: o
                .foreign_keys
                .iter()
                .filter(|f| keep.contains(f.table.as_str()))
                .cloned()
                .collect(),
            ..o.clone()
        })
        .collect();
    DocSchema {
        name: schema.name.clone(),
        objects,
    }
}

/// The Mermaid `erDiagram` of `schema`, which holds `total` tables or the first of them,
/// within `max_bytes`. When it is too big: as many whole tables as fit (the columns of a
/// first table that alone is too big are cut), then a comment saying so and pointing at
/// `tables`.
pub fn render(schema: &DocSchema, total: usize, max_bytes: usize) -> String {
    let all = schema.objects.len();
    let fits = |tables: usize, columns: usize| {
        mermaid::diagram(&first_tables(schema, tables, columns)).len() <= max_bytes
    };
    // The largest count of tables that fits: the text only grows with each table.
    let (mut lo, mut hi) = (0, all);
    while lo < hi {
        let mid = (lo + hi).div_ceil(2);
        if fits(mid, usize::MAX) {
            lo = mid;
        } else {
            hi = mid - 1;
        }
    }
    let mut columns = usize::MAX;
    if lo == 0 && all > 0 {
        lo = 1;
        columns = schema.objects[0].columns.len();
        while columns > 1 && !fits(1, columns) {
            columns /= 2;
        }
    }
    let shown = first_tables(schema, lo, columns);
    let mut out = mermaid::diagram(&shown);
    if lo == 0 {
        out.push_str("    %% the schema has no tables\n");
    } else if shown.objects.iter().all(|o| o.foreign_keys.is_empty()) {
        out.push_str("    %% no foreign keys between these tables\n");
    }
    if lo < total {
        let _ = writeln!(
            out,
            "    %% cut: {lo} of {total} tables shown (limit {max_bytes} bytes). Pass `tables` \
             (and `depth`) to choose the tables to draw."
        );
    }
    if let Some(first) = schema.objects.first()
        && columns < first.columns.len()
    {
        let _ = writeln!(
            out,
            "    %% cut: {columns} of the {} columns of the first table shown (limit {max_bytes} \
             bytes): describe_table has them all.",
            first.columns.len()
        );
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_lone_table_with_its_columns_cut_says_so() {
        let schema = DocSchema {
            name: "s".to_owned(),
            objects: vec![DocObject {
                name: "wide".to_owned(),
                kind: DocKind::Table,
                columns: (0..2000)
                    .map(|i| DocColumn {
                        name: format!("column_{i}"),
                        data_type: "text".to_owned(),
                        ..DocColumn::default()
                    })
                    .collect(),
                ..DocObject::default()
            }],
        };
        let text = render(&schema, 1, 4096);
        assert!(text.contains("of the 2000 columns"), "{text}");
        assert!(!text.contains("tables shown"), "{text}");
        assert!(!render(&schema, 1, 1 << 20).contains("cut:"));
    }

    fn edges(pairs: &[(&str, &str)]) -> HashMap<String, Vec<String>> {
        let mut map: HashMap<String, Vec<String>> = HashMap::new();
        for (a, b) in pairs {
            map.entry((*a).to_owned())
                .or_default()
                .push((*b).to_owned());
            map.entry((*b).to_owned())
                .or_default()
                .push((*a).to_owned());
        }
        map
    }

    fn names(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| (*s).to_owned()).collect()
    }

    #[test]
    fn no_hops_keeps_the_requested_tables() {
        let e = edges(&[("order", "customer")]);
        assert_eq!(within_hops(&names(&["order"]), 0, &e), names(&["order"]));
    }

    #[test]
    fn one_hop_follows_keys_both_ways() {
        // order -> customer (outgoing), item -> order (incoming); product is two hops away.
        let e = edges(&[
            ("order", "customer"),
            ("item", "order"),
            ("item", "product"),
        ]);
        assert_eq!(
            within_hops(&names(&["order"]), 1, &e),
            names(&["order", "customer", "item"])
        );
        assert_eq!(
            within_hops(&names(&["order"]), 2, &e),
            names(&["order", "customer", "item", "product"])
        );
    }

    #[test]
    fn cycles_and_self_references_end() {
        let e = edges(&[("a", "b"), ("b", "c"), ("c", "a"), ("a", "a")]);
        assert_eq!(within_hops(&names(&["a"]), 3, &e), names(&["a", "b", "c"]));
    }

    #[test]
    fn several_requested_tables_and_duplicates() {
        let e = edges(&[("a", "b"), ("c", "d")]);
        assert_eq!(
            within_hops(&names(&["a", "c", "a"]), 1, &e),
            names(&["a", "c", "b", "d"])
        );
    }

    #[test]
    fn depth_is_capped_and_defaults_to_one() {
        assert_eq!(hops(None), DEFAULT_DEPTH);
        assert_eq!(hops(Some(0)), 0);
        assert_eq!(hops(Some(2)), 2);
        assert_eq!(hops(Some(99)), MAX_DEPTH);
        assert_eq!(hops(Some(u64::MAX)), MAX_DEPTH);
    }

    fn table(name: &str, columns: usize, to: &[&str]) -> DocObject {
        DocObject {
            name: name.to_owned(),
            columns: (0..columns)
                .map(|i| DocColumn {
                    name: format!("col{i}"),
                    data_type: "int".to_owned(),
                    primary_key: i == 0,
                    ..DocColumn::default()
                })
                .collect(),
            foreign_keys: to
                .iter()
                .map(|t| DocForeignKey {
                    name: format!("{name}_{t}"),
                    columns: vec!["col1".to_owned()],
                    schema: "main".to_owned(),
                    table: (*t).to_owned(),
                    ref_columns: vec!["col0".to_owned()],
                })
                .collect(),
            ..DocObject::default()
        }
    }

    fn schema(objects: Vec<DocObject>) -> DocSchema {
        DocSchema {
            name: "main".to_owned(),
            objects,
        }
    }

    #[test]
    fn a_small_diagram_is_drawn_whole() {
        let s = schema(vec![table("a", 2, &["b"]), table("b", 1, &[])]);
        let out = render(&s, 2, 10_000);
        assert!(out.starts_with("erDiagram\n"));
        assert!(out.contains("\"a\" }o--|| \"b\" : \"a_b\""));
        assert!(!out.contains("cut:"));
    }

    #[test]
    fn a_big_diagram_is_cut_by_whole_tables_with_a_note() {
        let s = schema((0..50).map(|i| table(&format!("t{i}"), 5, &[])).collect());
        let out = render(&s, 50, 600);
        assert!(out.contains("cut: "), "{out}");
        assert!(out.contains("Pass `tables`"));
        // Every opened block is closed.
        assert_eq!(out.matches('{').count(), out.matches('}').count());
    }

    #[test]
    fn a_cut_leaves_no_key_to_a_table_left_out() {
        let s = schema(vec![table("a", 2, &["z"]), table("z", 20, &[])]);
        let out = render(&s, 2, 220);
        assert!(!out.contains("}o--||"), "{out}");
    }

    #[test]
    fn one_huge_table_loses_columns() {
        let s = schema(vec![table("big", 500, &[])]);
        let out = render(&s, 1, 800);
        assert!(out.contains("\"big\""));
        assert!(out.contains("col0"));
        assert!(!out.contains("col499"));
    }

    #[test]
    fn empty_schemas_and_missing_keys_answer_sensibly() {
        assert!(render(&schema(vec![]), 0, 1000).contains("no tables"));
        let out = render(&schema(vec![table("a", 1, &[])]), 1, 1000);
        assert!(out.contains("no foreign keys"));
    }
}
