//! What a quick filter on a result column tests in the `WHERE`: the column of the `FROM`
//! tables that the select list shows there. A computed column (an expression, an aggregate, a
//! window function, a literal) has none: a `WHERE` naming it is refused by the database, or
//! names another column.

use super::{Body, Tokens, flag, scan_tokens};
use crate::dialect::is_reserved;
use crate::lexer::unquote_ident;
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

/// Oracle's pseudo-columns, which a `WHERE` reads otherwise than the select list does.
const ORACLE_VALUE_WORDS: &[&str] = &["ROWNUM", "LEVEL", "CONNECT_BY_ISLEAF", "CONNECT_BY_ISCYCLE"];

/// Words before a name that make it an expression rather than a column and its alias
/// (`NOT active`, `PRIOR id`), besides the reserved words.
const PREFIX_WORDS: &[&str] = &["NOT", "PRIOR", "CONNECT_BY_ROOT", "BINARY", "INTERVAL"];

/// What a quick filter on one result column can name in the `WHERE`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FilterColumn {
    /// This SQL: the column the result column shows, as written or quoted for the dialect.
    Column(String),
    /// An expression, an aggregate, a window function or a literal.
    Computed,
    /// A column of `*` whose table cannot be told: another result column has its name, or
    /// other items stand between the stars around it.
    Ambiguous,
}

/// A star of the select list, with what its modifiers do to its columns.
#[derive(Debug, Default, PartialEq, Eq)]
struct Star {
    /// The qualifier as written (`t.`), empty for a bare `*`.
    qualifier: String,
    /// `RENAME (a AS b)`: the column shown as `b` is `a`, as written.
    renamed: Vec<(String, String)>,
    /// `REPLACE (a + 1 AS a)`: the columns shown with these names are expressions.
    replaced: Vec<String>,
    /// `APPLY (f)`: every column is an expression.
    applied: bool,
}

impl Star {
    /// What a filter on its column shown as `name` names, when a modifier changed it.
    fn changed(&self, name: &str) -> Option<FilterColumn> {
        let named = |shown: &String| shown.eq_ignore_ascii_case(name);
        if let Some((_, column)) = self.renamed.iter().find(|(shown, _)| named(shown)) {
            return Some(FilterColumn::Column(format!("{}{column}", self.qualifier)));
        }
        (self.applied || self.replaced.iter().any(named)).then_some(FilterColumn::Computed)
    }
}

/// What an item of the select list reads.
#[derive(Debug, PartialEq, Eq)]
enum Item {
    /// `*` or `t.*`: columns of the `FROM` tables.
    Star(Star),
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

    /// The name the token at `i` gives, without its quotes.
    fn name(&self, i: usize) -> String {
        let text = self.list[i].text(self.sql);
        match self.list[i].kind {
            TokenKind::QuotedIdent => unquote_ident(text).unwrap_or_else(|| text.to_owned()),
            _ => text.to_owned(),
        }
    }
}

/// The `expr AS name` elements of the tokens `from..to`, separated by commas: each as
/// written, with its name. `None` when one has no `AS name`.
fn aliases(t: &Tokens, from: usize, to: usize) -> Option<Vec<(String, String)>> {
    let base = *t.depth.get(from)?;
    let mut out = Vec::new();
    let mut start = from;
    for i in from..=to {
        if i < to && !(t.depth[i] == base && t.is_symbol(i, ",")) {
            continue;
        }
        if i < start + 3 || !t.is_word(i - 2, "AS") {
            return None;
        }
        out.push((t.joined(start, i - 2), t.name(i - 1)));
        start = i + 1;
    }
    Some(out)
}

