//! The text of CQL statements: where a script splits, which keyword a statement starts with,
//! and the quoting of names and strings.
//!
//! CQL is close to SQL, but the splitter cannot be SQL's: `$$ … $$` is a string, `//` starts a
//! comment, and a `BEGIN BATCH … APPLY BATCH` holds the `;` of its own statements.

use std::ops::Range;

/// What a statement is, as far as the driver treats it differently.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Kind {
    /// `USE keyspace`: changes the session's keyspace, which the driver keeps.
    Use(Keyspace),
    /// Anything else: the server decides.
    Other,
}

/// The keyspace a `USE` names.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Keyspace {
    /// The name, unquoted; folded to lower case when it was written bare.
    pub name: String,
}

/// The byte ranges of the statements of `script`, without their `;`. Empty statements and
/// comments are left out.
#[must_use]
pub fn split(script: &str) -> Vec<Range<usize>> {
    let mut out = Vec::new();
    let mut start = 0;
    let mut scanner = Scanner::new(script);
    while let Some(end) = scanner.next_semicolon() {
        push(script, start..end, &mut out);
        start = end + 1;
    }
    push(script, start..script.len(), &mut out);
    out
}

/// Adds `range` trimmed, unless nothing but blanks and comments is in it.
fn push(script: &str, range: Range<usize>, out: &mut Vec<Range<usize>>) {
    let text = &script[range.clone()];
    if !has_code(text) {
        return;
    }
    let lead = text.len() - text.trim_start().len();
    let trail = text.len() - text.trim_end().len();
    out.push(range.start + lead..range.end - trail);
}

/// Whether `text` holds anything besides blanks and comments.
fn has_code(text: &str) -> bool {
    let mut scanner = Scanner::new(text);
    while let Some((_, c)) = scanner.next_code() {
        if !c.is_whitespace() {
            return true;
        }
    }
    false
}

/// Walks a script, skipping strings, quoted names and comments.
struct Scanner<'a> {
    text: &'a str,
    pos: usize,
    /// The last two words seen outside strings and comments, upper case: a statement that
    /// begins a batch ends at `APPLY BATCH` instead of at the first `;`.
    words: [String; 2],
    word: String,
    in_batch: bool,
    first_word: bool,
}

impl<'a> Scanner<'a> {
    fn new(text: &'a str) -> Self {
        Self {
            text,
            pos: 0,
            words: [String::new(), String::new()],
            word: String::new(),
            in_batch: false,
            first_word: true,
        }
    }

    fn rest(&self) -> &'a str {
        &self.text[self.pos..]
    }

    /// Moves past a string, a quoted name or a comment that starts here; whether one did.
    fn skip_quoted(&mut self) -> bool {
        let rest = self.rest();
        let len = if rest.starts_with("--") || rest.starts_with("//") {
            rest.find('\n').unwrap_or(rest.len())
        } else if let Some(inner) = rest.strip_prefix("/*") {
            inner.find("*/").map_or(rest.len(), |i| i + 4)
        } else if let Some(inner) = rest.strip_prefix("$$") {
            inner.find("$$").map_or(rest.len(), |i| i + 4)
        } else if let Some(quote) = rest.chars().next().filter(|c| matches!(c, '\'' | '"')) {
            // A quote doubled inside is a quote of the text.
            let bytes = rest.as_bytes();
            let mut i = 1;
            while i < bytes.len() {
                if bytes[i] == quote as u8 {
                    if bytes.get(i + 1) == Some(&(quote as u8)) {
                        i += 2;
                        continue;
                    }
                    i += 1;
                    break;
                }
                i += 1;
            }
            i
        } else {
            return false;
        };
        self.flush_word();
        self.pos += len;
        true
    }

    /// The next character that is not in a string or comment, with its offset.
    fn next_code(&mut self) -> Option<(usize, char)> {
        loop {
            if self.pos >= self.text.len() {
                return None;
            }
            if self.skip_quoted() {
                continue;
            }
            let c = self.rest().chars().next()?;
            let at = self.pos;
            self.pos += c.len_utf8();
            if c.is_alphanumeric() || c == '_' {
                self.word.extend(c.to_uppercase());
            } else {
                self.flush_word();
            }
            return Some((at, c));
        }
    }

    fn flush_word(&mut self) {
        if self.word.is_empty() {
            return;
        }
        let word = std::mem::take(&mut self.word);
        if self.first_word {
            self.in_batch = word == "BEGIN";
            self.first_word = false;
        }
        if self.in_batch && self.words[1] == "APPLY" && word == "BATCH" {
            self.in_batch = false;
        }
        self.words = [std::mem::take(&mut self.words[1]), word];
    }

    /// The offset of the `;` that ends the current statement.
    fn next_semicolon(&mut self) -> Option<usize> {
        while let Some((at, c)) = self.next_code() {
            if c == ';' && !self.in_batch {
                self.first_word = true;
                self.words = [String::new(), String::new()];
                return Some(at);
            }
        }
        None
    }
}

