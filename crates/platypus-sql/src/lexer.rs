//! A forgiving SQL tokenizer: it never fails, so half-typed SQL in an editor still splits.
//! It only knows what statement boundaries depend on: quoting, comments and words.

use crate::Dialect;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TokenKind {
    Whitespace,
    LineComment,
    BlockComment,
    /// String literal, including dollar-quoted bodies.
    String,
    /// Quoted identifier: `"x"`, `` `x` ``, `[x]`.
    QuotedIdent,
    /// Keyword or bare identifier.
    Word,
    /// Any other character (operators, punctuation, `;`).
    Symbol,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Token {
    pub kind: TokenKind,
    pub start: usize,
    pub end: usize,
}

impl Token {
    #[must_use]
    pub fn text<'a>(&self, src: &'a str) -> &'a str {
        &src[self.start..self.end]
    }

    #[must_use]
    pub fn is_trivia(&self) -> bool {
        matches!(
            self.kind,
            TokenKind::Whitespace | TokenKind::LineComment | TokenKind::BlockComment
        )
    }
}

/// The name in a quoted identifier: quotes removed, doubled closing quotes undone
/// (Transact-SQL doubles `]` in `[...]` names).
pub(crate) fn unquote_ident(text: &str) -> Option<String> {
    let close = match text.chars().next()? {
        '[' => ']',
        c => c,
    };
    let inner = text.get(1..text.len().saturating_sub(1))?;
    Some(inner.replace(&format!("{close}{close}"), &close.to_string()))
}

fn is_ident_start(c: char) -> bool {
    c.is_alphabetic() || c == '_'
}

/// Whether a backslash escapes the next character inside quotes.
fn backslashes(dialect: Dialect) -> bool {
    matches!(
        dialect,
        Dialect::MySql | Dialect::ClickHouse | Dialect::BigQuery | Dialect::Databricks
    )
}

fn is_ident_char(c: char, dialect: Dialect) -> bool {
    c.is_alphanumeric()
        || c == '_'
        || (c == '$' && dialect != Dialect::Sqlite)
        // Transact-SQL names: `#temp`, `@variable`, `@@ROWCOUNT`.
        || (matches!(c, '#' | '@') && dialect == Dialect::SqlServer)
        // Oracle names may hold `#` after the first letter.
        || (c == '#' && dialect == Dialect::Oracle)
}

#[derive(Debug)]
pub struct Lexer<'a> {
    src: &'a str,
    pos: usize,
    dialect: Dialect,
}

impl<'a> Lexer<'a> {
    #[must_use]
    pub fn new(src: &'a str, dialect: Dialect) -> Self {
        Self {
            src,
            pos: 0,
            dialect,
        }
    }

