//! What the editor's linter checks in a statement: `SELECT *`, joins without a condition,
//! and the qualified column references (`o.total`) with the tables their qualifiers name,
//! which the editor looks up in the catalog it has.

use crate::analysis::Words;
use crate::{Dialect, Lexer, TableName, TokenKind};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LintKind {
    SelectStar,
    /// A `JOIN` without `ON` or `USING`, which joins every row with every row.
    JoinWithoutCondition,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LintIssue {
    pub kind: LintKind,
    pub start: usize,
    pub end: usize,
}

/// A name a statement gives a table in `FROM`, `JOIN` or `UPDATE`: its alias, or the
/// table's own name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TableAlias {
    pub alias: String,
    /// `None` for a subquery, a function or a common table expression.
    pub table: Option<TableName>,
}

/// `qualifier.column`; the range is the column's.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ColumnRef {
    pub qualifier: String,
    pub column: String,
    pub start: usize,
    pub end: usize,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Lint {
    pub issues: Vec<LintIssue>,
    pub tables: Vec<TableAlias>,
    pub columns: Vec<ColumnRef>,
}

/// Words that end the table reference of a `JOIN`.
const CLAUSE_END: &[&str] = &[
    "JOIN",
    "INNER",
    "LEFT",
    "RIGHT",
    "FULL",
    "CROSS",
    "NATURAL",
    "WHERE",
    "GROUP",
    "HAVING",
    "WINDOW",
    "ORDER",
    "LIMIT",
    "OFFSET",
    "FETCH",
    "FOR",
    "UNION",
    "EXCEPT",
    "INTERSECT",
    "RETURNING",
];

/// Words after a table that are not its alias.
const NOT_ALIAS: &[&str] = &[
    "ON",
    "USING",
    "SET",
    "AS",
    "OUTER",
    "STRAIGHT_JOIN",
    "TABLESAMPLE",
];

fn is_name(w: &Words<'_>, i: usize) -> bool {
    w.tokens
        .get(i)
        .is_some_and(|t| matches!(t.kind, TokenKind::Word | TokenKind::QuotedIdent))
}

/// The index of the `)` closing the `(` at `open`, or the last token.
fn closing(w: &Words<'_>, open: usize) -> usize {
    let mut depth = 0usize;
    for i in open..w.tokens.len() {
        if w.symbol(i, "(") {
            depth += 1;
        } else if w.symbol(i, ")") {
            depth -= 1;
            if depth == 0 {
                return i;
            }
        }
    }
    w.tokens.len().saturating_sub(1)
}

/// Names of the common table expressions: `WITH name AS (`, `, name AS (`.
fn ctes(w: &Words<'_>, dialect: Dialect) -> Vec<String> {
    (1..w.tokens.len())
        .filter(|&i| {
            (w.is(i - 1, &["WITH", "RECURSIVE"]) || w.symbol(i - 1, ","))
                && w.is(i + 1, &["AS"])
                && w.symbol(i + 2, "(")
        })
        .filter_map(|i| w.ident(i, dialect))
        .collect()
}

/// Reads the table list after `FROM`, `JOIN` or `UPDATE` at `at`; returns the token
/// indices that name tables, which are not column references.
fn tables(
    w: &Words<'_>,
    at: usize,
    dialect: Dialect,
    ctes: &[String],
    out: &mut Vec<TableAlias>,
) -> Vec<usize> {
    let mut names = Vec::new();
    let mut j = at + 1;
    loop {
        while w.is(j, &["ONLY", "LATERAL"]) {
            j += 1;
        }
        let mut table = if w.symbol(j, "(") {
            j = closing(w, j) + 1;
            None
        } else if is_name(w, j)
            && let Some(t) = w.table(j, dialect)
        {
            names.push(j);
            j += if t.schema.is_some() { 3 } else { 1 };
            Some(t)
        } else {
            break;
        };
        if w.symbol(j, "(") {
            // A function in FROM, e.g. `generate_series(1, 10) g`.
            j = closing(w, j) + 1;
            table = None;
        }
        if w.is(j, &["AS"]) {
            j += 1;
        }
        let alias = if is_name(w, j) && !w.is(j, CLAUSE_END) && !w.is(j, NOT_ALIAS) {
            j += 1;
            w.ident(j - 1, dialect)
        } else {
            table.as_ref().map(|t| t.name.clone())
        };
        if table
            .as_ref()
            .is_some_and(|t| t.schema.is_none() && ctes.contains(&t.name))
        {
            table = None;
        }
        if let Some(alias) = alias {
            out.push(TableAlias { alias, table });
        }
        if !(w.is(at, &["FROM"]) && w.symbol(j, ",")) {
            return names;
        }
        j += 1;
    }
    names
}

fn join_issue(w: &Words<'_>, at: usize) -> Option<LintIssue> {
    if w.is(at.wrapping_sub(1), &["CROSS", "NATURAL"]) || w.is(at.wrapping_sub(2), &["NATURAL"]) {
        return None;
    }
    let mut depth = 0usize;
    let mut last = at;
    for i in at + 1..w.tokens.len() {
        if w.symbol(i, "(") {
            depth += 1;
        } else if w.symbol(i, ")") {
            if depth == 0 {
                break;
            }
            depth -= 1;
        } else if depth == 0 {
            if w.is(i, &["ON", "USING"]) {
                return None;
            }
            if w.is(i, CLAUSE_END) || w.symbol(i, ";") {
                break;
            }
        }
        last = i;
    }
    Some(LintIssue {
        kind: LintKind::JoinWithoutCondition,
        start: w.tokens[at].start,
        end: w.tokens[last].end,
    })
}

