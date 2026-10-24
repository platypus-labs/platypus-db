//! The table a query reads, so the data editor can change the query's results: only a plain
//! `SELECT` from one table, without joins, grouping, set operations or `DISTINCT`.

use crate::{Dialect, Lexer, Token, TokenKind};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceTable {
    /// `None` when the query leaves it to the session's current schema.
    pub schema: Option<String>,
    pub name: String,
    pub items: Vec<SelectItem>,
}

/// What an item of the select list yields.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SelectItem {
    /// `*` or `t.*`: every column of the table, in order.
    All,
    /// A column of the table, named as the database stores it when the dialect folds case.
    Column(String),
    /// An expression or a renamed column, which cannot be written back.
    Other,
}

/// Words after the table that end the `FROM` clause of a single-table query.
const CLAUSES: &[&str] = &["WHERE", "ORDER", "LIMIT", "OFFSET", "FETCH", "FOR"];
/// Words that make a query read more than one table, or rows that are not the table's.
const NOT_SINGLE: &[&str] = &[
    "UNION",
    "INTERSECT",
    "EXCEPT",
    "GROUP",
    "HAVING",
    "WINDOW",
    "JOIN",
];
/// Words that cannot be a table alias.
const NOT_ALIAS: &[&str] = &[
    "JOIN",
    "INNER",
    "LEFT",
    "RIGHT",
    "FULL",
    "CROSS",
    "NATURAL",
    "STRAIGHT_JOIN",
    "UNION",
    "INTERSECT",
    "EXCEPT",
    "GROUP",
    "HAVING",
    "WINDOW",
    "USE",
    "IGNORE",
    "FORCE",
    "TABLESAMPLE",
    "PARTITION",
    "INDEXED",
    "NOT",
];

struct Query<'a> {
    src: &'a str,
    dialect: Dialect,
    tokens: Vec<Token>,
}

impl Query<'_> {
    fn is_word(&self, i: usize, words: &[&str]) -> bool {
        self.tokens.get(i).is_some_and(|t| {
            t.kind == TokenKind::Word
                && words
                    .iter()
                    .any(|w| t.text(self.src).eq_ignore_ascii_case(w))
        })
    }

    fn is_symbol(&self, i: usize, symbol: &str) -> bool {
        self.tokens
            .get(i)
            .is_some_and(|t| t.kind == TokenKind::Symbol && t.text(self.src) == symbol)
    }

    fn is_ident(&self, i: usize) -> bool {
        self.tokens
            .get(i)
            .is_some_and(|t| matches!(t.kind, TokenKind::Word | TokenKind::QuotedIdent))
    }

    /// An identifier's name: quotes removed, bare PostgreSQL names folded to lower case.
    fn name(&self, i: usize) -> String {
        let token = self.tokens[i];
        let text = token.text(self.src);
        if token.kind == TokenKind::Word {
            return if self.dialect == Dialect::Postgres {
                text.to_lowercase()
            } else {
                text.to_owned()
            };
        }
        let close = match text.chars().next() {
            Some('[') => ']',
            Some(c) => c,
            None => return String::new(),
        };
        let inner = text
            .get(1..text.len().saturating_sub(1))
            .unwrap_or_default();
        // Transact-SQL doubles `]` in `[...]` names.
        inner.replace(&format!("{close}{close}"), &close.to_string())
    }
}

/// One select item, `tokens[start..end]`: its kind and its qualifier, if any.
fn select_item(t: &Query<'_>, start: usize, end: usize) -> (SelectItem, Option<String>) {
    let len = end - start;
    if len == 1 && t.is_symbol(start, "*") {
        return (SelectItem::All, None);
    }
    // A column is one identifier or a dotted path of them; `t.*` is every column.
    let dotted = len % 2 == 1
        && (start..end).all(|i| {
            if (i - start) % 2 == 1 {
                t.is_symbol(i, ".")
            } else {
                t.is_ident(i) || (i == end - 1 && len > 1 && t.is_symbol(i, "*"))
            }
        });
    if !dotted {
        return (SelectItem::Other, None);
    }
    let qualifier = (len > 1).then(|| t.name(end - 3));
    let item = if t.is_symbol(end - 1, "*") {
        SelectItem::All
    } else {
        SelectItem::Column(t.name(end - 1))
    };
    (item, qualifier)
}

