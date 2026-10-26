//! What a quick filter on a result column tests in the `WHERE`: the column of the `FROM`
//! tables that the select list shows there. A computed column (an expression, an aggregate, a
//! window function, a literal) has none: a `WHERE` naming it is refused by the database, or
//! names another column.

use super::{Body, Tokens, flag, scan_tokens};
use crate::{Dialect, TokenKind};

/// Words that read as a bare name but are a value: an item made of one of them is computed.
const VALUE_WORDS: &[&str] = &[
    "NULL",
    "TRUE",
    "FALSE",
    "UNKNOWN",
    "DEFAULT",
    "CURRENT_DATE",
    "CURRENT_TIME",
    "CURRENT_TIMESTAMP",
    "LOCALTIME",
    "LOCALTIMESTAMP",
    "CURRENT_USER",
    "SESSION_USER",
    "SYSDATE",
    "SYSTIMESTAMP",
];

/// What an item of the select list reads.
#[derive(Debug, PartialEq, Eq)]
enum Item {
    /// `*` or `t.*`: columns of the `FROM` tables, with the qualifier as written (`t.`).
    Star(String),
    /// A column of the `FROM` tables, as written, without its alias.
    Column(String),
    /// Anything else: a `WHERE` cannot name it.
    Computed,
}

impl Tokens<'_> {
    fn is_ident(&self, i: usize) -> bool {
        self.list
            .get(i)
            .is_some_and(|t| matches!(t.kind, TokenKind::Word | TokenKind::QuotedIdent))
    }

    /// The text of the tokens `from..to`, without the whitespace and comments between them.
    fn joined(&self, from: usize, to: usize) -> String {
        self.list[from..to]
            .iter()
            .map(|t| t.text(self.sql))
            .collect()
    }
}

/// What the select-list item made of the tokens `from..to` reads.
fn item(t: &Tokens, from: usize, mut to: usize) -> Item {
    // Its alias: `AS n` (`AS 'n'` too), or a name right after another one (`t.a n`).
    if to >= from + 3 && t.is_word(to - 2, "AS") && t.list[to - 1].kind != TokenKind::Symbol {
        to -= 2;
    } else if to >= from + 2 && t.is_ident(to - 1) && t.is_ident(to - 2) {
        to -= 1;
    }
    if to <= from {
        return Item::Computed;
    }
    // A dotted path of names, or of qualifiers and a final `*`.
    let star = t.is_symbol(to - 1, "*");
    let path = (to - from) % 2 == 1
        && (from..to).all(|i| match (i - from) % 2 {
            1 => t.is_symbol(i, "."),
            _ => t.is_ident(i) || (star && i == to - 1),
        });
    let value = to == from + 1
        && t.word(from)
            .is_some_and(|w| VALUE_WORDS.contains(&w.as_str()));
    if !path || value {
        Item::Computed
    } else if star {
        Item::Star(t.joined(from, to - 1))
    } else {
        Item::Column(t.joined(from, to))
    }
}

/// The items of the select list: what its top-level commas separate.
fn items(t: &Tokens, body: &Body) -> Option<Vec<Item>> {
    let mut start = t.list.iter().position(|x| x.start == body.list)?;
    let end = t.list.iter().position(|x| Some(x.start) == body.from)?;
    if body.has(flag::DISTINCT_ON) && t.is_word(start, "ON") && t.is_symbol(start + 1, "(") {
        start = t.after_group(start + 1);
    }
    let mut items = Vec::new();
    let mut from = start;
    for i in start..end {
        if t.depth[i] == 0 && t.is_symbol(i, ",") {
            items.push(item(t, from, i));
            from = i + 1;
        }
    }
    items.push(item(t, from, end));
    Some(items)
}

