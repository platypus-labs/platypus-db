//! What a `CREATE TABLE` says that the drivers' details leave out: generated columns, column
//! collations, what the keys and constraints say beyond their columns (SQLite's deferral and
//! `ON CONFLICT`, MySQL's `NOT ENFORCED`) and, after the column list, table options (SQLite's
//! `WITHOUT ROWID`, `STRICT`; MySQL's engine, character set and partitioning). The table editor
//! keeps them so that redefining a column or creating the table again does not lose them.

use platypus_sql::{Dialect, Lexer, Token, TokenKind};

use crate::Deferral;

#[derive(Debug, Default, PartialEq, Eq)]
pub struct Definition {
    pub columns: Vec<ColumnDefinition>,
    /// The text after the column list, without a final `;`: MySQL's without its
    /// `AUTO_INCREMENT` counter and `COMMENT`, and with its executable comments as code.
    pub options: Option<String>,
    /// Whether a column is `AUTOINCREMENT` (SQLite).
    pub autoincrement: bool,
    /// The plain, unique, `FULLTEXT` and `SPATIAL` indexes of MySQL's `SHOW CREATE TABLE`.
    pub indexes: Vec<IndexDefinition>,
    /// The foreign keys, of the table or of a column.
    pub foreign_keys: Vec<KeyDefinition>,
    /// The UNIQUE constraints, of the table or of a column.
    pub uniques: Vec<UniqueDefinition>,
    /// The named CHECK constraints of the table.
    pub checks: Vec<CheckDefinition>,
}

/// A `FOREIGN KEY (..) REFERENCES ..`, or a column's `REFERENCES ..`.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct KeyDefinition {
    pub columns: Vec<String>,
    pub ref_table: String,
    pub deferral: Deferral,
    /// `FULL` or `PARTIAL`.
    pub match_type: Option<String>,
}

/// A `UNIQUE (..)`, or a column's `UNIQUE`.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct UniqueDefinition {
    pub columns: Vec<String>,
    /// SQLite's `ON CONFLICT` resolution, upper case.
    pub on_conflict: Option<String>,
}

/// A `CONSTRAINT name CHECK (..)`.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct CheckDefinition {
    pub name: String,
    /// MySQL's `NOT ENFORCED`.
    pub not_enforced: bool,
}