/// The first word of `statement` in upper case, comments skipped.
#[must_use]
pub fn first_word(statement: &str) -> String {
    let mut scanner = Scanner::new(statement);
    let mut word = String::new();
    while let Some((_, c)) = scanner.next_code() {
        if c.is_alphanumeric() || c == '_' {
            word.extend(c.to_uppercase());
        } else if !word.is_empty() {
            break;
        }
    }
    word
}

/// `text` rewritten so that the standard SQL lexer reads it as CQL does, with every byte at the
/// same offset: `//` comments become `--` comments and a `$$ … $$` string becomes a `'…'` string
/// (quotes inside it are blanked out so they cannot end it). Without this, a `//` comment would
/// hide the statement after it from the safety analysis and a `?` in a comment or in a function
/// body would be taken for a parameter.
#[must_use]
pub fn for_analysis(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while !rest.is_empty() {
        if let Some(comment) = rest.strip_prefix("//") {
            let len = comment.find('\n').unwrap_or(comment.len());
            out.push_str("--");
            out.push_str(&comment[..len]);
            rest = &comment[len..];
        } else if let Some(body) = rest.strip_prefix("$$") {
            // The delimiters become a space and a quote so the length does not change.
            let (inner, after) = body
                .find("$$")
                .map_or((body, ""), |i| (&body[..i], &body[i + 2..]));
            out.push_str(" '");
            out.push_str(&inner.replace('\'', "_"));
            if after.len() + inner.len() < body.len() {
                out.push_str("' ");
            }
            rest = after;
        } else {
            // Strings, quoted names and the other comments are copied as they are.
            let mut scanner = Scanner::new(rest);
            let len = if scanner.skip_quoted() {
                scanner.pos
            } else {
                rest.chars().next().map_or(0, char::len_utf8)
            };
            out.push_str(&rest[..len]);
            rest = &rest[len..];
        }
    }
    out
}

/// What the driver does with `statement` by itself.
#[must_use]
pub fn kind(statement: &str) -> Kind {
    if first_word(statement) == "USE"
        && let Some(keyspace) = use_keyspace(statement)
    {
        return Kind::Use(keyspace);
    }
    Kind::Other
}

/// `text` without the blanks and comments (`--`, `//`, `/* */`) at its start.
fn skip_blank(mut text: &str) -> &str {
    loop {
        text = text.trim_start();
        if text.starts_with("--") || text.starts_with("//") {
            text = &text[text.find('\n').unwrap_or(text.len())..];
        } else if let Some(inner) = text.strip_prefix("/*") {
            text = inner.find("*/").map_or("", |i| &inner[i + 2..]);
        } else {
            return text;
        }
    }
}

/// The keyspace of `USE name` or `USE "Name"`, with comments allowed around the name.
fn use_keyspace(statement: &str) -> Option<Keyspace> {
    let rest = skip_blank(statement).get(3..)?;
    let rest = skip_blank(rest);
    let (name, rest) = if let Some(quoted) = rest.strip_prefix('"') {
        // A quote doubled inside is a quote of the name.
        let mut end = None;
        let mut chars = quoted.char_indices().peekable();
        while let Some((i, c)) = chars.next() {
            if c == '"' && chars.next_if(|&(_, n)| n == '"').is_none() {
                end = Some(i);
                break;
            }
        }
        let end = end?;
        (quoted[..end].replace("\"\"", "\""), &quoted[end + 1..])
    } else {
        let len = rest
            .find(|c: char| !(c.is_alphanumeric() || c == '_'))
            .unwrap_or(rest.len());
        (rest[..len].to_lowercase(), &rest[len..])
    };
    let rest = skip_blank(rest);
    let rest = skip_blank(rest.strip_prefix(';').unwrap_or(rest));
    (!name.is_empty() && rest.is_empty()).then_some(Keyspace { name })
}

/// A name that is safe to write bare in CQL: lower-case letters, digits and `_`, starting with
/// a letter, and not a word the grammar reserves.
fn is_bare(name: &str) -> bool {
    let mut chars = name.chars();
    chars.next().is_some_and(|c| c.is_ascii_lowercase())
        && name
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
        && !RESERVED.contains(&name)
}

/// The reserved words of CQL (they cannot name a column or table bare).
const RESERVED: &[&str] = &[
    "add",
    "allow",
    "alter",
    "and",
    "apply",
    "asc",
    "authorize",
    "batch",
    "begin",
    "by",
    "columnfamily",
    "create",
    "delete",
    "desc",
    "describe",
    "drop",
    "entries",
    "execute",
    "from",
    "full",
    "grant",
    "if",
    "in",
    "index",
    "infinity",
    "insert",
    "into",
    "is",
    "keyspace",
    "limit",
    "materialized",
    "modify",
    "nan",
    "norecursive",
    "not",
    "null",
    "of",
    "on",
    "or",
    "order",
    "primary",
    "rename",
    "replace",
    "revoke",
    "schema",
    "select",
    "set",
    "table",
    "to",
    "token",
    "truncate",
    "unlogged",
    "update",
    "use",
    "using",
    "view",
    "where",
    "with",
];

