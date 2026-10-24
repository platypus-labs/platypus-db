//! The editor's SQL formatter (Ctrl+Shift+F). It works on tokens, not a parse tree, so
//! half-typed or dialect-specific SQL still formats. Clauses start new lines. Items of
//! `SELECT`, `SET`, `GROUP BY`, `ORDER BY` and `VALUES`, and the columns of `CREATE TABLE`,
//! get a line each. Subqueries are indented. Only whitespace and the case of keywords
//! change: comments, strings, quoted identifiers and routine bodies are kept as written.

use crate::{Dialect, Lexer, Token, TokenKind, split};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum KeywordCase {
    #[default]
    Upper,
    Lower,
    /// Keywords keep the case they were typed in.
    Preserve,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FormatOptions {
    pub keyword_case: KeywordCase,
    /// Spaces per indentation level.
    pub indent: usize,
}

impl Default for FormatOptions {
    fn default() -> Self {
        Self {
            keyword_case: KeywordCase::Upper,
            indent: 2,
        }
    }
}

/// Words whose case the formatter changes, sorted. Words that often name tables or columns
/// (`user`, `date`, `name`, ...) are left out: MySQL table names can be case-sensitive.
const KEYWORDS: &[&str] = &[
    "ADD",
    "ALL",
    "ALTER",
    "ALWAYS",
    "ANALYZE",
    "AND",
    "ANY",
    "AS",
    "ASC",
    "BEGIN",
    "BETWEEN",
    "BY",
    "CASCADE",
    "CASE",
    "CAST",
    "CHECK",
    "COALESCE",
    "COLLATE",
    "COLUMN",
    "COMMIT",
    "CONCURRENTLY",
    "CONFLICT",
    "CONSTRAINT",
    "CREATE",
    "CROSS",
    "CURRENT_DATE",
    "CURRENT_TIME",
    "CURRENT_TIMESTAMP",
    "DATABASE",
    "DEFAULT",
    "DELETE",
    "DESC",
    "DESCRIBE",
    "DISTINCT",
    "DO",
    "DROP",
    "DUPLICATE",
    "ELSE",
    "END",
    "ESCAPE",
    "EXCEPT",
    "EXISTS",
    "EXPLAIN",
    "EXTRACT",
    "FALSE",
    "FETCH",
    "FILTER",
    "FIRST",
    "FOLLOWING",
    "FOR",
    "FOREIGN",
    "FROM",
    "FULL",
    "FUNCTION",
    "GENERATED",
    "GRANT",
    "GROUP",
    "HAVING",
    "IDENTITY",
    "IF",
    "IGNORE",
    "ILIKE",
    "IN",
    "INDEX",
    "INNER",
    "INSERT",
    "INTERSECT",
    "INTERVAL",
    "INTO",
    "IS",
    "JOIN",
    "KEY",
    "LAST",
    "LATERAL",
    "LEFT",
    "LIKE",
    "LIMIT",
    "MATERIALIZED",
    "NATURAL",
    "NOT",
    "NOTHING",
    "NULL",
    "NULLIF",
    "NULLS",
    "OFFSET",
    "ON",
    "ONLY",
    "OR",
    "ORDER",
    "OUTER",
    "OVER",
    "PARTITION",
    "POSITION",
    "PRECEDING",
    "PRIMARY",
    "RECURSIVE",
    "REFERENCES",
    "RENAME",
    "REPLACE",
    "RESTRICT",
    "RETURNING",
    "REVOKE",
    "RIGHT",
    "ROLLBACK",
    "SCHEMA",
    "SELECT",
    "SEQUENCE",
    "SET",
    "SHOW",
    "SIMILAR",
    "SOME",
    "SUBSTRING",
    "TABLE",
    "TEMP",
    "TEMPORARY",
    "THEN",
    "TO",
    "TRANSACTION",
    "TRIGGER",
    "TRIM",
    "TRUE",
    "TRUNCATE",
    "UNBOUNDED",
    "UNION",
    "UNIQUE",
    "UNLOGGED",
    "UPDATE",
    "USING",
    "VACUUM",
    "VALUES",
    "VIEW",
    "WHEN",
    "WHERE",
    "WINDOW",
    "WITH",
    "WITHIN",
];

/// Keywords that are also functions: `CAST(x AS int)`, not `CAST (x AS int)`.
const FUNCTION_KEYWORDS: &[&str] = &[
    "ANY",
    "CAST",
    "COALESCE",
    "EXTRACT",
    "IF",
    "LEFT",
    "NULLIF",
    "POSITION",
    "REPLACE",
    "RIGHT",
    "SOME",
    "SUBSTRING",
    "TRIM",
];

/// After these a name is a table, so a following `(` is a column list: `INTO t (a, b)`.
const NAME_KEYWORDS: &[&str] = &["EXISTS", "INTO", "REFERENCES", "TABLE", "VIEW"];

/// Operators of several characters, longest first.
const OPERATORS: &[&str] = &[
    "->>", "#>>", "!~*", "<=>", "<>", "<=", ">=", "!=", "||", "->", "#>", "@>", "<@", "&&", "::",
    "=>", ":=", "~*", "!~", "?|", "?&", "<<", ">>", "~~",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Keyword,
    Word,
    /// Strings, numbers, quoted identifiers and parameters.
    Literal,
    Operator,
    Open,
    Close,
    Comma,
    Dot,
    Semicolon,
    BlockComment,
    LineComment,
}

#[derive(Debug)]
struct Atom<'a> {
    kind: Kind,
    text: &'a str,
    upper: String,
    /// The SQL had a line break before it.
    newline_before: bool,
}