    fn rest(&self) -> &'a str {
        &self.src[self.pos..]
    }

    fn peek(&self) -> Option<char> {
        self.rest().chars().next()
    }

    fn peek_at(&self, n: usize) -> Option<char> {
        self.rest().chars().nth(n)
    }

    fn bump(&mut self) -> Option<char> {
        let c = self.peek()?;
        self.pos += c.len_utf8();
        Some(c)
    }

    fn eat_while(&mut self, f: impl Fn(char) -> bool) {
        while self.peek().is_some_and(&f) {
            self.bump();
        }
    }

    /// Consumes an Oracle alternative-quoted string, `q'[...]'` or `nq'{...}'`, whose
    /// delimiter is any character, paired for brackets; `false` when none starts here.
    fn q_quote(&mut self) -> bool {
        let rest = self.rest();
        let prefix = usize::from(rest.starts_with(['n', 'N']));
        let mut chars = rest[prefix..].chars();
        let (Some('q' | 'Q'), Some('\''), Some(open)) = (chars.next(), chars.next(), chars.next())
        else {
            return false;
        };
        let close = match open {
            '[' => ']',
            '{' => '}',
            '<' => '>',
            '(' => ')',
            c => c,
        };
        let body = prefix + 2 + open.len_utf8();
        let mut end = [0; 4];
        let terminator = format!("{}'", close.encode_utf8(&mut end));
        self.pos += rest[body..]
            .find(&terminator)
            .map_or(rest.len(), |i| body + i + terminator.len());
        true
    }

    /// Consumes a quoted run ending at `close`. A doubled `close` is an escaped quote;
    /// with `backslash`, `\x` escapes any character. Unterminated runs end at EOF.
    fn quoted(&mut self, close: char, backslash: bool) {
        while let Some(c) = self.bump() {
            if backslash && c == '\\' {
                self.bump();
            } else if c == close {
                if self.peek() == Some(close) {
                    self.bump();
                } else {
                    return;
                }
            }
        }
    }

    fn block_comment(&mut self) {
        // PostgreSQL, DuckDB, SQL Server and ClickHouse nest block comments; MySQL and
        // SQLite do not.
        let nested = matches!(
            self.dialect,
            Dialect::Postgres | Dialect::DuckDb | Dialect::SqlServer | Dialect::ClickHouse
        );
        let mut depth = 1;
        while depth > 0 {
            if self.rest().starts_with("*/") {
                self.pos += 2;
                depth -= 1;
            } else if nested && self.rest().starts_with("/*") {
                self.pos += 2;
                depth += 1;
            } else if self.bump().is_none() {
                return;
            }
        }
    }

    /// `$tag$ ... $tag$` (PostgreSQL). Returns false, consuming nothing, if this `$` does
    /// not open a dollar quote (e.g. a `$1` parameter).
    fn dollar_quote(&mut self) -> bool {
        let rest = self.rest();
        let tag_len = rest[1..]
            .char_indices()
            .find(|&(i, c)| {
                !(if i == 0 {
                    is_ident_start(c)
                } else {
                    is_ident_char(c, Dialect::Sqlite)
                })
            })
            .map_or(rest.len() - 1, |(i, _)| i);
        if !rest[1 + tag_len..].starts_with('$') {
            return false;
        }
        let delimiter = &rest[..tag_len + 2];
        self.pos += delimiter.len();
        match self.rest().find(delimiter) {
            Some(end) => self.pos += end + delimiter.len(),
            None => self.pos = self.src.len(),
        }
        true
    }

    /// The previous character continues a word, so `$` belongs to that identifier.
    fn after_word(&self) -> bool {
        self.src[..self.pos]
            .chars()
            .next_back()
            .is_some_and(|c| is_ident_char(c, self.dialect))
    }

    fn next_token(&mut self) -> Option<Token> {
        let start = self.pos;
        let c = self.peek()?;
        let d = self.dialect;
        let kind = if c.is_whitespace() {
            self.eat_while(char::is_whitespace);
            TokenKind::Whitespace
        } else if c == '-'
            && self.peek_at(1) == Some('-')
            // MySQL needs whitespace (or end of input) after `--`.
            && (d != Dialect::MySql || self.peek_at(2).is_none_or(char::is_whitespace))
        {
            self.eat_while(|c| c != '\n');
            TokenKind::LineComment
        } else if c == '#'
            && (matches!(d, Dialect::MySql | Dialect::BigQuery)
                // ClickHouse reads `# ` and `#!` as comments.
                || (d == Dialect::ClickHouse
                    && self.peek_at(1).is_none_or(|n| n == '!' || n.is_whitespace())))
        {
            self.eat_while(|c| c != '\n');
            TokenKind::LineComment
        } else if c == '/' && self.peek_at(1) == Some('*') {
            self.pos += 2;
            self.block_comment();
            TokenKind::BlockComment
        } else if c == '\'' {
            self.bump();
            self.quoted('\'', backslashes(d));
            TokenKind::String
        } else if (c == 'e' || c == 'E')
            && self.peek_at(1) == Some('\'')
            && matches!(d, Dialect::Postgres | Dialect::DuckDb)
        {
            self.pos += 2;
            self.quoted('\'', true);
            TokenKind::String
        } else if (c == 'n' || c == 'N') && self.peek_at(1) == Some('\'') && d == Dialect::SqlServer
        {
            self.pos += 2;
            self.quoted('\'', false);
            TokenKind::String
        } else if c == '"' {
            self.bump();
            // MySQL treats "..." as a string (ANSI_QUOTES off by default), and so do
            // BigQuery and Spark.
            self.quoted('"', backslashes(d));
            if matches!(d, Dialect::MySql | Dialect::BigQuery | Dialect::Databricks) {
                TokenKind::String
            } else {
                TokenKind::QuotedIdent
            }
        } else if c == '`'
            && matches!(
                d,
                Dialect::MySql
                    | Dialect::Sqlite
                    | Dialect::DuckDb
                    | Dialect::ClickHouse
                    | Dialect::BigQuery
                    | Dialect::Databricks
            )
        {
            self.bump();
            self.quoted('`', matches!(d, Dialect::ClickHouse | Dialect::BigQuery));
            TokenKind::QuotedIdent
        } else if c == '[' && d == Dialect::Sqlite {
            self.bump();
            self.eat_while(|c| c != ']');
            self.bump();
            TokenKind::QuotedIdent
        } else if c == '[' && d == Dialect::SqlServer {
            self.bump();
            self.quoted(']', false);
            TokenKind::QuotedIdent
        } else if c == '#' && d == Dialect::SqlServer {
            self.eat_while(|c| is_ident_char(c, d));
            TokenKind::Word
        } else if (c == '$'
            // ClickHouse calls dollar quotes heredocs.
            && matches!(d, Dialect::Postgres | Dialect::DuckDb | Dialect::ClickHouse)
            && !self.after_word()
            && self.dollar_quote())
            || (matches!(c, 'q' | 'Q' | 'n' | 'N') && d == Dialect::Oracle && self.q_quote())
        {
            TokenKind::String
        } else if is_ident_start(c) {
            self.eat_while(|c| is_ident_char(c, d));
            TokenKind::Word
        } else {
            self.bump();
            TokenKind::Symbol
        };
        Some(Token {
            kind,
            start,
            end: self.pos,
        })
    }
}