/// `name` as CQL reads it: bare when it can be, otherwise between double quotes.
#[must_use]
pub fn quote_ident(name: &str) -> String {
    if is_bare(name) {
        name.to_owned()
    } else {
        format!("\"{}\"", name.replace('"', "\"\""))
    }
}

/// `keyspace.table`, each part quoted as needed.
#[must_use]
pub fn qualified(keyspace: &str, name: &str) -> String {
    format!("{}.{}", quote_ident(keyspace), quote_ident(name))
}

/// `text` as a string literal: single quotes, doubled inside.
#[must_use]
pub fn quote_string(text: &str) -> String {
    format!("'{}'", text.replace('\'', "''"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parts(script: &str) -> Vec<&str> {
        split(script).into_iter().map(|r| &script[r]).collect()
    }

    #[test]
    fn splits_on_semicolons_outside_text() {
        assert_eq!(
            parts("SELECT 'a;b' FROM t; -- x; y\nDROP \"a;b\"; /* ; */ SELECT 1"),
            [
                "SELECT 'a;b' FROM t",
                "-- x; y\nDROP \"a;b\"",
                "/* ; */ SELECT 1"
            ]
        );
        assert_eq!(parts("  ;; // only a comment;\n ;"), Vec::<&str>::new());
        assert_eq!(
            parts(
                "CREATE FUNCTION f() RETURNS NULL ON NULL INPUT RETURNS int LANGUAGE java AS $$ return 1; $$; SELECT 2"
            ),
            [
                "CREATE FUNCTION f() RETURNS NULL ON NULL INPUT RETURNS int LANGUAGE java AS $$ return 1; $$",
                "SELECT 2"
            ]
        );
        assert_eq!(
            parts("SELECT 'it''s;'; SELECT 2"),
            ["SELECT 'it''s;'", "SELECT 2"]
        );
    }

    #[test]
    fn a_batch_is_one_statement() {
        let script = "BEGIN BATCH INSERT INTO a (k) VALUES (1); INSERT INTO a (k) VALUES (2); APPLY BATCH; SELECT 1;";
        assert_eq!(
            parts(script),
            [
                "BEGIN BATCH INSERT INTO a (k) VALUES (1); INSERT INTO a (k) VALUES (2); APPLY BATCH",
                "SELECT 1"
            ]
        );
    }

    #[test]
    fn finds_the_first_word_and_use() {
        assert_eq!(first_word("  -- x\n select * from t"), "SELECT");
        assert_eq!(
            kind("USE shop"),
            Kind::Use(Keyspace {
                name: "shop".into()
            })
        );
        assert_eq!(
            kind("use \"Shop\";"),
            Kind::Use(Keyspace {
                name: "Shop".into()
            })
        );
        assert_eq!(kind("USE a b"), Kind::Other);
        let shop = Kind::Use(Keyspace {
            name: "shop".into(),
        });
        assert_eq!(kind("-- switch\nUSE shop"), shop);
        assert_eq!(kind("USE shop -- go"), shop);
        assert_eq!(kind("USE /*x*/ shop /*y*/ ; // z"), shop);
        assert_eq!(
            kind("USE /*x*/ \"Sh\"\"op\""),
            Kind::Use(Keyspace {
                name: "Sh\"op".into()
            })
        );
        assert_eq!(kind("USE \"Shop"), Kind::Other);
        assert_eq!(kind("USE"), Kind::Other);
        assert_eq!(kind("USE shop; x"), Kind::Other);
        assert_eq!(kind("SELECT 1"), Kind::Other);
    }

    #[test]
    fn analysis_text_reads_cql_comments_and_dollar_strings() {
        let text = "// x\nTRUNCATE t; SELECT 'a//b', \"c\" -- k\n /* // */ $$ it's ? $$ ?";
        let out = for_analysis(text);
        assert_eq!(out.len(), text.len());
        assert!(out.starts_with("-- x\nTRUNCATE t;"));
        assert!(out.contains("'a//b'") && out.contains("/* // */"));
        assert!(out.ends_with("  ' it_s ? '  ?"));
        // Unterminated strings stay the same length.
        assert_eq!(for_analysis("a $$ b").len(), 6);
        assert_eq!(for_analysis("é // ü").len(), "é // ü".len());
    }

    #[test]
    fn quotes_names_that_need_it() {
        assert_eq!(quote_ident("orders"), "orders");
        assert_eq!(quote_ident("Orders"), "\"Orders\"");
        assert_eq!(quote_ident("select"), "\"select\"");
        assert_eq!(quote_ident("a\"b"), "\"a\"\"b\"");
        assert_eq!(qualified("shop", "Order Items"), "shop.\"Order Items\"");
        assert_eq!(quote_string("it's"), "'it''s'");
    }
}