fn is_digit(src: &str, t: &Token) -> bool {
    t.kind == TokenKind::Symbol && t.text(src).starts_with(|c: char| c.is_ascii_digit())
}

/// Index after the run of contiguous tokens from `i` that `take` accepts.
fn run_end(tokens: &[Token], i: usize, take: impl Fn(&Token) -> bool) -> usize {
    let mut j = i + 1;
    while j < tokens.len() && tokens[j].start == tokens[j - 1].end && take(&tokens[j]) {
        j += 1;
    }
    j
}

/// Classifies the symbol at `i`, joining it with the tokens it continues: numbers,
/// parameters (`$1`, `:name`, `${var}`, `@var`) and operators like `<=`.
fn symbol(src: &str, tokens: &[Token], i: usize) -> (Kind, usize) {
    let t = &tokens[i];
    let rest = &src[t.start..];
    let next = tokens.get(i + 1).filter(|n| n.start == t.end);
    let next_text = next.map_or("", |n| n.text(src));
    if is_digit(src, t) || (rest.starts_with('.') && next.is_some_and(|n| is_digit(src, n))) {
        let end = run_end(tokens, i, |n| {
            n.kind == TokenKind::Word || is_digit(src, n) || n.text(src) == "."
        });
        return (Kind::Literal, end);
    }
    if let Some(op) = OPERATORS.iter().find(|op| {
        rest.starts_with(*op)
            && i + op.len() <= tokens.len()
            && tokens[i..i + op.len()]
                .windows(2)
                .all(|w| w[1].start == w[0].end)
    }) {
        return (Kind::Operator, i + op.len());
    }
    let single = |kind| (kind, i + 1);
    match t.text(src) {
        "$" if next_text == "{" => {
            let close = (i + 1..tokens.len()).find(|&j| tokens[j].text(src) == "}");
            (Kind::Literal, close.map_or(tokens.len(), |j| j + 1))
        }
        "$" if next.is_some_and(|n| is_digit(src, n)) => {
            (Kind::Literal, run_end(tokens, i, |n| is_digit(src, n)))
        }
        ":" | "@" if next.is_some_and(|n| n.kind == TokenKind::Word) => (Kind::Literal, i + 2),
        "@" if next_text == "@" => (
            Kind::Literal,
            run_end(tokens, i, |n| {
                n.kind == TokenKind::Word || matches!(n.text(src), "@" | ".")
            }),
        ),
        "?" => single(Kind::Literal),
        "(" | "[" => single(Kind::Open),
        ")" | "]" => single(Kind::Close),
        "," => single(Kind::Comma),
        "." => single(Kind::Dot),
        ";" => single(Kind::Semicolon),
        _ => single(Kind::Operator),
    }
}

