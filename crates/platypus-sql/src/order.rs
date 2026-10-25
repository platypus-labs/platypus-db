//! Rewrites the top-level `ORDER BY` of a query, for the data viewer: its sort changes only
//! that clause and leaves the rest of the text as the user wrote it. Only the top level of
//! the first statement counts: parentheses (subqueries, window definitions) are skipped, and
//! strings and comments never match.

use crate::{Dialect, Lexer, Token, TokenKind};

/// Words that end an `ORDER BY` clause, or come after where one would go.
const TAIL: &[&str] = &["LIMIT", "OFFSET", "FETCH", "FOR"];

/// Where the pieces of a statement's top level are, as byte offsets.
#[derive(Debug, Default)]
struct Layout {
    /// End of the first statement: its `;`, or the end of the text.
    end: usize,
    /// The `ORDER BY` clause: from `ORDER` to the end of its last token.
    order: Option<(usize, usize)>,
    /// Start of the whitespace before `ORDER`, when it can go with the clause.
    order_gap: Option<usize>,
    /// End of the last token before `LIMIT`, `OFFSET`, `FETCH` or `FOR`, or of the statement.
    insert_at: usize,
    /// The statement has an `OFFSET` or `FETCH`.
    paged: bool,
}

/// Whether the whitespace token just before `tokens[i]` can be removed with it: it is not
/// the end of a line comment's line.
fn gap_before(tokens: &[Token], i: usize) -> Option<usize> {
    let gap = tokens[..i]
        .last()
        .filter(|t| t.kind == TokenKind::Whitespace)?;
    let before = tokens[..i - 1].last();
    before
        .is_none_or(|t| t.kind != TokenKind::LineComment)
        .then_some(gap.start)
}

fn layout(sql: &str, dialect: Dialect) -> Layout {
    let tokens: Vec<Token> = Lexer::new(sql, dialect).collect();
    let mut found = Layout {
        end: sql.len(),
        ..Layout::default()
    };
    let (mut depth, mut last, mut tail, mut in_order) = (0usize, 0, false, false);
    for (i, token) in tokens.iter().enumerate() {
        if token.is_trivia() {
            continue;
        }
        let text = token.text(sql);
        if token.kind == TokenKind::Symbol {
            match text {
                "(" => depth += 1,
                ")" => depth = depth.saturating_sub(1),
                ";" if depth == 0 => {
                    found.end = token.start;
                    break;
                }
                _ => {}
            }
        }
        if depth == 0 && token.kind == TokenKind::Word {
            if TAIL.iter().any(|w| text.eq_ignore_ascii_case(w)) {
                if in_order {
                    found.order = found.order.map(|(start, _)| (start, last));
                    in_order = false;
                }
                if !tail {
                    found.insert_at = last;
                    tail = true;
                }
                found.paged |=
                    !(text.eq_ignore_ascii_case("LIMIT") || text.eq_ignore_ascii_case("FOR"));
            } else if found.order.is_none()
                && text.eq_ignore_ascii_case("ORDER")
                && tokens[i + 1..]
                    .iter()
                    .find(|t| !t.is_trivia())
                    .is_some_and(|t| t.text(sql).eq_ignore_ascii_case("BY"))
            {
                found.order = Some((token.start, token.end));
                found.order_gap = gap_before(&tokens, i);
                in_order = true;
            }
        }
        last = token.end;
    }
    if in_order {
        found.order = found.order.map(|(start, _)| (start, last));
    }
    if !tail {
        found.insert_at = last;
    }
    found
}

/// `sql` with its top-level `ORDER BY` replaced by one on `column` (`true`: descending), or
/// removed with `None`. Without one, a new clause goes before a trailing `LIMIT`, `OFFSET`
/// or `FETCH`, otherwise at the end of the statement.
#[must_use]
pub fn set_order_by(sql: &str, dialect: Dialect, order: Option<(&str, bool)>) -> String {
    let at = layout(sql, dialect);
    let clause = order.map(|(column, desc)| {
        let direction = if desc { "DESC" } else { "ASC" };
        format!("ORDER BY {} {direction}", dialect.quote_ident(column))
    });
    let (from, to, text) = match (at.order, clause) {
        (Some((start, end)), Some(clause)) => (start, end, clause),
        (Some((start, end)), None) => (at.order_gap.unwrap_or(start), end, String::new()),
        (None, Some(clause)) if at.insert_at > 0 => {
            (at.insert_at, at.insert_at, format!("\n{clause}"))
        }
        (None, _) => return sql.to_owned(),
    };
    format!("{}{text}{}", &sql[..from], &sql[to..])
}