/// What a `CREATE INDEX` says beyond the columns the catalogs list.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct IndexClause {
    /// The key parts as written: `lower(name)`, `n DESC`.
    pub parts: Vec<String>,
    /// The condition after `WHERE`.
    pub predicate: Option<String>,
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
    /// `INVISIBLE` (MySQL) or `IGNORED` (MariaDB).
    pub visibility: Option<String>,
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

    /// The names of the list `(a, "b" DESC, ..)` opening at `open`, each without its quotes
    /// and what follows it, and the index of its `)`.
    fn names(&self, open: usize) -> Option<(Vec<String>, usize)> {
        if self.text(open) != "(" {
            return None;
        }
        let close = self.closing(open)?;
        let mut names = Vec::new();
        let mut first = true;
        for i in open + 1..close {
            match self.text(i) {
                "," => first = true,
                text if first => {
                    names.push(unquote(text));
                    first = false;
                }
                _ => {}
            }
        }
        Some((names, close))
    }

    /// The deferral the words `from..to` of a key or constraint say.
    fn deferral(&self, from: usize, to: usize) -> Deferral {
        let deferrable = (from..to)
            .any(|i| self.is_word(i, "DEFERRABLE") && !(i > from && self.is_word(i - 1, "NOT")));
        let deferred =
            (from..to).any(|i| self.is_word(i, "INITIALLY") && self.is_word(i + 1, "DEFERRED"));
        Deferral::of(deferrable, deferred)
    }

    /// The key that `REFERENCES` at `at` starts, on `columns`, its clause ending before `to`.
    fn key(&self, columns: Vec<String>, at: usize, to: usize) -> KeyDefinition {
        // `REFERENCES [schema.]table [(columns)]`.
        let mut name = at + 1;
        while self.text(name + 1) == "." && name + 2 < to {
            name += 2;
        }
        let match_type = (at..to)
            .find(|&i| self.is_word(i, "MATCH"))
            .map(|i| self.text(i + 1).to_ascii_uppercase())
            .filter(|m| m == "FULL" || m == "PARTIAL");
        KeyDefinition {
            columns,
            ref_table: unquote(self.text(name)),
            deferral: self.deferral(at, to),
            match_type,
        }
    }

    /// The `ON CONFLICT` resolution at `at`, if there is one.
    fn on_conflict(&self, at: usize, to: usize) -> Option<String> {
        (at + 2 < to && self.is_word(at, "ON") && self.is_word(at + 1, "CONFLICT"))
            .then(|| self.text(at + 2).to_ascii_uppercase())
    }

    /// The tokens `from..end` as one line: the spaces between them collapsed, MySQL's
    /// `AUTO_INCREMENT` counter (which follows the rows) and `COMMENT` (the design's own)
    /// left out.
    fn mysql_options(&self, from: usize, end: usize) -> Option<String> {
        let mut out = String::new();
        let mut i = from;
        while i < end {
            if self.is_word(i, "AUTO_INCREMENT") || self.is_word(i, "COMMENT") {
                i += if self.text(i + 1) == "=" { 3 } else { 2 };
                continue;
            }
            if !out.is_empty() && self.list[i].start > self.list[i - 1].end {
                out.push(' ');
            }
            out.push_str(self.text(i));
            i += 1;
        }
        (!out.is_empty()).then_some(out)
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
    // MySQL's `SHOW CREATE TABLE` writes some clauses as executable comments, which the server
    // runs: `/*!80000 INVISIBLE */`, `/*!50100 PARTITION BY .. */`.
    let t = Tokens {
        sql,
        list: Lexer::executable(sql, dialect)
            .filter(|t| !t.is_trivia() && t.kind != TokenKind::ExecComment)
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
        if from >= to {
            continue;
        }
        if CONSTRAINT_WORDS.iter().any(|w| t.is_word(from, w)) {
            constraint(&t, from, to, &mut out);
            continue;
        }
        out.autoincrement |= (from..to).any(|i| t.is_word(i, "AUTOINCREMENT"));
        let c = column(&t, from, to);
        column_constraints(&t, &c.name, from, to, &mut out);
        out.columns.push(c);
    }
    let last = t.list.len();
    let end = if t.text(last - 1) == ";" {
        last - 1
    } else {
        last
    };
    if close + 1 < end {
        out.options = if dialect == Dialect::MySql {
            t.mysql_options(close + 1, end)
        } else {
            Some(t.source(close + 1, end - 1).to_owned())
        };
    }
    out
}

/// What `CREATE INDEX` `sql` says of its key parts and `WHERE`; `None` when it is not one.
#[must_use]
pub fn index(sql: &str, dialect: Dialect) -> Option<IndexClause> {
    let t = Tokens {
        sql,
        list: Lexer::new(sql, dialect)
            .filter(|t| !t.is_trivia())
            .collect(),
    };
    let on = (0..t.list.len()).find(|&i| t.is_word(i, "ON"))?;
    let open = (on..t.list.len()).find(|&i| t.text(i) == "(")?;
    let close = t.closing(open)?;
    let parts = if close > open + 1 {
        crate::catalog::split_list(t.source(open + 1, close - 1))
    } else {
        Vec::new()
    };
    let mut end = t.list.len();
    if t.text(end - 1) == ";" {
        end -= 1;
    }
    let predicate = (t.is_word(close + 1, "WHERE") && close + 2 < end)
        .then(|| t.source(close + 2, end - 1).to_owned());
    Some(IndexClause { parts, predicate })
}