/// The star made of the tokens `from..to`: qualifiers and dots up to a `*`, then its modifiers
/// (`EXCLUDE`, `EXCEPT`, `ILIKE`, `LIKE`, `REPLACE`, `RENAME`, `APPLY`), each with a
/// parenthesised list or a single argument. `None` when it is not a star, or has a modifier it
/// does not know.
fn star(t: &Tokens, from: usize, to: usize) -> Option<Star> {
    let mut i = from;
    while i + 1 < to && t.is_ident(i) && t.is_symbol(i + 1, ".") {
        i += 2;
    }
    if i >= to || !t.is_symbol(i, "*") {
        return None;
    }
    let mut star = Star {
        qualifier: t.joined(from, i),
        ..Star::default()
    };
    i += 1;
    while i < to {
        let word = t.word(i)?;
        // Its argument: a parenthesised list, `a AS b`, or one token.
        let (arg, next) = if t.is_symbol(i + 1, "(") {
            let end = t.after_group(i + 1);
            ((i + 2, end - 1), end)
        } else if t.is_word(i + 2, "AS") {
            ((i + 1, i + 4), i + 4)
        } else {
            ((i + 1, i + 2), i + 2)
        };
        if next > to {
            return None;
        }
        match word.as_str() {
            // The other columns keep their names and values.
            "EXCLUDE" | "EXCEPT" | "ILIKE" | "LIKE" => {}
            "REPLACE" => star
                .replaced
                .extend(aliases(t, arg.0, arg.1)?.into_iter().map(|(_, name)| name)),
            "RENAME" => star.renamed.extend(
                aliases(t, arg.0, arg.1)?
                    .into_iter()
                    .map(|(column, name)| (name, column)),
            ),
            "APPLY" => star.applied = true,
            _ => return None,
        }
        i = next;
    }
    Some(star)
}

/// Whether the word at `i` makes the name after it part of an expression (`NOT active`).
fn prefix_word(t: &Tokens, i: usize, dialect: Dialect) -> bool {
    t.word(i)
        .is_some_and(|w| PREFIX_WORDS.contains(&w.as_str()) || is_reserved(&w, dialect))
}

/// What the select-list item made of the tokens `from..to` reads.
fn item(t: &Tokens, from: usize, mut to: usize, dialect: Dialect) -> Item {
    if let Some(star) = star(t, from, to) {
        return Item::Star(star);
    }
    // Its alias: `AS n` (`AS 'n'` too), or a name right after another one (`t.a n`).
    if to >= from + 3 && t.is_word(to - 2, "AS") && t.list[to - 1].kind != TokenKind::Symbol {
        to -= 2;
    } else if to >= from + 2
        && t.is_ident(to - 1)
        && t.is_ident(to - 2)
        && !prefix_word(t, to - 2, dialect)
    {
        to -= 1;
    }
    // A dotted path of names.
    let path = to > from
        && (to - from) % 2 == 1
        && (from..to).all(|i| match (i - from) % 2 {
            1 => t.is_symbol(i, "."),
            _ => t.is_ident(i),
        });
    let value = to == from + 1
        && t.word(from).is_some_and(|w| {
            VALUE_WORDS.contains(&w.as_str())
                || (dialect == Dialect::Oracle && ORACLE_VALUE_WORDS.contains(&w.as_str()))
        });
    if path && !value {
        Item::Column(t.joined(from, to))
    } else {
        Item::Computed
    }
}

/// The items of the select list: what its top-level commas separate.
fn items(t: &Tokens, body: &Body, dialect: Dialect) -> Option<Vec<Item>> {
    let mut start = t.list.iter().position(|x| x.start == body.list)?;
    let end = t.list.iter().position(|x| Some(x.start) == body.from)?;
    if body.has(flag::DISTINCT_ON) && t.is_word(start, "ON") && t.is_symbol(start + 1, "(") {
        start = t.after_group(start + 1);
    }
    let mut items = Vec::new();
    let mut from = start;
    for i in start..end {
        if t.depth[i] == 0 && t.is_symbol(i, ",") {
            items.push(item(t, from, i, dialect));
            from = i + 1;
        }
    }
    items.push(item(t, from, end, dialect));
    Some(items)
}

/// What a filter on the column `name` of `stars`, side by side, names: the column a modifier
/// shows under that name, else the name, quoted for `dialect` and qualified like a lone star.
/// `duplicated`: another result column has the same name.
fn star_column(stars: &[&Star], name: &str, duplicated: bool, dialect: Dialect) -> FilterColumn {
    if let Some(changed) = stars.iter().find_map(|s| s.changed(name)) {
        return changed;
    }
    match stars {
        [star] if !star.qualifier.is_empty() => {
            FilterColumn::Column(format!("{}{}", star.qualifier, dialect.quote_ident(name)))
        }
        _ if duplicated => FilterColumn::Ambiguous,
        _ => FilterColumn::Column(dialect.quote_ident(name).into_owned()),
    }
}