/// For each column of the result of `sql`, named `names` in order, what a quick filter on it
/// tests in the `WHERE`: the column the select list shows there as written (`p.name` for
/// `p.name AS n`), or a column of `*` by its name, quoted for `dialect`. `None` for a computed
/// column. The items before the first `*` and after the last one give the first and last
/// columns; the rest are the stars' columns, which only one star, or stars alone, tell apart.
///
/// `None` overall when `add_condition` cannot add a condition to `sql`, or when the select
/// list does not fit `names` (another statement's result).
#[must_use]
pub fn filter_columns(sql: &str, dialect: Dialect, names: &[&str]) -> Option<Vec<Option<String>>> {
    let (t, body) = scan_tokens(sql, dialect)?;
    if body.from.is_none() || body.has(flag::SET_OPERATION) {
        return None;
    }
    let items = items(&t, &body)?;
    let is_star = |i: &Item| matches!(i, Item::Star(_));
    let stars = items
        .iter()
        .position(is_star)
        .zip(items.iter().rposition(is_star));
    let (head, tail) = stars.map_or((items.len(), 0), |(first, last)| {
        (first, items.len() - last - 1)
    });
    if names.len() < head + tail || (stars.is_none() && names.len() != items.len()) {
        return None;
    }
    // The qualifier of the stars' columns: one star's, none for several stars side by side.
    let qualifier = stars.and_then(|(first, last)| match &items[first..=last] {
        [Item::Star(qualifier)] => Some(qualifier.clone()),
        between => between.iter().all(is_star).then(String::new),
    });
    let item_of = |i: usize| {
        if i < head {
            Some(&items[i])
        } else if i >= names.len() - tail {
            Some(&items[items.len() - (names.len() - i)])
        } else {
            None
        }
    };
    let columns = names.iter().enumerate().map(|(i, name)| match item_of(i) {
        Some(Item::Column(column)) => Some(column.clone()),
        Some(_) => None,
        None => qualifier
            .as_ref()
            .map(|q| format!("{q}{}", dialect.quote_ident(name))),
    });
    Some(columns.collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    const PG: Dialect = Dialect::Postgres;
    const MY: Dialect = Dialect::MySql;
    const MS: Dialect = Dialect::SqlServer;

    /// What the quick filters test for each column, `-` for none.
    fn tested(sql: &str, dialect: Dialect, names: &[&str]) -> Vec<String> {
        filter_columns(sql, dialect, names)
            .unwrap()
            .into_iter()
            .map(|c| c.unwrap_or_else(|| "-".to_owned()))
            .collect()
    }

    #[test]
    fn a_plain_column_is_tested_as_written() {
        assert_eq!(
            tested(
                "SELECT id, p.name, \"Total\" FROM p",
                PG,
                &["id", "name", "Total"]
            ),
            ["id", "p.name", "\"Total\""]
        );
        assert_eq!(
            tested("SELECT TOP 5 [Order Date] FROM t", MS, &["Order Date"]),
            ["[Order Date]"]
        );
        assert_eq!(
            tested("SELECT DISTINCT ON (a) a, b FROM t", PG, &["a", "b"]),
            ["a", "b"]
        );
    }

    #[test]
    fn an_alias_of_a_column_tests_the_column() {
        assert_eq!(
            tested(
                "SELECT name AS n, p.price \"Price\", q /* c */ AS r FROM p",
                PG,
                &["n", "Price", "r"]
            ),
            ["name", "p.price", "q"]
        );
        // MySQL, SQLite and Transact-SQL also take a string as the alias.
        assert_eq!(tested("SELECT name AS 'n' FROM t", MY, &["n"]), ["name"]);
    }

    #[test]
    fn a_computed_column_is_not_tested() {
        let sql = "SELECT count(*) AS n, price * qty AS total, row_number() OVER (ORDER BY id) rn, \
                   1 AS one, 'x', NULL, CURRENT_DATE, (SELECT max(a) FROM u) AS m, a::int, \
                   CASE WHEN a THEN 1 END, -a, id FROM t";
        let names = [
            "n", "total", "rn", "one", "x", "null", "d", "m", "a", "case", "neg", "id",
        ];
        let mut expected = vec!["-"; 11];
        expected.push("id");
        assert_eq!(tested(sql, PG, &names), expected);
        // A grouped query: the groups' columns, never the aggregates.
        assert_eq!(
            tested(
                "SELECT grp, count(*) FROM t GROUP BY grp",
                PG,
                &["grp", "count"]
            ),
            ["grp", "-"]
        );
        // A quoted name is a column, even one spelled like a value.
        assert_eq!(
            tested("SELECT \"null\" FROM t", PG, &["null"]),
            ["\"null\""]
        );
    }

    #[test]
    fn a_star_column_is_tested_by_its_name_quoted_per_dialect() {
        let names = ["id", "Order Date", "select"];
        assert_eq!(
            tested("SELECT * FROM t", PG, &names),
            ["id", "\"Order Date\"", "\"select\""]
        );
        assert_eq!(
            tested("SELECT * FROM t", MY, &names),
            ["id", "`Order Date`", "`select`"]
        );
        assert_eq!(
            tested("SELECT * FROM t", MS, &names),
            ["id", "[Order Date]", "[select]"]
        );
    }

    #[test]
    fn the_items_around_a_star_give_the_first_and_last_columns() {
        assert_eq!(
            tested(
                "SELECT price * 2 AS double, t.*, u.label FROM t JOIN u ON u.id = t.id",
                PG,
                &["double", "id", "name", "label"]
            ),
            ["-", "t.id", "t.name", "u.label"]
        );
        assert_eq!(
            tested(
                "SELECT *, count(*) OVER () AS total FROM t",
                PG,
                &["id", "total"]
            ),
            ["id", "-"]
        );
        // Several stars: their columns cannot be told apart, nor from an item between them.
        assert_eq!(
            tested("SELECT a.*, b.* FROM a, b", PG, &["x", "y"]),
            ["x", "y"]
        );
        assert_eq!(
            tested(
                "SELECT a.*, 1 AS one, b.* FROM a, b",
                PG,
                &["x", "one", "y"]
            ),
            ["-", "-", "-"]
        );
    }

    #[test]
    fn gives_up_where_no_condition_can_go_or_the_columns_do_not_fit() {
        for (sql, names) in [
            ("SELECT a FROM x UNION SELECT a FROM y", &["a"][..]),
            ("SELECT 1", &["1"]),
            ("SELECT a, b FROM t", &["a"]),
            ("SELECT a, *, b FROM t", &["a"]),
            ("UPDATE t SET a = 1", &[]),
        ] {
            assert_eq!(filter_columns(sql, PG, names), None, "{sql}");
        }
    }
}