/// A query counting the rows of `sql`'s first statement: `SELECT COUNT(*) FROM (…) t`. Its
/// top-level `ORDER BY` is dropped, as Transact-SQL refuses one in a subquery, unless
/// `OFFSET … FETCH` needs it.
#[must_use]
pub fn count_query(sql: &str, dialect: Dialect) -> String {
    let at = layout(sql, dialect);
    let statement = &sql[..at.end];
    let body = if at.paged && dialect == Dialect::SqlServer {
        statement.to_owned()
    } else {
        set_order_by(statement, dialect, None)
    };
    format!("SELECT COUNT(*) FROM (\n{}\n) t", body.trim())
}

#[cfg(test)]
mod tests {
    use super::*;

    const PG: Dialect = Dialect::Postgres;

    fn sort(sql: &str, dialect: Dialect, column: &str, desc: bool) -> String {
        set_order_by(sql, dialect, Some((column, desc)))
    }

    #[test]
    fn adds_to_a_plain_query() {
        assert_eq!(
            sort(
                "SELECT * FROM form WHERE sendable_email = true",
                PG,
                "id",
                false
            ),
            "SELECT * FROM form WHERE sendable_email = true\nORDER BY id ASC"
        );
    }

    #[test]
    fn replaces_an_existing_order_by() {
        assert_eq!(
            sort(
                "select * from t order by a, b desc nulls last limit 5",
                PG,
                "c",
                true
            ),
            "select * from t ORDER BY c DESC limit 5"
        );
        assert_eq!(
            sort("SELECT * FROM t\nORDER BY a", PG, "c", false),
            "SELECT * FROM t\nORDER BY c ASC"
        );
    }

    #[test]
    fn removing_the_sort_removes_the_clause() {
        assert_eq!(
            set_order_by("SELECT * FROM t\nORDER BY a DESC", PG, None),
            "SELECT * FROM t"
        );
        assert_eq!(
            set_order_by("SELECT * FROM t ORDER BY a LIMIT 3", PG, None),
            "SELECT * FROM t LIMIT 3"
        );
        assert_eq!(set_order_by("SELECT * FROM t", PG, None), "SELECT * FROM t");
    }

    #[test]
    fn goes_before_limit_offset_and_fetch() {
        assert_eq!(
            sort("SELECT * FROM t LIMIT 10 OFFSET 5", PG, "a", false),
            "SELECT * FROM t\nORDER BY a ASC LIMIT 10 OFFSET 5"
        );
        assert_eq!(
            sort("SELECT * FROM t OFFSET 5", Dialect::MySql, "a", false),
            "SELECT * FROM t\nORDER BY a ASC OFFSET 5"
        );
        assert_eq!(
            sort(
                "SELECT * FROM t OFFSET 10 ROWS FETCH NEXT 5 ROWS ONLY",
                Dialect::SqlServer,
                "a",
                true
            ),
            "SELECT * FROM t\nORDER BY a DESC OFFSET 10 ROWS FETCH NEXT 5 ROWS ONLY"
        );
        assert_eq!(
            sort(
                "SELECT * FROM t ORDER BY x OFFSET 10 ROWS FETCH NEXT 5 ROWS ONLY",
                Dialect::SqlServer,
                "a",
                true
            ),
            "SELECT * FROM t ORDER BY a DESC OFFSET 10 ROWS FETCH NEXT 5 ROWS ONLY"
        );
    }

    #[test]
    fn keeps_top_in_transact_sql() {
        assert_eq!(
            sort("SELECT TOP (10) * FROM t", Dialect::SqlServer, "a", false),
            "SELECT TOP (10) * FROM t\nORDER BY a ASC"
        );
    }