fn atoms(src: &str, dialect: Dialect) -> Vec<Atom<'_>> {
    let tokens: Vec<Token> = Lexer::new(src, dialect).collect();
    let mut atoms = Vec::new();
    let mut newline_before = false;
    let mut i = 0;
    while i < tokens.len() {
        let t = &tokens[i];
        let (kind, end) = match t.kind {
            TokenKind::Whitespace => {
                newline_before |= t.text(src).contains('\n');
                i += 1;
                continue;
            }
            TokenKind::LineComment => (Kind::LineComment, i + 1),
            TokenKind::BlockComment => (Kind::BlockComment, i + 1),
            TokenKind::String | TokenKind::QuotedIdent => (Kind::Literal, i + 1),
            // A prefixed string: `N'x'`, `X'0F'`, `_utf8mb4'x'`.
            TokenKind::Word
                if tokens
                    .get(i + 1)
                    .is_some_and(|n| n.start == t.end && n.kind == TokenKind::String) =>
            {
                (Kind::Literal, i + 2)
            }
            TokenKind::Word => {
                let upper = t.text(src).to_ascii_uppercase();
                let kind = if KEYWORDS.binary_search(&upper.as_str()).is_ok() {
                    Kind::Keyword
                } else {
                    Kind::Word
                };
                (kind, i + 1)
            }
            TokenKind::Symbol => symbol(src, &tokens, i),
        };
        let text = &src[t.start..tokens[end - 1].end];
        atoms.push(Atom {
            kind,
            text,
            upper: if matches!(kind, Kind::Keyword | Kind::Word) {
                text.to_ascii_uppercase()
            } else {
                String::new()
            },
            newline_before,
        });
        newline_before = false;
        i = end;
    }
    atoms
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Layout {
    /// The clause on its own line, its content indented below: `SELECT`, `WHERE`.
    Indent,
    /// The clause starts a line and its content follows: `INSERT INTO t`, `LIMIT 10`.
    Inline,
    /// The clause alone on a line: `UNION ALL`.
    Alone,
    /// Like `Inline`, one level deeper: `LEFT JOIN t ON ...`.
    Join,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Lead {
    /// Anywhere in a query.
    Any,
    /// Only where a statement starts: first in its parentheses, or after a CTE.
    Start,
    /// Where a statement starts, or later in one.
    StartOrQuery,
    /// Only after a statement has started.
    Query,
}

/// Clause keywords, longest first where they share a prefix.
const CLAUSES: &[(&[&str], Layout, Lead)] = &[
    (&["SELECT", "DISTINCT"], Layout::Indent, Lead::Any),
    (&["SELECT", "ALL"], Layout::Indent, Lead::Any),
    (&["SELECT"], Layout::Indent, Lead::Any),
    (&["WITH", "RECURSIVE"], Layout::Inline, Lead::Start),
    (&["WITH"], Layout::Inline, Lead::Start),
    (&["INSERT", "INTO"], Layout::Inline, Lead::Start),
    (&["REPLACE", "INTO"], Layout::Inline, Lead::Start),
    (&["UPDATE"], Layout::Inline, Lead::Start),
    (&["DELETE", "FROM"], Layout::Inline, Lead::Start),
    (&["VALUES"], Layout::Indent, Lead::StartOrQuery),
    (&["FROM"], Layout::Indent, Lead::Query),
    (&["WHERE"], Layout::Indent, Lead::Query),
    (&["GROUP", "BY"], Layout::Indent, Lead::Query),
    (&["HAVING"], Layout::Indent, Lead::Query),
    (&["WINDOW"], Layout::Indent, Lead::Query),
    (&["ORDER", "BY"], Layout::Indent, Lead::Query),
    (&["SET"], Layout::Indent, Lead::Query),
    (&["RETURNING"], Layout::Indent, Lead::Query),
    (
        &["ON", "DUPLICATE", "KEY", "UPDATE"],
        Layout::Indent,
        Lead::Query,
    ),
    (&["ON", "CONFLICT"], Layout::Inline, Lead::Query),
    (&["FOR", "UPDATE"], Layout::Inline, Lead::Query),
    (&["LIMIT"], Layout::Inline, Lead::Query),
    (&["OFFSET"], Layout::Inline, Lead::Query),
    (&["FETCH"], Layout::Inline, Lead::Query),
    (&["UNION", "ALL"], Layout::Alone, Lead::Query),
    (&["UNION"], Layout::Alone, Lead::Query),
    (&["INTERSECT"], Layout::Alone, Lead::Query),
    (&["EXCEPT"], Layout::Alone, Lead::Query),
    (&["LEFT", "OUTER", "JOIN"], Layout::Join, Lead::Query),
    (&["RIGHT", "OUTER", "JOIN"], Layout::Join, Lead::Query),
    (&["FULL", "OUTER", "JOIN"], Layout::Join, Lead::Query),
    (&["LEFT", "JOIN"], Layout::Join, Lead::Query),
    (&["RIGHT", "JOIN"], Layout::Join, Lead::Query),
    (&["FULL", "JOIN"], Layout::Join, Lead::Query),
    (&["INNER", "JOIN"], Layout::Join, Lead::Query),
    (&["CROSS", "JOIN"], Layout::Join, Lead::Query),
    (&["NATURAL", "JOIN"], Layout::Join, Lead::Query),
    (&["JOIN"], Layout::Join, Lead::Query),
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FrameKind {
    /// The statement, or a subquery in parentheses.
    Query,
    /// The column list of `CREATE TABLE`: one item per line.
    List,
    /// Any other parentheses: function calls, `IN (...)`, types. Kept on one line.
    Inline,
}

#[derive(Debug)]
struct Frame {
    kind: FrameKind,
    /// Indentation of the clauses (or list items) inside.
    base: usize,
    /// Indentation of the line holding the closing parenthesis.
    close_level: usize,
    /// A query has started, so clause keywords break lines.
    query: bool,
    clause: Option<Layout>,
    /// Inside `BETWEEN ... AND`, whose `AND` stays on the line.
    between: bool,
    /// Nothing written yet inside.
    first: bool,
}

impl Frame {
    fn new(kind: FrameKind, base: usize, close_level: usize) -> Self {
        Self {
            kind,
            base,
            close_level,
            query: false,
            clause: None,
            between: false,
            first: true,
        }
    }

    /// Indentation of a clause's content and of its wrapped `AND`/`OR`.
    fn content_level(&self) -> usize {
        self.base
            + if self.clause == Some(Layout::Join) {
                2
            } else {
                1
            }
    }
}

struct Formatter<'s, 'a> {
    atoms: &'s [Atom<'a>],
    options: FormatOptions,
    out: String,
    /// Indentation of the current line.
    level: usize,
    /// A line break to make before the next atom, at this level.
    pending: Option<usize>,
    frames: Vec<Frame>,
    /// Index of the last atom written, comments aside.
    prev: Option<usize>,
    /// Clause keywords break lines; off for `GRANT SELECT ...` and `ALTER`.
    clauses: bool,
    /// `CREATE TABLE`'s column list is still to come.
    table_list: bool,
    /// A table name follows a `NAME_KEYWORDS` keyword.
    after_name_keyword: bool,
}

impl<'s, 'a> Formatter<'s, 'a> {
    fn new(atoms: &'s [Atom<'a>], options: FormatOptions) -> Self {
        let head: Vec<&str> = atoms
            .iter()
            .filter(|a| matches!(a.kind, Kind::Keyword | Kind::Word))
            .take(5)
            .map(|a| a.upper.as_str())
            .collect();
        Self {
            atoms,
            options,
            out: String::new(),
            level: 0,
            pending: None,
            frames: vec![Frame::new(FrameKind::Query, 0, 0)],
            prev: None,
            clauses: !matches!(head.first(), Some(&("GRANT" | "REVOKE" | "ALTER")))
                && head.get(1) != Some(&"POLICY"),
            table_list: head.first() == Some(&"CREATE") && head.contains(&"TABLE"),
            after_name_keyword: false,
        }
    }

    fn frame(&mut self) -> &mut Frame {
        let last = self.frames.len() - 1;
        &mut self.frames[last]
    }

    /// Nothing written yet on the current line.
    fn line_start(&self) -> bool {
        self.out.is_empty() || self.out.ends_with('\n')
    }

    fn newline(&mut self, level: usize) {
        self.pending = None;
        self.level = level;
        if self.line_start() {
            return;
        }
        self.out.truncate(self.out.trim_end_matches(' ').len());
        self.out.push('\n');
    }

    fn push(&mut self, text: &str, space: bool) {
        if let Some(level) = self.pending.take() {
            self.newline(level);
        }
        if self.line_start() {
            self.out
                .extend(std::iter::repeat_n(' ', self.level * self.options.indent));
        } else if space {
            self.out.push(' ');
        }
        self.out.push_str(text);
    }

    fn is_unary(&self, i: usize) -> bool {
        let atom = &self.atoms[i];
        if atom.kind != Kind::Operator || !matches!(atom.text, "-" | "+") {
            return false;
        }
        let before = self.atoms[..i]
            .iter()
            .rev()
            .find(|a| !matches!(a.kind, Kind::LineComment | Kind::BlockComment));
        before.is_none_or(|b| match b.kind {
            Kind::Operator | Kind::Open | Kind::Comma => true,
            Kind::Keyword => !matches!(b.upper.as_str(), "NULL" | "TRUE" | "FALSE" | "END"),
            _ => false,
        })
    }

    fn space_before(&self, atom: &Atom) -> bool {
        let Some(p) = self.prev else {
            return false;
        };
        let prev = &self.atoms[p];
        match (prev.kind, atom.kind) {
            (_, Kind::Comma | Kind::Close | Kind::Dot | Kind::Semicolon)
            | (Kind::Open | Kind::Dot, _) => false,
            _ if prev.text == "::" || atom.text == "::" => false,
            // Joining `- -1` into `--1` would start a comment.
            (Kind::Operator, Kind::Operator) => true,
            _ if self.is_unary(p) => false,
            (_, Kind::Open) if atom.text == "[" => false,
            (Kind::Keyword, Kind::Open) => FUNCTION_KEYWORDS
                .binary_search(&prev.upper.as_str())
                .is_err(),
            (Kind::Word | Kind::Literal | Kind::Close, Kind::Open) => self.after_name_keyword,
            _ => true,
        }
    }

    /// Writes atom `i` as it is, keywords in the configured case.
    fn emit(&mut self, i: usize) {
        let atoms = self.atoms;
        let atom = &atoms[i];
        let space = self.space_before(atom);
        let text = match (atom.kind, self.options.keyword_case) {
            (Kind::Keyword, KeywordCase::Upper) => atom.upper.clone(),
            (Kind::Keyword, KeywordCase::Lower) => atom.text.to_ascii_lowercase(),
            _ => atom.text.to_owned(),
        };
        self.push(&text, space);
        self.after_name_keyword = match atom.kind {
            Kind::Keyword => NAME_KEYWORDS.contains(&atom.upper.as_str()),
            Kind::Word | Kind::Literal | Kind::Dot => self.after_name_keyword,
            _ => false,
        };
        self.prev = Some(i);
        self.frame().first = false;
    }

    /// The clause starting at atom `i`, if any: its word count and layout.
    fn clause_at(&self, i: usize) -> Option<(usize, Layout)> {
        let frame = self.frames.last()?;
        let prev = self.prev.map(|p| &self.atoms[p]);
        // After an operator a keyword is a value: `x = VALUES(x)`. `*` ends a select list.
        if !self.clauses
            || frame.kind != FrameKind::Query
            || prev.is_some_and(|p| p.kind == Kind::Operator && p.text != "*")
        {
            return None;
        }
        let at_start = frame.first || prev.is_some_and(|p| p.kind == Kind::Close);
        let words: Vec<&str> = self.atoms[i..]
            .iter()
            .take(4)
            .take_while(|a| matches!(a.kind, Kind::Keyword | Kind::Word))
            .map(|a| a.upper.as_str())
            .collect();
        CLAUSES
            .iter()
            .find(|(pattern, _, lead)| {
                words.starts_with(pattern)
                    && match lead {
                        Lead::Any => true,
                        Lead::Start => at_start,
                        Lead::StartOrQuery => at_start || frame.query,
                        Lead::Query => frame.query,
                    }
            })
            .map(|(pattern, layout, _)| (pattern.len(), *layout))
    }

    fn clause(&mut self, i: usize, words: usize, layout: Layout) {
        let frame = self.frame();
        frame.query = true;
        frame.clause = Some(layout);
        frame.between = false;
        let base = frame.base;
        self.newline(if layout == Layout::Join {
            base + 1
        } else {
            base
        });
        for k in i..i + words {
            self.emit(k);
        }
        match layout {
            Layout::Indent => self.pending = Some(base + 1),
            Layout::Alone => self.pending = Some(base),
            Layout::Inline | Layout::Join => {}
        }
    }

    fn keyword(&mut self, i: usize) -> usize {
        if let Some((words, layout)) = self.clause_at(i) {
            self.clause(i, words, layout);
            return i + words;
        }
        let atoms = self.atoms;
        let frame = self.frame();
        match atoms[i].upper.as_str() {
            "BETWEEN" => frame.between = true,
            "AND" if frame.between => frame.between = false,
            "AND" | "OR" if frame.kind == FrameKind::Query && frame.clause.is_some() => {
                let level = frame.content_level();
                self.newline(level);
            }
            _ => {}
        }
        self.emit(i);
        i + 1
    }

    fn comma(&mut self, i: usize) {
        self.emit(i);
        let frame = self.frame();
        let (kind, base, clause) = (frame.kind, frame.base, frame.clause);
        match kind {
            FrameKind::List => self.pending = Some(base),
            FrameKind::Query if clause == Some(Layout::Indent) => self.pending = Some(base + 1),
            _ => {}
        }
    }

    fn open(&mut self, i: usize) {
        let next = self.atoms[i + 1..]
            .iter()
            .find(|a| !matches!(a.kind, Kind::LineComment | Kind::BlockComment));
        let subquery =
            next.is_some_and(|n| matches!(n.upper.as_str(), "SELECT" | "WITH" | "VALUES"));
        let list = !subquery && self.table_list && self.frames.len() == 1;
        self.emit(i);
        let level = self.level;
        if subquery {
            self.frames
                .push(Frame::new(FrameKind::Query, level + 1, level));
        } else if list {
            self.table_list = false;
            self.frames
                .push(Frame::new(FrameKind::List, level + 1, level));
            self.pending = Some(level + 1);
        } else {
            self.frames
                .push(Frame::new(FrameKind::Inline, level, level));
        }
    }

    fn close(&mut self, i: usize) {
        if self.frames.len() > 1
            && let Some(frame) = self.frames.pop()
            && frame.kind != FrameKind::Inline
        {
            self.newline(frame.close_level);
        }
        self.emit(i);
    }

    fn comment(&mut self, i: usize) {
        let atoms = self.atoms;
        let atom = &atoms[i];
        if atom.newline_before {
            self.newline(self.pending.unwrap_or(self.level));
        }
        let level = self.pending.unwrap_or(self.level);
        self.push(atom.text, true);
        if atom.kind == Kind::LineComment {
            self.pending = Some(level);
        }
    }

    fn run(mut self) -> String {
        let mut i = 0;
        while i < self.atoms.len() {
            match self.atoms[i].kind {
                Kind::Keyword => {
                    i = self.keyword(i);
                    continue;
                }
                Kind::Comma => self.comma(i),
                Kind::Open => self.open(i),
                Kind::Close => self.close(i),
                Kind::LineComment | Kind::BlockComment => self.comment(i),
                Kind::Semicolon => {
                    self.emit(i);
                    self.pending = Some(self.frame().base);
                }
                Kind::Word | Kind::Literal | Kind::Operator | Kind::Dot => self.emit(i),
            }
            i += 1;
        }
        self.out
    }
}

