//! The names a quoted identifier stands for once the server decodes it: PostgreSQL's
//! `U&"…"` with its Unicode escapes (and `UESCAPE`), the backslash escapes ClickHouse and
//! BigQuery read in quoted names. The read-only check looks at each of them, so a name
//! cannot hide behind an encoding.

use platypus_sql::{Dialect, Token, TokenKind};

/// A quoted identifier among `tokens` (without trivia) at `i`: the names it may stand for
/// and the index of its last token (after a `UESCAPE 'c'` clause).
pub(crate) struct Decoded {
    pub names: Vec<String>,
    pub last: usize,
    /// It is encoded in a way the server rejects, or this decoding does not know.
    pub undecodable: bool,
}

/// What the quoted identifier `tokens[i]` of `src` stands for in `dialect`: the name with
/// its quotes removed, then the decoded name if it is encoded. An encoding that does not
/// decode is marked [`Decoded::undecodable`], for the caller to refuse rather than guess.
pub(crate) fn decode(src: &str, tokens: &[Token], i: usize, dialect: Dialect) -> Decoded {
    let text = tokens[i].text(src);
    let inner = inner(text);
    let mut names = vec![undouble(inner, text)];
    let mut last = i;
    let mut undecodable = false;
    if dialect == Dialect::Postgres && unicode_prefix(src, tokens, i) {
        let (escape, uescape_last) = uescape(src, tokens, i);
        last = uescape_last;
        match escape.and_then(|c| unicode_unescape(&names[0], c)) {
            Some(decoded) => names.push(decoded),
            None => undecodable = true,
        }
    } else if matches!(dialect, Dialect::ClickHouse | Dialect::BigQuery) && inner.contains('\\') {
        names.push(backslash_unescape(inner));
    }
    Decoded {
        names,
        last,
        undecodable,
    }
}

/// The text between the quotes.
fn inner(text: &str) -> &str {
    let mut chars = text.chars();
    chars.next();
    chars.next_back();
    chars.as_str()
}

/// `inner` with doubled closing quotes undone.
fn undouble(inner: &str, text: &str) -> String {
    let close = match text.chars().next() {
        Some('[') => ']',
        Some(c) => c,
        None => return String::new(),
    };
    inner.replace(&format!("{close}{close}"), &close.to_string())
}

/// Whether `tokens[i]` follows `U&` (any case), as `U&"…"`.
fn unicode_prefix(src: &str, tokens: &[Token], i: usize) -> bool {
    i >= 2
        && tokens[i - 1].kind == TokenKind::Symbol
        && tokens[i - 1].text(src) == "&"
        && tokens[i - 2].kind == TokenKind::Word
        && tokens[i - 2].text(src).eq_ignore_ascii_case("u")
}

/// The escape character of the `U&` identifier at `i` (`\` unless a `UESCAPE 'c'` follows,
/// `None` when that clause names no valid one) and the index of the clause's last token.
fn uescape(src: &str, tokens: &[Token], i: usize) -> (Option<char>, usize) {
    let clause = tokens
        .get(i + 1)
        .is_some_and(|t| t.kind == TokenKind::Word && t.text(src).eq_ignore_ascii_case("uescape"));
    if !clause {
        return (Some('\\'), i);
    }
    let Some(literal) = tokens.get(i + 2).filter(|t| t.kind == TokenKind::String) else {
        return (None, i + 1);
    };
    let text = literal.text(src);
    let escape = text
        .strip_prefix('\'')
        .and_then(|t| t.strip_suffix('\''))
        .and_then(|body| {
            let mut chars = body.chars();
            match (chars.next(), chars.next()) {
                (Some(c), None)
                    if !c.is_ascii_hexdigit()
                        && !matches!(c, '+' | '\'' | '"')
                        && !c.is_whitespace() =>
                {
                    Some(c)
                }
                _ => None,
            }
        });
    (escape, i + 2)
}

/// PostgreSQL's decoding of a `U&` name: `<e>XXXX`, `<e>+XXXXXX`, `<e><e>` for the escape
/// itself, surrogate pairs joined. `None` where PostgreSQL reports an error.
fn unicode_unescape(name: &str, escape: char) -> Option<String> {
    let chars: Vec<char> = name.chars().collect();
    let mut out = String::with_capacity(name.len());
    let mut pending_high: Option<u32> = None;
    let mut i = 0;
    while i < chars.len() {
        if chars[i] != escape {
            if pending_high.is_some() {
                return None;
            }
            out.push(chars[i]);
            i += 1;
            continue;
        }
        if chars.get(i + 1) == Some(&escape) {
            if pending_high.is_some() {
                return None;
            }
            out.push(escape);
            i += 2;
            continue;
        }
        let (start, len) = if chars.get(i + 1) == Some(&'+') {
            (i + 2, 6)
        } else {
            (i + 1, 4)
        };
        let digits: String = chars.get(start..start + len)?.iter().collect();
        if !digits.chars().all(|c| c.is_ascii_hexdigit()) {
            return None;
        }
        let code = u32::from_str_radix(&digits, 16).ok()?;
        i = start + len;
        match (pending_high.take(), code) {
            (None, 0xD800..=0xDBFF) => pending_high = Some(code),
            (Some(high), 0xDC00..=0xDFFF) => {
                out.push(char::from_u32(
                    0x10000 + ((high - 0xD800) << 10) + (code - 0xDC00),
                )?);
            }
            (Some(_), _) | (None, 0xDC00..=0xDFFF | 0) => return None,
            (None, code) => out.push(char::from_u32(code)?),
        }
    }
    pending_high.is_none().then_some(out)
}