/// The table constraint of the tokens `from..to`:
/// `[CONSTRAINT name] UNIQUE (..) | CHECK (..) | FOREIGN KEY (..) REFERENCES ..`.
fn constraint(t: &Tokens<'_>, from: usize, to: usize, out: &mut Definition) {
    let (name, at) = if t.is_word(from, "CONSTRAINT") {
        (Some(unquote(t.text(from + 1))), from + 2)
    } else {
        (None, from)
    };
    if t.is_word(at, "UNIQUE") {
        let open = (at..to).find(|&i| t.text(i) == "(");
        if let Some((columns, close)) = open.and_then(|o| t.names(o)) {
            out.uniques.push(UniqueDefinition {
                columns,
                on_conflict: t.on_conflict(close + 1, to),
            });
        }
    } else if t.is_word(at, "CHECK") && t.text(at + 1) == "(" {
        if let (Some(name), Some(close)) = (name, t.closing(at + 1)) {
            let not_enforced =
                (close..to).any(|i| t.is_word(i, "NOT") && t.is_word(i + 1, "ENFORCED"));
            out.checks.push(CheckDefinition { name, not_enforced });
        }
    } else if t.is_word(at, "FOREIGN") {
        let reference = (at..to).find(|&i| t.is_word(i, "REFERENCES"));
        let columns = t.names(at + 2).map(|(c, _)| c);
        if let (Some(at), Some(columns)) = (reference, columns) {
            out.foreign_keys.push(t.key(columns, at, to));
        }
    }
}

