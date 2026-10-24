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

/// Every placeholder of `src`, in order.
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
    let mut more = Vec::new();
    for (i, t) in tokens.iter().enumerate() {
        if t.kind != TokenKind::Symbol || in_variable(t.start) {
            continue;
        }
        match t.text(src) {
            "?" if dialect != Dialect::Postgres => more.push(Parameter {
                kind: ParameterKind::Positional,
                name: String::new(),
                start: t.start,
                end: t.end,
            }),
            ":" => {
                // Not `::type`, `a[1:n]` or `x:=`: the colon must follow neither a name nor
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
    }
}
