//! Where a table's columns are used: the views, routines, triggers and scripts whose SQL names
//! them. A text uses a column when it names the table and, somewhere, the column: a name read
//! as a word or a quoted identifier, outside comments and strings, case ignored. The body of a
//! dollar-quoted string (a PostgreSQL routine's) is read as SQL too.

use crate::lexer::unquote_ident;
use crate::{Dialect, Lexer, TokenKind};

/// A name of `sql`, lower case, with the line (from 1) it is on.
struct Name {
    name: String,
    line: usize,
}

/// The names of `sql`, which starts on line `first` of its text.
fn names(sql: &str, dialect: Dialect, first: usize, out: &mut Vec<Name>) {
    let mut line = first;
    let mut at = 0;
    for token in Lexer::new(sql, dialect) {
        line += sql[at..token.start].matches('\n').count();
        at = token.start;
        let text = token.text(sql);
        match token.kind {
            TokenKind::Word | TokenKind::QuotedIdent => {
                let name = if token.kind == TokenKind::Word {
                    Some(text.to_owned())
                } else {
                    unquote_ident(text)
                };
                if let Some(name) = name {
                    out.push(Name {
                        name: name.to_lowercase(),
                        line,
                    });
                }
            }
            TokenKind::String => {
                if let Some(body) = dollar_body(text) {
                    let offset = text.find(body).unwrap_or(0);
                    let before = text[..offset].matches('\n').count();
                    names(body, dialect, line + before, out);
                }
            }
            _ => {}
        }
    }
}

/// The body of a dollar-quoted string, `$$…$$` or `$tag$…$tag$`.
fn dollar_body(text: &str) -> Option<&str> {
    let rest = text.strip_prefix('$')?;
    let close = rest.find('$')?;
    let tag = &text[..close + 2];
    text.get(tag.len()..text.len().checked_sub(tag.len())?)
        .filter(|_| text.ends_with(tag) && text.len() >= 2 * tag.len())
}

/// The columns of `columns` that `sql` uses, each as its index in `columns` with the first line
/// naming it; none when `sql` does not name `table`.
#[must_use]
pub fn column_uses(
    sql: &str,
    dialect: Dialect,
    table: &str,
    columns: &[String],
) -> Vec<(usize, usize)> {
    let mut found = Vec::new();
    names(sql, dialect, 1, &mut found);
    let table = table.to_lowercase();
    if !found.iter().any(|n| n.name == table) {
        return Vec::new();
    }
    columns
        .iter()
        .enumerate()
        .filter_map(|(i, column)| {
            let column = column.to_lowercase();
            found.iter().find(|n| n.name == column).map(|n| (i, n.line))
        })
        .collect()
}

/// Line `line` (from 1) of `text`, trimmed and cut to `max` characters, as an excerpt.
#[must_use]
pub fn excerpt(text: &str, line: usize, max: usize) -> String {
    let full = text
        .lines()
        .nth(line.saturating_sub(1))
        .unwrap_or("")
        .trim();
    if full.chars().count() <= max {
        full.to_owned()
    } else {
        let cut: String = full.chars().take(max.saturating_sub(1)).collect();
        format!("{cut}…")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cols(names: &[&str]) -> Vec<String> {
        names.iter().map(|s| (*s).to_owned()).collect()
    }

    #[test]
    fn finds_the_columns_a_view_names() {
        let sql = "CREATE VIEW v AS\nSELECT id,\n  Email\nFROM users WHERE active";
        let uses = column_uses(
            sql,
            Dialect::Postgres,
            "users",
            &cols(&["id", "email", "phone"]),
        );
        assert_eq!(uses, [(0, 2), (1, 3)]);
    }

    #[test]
    fn needs_the_table_named() {
        let sql = "SELECT email FROM customers";
        assert_eq!(
            column_uses(sql, Dialect::Postgres, "users", &cols(&["email"])),
            []
        );
    }

    #[test]
    fn leaves_out_comments_and_strings_but_reads_quoted_names() {
        let sql = "-- email is gone\nSELECT 'email' AS x, \"Phone\" FROM \"Users\" /* email */";
        let uses = column_uses(sql, Dialect::Postgres, "users", &cols(&["email", "phone"]));
        assert_eq!(uses, [(1, 2)]);
        let sql = "SELECT [e-mail] FROM [dbo].[users]";
        assert_eq!(
            column_uses(sql, Dialect::SqlServer, "users", &cols(&["e-mail"])),
            [(0, 1)]
        );
    }

    #[test]
    fn reads_the_body_of_a_dollar_quoted_routine() {
        let sql = "CREATE FUNCTION f() RETURNS int LANGUAGE sql AS $body$\nSELECT count(*)\nFROM users\nWHERE email IS NULL\n$body$;";
        let uses = column_uses(sql, Dialect::Postgres, "users", &cols(&["email"]));
        assert_eq!(uses, [(0, 4)]);
        assert_eq!(dollar_body("$$x$$"), Some("x"));
        assert_eq!(dollar_body("'x'"), None);
    }

    #[test]
    fn cuts_long_excerpts() {
        assert_eq!(
            excerpt("a\n   where email = 1   \nb", 2, 40),
            "where email = 1"
        );
        assert_eq!(excerpt("abcdef", 1, 4), "abc…");
        assert_eq!(excerpt("a", 5, 4), "");
    }
}
