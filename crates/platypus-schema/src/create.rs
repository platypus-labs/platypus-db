//! What a `CREATE TABLE` says that the drivers' details leave out: generated columns, column
//! collations and, after the column list, table options (SQLite's `WITHOUT ROWID`, `STRICT`).
//! The table editor keeps them so that redefining a column or creating the table again does
//! not lose them.

use platypus_sql::{Dialect, Lexer, Token, TokenKind};

#[derive(Debug, Default, PartialEq, Eq)]
pub struct Definition {
    pub columns: Vec<ColumnDefinition>,
    /// The text after the column list, without a final `;`.
    pub options: Option<String>,
    /// Whether a column is `AUTOINCREMENT` (SQLite).
    pub autoincrement: bool,
    /// The plain, unique, `FULLTEXT` and `SPATIAL` indexes of MySQL's `SHOW CREATE TABLE`.
    pub indexes: Vec<IndexDefinition>,
}

/// An index of MySQL's `SHOW CREATE TABLE`, a `KEY`, a `UNIQUE KEY`, a `FULLTEXT KEY` or a
/// `SPATIAL KEY`.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct IndexDefinition {
    pub name: String,
    /// A `UNIQUE KEY`.
    pub unique: bool,
    /// `FULLTEXT` or `SPATIAL`; `None` for a plain or unique index.
    pub method: Option<String>,
    /// The key parts as written: `` `body`(50) ``, `` `a` DESC ``, `(lower(`b`))`.
    pub parts: Vec<String>,
}

#[derive(Debug, Default, PartialEq, Eq)]
pub struct ColumnDefinition {
    pub name: String,
    /// `GENERATED ALWAYS AS (…) VIRTUAL`, as written (`AS (…)` alone is written in full).
    pub generated: Option<String>,
    /// The name after `COLLATE`, as written.
    pub collation: Option<String>,
}

/// Words that start a table constraint rather than a column in the column list; reserved, so
/// no bare column name is one.
const CONSTRAINT_WORDS: [&str; 5] = ["CONSTRAINT", "PRIMARY", "UNIQUE", "CHECK", "FOREIGN"];

/// The words that start an index in MySQL's `SHOW CREATE TABLE`, all reserved there.
const MYSQL_INDEX_WORDS: [&str; 5] = ["KEY", "INDEX", "UNIQUE", "FULLTEXT", "SPATIAL"];

struct Tokens<'a> {
    sql: &'a str,
    list: Vec<Token>,
}

impl Tokens<'_> {
    fn text(&self, i: usize) -> &str {
        self.list.get(i).map_or("", |t| t.text(self.sql))
    }

    fn is_word(&self, i: usize, word: &str) -> bool {
        self.list.get(i).is_some_and(|t| {
            t.kind == TokenKind::Word && t.text(self.sql).eq_ignore_ascii_case(word)
        })
    }

    /// The index of the `)` that closes the `(` at `open`.
    fn closing(&self, open: usize) -> Option<usize> {
        let mut depth = 0usize;
        for i in open..self.list.len() {
            match self.text(i) {
                "(" => depth += 1,
                ")" => {
                    depth -= 1;
                    if depth == 0 {
                        return Some(i);
                    }
                }
                _ => {}
            }
        }
        None
    }

    /// The source from token `from` to token `to`, both included.
    fn source(&self, from: usize, to: usize) -> &str {
        &self.sql[self.list[from].start..self.list[to].end]
    }
}

/// A name as written, without its quotes.
fn unquote(token: &str) -> String {
    match (token.chars().next(), token.chars().last()) {
        (Some(open @ ('"' | '`')), Some(close)) if close == open && token.len() >= 2 => {
            let quote = open.to_string();
            token[1..token.len() - 1].replace(&quote.repeat(2), &quote)
        }
        (Some('['), Some(']')) => token[1..token.len() - 1].replace("]]", "]"),
        _ => token.to_owned(),
    }
}

