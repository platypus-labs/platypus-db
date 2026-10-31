//! The parameters the editor asks values for before running: `:name`, `?` (MySQL and
//! SQLite; in PostgreSQL it is a JSON operator) and `${name}` variables, which are replaced
//! as plain text, inside strings too.

use crate::{Dialect, Lexer, TokenKind};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParameterKind {
    /// `:name`, replaced by a literal.
    Named,
    /// `?`, replaced by a literal; its name is empty.
    Positional,
    /// `${name}`, replaced by the text as typed.
    Variable,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Parameter {
    pub kind: ParameterKind,
    pub name: String,
    /// Byte range of the whole placeholder.
    pub start: usize,
    pub end: usize,
}

fn is_name_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// The `${name}` placeholders of `src` outside comments.
fn variables(src: &str, comments: &[(usize, usize)]) -> Vec<Parameter> {
    let mut found = Vec::new();
    let mut from = 0;
    while let Some(i) = src[from..].find("${").map(|i| from + i) {
        from = i + 2;
        let Some(len) = src[from..].find('}') else {
            break;
        };
        let name = &src[from..from + len];
        if !name.is_empty()
            && name.chars().all(|c| is_name_char(c) || c == '.')
            && !comments.iter().any(|&(s, e)| s <= i && i < e)
        {
            found.push(Parameter {
                kind: ParameterKind::Variable,
                name: name.to_owned(),
                start: i,
                end: from + len + 1,
            });
            from += len + 1;
        }
    }
    found
}

/// Whether `src` creates an Oracle routine, package, trigger or type: its PL/SQL has no bind
/// variables, and `:NEW`, `:OLD` and `:PARENT` in a trigger name the rows it fires on.
fn oracle_stored_plsql(src: &str) -> bool {
    Lexer::new(src, Dialect::Oracle)
        .find(|t| t.kind == TokenKind::Word)
        .is_some_and(|t| t.text(src).eq_ignore_ascii_case("CREATE"))
        && crate::is_plsql(src)
}

/// Every placeholder of `src`, in order: `:name` (and in SQLite, `@name`, `$name` and `?1`),
/// `?` outside PostgreSQL and ClickHouse, and `${name}`.
#[must_use]
pub fn parameters(src: &str, dialect: Dialect) -> Vec<Parameter> {
    let tokens: Vec<_> = Lexer::new(src, dialect).collect();
    let comments: Vec<_> = tokens
        .iter()
        .filter(|t| matches!(t.kind, TokenKind::LineComment | TokenKind::BlockComment))
        .map(|t| (t.start, t.end))
        .collect();
    let mut found = variables(src, &comments);
    let in_variable = |pos: usize| found.iter().any(|v| v.start <= pos && pos < v.end);
    let named = !(dialect == Dialect::Oracle && oracle_stored_plsql(src));
    let mut more = Vec::new();
    for (i, t) in tokens.iter().enumerate() {
        if t.kind != TokenKind::Symbol || in_variable(t.start) {
            continue;
        }
        let prefix = t.text(src);
        match prefix {
            // SQLite's `?1`, `?2`: numbered, the same number is the same value. Its name keeps
            // the `?`, which the editor's own keys of plain `?` (`?1`, `?2`…) share.
            "?" if dialect == Dialect::Sqlite
                && let Some(number) = tokens.get(i + 1)
                && number.start == t.end
                && number.text(src).bytes().all(|b| b.is_ascii_digit()) =>
            {
                more.push(Parameter {
                    kind: ParameterKind::Named,
                    name: src[t.start..number.end].to_owned(),
                    start: t.start,
                    end: number.end,
                });
            }
            // PostgreSQL's `?` is a JSON operator, ClickHouse's the ternary `a ? b : c`.
            "?" if !matches!(dialect, Dialect::Postgres | Dialect::ClickHouse) => {
                more.push(Parameter {
                    kind: ParameterKind::Positional,
                    name: String::new(),
                    start: t.start,
                    end: t.end,
                })
            }
            ":" | "@" | "$" if named && (prefix == ":" || dialect == Dialect::Sqlite) => {
                // Not `::type`, `a[1:n]` or `x:=`: the prefix must follow neither a name nor
                // another colon, and touch the name after it.
                let before = src[..t.start].chars().next_back();
                if before.is_some_and(|c| is_name_char(c) || c == ':') {
                    continue;
                }
                if let Some(word) = tokens.get(i + 1)
                    && word.kind == TokenKind::Word
                    && word.start == t.end
                {
                    more.push(Parameter {
                        kind: ParameterKind::Named,
                        name: word.text(src).to_owned(),
                        start: t.start,
                        end: word.end,
                    });
                }
            }
            _ => {}
        }
    }
    found.extend(more);
    found.sort_by_key(|p| p.start);
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(src: &str, dialect: Dialect) -> Vec<(ParameterKind, String, &str)> {
        parameters(src, dialect)
            .into_iter()
            .map(|p| (p.kind, p.name, &src[p.start..p.end]))
            .collect()
    }

    #[test]
    fn finds_named_and_variables() {
        use ParameterKind::*;
        let src = "select * from ${table} where id = :id and name = '${who}' -- :no ${no}\n\
                   and x::text = 'a:b' and y = arr[1:n] and z = (:z)";
        assert_eq!(
            names(src, Dialect::Postgres),
            [
                (Variable, "table".to_owned(), "${table}"),
                (Named, "id".to_owned(), ":id"),
                (Variable, "who".to_owned(), "${who}"),
                (Named, "z".to_owned(), ":z"),
            ]
        );
    }

    #[test]
    fn question_marks_outside_postgres() {
        let src = "select ? , '?' from t where a = ?";
        assert_eq!(names(src, Dialect::MySql).len(), 2);
        assert_eq!(names(src, Dialect::Sqlite)[0].0, ParameterKind::Positional);
        assert_eq!(names("select '{}'::jsonb ? 'a'", Dialect::Postgres), []);
        assert_eq!(names("set @x := 1", Dialect::MySql), []);
        assert_eq!(names("${unclosed", Dialect::Sqlite), []);
        // ClickHouse's ternary operator.
        assert_eq!(names("SELECT 1 ? 'a' : 'b' AS t", Dialect::ClickHouse), []);
    }

    #[test]
    fn sqlite_numbered_question_marks() {
        use ParameterKind::{Named, Positional};
        assert_eq!(
            names("SELECT ?1 AS x, ?2 AS y, ?1 AS z, ?", Dialect::Sqlite),
            [
                (Named, "?1".to_owned(), "?1"),
                (Named, "?2".to_owned(), "?2"),
                (Named, "?1".to_owned(), "?1"),
                (Positional, String::new(), "?"),
            ]
        );
        // `? 1` is a placeholder followed by a number; elsewhere `?1` is not numbered.
        assert_eq!(names("SELECT ? 1", Dialect::Sqlite)[0].1, "");
        assert_eq!(names("SELECT ?1", Dialect::MySql)[0].1, "");
    }

    #[test]
    fn sqlite_names_parameters_three_ways() {
        use ParameterKind::Named;
        assert_eq!(
            names(
                "SELECT @c AS c, $d AS d, :e WHERE x = '@no'",
                Dialect::Sqlite
            ),
            [
                (Named, "c".to_owned(), "@c"),
                (Named, "d".to_owned(), "$d"),
                (Named, "e".to_owned(), ":e"),
            ]
        );
        // Elsewhere `@x` is a variable and `$1` a parameter of the server.
        assert_eq!(names("SELECT @c, $1", Dialect::MySql), []);
        assert_eq!(names("SELECT @c", Dialect::SqlServer), []);
    }

    #[test]
    fn oracle_triggers_have_no_bind_variables() {
        let trigger = "CREATE OR REPLACE TRIGGER t BEFORE UPDATE ON emp FOR EACH ROW\n\
                       BEGIN\n  :NEW.changed := SYSDATE;\n  IF :old.sal <> :new.sal THEN NULL; END IF;\nEND;";
        assert_eq!(names(trigger, Dialect::Oracle), []);
        let procedure = "CREATE PROCEDURE p IS BEGIN UPDATE t SET a = 1; END;";
        assert_eq!(names(procedure, Dialect::Oracle), []);
        // Anonymous blocks and plain SQL take binds; other dialects are unchanged.
        assert_eq!(names("BEGIN :x := 1; END;", Dialect::Oracle).len(), 1);
        assert_eq!(
            names("select * from t where a = :new", Dialect::Oracle).len(),
            1
        );
        assert_eq!(
            names("CREATE TRIGGER t BEGIN SELECT :a; END", Dialect::Postgres).len(),
            1
        );
        // `${name}` variables stay.
        assert_eq!(
            names(
                "CREATE TRIGGER ${name} BEFORE INSERT ON t BEGIN :NEW.a := 1; END;",
                Dialect::Oracle
            ),
            [(ParameterKind::Variable, "name".to_owned(), "${name}")]
        );
    }
}