/// For each column of the result of `sql`, named `names` in order, what a quick filter on it
/// names in the `WHERE`: the column the select list shows there as written (`p.name` for
/// `p.name AS n`), or a column of a star by its name, quoted for `dialect`; see `FilterColumn`
/// for the others. The items before the first star and after the last one give the first and
/// last columns; the rest are the stars' columns, told apart only when the stars stand side
/// by side.
///
/// `None` overall when `add_condition` cannot add a condition to `sql`, or when the select
/// list does not fit `names` (another statement's result).
#[must_use]
pub fn filter_columns(sql: &str, dialect: Dialect, names: &[&str]) -> Option<Vec<FilterColumn>> {
    let (t, body) = scan_tokens(sql, dialect)?;
    if body.from.is_none() || body.has(flag::SET_OPERATION) {
        return None;
    }
    let items = items(&t, &body, dialect)?;
    let is_star = |i: &Item| matches!(i, Item::Star(_));
    let span = items
        .iter()
        .position(is_star)
        .zip(items.iter().rposition(is_star));
    let (head, tail) = span.map_or((items.len(), 0), |(first, last)| {
        (first, items.len() - last - 1)
    });
    if names.len() < head + tail || (span.is_none() && names.len() != items.len()) {
        return None;
    }
    // The stars, when nothing else stands between them.
    let stars: Option<Vec<&Star>> = span.and_then(|(first, last)| {
        items[first..=last]
            .iter()
            .map(|i| match i {
                Item::Star(star) => Some(star),
                _ => None,
            })
            .collect()
    });
    let duplicated = |name: &str| {
        names
            .iter()
            .filter(|n| n.eq_ignore_ascii_case(name))
            .count()
            > 1
    };
    let columns = names.iter().enumerate().map(|(i, name)| {
        let item = if i < head {
            Some(&items[i])
        } else if i >= names.len() - tail {
            Some(&items[items.len() - (names.len() - i)])
        } else {
            None
        };
        match item {
            Some(Item::Column(column)) => FilterColumn::Column(column.clone()),
            Some(_) => FilterColumn::Computed,
            None => stars.as_ref().map_or(FilterColumn::Ambiguous, |stars| {
                star_column(stars, name, duplicated(name), dialect)
            }),
        }
    });
    Some(columns.collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    const PG: Dialect = Dialect::Postgres;
    const MY: Dialect = Dialect::MySql;
    const MS: Dialect = Dialect::SqlServer;
    const ORA: Dialect = Dialect::Oracle;
    const DUCK: Dialect = Dialect::DuckDb;

    /// What the quick filters name for each column: `-` for a computed one, `?` for one whose
    /// table cannot be told.
    fn tested(sql: &str, dialect: Dialect, names: &[&str]) -> Vec<String> {
        filter_columns(sql, dialect, names)
            .unwrap()
            .into_iter()
            .map(|c| match c {
                FilterColumn::Column(sql) => sql,
                FilterColumn::Computed => "-".to_owned(),
                FilterColumn::Ambiguous => "?".to_owned(),
            })
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
        // Oracle's UNIQUE is DISTINCT, not a column.
        assert_eq!(
            tested("SELECT UNIQUE a, b FROM t", ORA, &["A", "B"]),
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
        // Without AS, a string after a name may be a typed literal (`DATE '…'`): not read.
        assert_eq!(tested("SELECT name 'n' FROM t", MY, &["n"]), ["-"]);
        // Transact-SQL's `alias = expression` is not read either.
        assert_eq!(tested("SELECT n = name FROM t", MS, &["n"]), ["-"]);
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
    fn a_word_before_a_name_makes_an_expression_not_an_alias() {
        assert_eq!(
            tested("SELECT NOT active, id FROM t", PG, &["?column?", "id"]),
            ["-", "id"]
        );
        assert_eq!(tested("SELECT BINARY name FROM t", MY, &["name"]), ["-"]);
        assert_eq!(
            tested(
                "SELECT PRIOR id, CONNECT_BY_ROOT name, LEVEL, ROWNUM, id FROM t",
                ORA,
                &["PRIORID", "ROOT", "LEVEL", "ROWNUM", "ID"]
            ),
            ["-", "-", "-", "-", "id"]
        );
        // Elsewhere `level` is a name like any other.
        assert_eq!(tested("SELECT level FROM t", PG, &["level"]), ["level"]);
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
    fn a_star_without_some_columns_keeps_the_others() {
        // Snowflake folds bare names to upper case: its result columns are named so.
        let (lower, upper) = (["id", "user_id"], ["ID", "USER_ID"]);
        for (sql, dialect, names) in [
            ("SELECT * EXCLUDE (payload) FROM events", DUCK, lower),
            (
                "SELECT * EXCLUDE payload FROM events",
                Dialect::Snowflake,
                upper,
            ),
            (
                "SELECT * ILIKE '%id' FROM events",
                Dialect::Snowflake,
                upper,
            ),
            (
                "SELECT * EXCEPT (payload) FROM events",
                Dialect::BigQuery,
                lower,
            ),
            (
                "SELECT * EXCEPT (payload) FROM events",
                Dialect::Databricks,
                lower,
            ),
            (
                "SELECT * EXCEPT (payload) FROM events",
                Dialect::ClickHouse,
                lower,
            ),
        ] {
            assert_eq!(tested(sql, dialect, &names), names, "{sql}");
        }
        assert_eq!(
            tested(
                "SELECT e.* EXCEPT (payload), 1 AS one FROM events e",
                Dialect::BigQuery,
                &["id", "one"]
            ),
            ["e.id", "-"]
        );
    }

    #[test]
    fn a_star_that_renames_or_replaces_columns_shows_what_they_read() {
        assert_eq!(
            tested(
                "SELECT * REPLACE (price * 2 AS price) RENAME (code AS sku) FROM items",
                DUCK,
                &["id", "price", "sku"]
            ),
            ["id", "-", "code"]
        );
        assert_eq!(
            tested(
                "SELECT i.* RENAME code AS sku FROM items i",
                Dialect::Snowflake,
                &["ID", "SKU"]
            ),
            ["i.ID", "i.code"]
        );
        assert_eq!(
            tested(
                "SELECT * REPLACE (upper(name) AS name) FROM t",
                Dialect::BigQuery,
                &["id", "name"]
            ),
            ["id", "-"]
        );
        assert_eq!(
            tested(
                "SELECT * APPLY (toString) FROM t",
                Dialect::ClickHouse,
                &["id"]
            ),
            ["-"]
        );
        // Columns chosen by a pattern: nothing to tell them by.
        assert_eq!(
            filter_columns("SELECT COLUMNS('^a') FROM t", DUCK, &["a1", "a2"]),
            None
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
        // Several stars side by side: a name shown once tells its column.
        assert_eq!(
            tested("SELECT a.*, b.* FROM a, b", PG, &["x", "y"]),
            ["x", "y"]
        );
        // An item between them leaves the stars' columns untold.
        assert_eq!(
            tested(
                "SELECT a.*, 1 AS one, b.* FROM a, b",
                PG,
                &["x", "one", "y"]
            ),
            ["?", "?", "?"]
        );
    }

    #[test]
    fn a_name_two_tables_show_is_not_told_by_a_bare_star() {
        assert_eq!(
            tested(
                "SELECT * FROM a JOIN b ON b.a_id = a.id",
                PG,
                &["id", "name", "id", "a_id"]
            ),
            ["?", "name", "?", "a_id"]
        );
        assert_eq!(
            tested("SELECT a.*, b.* FROM a, b", PG, &["ID", "x", "id"]),
            ["?", "x", "?"]
        );
        // A qualified star tells its own columns.
        assert_eq!(
            tested("SELECT a.*, b.id FROM a, b", PG, &["id", "id"]),
            ["a.id", "b.id"]
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