/// Routine and trigger definitions keep their layout: their bodies are code.
fn is_verbatim(atoms: &[Atom]) -> bool {
    let head: Vec<&str> = atoms
        .iter()
        .filter(|a| matches!(a.kind, Kind::Keyword | Kind::Word))
        .take(8)
        .map(|a| a.upper.as_str())
        .collect();
    match head.first() {
        Some(&("CREATE" | "ALTER")) => head.iter().any(|w| {
            matches!(
                *w,
                "FUNCTION" | "PROCEDURE" | "TRIGGER" | "EVENT" | "PACKAGE"
            )
        }),
        Some(&"DO") => true,
        _ => false,
    }
}

fn format_statement(sql: &str, dialect: Dialect, options: FormatOptions) -> String {
    let atoms = atoms(sql, dialect);
    if is_verbatim(&atoms) {
        return sql.to_owned();
    }
    Formatter::new(&atoms, options).run()
}

fn is_comment(line: &str) -> bool {
    line.starts_with("--") || line.starts_with("/*") || line.starts_with('#')
}

/// Writes the text between two statements: the delimiter stays on the statement's last
/// line, comments and client commands (`DELIMITER`) go on their own lines.
fn push_gap(out: &mut String, gap: &str, after_statement: bool, before_statement: bool) {
    let mut rest = gap;
    if after_statement {
        let (head, tail) = gap.split_once('\n').unwrap_or((gap, ""));
        let head = head.trim();
        if !head.is_empty() {
            if is_comment(head) {
                out.push(' ');
            }
            out.push_str(head);
        }
        rest = tail;
    }
    let lines: Vec<&str> = rest
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .collect();
    for (k, line) in lines.iter().enumerate() {
        if !out.is_empty() {
            out.push_str(if k == 0 { "\n\n" } else { "\n" });
        }
        out.push_str(line);
    }
    if before_statement && !out.is_empty() {
        out.push_str(if lines.is_empty() { "\n\n" } else { "\n" });
    }
}