fn select_star(w: &Words<'_>, i: usize) -> bool {
    if !w.symbol(i, "*") || i == 0 {
        return false;
    }
    if w.is(i - 1, &["SELECT"]) && i >= 3 && w.symbol(i - 2, "(") && w.is(i - 3, &["EXISTS"]) {
        return false;
    }
    w.is(i - 1, &["SELECT", "DISTINCT", "ALL"]) || w.symbol(i - 1, ",") || w.symbol(i - 1, ".")
}

/// Lints one statement; offsets are byte offsets into `sql`.
#[must_use]
pub fn lint(sql: &str, dialect: Dialect) -> Lint {
    let w = Words {
        src: sql,
        tokens: Lexer::new(sql, dialect)
            .filter(|t| !t.is_trivia())
            .collect(),
    };
    let ctes = ctes(&w, dialect);
    let mut lint = Lint::default();
    let mut table_names = Vec::new();
    for i in 0..w.tokens.len() {
        if w.is(i, &["FROM", "JOIN", "UPDATE"]) {
            table_names.extend(tables(&w, i, dialect, &ctes, &mut lint.tables));
        }
        if w.is(i, &["JOIN"]) {
            lint.issues.extend(join_issue(&w, i));
        }
        if select_star(&w, i) {
            let t = w.tokens[i];
            lint.issues.push(LintIssue {
                kind: LintKind::SelectStar,
                start: t.start,
                end: t.end,
            });
        }
    }
    // `a.b` or `s.a.b`, starting at a name that does not follow a dot.
    for i in 0..w.tokens.len() {
        if !is_name(&w, i) || (i > 0 && w.symbol(i - 1, ".")) || table_names.contains(&i) {
            continue;
        }
        let mut parts = vec![i];
        while w.symbol(parts[parts.len() - 1] + 1, ".") && is_name(&w, parts[parts.len() - 1] + 2) {
            parts.push(parts[parts.len() - 1] + 2);
        }
        let (Some(&column), true) = (parts.last(), parts.len() >= 2) else {
            continue;
        };
        // A function call such as `pg_catalog.now()`.
        if w.symbol(column + 1, "(") || w.symbol(column + 1, ".") {
            continue;
        }
        let qualifier = parts[parts.len() - 2];
        if let (Some(q), Some(c)) = (w.ident(qualifier, dialect), w.ident(column, dialect)) {
            let t = w.tokens[column];
            lint.columns.push(ColumnRef {
                qualifier: q,
                column: c,
                start: t.start,
                end: t.end,
            });
        }
    }
    lint
}

#[cfg(test)]
mod tests {
    use super::*;

    fn table(schema: Option<&str>, name: &str) -> TableName {
        TableName {
            schema: schema.map(str::to_owned),
            name: name.to_owned(),
        }
    }

    fn kinds(sql: &str) -> Vec<(LintKind, &str)> {
        lint(sql, Dialect::Postgres)
            .issues
            .into_iter()
            .map(|i| (i.kind, &sql[i.start..i.end]))
            .collect()
    }

    #[test]
    fn finds_stars_and_joins_without_condition() {
        use LintKind::*;
        assert_eq!(
            kinds("select * from a join b where x = 1"),
            [(SelectStar, "*"), (JoinWithoutCondition, "join b")]
        );
        assert_eq!(
            kinds("select o.* from a o join b on true"),
            [(SelectStar, "*")]
        );
        for sql in [
            "select count(*) from a cross join b",
            "select a from x natural left join y",
            "select 2 * 3 from a join b using (id)",
            "select 1 where exists (select * from t)",
            "select 1 from a left join (select 1 from c join d on true) s on true",
        ] {
            assert_eq!(kinds(sql), [], "{sql}");
        }
        assert_eq!(
            kinds("select 1 from (select 1 from c join d) s")[0].1,
            "join d"
        );
    }

    #[test]
    fn collects_tables_and_column_references() {
        let sql = "with t as (select 1 as x) \
                   select o.total, s.orders.id, t.x, \"O\".a, now(), pg_catalog.now() \
                   from shop.orders o, t join items as i on i.order_id = o.id \
                   join generate_series(1, 2) g on true join (select 1) q on true";
        let l = lint(sql, Dialect::Postgres);
        let tables: Vec<_> = l
            .tables
            .iter()
            .map(|t| (t.alias.as_str(), t.table.clone()))
            .collect();
        assert_eq!(
            tables,
            [
                ("o", Some(table(Some("shop"), "orders"))),
                ("t", None),
                ("i", Some(table(None, "items"))),
                ("g", None),
                ("q", None),
            ]
        );
        let columns: Vec<_> = l
            .columns
            .iter()
            .map(|c| {
                (
                    c.qualifier.as_str(),
                    c.column.as_str(),
                    &sql[c.start..c.end],
                )
            })
            .collect();
        assert_eq!(
            columns,
            [
                ("o", "total", "total"),
                ("orders", "id", "id"),
                ("t", "x", "x"),
                ("O", "a", "a"),
                ("i", "order_id", "order_id"),
                ("o", "id", "id"),
            ]
        );
        let update = lint("update orders set total = orders.total + 1", Dialect::MySql);
        assert_eq!(update.tables[0].alias, "orders");
    }
}