impl Iterator for Lexer<'_> {
    type Item = Token;

    fn next(&mut self) -> Option<Token> {
        self.next_token()
    }
}

impl Lexer<'_> {
    /// Repositions the lexer, e.g. after a MySQL `DELIMITER` line.
    pub fn seek(&mut self, pos: usize) {
        self.pos = pos;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kinds(src: &str, d: Dialect) -> Vec<(TokenKind, &str)> {
        Lexer::new(src, d)
            .filter(|t| t.kind != TokenKind::Whitespace)
            .map(|t| (t.kind, t.text(src)))
            .collect()
    }

    #[test]
    fn postgres_quoting() {
        use TokenKind::*;
        let src = r#"select 'it''s', E'a\'b', "x""y", $$a;b$$, $fn$ $$ $fn$, $1, a$b"#;
        assert_eq!(
            kinds(src, Dialect::Postgres),
            [
                (Word, "select"),
                (String, "'it''s'"),
                (Symbol, ","),
                (String, r"E'a\'b'"),
                (Symbol, ","),
                (QuotedIdent, r#""x""y""#),
                (Symbol, ","),
                (String, "$$a;b$$"),
                (Symbol, ","),
                (String, "$fn$ $$ $fn$"),
                (Symbol, ","),
                (Symbol, "$"),
                (Symbol, "1"),
                (Symbol, ","),
                (Word, "a$b"),
            ]
        );
    }

    #[test]
    fn nested_comments_only_in_postgres() {
        let src = "/* a /* b */ c */x";
        assert_eq!(
            kinds(src, Dialect::Postgres).last(),
            Some(&(TokenKind::Word, "x"))
        );
        assert_eq!(
            kinds(src, Dialect::MySql)[0],
            (TokenKind::BlockComment, "/* a /* b */")
        );
    }

    #[test]
    fn clickhouse_quoting_and_comments() {
        use TokenKind::*;
        let src = "select 'a\\'b', \"c\\\"d\", `e\\`f`, $h$x;y$h$ # note\n#!x\n/* a /* b */ c */";
        assert_eq!(
            kinds(src, Dialect::ClickHouse),
            [
                (Word, "select"),
                (String, "'a\\'b'"),
                (Symbol, ","),
                (QuotedIdent, "\"c\\\"d\""),
                (Symbol, ","),
                (QuotedIdent, "`e\\`f`"),
                (Symbol, ","),
                (String, "$h$x;y$h$"),
                (LineComment, "# note"),
                (LineComment, "#!x"),
                (BlockComment, "/* a /* b */ c */"),
            ]
        );
    }

    #[test]
    fn duckdb_quoting_and_comments() {
        use TokenKind::*;
        let src = r"select E'a\'b', $$a;b$$, `c``d`, a$b, $1 /* x /* y */ z */";
        assert_eq!(
            kinds(src, Dialect::DuckDb),
            [
                (Word, "select"),
                (String, r"E'a\'b'"),
                (Symbol, ","),
                (String, "$$a;b$$"),
                (Symbol, ","),
                (QuotedIdent, "`c``d`"),
                (Symbol, ","),
                (Word, "a$b"),
                (Symbol, ","),
                (Symbol, "$"),
                (Symbol, "1"),
                (BlockComment, "/* x /* y */ z */"),
            ]
        );
    }

    #[test]
    fn mysql_quoting_and_comments() {
        use TokenKind::*;
        let src = "select 'a\\'b', \"c;d\", `e``f`, 1--2\n# hash\n-- dash";
        assert_eq!(
            kinds(src, Dialect::MySql),
            [
                (Word, "select"),
                (String, "'a\\'b'"),
                (Symbol, ","),
                (String, "\"c;d\""),
                (Symbol, ","),
                (QuotedIdent, "`e``f`"),
                (Symbol, ","),
                (Symbol, "1"),
                (Symbol, "-"),
                (Symbol, "-"),
                (Symbol, "2"),
                (LineComment, "# hash"),
                (LineComment, "-- dash"),
            ]
        );
    }

    #[test]
    fn sql_server_quoting() {
        use TokenKind::*;
        let src = "select N'it''s', [a]]b], \"q\", #tmp, @@rowcount, /* a /* b */ c */ x";
        assert_eq!(
            kinds(src, Dialect::SqlServer),
            [
                (Word, "select"),
                (String, "N'it''s'"),
                (Symbol, ","),
                (QuotedIdent, "[a]]b]"),
                (Symbol, ","),
                (QuotedIdent, "\"q\""),
                (Symbol, ","),
                (Word, "#tmp"),
                (Symbol, ","),
                // `@` starts no word, so that client commands (`@set`) stay recognisable.
                (Symbol, "@"),
                (Symbol, "@"),
                (Word, "rowcount"),
                (Symbol, ","),
                (BlockComment, "/* a /* b */ c */"),
                (Word, "x"),
            ]
        );
    }

    #[test]
    fn unterminated_runs_end_at_eof() {
        assert_eq!(
            kinds("select 'abc", Dialect::Postgres)[1],
            (TokenKind::String, "'abc")
        );
        assert_eq!(
            kinds("/* open", Dialect::Postgres)[0],
            (TokenKind::BlockComment, "/* open")
        );
        assert_eq!(
            kinds("$q$ open", Dialect::Postgres)[0],
            (TokenKind::String, "$q$ open")
        );
    }

    #[test]
    fn bridge_dialects_quoting() {
        // BigQuery and Spark: "…" is a string, `…` a name, backslashes escape.
        assert_eq!(
            kinds(r#"SELECT "a\"b", `c\`d` # note"#, Dialect::BigQuery),
            [
                (TokenKind::Word, "SELECT"),
                (TokenKind::String, r#""a\"b""#),
                (TokenKind::Symbol, ","),
                (TokenKind::QuotedIdent, r"`c\`d`"),
                (TokenKind::LineComment, "# note"),
            ]
        );
        assert_eq!(
            kinds(r#""x" `y``z`"#, Dialect::Databricks),
            [
                (TokenKind::String, r#""x""#),
                (TokenKind::QuotedIdent, "`y``z`")
            ]
        );
        assert_eq!(
            kinds(r#""x" 'it''s'"#, Dialect::Ansi),
            [
                (TokenKind::QuotedIdent, r#""x""#),
                (TokenKind::String, "'it''s'")
            ]
        );
    }

    #[test]
    fn oracle_alternative_quotes() {
        assert_eq!(
            kinds("q'[it's]' Nq'{a}b}' q'!x!' quota a#b", Dialect::Oracle),
            [
                (TokenKind::String, "q'[it's]'"),
                (TokenKind::String, "Nq'{a}b}'"),
                (TokenKind::String, "q'!x!'"),
                (TokenKind::Word, "quota"),
                (TokenKind::Word, "a#b"),
            ]
        );
        assert_eq!(kinds("q'[open", Dialect::Oracle).len(), 1);
    }
}