    #[test]
    fn leaves_subqueries_and_ctes_alone() {
        let cte = "WITH c AS (SELECT * FROM t ORDER BY a) SELECT * FROM c";
        assert_eq!(sort(cte, PG, "b", false), format!("{cte}\nORDER BY b ASC"));
        let sub = "SELECT * FROM (SELECT * FROM t ORDER BY a LIMIT 3) s";
        assert_eq!(sort(sub, PG, "b", true), format!("{sub}\nORDER BY b DESC"));
        let window = "SELECT row_number() OVER (ORDER BY a) FROM t";
        assert_eq!(set_order_by(window, PG, None), window);
        assert_eq!(
            sort("SELECT * FROM (SELECT 1 LIMIT 1) s LIMIT 2", PG, "b", false),
            "SELECT * FROM (SELECT 1 LIMIT 1) s\nORDER BY b ASC LIMIT 2"
        );
    }

    #[test]
    fn ignores_strings_and_comments() {
        assert_eq!(
            sort("SELECT 'order by x limit 1' FROM t", PG, "a", false),
            "SELECT 'order by x limit 1' FROM t\nORDER BY a ASC"
        );
        assert_eq!(
            sort(
                "SELECT * FROM t -- order by z\n/* limit 1 */",
                PG,
                "a",
                false
            ),
            "SELECT * FROM t\nORDER BY a ASC -- order by z\n/* limit 1 */"
        );
        assert_eq!(
            set_order_by("SELECT * FROM t -- note\nORDER BY a", PG, None),
            "SELECT * FROM t -- note\n"
        );
    }

    #[test]
    fn stops_at_the_first_semicolon() {
        assert_eq!(
            sort("SELECT * FROM t;", PG, "a", false),
            "SELECT * FROM t\nORDER BY a ASC;"
        );
        assert_eq!(
            sort(
                "SELECT * FROM t ORDER BY a;\nSELECT 2 ORDER BY 1;",
                PG,
                "b",
                false
            ),
            "SELECT * FROM t ORDER BY b ASC;\nSELECT 2 ORDER BY 1;"
        );
    }

    #[test]
    fn quotes_the_column_per_dialect() {
        let q = "SELECT * FROM t";
        assert_eq!(
            sort(q, PG, "Full Name", false),
            "SELECT * FROM t\nORDER BY \"Full Name\" ASC"
        );
        assert_eq!(
            sort(q, Dialect::MySql, "Full Name", true),
            "SELECT * FROM t\nORDER BY `Full Name` DESC"
        );
        assert_eq!(
            sort(q, Dialect::SqlServer, "Full Name", false),
            "SELECT * FROM t\nORDER BY [Full Name] ASC"
        );
        assert_eq!(
            sort(q, Dialect::Sqlite, "order", false),
            "SELECT * FROM t\nORDER BY \"order\" ASC"
        );
    }

    #[test]
    fn counts_the_query_without_its_order_by() {
        assert_eq!(
            count_query("SELECT * FROM t WHERE a = 1 ORDER BY b DESC;", PG),
            "SELECT COUNT(*) FROM (\nSELECT * FROM t WHERE a = 1\n) t"
        );
        assert_eq!(
            count_query("SELECT * FROM t -- last\n", Dialect::Oracle),
            "SELECT COUNT(*) FROM (\nSELECT * FROM t -- last\n) t"
        );
        assert_eq!(
            count_query("SELECT * FROM t ORDER BY a LIMIT 5", Dialect::MySql),
            "SELECT COUNT(*) FROM (\nSELECT * FROM t LIMIT 5\n) t"
        );
        assert_eq!(
            count_query(
                "SELECT * FROM t ORDER BY a OFFSET 1 ROWS FETCH NEXT 2 ROWS ONLY",
                Dialect::SqlServer
            ),
            "SELECT COUNT(*) FROM (\nSELECT * FROM t ORDER BY a OFFSET 1 ROWS FETCH NEXT 2 ROWS ONLY\n) t"
        );
    }
}