/// Reads `sql`, a `CREATE TABLE … (…) …` statement; anything it does not understand is left
/// out of the result.
#[must_use]
pub fn parse(sql: &str, dialect: Dialect) -> Definition {
    let t = Tokens {
        sql,
        list: Lexer::new(sql, dialect)
            .filter(|t| !t.is_trivia())
            .collect(),
    };
    let mut out = Definition::default();
    let Some(open) = (0..t.list.len()).find(|&i| t.text(i) == "(") else {
        return out;
    };
    let Some(close) = t.closing(open) else {
        return out;
    };
    // The items of the column list, split at its top-level commas.
    let mut items = Vec::new();
    let (mut start, mut depth) = (open + 1, 0usize);
    for i in open + 1..close {
        match t.text(i) {
            "(" => depth += 1,
            ")" => depth = depth.saturating_sub(1),
            "," if depth == 0 => {
                items.push((start, i));
                start = i + 1;
            }
            _ => {}
        }
    }
    items.push((start, close));
    for (from, to) in items {
        let index =
            dialect == Dialect::MySql && MYSQL_INDEX_WORDS.iter().any(|w| t.is_word(from, w));
        if index && from < to {
            out.indexes.extend(mysql_index(&t, from, to));
            continue;
        }
        if from >= to || CONSTRAINT_WORDS.iter().any(|w| t.is_word(from, w)) {
            continue;
        }
        out.autoincrement |= (from..to).any(|i| t.is_word(i, "AUTOINCREMENT"));
        out.columns.push(column(&t, from, to));
    }
    let last = t.list.len();
    let end = if t.text(last - 1) == ";" {
        last - 1
    } else {
        last
    };
    if close + 1 < end {
        out.options = Some(t.source(close + 1, end - 1).to_owned());
    }
    out
}

/// The index defined by the tokens `from..to`:
/// `[UNIQUE | FULLTEXT | SPATIAL] KEY name (parts) ..`.
fn mysql_index(t: &Tokens<'_>, from: usize, to: usize) -> Option<IndexDefinition> {
    let method = ["FULLTEXT", "SPATIAL"]
        .into_iter()
        .find(|w| t.is_word(from, w))
        .map(str::to_owned);
    let unique = t.is_word(from, "UNIQUE");
    let mut i = from + usize::from(method.is_some() || unique);
    if t.is_word(i, "KEY") || t.is_word(i, "INDEX") {
        i += 1;
    }
    let name = unquote(t.text(i));
    let open = (i..to).find(|&j| t.text(j) == "(")?;
    let close = t.closing(open).filter(|&c| c < to)?;
    let parts = if close > open + 1 {
        crate::catalog::split_list(t.source(open + 1, close - 1))
    } else {
        Vec::new()
    };
    Some(IndexDefinition {
        name,
        unique,
        method,
        parts,
    })
}