/// The key and UNIQUE a column of the tokens `from..to` declares: `REFERENCES t (id)
/// DEFERRABLE INITIALLY DEFERRED`, `UNIQUE ON CONFLICT REPLACE`.
fn column_constraints(t: &Tokens<'_>, name: &str, from: usize, to: usize, out: &mut Definition) {
    let mut depth = 0usize;
    for i in from + 1..to {
        match t.text(i) {
            "(" => depth += 1,
            ")" => depth = depth.saturating_sub(1),
            _ if depth > 0 => {}
            _ if t.is_word(i, "REFERENCES") => {
                out.foreign_keys.push(t.key(vec![name.to_owned()], i, to));
            }
            _ if t.is_word(i, "UNIQUE") => out.uniques.push(UniqueDefinition {
                columns: vec![name.to_owned()],
                on_conflict: t.on_conflict(i + 1, to),
            }),
            _ => {}
        }
    }
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
    let visibility = (close + 1..to)
        .find(|&i| t.is_word(i, "INVISIBLE") || t.is_word(i, "IGNORED"))
        .map(|i| t.text(i).to_ascii_uppercase());
    Some(IndexDefinition {
        name,
        unique,
        method,
        parts,
        visibility,
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
                    parts: vec!["`body`(20)".into(), "`id` DESC".into()],
                    ..Default::default()
                },
            ]
        );
    }

    #[test]
    fn reads_mysql_executable_clauses_and_table_options() {
        let d = parse(
            "CREATE TABLE `ix` (\n  `id` int NOT NULL,\n  `b` int DEFAULT NULL,\n  `w` int,\n  \
             PRIMARY KEY (`id`),\n  KEY `i2` (`b`) /*!80000 INVISIBLE */,\n  KEY `i3` (`w`),\n  \
             CONSTRAINT `w_chk` CHECK ((`w` < 100)) /*!80016 NOT ENFORCED */,\n  \
             CONSTRAINT `b_chk` CHECK ((`b` > 0))\n) ENGINE=MyISAM AUTO_INCREMENT=5 DEFAULT \
             CHARSET=latin1 COLLATE=latin1_general_ci COMMENT='a, b'\n\
             /*!50100 PARTITION BY RANGE (year(`d`))\n(PARTITION p0 VALUES LESS THAN (2020) ENGINE = InnoDB,\n \
             PARTITION p1 VALUES LESS THAN MAXVALUE ENGINE = InnoDB) */",
            Dialect::MySql,
        );
        let visibility: Vec<Option<&str>> =
            d.indexes.iter().map(|i| i.visibility.as_deref()).collect();
        assert_eq!(visibility, [Some("INVISIBLE"), None]);
        assert_eq!(
            d.checks,
            [
                CheckDefinition {
                    name: "w_chk".into(),
                    not_enforced: true
                },
                CheckDefinition {
                    name: "b_chk".into(),
                    not_enforced: false
                },
            ]
        );
        assert_eq!(
            d.options.as_deref(),
            Some(
                "ENGINE=MyISAM DEFAULT CHARSET=latin1 COLLATE=latin1_general_ci PARTITION BY RANGE \
                 (year(`d`)) (PARTITION p0 VALUES LESS THAN (2020) ENGINE = InnoDB, PARTITION p1 \
                 VALUES LESS THAN MAXVALUE ENGINE = InnoDB)"
            )
        );
        let mariadb = parse(
            "CREATE TABLE `sv` (\n  `id` int(11) NOT NULL,\n  KEY `i` (`id`) IGNORED\n) \
             ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 WITH SYSTEM VERSIONING",
            Dialect::MySql,
        );
        assert_eq!(
            mariadb.options.as_deref(),
            Some("ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 WITH SYSTEM VERSIONING")
        );
        assert_eq!(mariadb.indexes[0].visibility.as_deref(), Some("IGNORED"));
    }

    #[test]
    fn reads_sqlite_keys_and_unique_clauses() {
        let d = parse(
            "CREATE TABLE t (id INTEGER PRIMARY KEY,
               r INT REFERENCES \"f0\"(id) ON DELETE CASCADE DEFERRABLE INITIALLY DEFERRED,
               s INT REFERENCES f1 NOT DEFERRABLE, u TEXT UNIQUE ON CONFLICT REPLACE,
               a INT, b INT, CHECK (a > 0),
               CONSTRAINT k UNIQUE (a, \"b\") ON CONFLICT IGNORE,
               FOREIGN KEY (a, b) REFERENCES p (x, y) MATCH FULL DEFERRABLE)",
            Dialect::Sqlite,
        );
        assert_eq!(
            d.foreign_keys,
            [
                KeyDefinition {
                    columns: vec!["r".into()],
                    ref_table: "f0".into(),
                    deferral: Deferral::Deferred,
                    match_type: None,
                },
                KeyDefinition {
                    columns: vec!["s".into()],
                    ref_table: "f1".into(),
                    ..Default::default()
                },
                KeyDefinition {
                    columns: vec!["a".into(), "b".into()],
                    ref_table: "p".into(),
                    deferral: Deferral::Immediate,
                    match_type: Some("FULL".into()),
                },
            ]
        );
        assert_eq!(
            d.uniques,
            [
                UniqueDefinition {
                    columns: vec!["u".into()],
                    on_conflict: Some("REPLACE".into()),
                },
                UniqueDefinition {
                    columns: vec!["a".into(), "b".into()],
                    on_conflict: Some("IGNORE".into()),
                },
            ]
        );
        assert_eq!(d.columns.len(), 6);
    }

    #[test]
    fn reads_index_expressions_and_conditions() {
        let i = index(
            "CREATE UNIQUE INDEX main.e ON e1 (lower(name), n DESC) WHERE live = 1 AND n > 0;",
            Dialect::Sqlite,
        )
        .unwrap();
        assert_eq!(i.parts, ["lower(name)", "n DESC"]);
        assert_eq!(i.predicate.as_deref(), Some("live = 1 AND n > 0"));
        let plain = index("CREATE INDEX i ON t (a)", Dialect::Sqlite).unwrap();
        assert_eq!((plain.parts, plain.predicate), (vec!["a".to_owned()], None));
    }

    #[test]
    fn a_table_without_options_has_none() {
        let d = parse("CREATE TABLE t (a INT)", Dialect::Sqlite);
        assert_eq!(d.options, None);
        assert!(!d.autoincrement);
    }
}