/// A quoted name with C-style backslash escapes decoded: `\xHH`, `\uXXXX`, `\UXXXXXXXX`,
/// octal `\ooo`, the letter escapes, and any other character standing for itself. It
/// decodes more than either server does, which only adds names to check.
fn backslash_unescape(inner: &str) -> String {
    let mut out = String::with_capacity(inner.len());
    let mut chars = inner.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        let Some(e) = chars.next() else { break };
        let hex = |chars: &mut std::iter::Peekable<std::str::Chars>, n: usize| {
            let digits: String = (0..n)
                .map_while(|_| chars.next_if(char::is_ascii_hexdigit))
                .collect();
            u32::from_str_radix(&digits, 16)
                .ok()
                .and_then(char::from_u32)
        };
        let decoded = match e {
            'x' | 'X' => hex(&mut chars, 2),
            'u' => hex(&mut chars, 4),
            'U' => hex(&mut chars, 8),
            '0'..='7' => {
                let mut code = e.to_digit(8).unwrap_or(0);
                for _ in 0..2 {
                    match chars.next_if(|c| ('0'..='7').contains(c)) {
                        Some(d) => code = code * 8 + d.to_digit(8).unwrap_or(0),
                        None => break,
                    }
                }
                char::from_u32(code)
            }
            'n' => Some('\n'),
            't' => Some('\t'),
            'r' => Some('\r'),
            'a' => Some('\x07'),
            'b' => Some('\x08'),
            'f' => Some('\x0c'),
            'v' => Some('\x0b'),
            other => Some(other),
        };
        if let Some(d) = decoded {
            out.push(d);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use platypus_sql::Lexer;

    fn decoded(sql: &str, dialect: Dialect) -> Decoded {
        let tokens: Vec<Token> = Lexer::new(sql, dialect)
            .filter(|t| !t.is_trivia())
            .collect();
        let i = tokens
            .iter()
            .position(|t| t.kind == TokenKind::QuotedIdent)
            .unwrap();
        decode(sql, &tokens, i, dialect)
    }

    fn names(sql: &str, dialect: Dialect) -> Vec<String> {
        decoded(sql, dialect).names
    }

    #[test]
    fn decodes_postgres_unicode_identifiers() {
        let pg = Dialect::Postgres;
        assert_eq!(names(r#"select U&"\0070g_x""#, pg)[1], "pg_x");
        assert_eq!(names(r#"select u&"\+000070g_x""#, pg)[1], "pg_x");
        assert_eq!(names(r#"select U&"!0070g_x" UESCAPE '!'"#, pg)[1], "pg_x");
        assert_eq!(
            names(r#"select U&"a\\b""q""#, pg),
            [r#"a\\b"q"#, r#"a\b"q"#]
        );
        assert_eq!(names(r#"select U&"\D83D\DE00""#, pg)[1], "\u{1F600}");
        // What PostgreSQL rejects does not decode.
        for bad in [
            r#"select U&"\00zz""#,
            r#"select U&"\D83D""#,
            r#"select U&"!0070" UESCAPE 'a'"#,
            r#"select U&"!0070" UESCAPE"#,
        ] {
            assert!(decoded(bad, pg).undecodable, "{bad}");
        }
        // Without `U&` the name is plain.
        assert_eq!(names(r#"select "\0070g_x""#, pg), [r"\0070g_x"]);
    }

    #[test]
    fn decodes_backslash_escapes_in_quoted_names() {
        let ch = Dialect::ClickHouse;
        assert_eq!(names(r"select * from `fi\x6ce`('x')", ch)[1], "file");
        assert_eq!(names(r"select * from `\u0075rl`('x')", ch)[1], "url");
        assert_eq!(
            names(r"select * from `\145xternal_query`('x')", Dialect::BigQuery)[1],
            "external_query"
        );
        assert_eq!(names(r"select * from `f\ile`('x')", ch)[1], "file");
    }
}