/// Formats a script; statements are separated by a blank line. The result ends with a
/// line break when `src` does.
#[must_use]
pub fn format(src: &str, dialect: Dialect, options: FormatOptions) -> String {
    let mut out = String::with_capacity(src.len() + src.len() / 4);
    let mut pos = 0;
    for (n, statement) in split(src, dialect).iter().enumerate() {
        push_gap(&mut out, &src[pos..statement.start], n > 0, true);
        let text = statement.text(src);
        // Client commands such as `@include file.sql` stay as typed.
        if text.starts_with('@') {
            out.push_str(text);
        } else {
            out.push_str(&format_statement(text, dialect, options));
        }
        pos = statement.end;
    }
    push_gap(&mut out, &src[pos..], pos > 0, false);
    if src.ends_with('\n') && !out.is_empty() {
        out.push('\n');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pg(src: &str) -> String {
        format(src, Dialect::Postgres, FormatOptions::default())
    }

    #[test]
    fn word_lists_are_sorted() {
        for list in [KEYWORDS, FUNCTION_KEYWORDS] {
            assert!(list.windows(2).all(|w| w[0] < w[1]), "{list:?}");
        }
    }

    #[test]
    fn formats_a_query() {
        assert_eq!(
            pg(
                "select a, count(*) as n from t join u on u.id = t.u_id and u.x > -1 \
                where a between 1 and 2 or b is null group by a order by n desc limit 10"
            ),
            "SELECT\n  a,\n  count(*) AS n\nFROM\n  t\n  JOIN u ON u.id = t.u_id\n    AND u.x > -1\n\
             WHERE\n  a BETWEEN 1 AND 2\n  OR b IS NULL\nGROUP BY\n  a\nORDER BY\n  n DESC\nLIMIT 10"
        );
    }

    #[test]
    fn indents_subqueries_and_ctes() {
        assert_eq!(
            pg("with x as (select id from t) select * from x where id in (select id from u)"),
            "WITH x AS (\n  SELECT\n    id\n  FROM\n    t\n)\nSELECT\n  *\nFROM\n  x\n\
             WHERE\n  id IN (\n    SELECT\n      id\n    FROM\n      u\n  )"
        );
    }

    #[test]
    fn formats_dml() {
        assert_eq!(
            pg(
                "insert into t(a,b) values (1,'x'),(2,'y') on conflict (a) do update set b=excluded.b"
            ),
            "INSERT INTO t (a, b)\nVALUES\n  (1, 'x'),\n  (2, 'y')\nON CONFLICT (a) DO UPDATE\n\
             SET\n  b = excluded.b"
        );
        assert_eq!(
            pg("update t set a=1,b=2 where id=$1 returning *"),
            "UPDATE t\nSET\n  a = 1,\n  b = 2\nWHERE\n  id = $1\nRETURNING\n  *"
        );
    }

    #[test]
    fn create_table_columns_get_a_line_each() {
        assert_eq!(
            pg("create table if not exists t (id int primary key, n numeric(10,2) check (n > 0))"),
            "CREATE TABLE IF NOT EXISTS t (\n  id int PRIMARY KEY,\n  n numeric(10, 2) CHECK (n > 0)\n)"
        );
    }

    #[test]
    fn keeps_literals_operators_and_comments() {
        assert_eq!(
            pg(
                "select x::int, 'a  b', \"Mixed\"  ,j->>'k', 1.5e3, :name, ${v} -- note\nfrom t /* c */"
            ),
            "SELECT\n  x::int,\n  'a  b',\n  \"Mixed\",\n  j ->> 'k',\n  1.5e3,\n  :name,\n  ${v} -- note\n\
             FROM\n  t /* c */"
        );
        assert_eq!(
            format(
                "select @v, N'x', a<=>b",
                Dialect::MySql,
                FormatOptions::default()
            ),
            "SELECT\n  @v,\n  N'x',\n  a <=> b"
        );
    }

    #[test]
    fn keyword_case_and_indent_are_options() {
        let options = FormatOptions {
            keyword_case: KeywordCase::Lower,
            indent: 4,
        };
        assert_eq!(
            format("SELECT A FROM T", Dialect::Postgres, options),
            "select\n    A\nfrom\n    T"
        );
        let preserve = FormatOptions {
            keyword_case: KeywordCase::Preserve,
            indent: 2,
        };
        assert_eq!(
            format("Select a From t", Dialect::Postgres, preserve),
            "Select\n  a\nFrom\n  t"
        );
    }

    #[test]
    fn separates_statements_and_keeps_comments_between_them() {
        assert_eq!(
            pg("-- head\nselect 1;  -- one\n\n\n-- two\nselect 2;\n"),
            "-- head\nSELECT\n  1;  -- one\n\n-- two\nSELECT\n  2;\n"
        );
    }

    #[test]
    fn leaves_routines_and_grants_alone() {
        let body = "create function f() returns int language sql as $$ select   1 $$";
        assert_eq!(pg(body), body);
        assert_eq!(
            pg("grant select, insert on t to u"),
            "GRANT SELECT, INSERT ON t TO u"
        );
        let mysql =
            "DELIMITER //\nCREATE PROCEDURE p() BEGIN SELECT 1; END//\nDELIMITER ;\nselect 1";
        assert_eq!(
            format(mysql, Dialect::MySql, FormatOptions::default()),
            "DELIMITER //\nCREATE PROCEDURE p() BEGIN SELECT 1; END//\n\nDELIMITER ;\nSELECT\n  1"
        );
        assert_eq!(
            pg("@include select.sql\nselect 1"),
            "@include select.sql\n\nSELECT\n  1"
        );
    }

    /// The tokens, upper-cased, without whitespace: formatting must not change them.
    fn tokens(src: &str, d: Dialect) -> Vec<String> {
        Lexer::new(src, d)
            .filter(|t| t.kind != TokenKind::Whitespace)
            .map(|t| t.text(src).to_ascii_uppercase())
            .collect()
    }

    #[test]
    fn only_whitespace_and_keyword_case_change() {
        let corpus = [
            "select a - -1, x=-1, 1e-5, -(2), b||'x', arr[1], t.*, count(distinct a) from t",
            "select * from a natural join b cross join c left outer join d using (id)",
            "select case when a then 'x' else 'y' end, cast(a as int), a::numeric(10,2)",
            "select row_number() over (partition by a order by b) from t for update",
            "insert into t select * from u union all select * from v except select 1",
            "delete from t where exists (select 1 from u where u.id = t.id) returning id",
            "create index concurrently i on t (lower(a)); alter table t add column b int",
            "select $1, :p, ?, ${v}, @x, @@global.y, e'\\n', $$a$$, \"q\"\"x\" -- c\n from t",
            "select - -1, 1 - - 1, - -a",
            "select 'unterminated",
            "select (select 1), ((1 + 2)) * 3 /* c */ from t where a in (1, 2) and b like 'x%'",
        ];
        for d in [
            Dialect::Postgres,
            Dialect::MySql,
            Dialect::Sqlite,
            Dialect::DuckDb,
            Dialect::SqlServer,
        ] {
            for sql in corpus {
                let formatted = format(sql, d, FormatOptions::default());
                assert_eq!(
                    tokens(&formatted, d),
                    tokens(sql, d),
                    "{d:?}: {sql}\n{formatted}"
                );
                assert_eq!(
                    format(&formatted, d, FormatOptions::default()),
                    formatted,
                    "{d:?}: formatting twice changes nothing"
                );
            }
        }
    }

    #[test]
    fn half_typed_sql_still_formats() {
        assert_eq!(
            pg("select a, from t where ("),
            "SELECT\n  a,\nFROM\n  t\nWHERE\n  ("
        );
        assert_eq!(pg("select 'open"), "SELECT\n  'open");
        assert_eq!(pg(""), "");
    }
}