/// The table `sql` reads, when it is a `SELECT` whose rows are rows of that one table.
#[must_use]
pub fn source_table(sql: &str, dialect: Dialect) -> Option<SourceTable> {
    let mut tokens: Vec<Token> = Lexer::new(sql, dialect)
        .filter(|t| !t.is_trivia())
        .collect();
    while tokens
        .last()
        .is_some_and(|t| t.kind == TokenKind::Symbol && t.text(sql) == ";")
    {
        tokens.pop();
    }
    let t = Query {
        src: sql,
        dialect,
        tokens,
    };
    if !t.is_word(0, &["SELECT"]) || t.is_word(1, &["DISTINCT", "DISTINCTROW", "TOP"]) {
        return None;
    }
    let mut i = if t.is_word(1, &["ALL"]) { 2 } else { 1 };

    // The select list, up to FROM.
    let mut items = Vec::new();
    let mut depth = 0usize;
    let mut start = i;
    loop {
        if i >= t.tokens.len() {
            return None;
        }
        if t.is_symbol(i, "(") {
            depth += 1;
        } else if t.is_symbol(i, ")") {
            depth = depth.checked_sub(1)?;
        } else if depth == 0 && (t.is_symbol(i, ",") || t.is_word(i, &["FROM"])) {
            if i == start {
                return None;
            }
            items.push(select_item(&t, start, i));
            if t.is_word(i, &["FROM"]) {
                break;
            }
            start = i + 1;
        }
        i += 1;
    }

    // The table, an optional alias, then nothing but clauses that filter or order rows.
    i += 1;
    let mut path = Vec::new();
    while t.is_ident(i) && !t.is_symbol(i + 1, "(") {
        path.push(t.name(i));
        i += 1;
        if !t.is_symbol(i, ".") {
            break;
        }
        i += 1;
    }
    let (schema, name) = match path.len() {
        1 => (None, path.pop()?),
        2 => {
            let name = path.pop()?;
            (path.pop(), name)
        }
        _ => return None,
    };
    let mut alias = None;
    if t.is_word(i, &["AS"]) {
        if !t.is_ident(i + 1) {
            return None;
        }
        alias = Some(t.name(i + 1));
        i += 2;
    } else if t.is_ident(i) && !t.is_word(i, CLAUSES) && !t.is_word(i, NOT_ALIAS) {
        alias = Some(t.name(i));
        i += 1;
    }
    if i < t.tokens.len() && !t.is_word(i, CLAUSES) {
        return None;
    }
    let mut depth = 0usize;
    for j in i..t.tokens.len() {
        if t.is_symbol(j, "(") {
            depth += 1;
        } else if t.is_symbol(j, ")") {
            depth = depth.saturating_sub(1);
        } else if depth == 0 && t.is_word(j, NOT_SINGLE) {
            return None;
        }
    }

    // Qualified items must name this table.
    let mut select = Vec::with_capacity(items.len());
    for (item, qualifier) in items {
        if let Some(q) = qualifier {
            let matches = |n: &str| n.eq_ignore_ascii_case(&q);
            if !matches(&name) && !alias.as_deref().is_some_and(matches) {
                return None;
            }
        }
        select.push(item);
    }
    Some(SourceTable {
        schema,
        name,
        items: select,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use SelectItem::{All, Column, Other};

    fn source(sql: &str) -> Option<SourceTable> {
        source_table(sql, Dialect::Postgres)
    }

    #[test]
    fn reads_plain_selects() {
        assert_eq!(
            source("select * from items where id > 1 order by name limit 10;"),
            Some(SourceTable {
                schema: None,
                name: "items".into(),
                items: vec![All],
            })
        );
        assert_eq!(
            source(r#"SELECT i.Id, "Name", price * 2, note AS n, i.* FROM shop."Items" AS i"#),
            Some(SourceTable {
                schema: Some("shop".into()),
                name: "Items".into(),
                items: vec![
                    Column("id".into()),
                    Column("Name".into()),
                    Other,
                    Other,
                    All
                ],
            })
        );
        assert_eq!(
            source("select coalesce(a, b), c from t x for update")
                .unwrap()
                .items,
            [Other, Column("c".into())]
        );
        let mysql = source_table("SELECT `Order`, Name FROM `my db`.`t`", Dialect::MySql).unwrap();
        assert_eq!(mysql.schema.as_deref(), Some("my db"));
        assert_eq!(mysql.items, [Column("Order".into()), Column("Name".into())]);
    }

    #[test]
    fn rejects_queries_over_other_rows() {
        for sql in [
            "select * from a join b on a.id = b.id",
            "select * from a, b",
            "select * from a natural join b",
            "select distinct name from a",
            "select name, count(*) from a group by name",
            "select * from a union select * from b",
            "select * from (select 1) s",
            "select * from generate_series(1, 3)",
            "with x as (select 1) select * from x",
            "select b.id from a",
            "select 1",
            "update a set x = 1",
        ] {
            assert_eq!(source(sql), None, "{sql}");
        }
        // Subqueries in the filter do not change which rows are returned.
        assert!(source("select * from a where id in (select id from b union select 2)").is_some());
    }
}