/// The column defined by the tokens `from..to`.
fn column(t: &Tokens<'_>, from: usize, to: usize) -> ColumnDefinition {
    let mut c = ColumnDefinition {
        name: unquote(t.text(from)),
        ..ColumnDefinition::default()
    };
    let mut i = from + 1;
    while i < to {
        if t.is_word(i, "COLLATE") && i + 1 < to {
            c.collation = Some(t.text(i + 1).to_owned());
            i += 2;
            continue;
        }
        // `GENERATED ALWAYS AS (…)` or `AS (…)`, then `VIRTUAL` or `STORED`.
        let generated =
            t.is_word(i, "GENERATED") && t.is_word(i + 1, "ALWAYS") && t.is_word(i + 2, "AS");
        let short = t.is_word(i, "AS") && t.text(i + 1) == "(";
        if generated || short {
            let open = if generated { i + 3 } else { i + 1 };
            let Some(close) = t.closing(open).filter(|&c| c < to) else {
                break;
            };
            let storage = [close + 1].into_iter().find(|&s| {
                s < to
                    && ["VIRTUAL", "STORED", "PERSISTENT"]
                        .iter()
                        .any(|w| t.is_word(s, w))
            });
            let mut clause = format!("GENERATED ALWAYS AS {}", t.source(open, close));
            if let Some(s) = storage {
                clause.push(' ');
                clause.push_str(&t.text(s).to_ascii_uppercase());
            }
            c.generated = Some(clause);
            i = storage.unwrap_or(close) + 1;
            continue;
        }
        i += 1;
    }
    c
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_sqlite_generated_columns_collations_and_options() {
        let d = parse(
            "CREATE TABLE g (id INTEGER PRIMARY KEY AUTOINCREMENT, a INT,
               b INT GENERATED ALWAYS AS (a * (2)) VIRTUAL, \"c d\" TEXT COLLATE NOCASE,
               e AS (a + 1), CONSTRAINT k UNIQUE (a), CHECK (a > 0)) WITHOUT ROWID, STRICT;",
            Dialect::Sqlite,
        );
        assert!(d.autoincrement);
        assert_eq!(d.options.as_deref(), Some("WITHOUT ROWID, STRICT"));
        let names: Vec<&str> = d.columns.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(names, ["id", "a", "b", "c d", "e"]);
        assert_eq!(
            d.columns[2].generated.as_deref(),
            Some("GENERATED ALWAYS AS (a * (2)) VIRTUAL")
        );
        assert_eq!(d.columns[3].collation.as_deref(), Some("NOCASE"));
        assert_eq!(
            d.columns[4].generated.as_deref(),
            Some("GENERATED ALWAYS AS (a + 1)")
        );
        assert_eq!(
            d.columns[1],
            ColumnDefinition {
                name: "a".into(),
                ..Default::default()
            }
        );
    }

    #[test]
    fn reads_mysql_show_create_table() {
        let d = parse(
            "CREATE TABLE `qa_nav_g` (\n  `id` int NOT NULL,\n  `qty` int DEFAULT NULL,\n  \
             `dbl` int GENERATED ALWAYS AS ((`qty` * 2)) VIRTUAL,\n  \
             `code` varchar(10) CHARACTER SET utf8mb4 COLLATE utf8mb4_bin DEFAULT NULL,\n  \
             PRIMARY KEY (`id`)\n) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4",
            Dialect::MySql,
        );
        assert_eq!(
            d.columns[2].generated.as_deref(),
            Some("GENERATED ALWAYS AS ((`qty` * 2)) VIRTUAL")
        );
        assert_eq!(d.columns[3].collation.as_deref(), Some("utf8mb4_bin"));
        assert_eq!(d.columns.len(), 4);
    }

    #[test]
    fn reads_mysql_index_kinds_and_key_parts() {
        let d = parse(
            "CREATE TABLE `d` (\n  `id` int NOT NULL,\n  `body` text,\n  PRIMARY KEY (`id`),\n  \
             KEY `d_body` (`body`(50)),\n  KEY `d_two` (`id` DESC,(lower(`body`))),\n  \
             FULLTEXT KEY `d_ft` (`body`),\n  UNIQUE KEY `d_u` (`body`(20),`id` DESC)\n) ENGINE=InnoDB",
            Dialect::MySql,
        );
        assert_eq!(d.columns.len(), 2);
        assert_eq!(
            d.indexes,
            [
                IndexDefinition {
                    name: "d_body".into(),
                    parts: vec!["`body`(50)".into()],
                    ..Default::default()
                },
                IndexDefinition {
                    name: "d_two".into(),
                    parts: vec!["`id` DESC".into(), "(lower(`body`))".into()],
                    ..Default::default()
                },
                IndexDefinition {
                    name: "d_ft".into(),
                    method: Some("FULLTEXT".into()),
                    parts: vec!["`body`".into()],
                    ..Default::default()
                },
                IndexDefinition {
                    name: "d_u".into(),
                    unique: true,
                    method: None,
                    parts: vec!["`body`(20)".into(), "`id` DESC".into()],
                },
            ]
        );
    }

    #[test]
    fn a_table_without_options_has_none() {
        let d = parse("CREATE TABLE t (a INT)", Dialect::Sqlite);
        assert_eq!(d.options, None);
        assert!(!d.autoincrement);
    }
}
