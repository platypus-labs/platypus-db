//! Long literals as LOB binds. Oracle takes at most 4,000 bytes in a string literal of SQL
//! (ORA-01704), so a longer text, or `HEXTORAW` of more than 2,000 bytes, which the data
//! editor, imports and exported scripts write for CLOB and BLOB values, cannot run as
//! written: such literals of a DML statement or query are sent as CLOB and BLOB binds.

use std::borrow::Cow;

use platypus_sql::{Dialect, Lexer, Token, TokenKind};

/// Bytes of the longest string literal Oracle takes in SQL.
const MAX_LITERAL_BYTES: usize = 4000;

/// A value bound in place of a literal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Lob {
    Clob(String),
    Blob(Vec<u8>),
}

/// The statements whose literals may be binds: a DDL statement takes no binds, and PL/SQL
/// takes literals of up to 32 KB.
const DML: &[&str] = &["SELECT", "WITH", "INSERT", "UPDATE", "DELETE", "MERGE"];

/// `sql` with its literals too long for Oracle as binds `:platypus_lob_1`, … and their values,
/// in order; `sql` itself when it has none.
#[must_use]
pub fn lob_binds(sql: &str) -> (Cow<'_, str>, Vec<(String, Lob)>) {
    let tokens: Vec<Token> = Lexer::new(sql, Dialect::Oracle)
        .filter(|t| !t.is_trivia())
        .collect();
    let dml = tokens
        .iter()
        .find(|t| t.kind == TokenKind::Word)
        .is_some_and(|t| DML.iter().any(|w| t.text(sql).eq_ignore_ascii_case(w)));
    if !dml || sql.len() <= MAX_LITERAL_BYTES {
        return (Cow::Borrowed(sql), Vec::new());
    }
    let mut out = String::with_capacity(sql.len());
    let mut lobs = Vec::new();
    let mut copied = 0;
    let mut i = 0;
    while i < tokens.len() {
        let Some((start, end, lob, next)) = long_literal(sql, &tokens, i) else {
            i += 1;
            continue;
        };
        let name = format!("platypus_lob_{}", lobs.len() + 1);
        out.push_str(&sql[copied..start]);
        out.push(':');
        out.push_str(&name);
        copied = end;
        lobs.push((name, lob));
        i = next;
    }
    if lobs.is_empty() {
        return (Cow::Borrowed(sql), lobs);
    }
    out.push_str(&sql[copied..]);
    (Cow::Owned(out), lobs)
}

/// The text of a plain string literal (`'it''s'`), or `None` for another kind (`q'[…]'`).
fn string_value(text: &str) -> Option<String> {
    let inner = text.strip_prefix('\'')?.strip_suffix('\'')?;
    Some(inner.replace("''", "'"))
}

fn hex_bytes(hex: &str) -> Option<Vec<u8>> {
    if !hex.len().is_multiple_of(2) {
        return None;
    }
    (0..hex.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(hex.get(i..i + 2)?, 16).ok())
        .collect()
}

/// A literal too long for Oracle at token `i`: its span, its value and the token after it.
/// `HEXTORAW('…')` is a BLOB, a string (`N'…'` too) a CLOB.
fn long_literal(sql: &str, tokens: &[Token], i: usize) -> Option<(usize, usize, Lob, usize)> {
    let t = tokens[i];
    let symbol = |j: usize, s: &str| {
        tokens
            .get(j)
            .is_some_and(|t| t.kind == TokenKind::Symbol && t.text(sql) == s)
    };
    if t.kind == TokenKind::Word && t.text(sql).eq_ignore_ascii_case("HEXTORAW") {
        let literal = tokens.get(i + 2).filter(|l| l.kind == TokenKind::String)?;
        if !symbol(i + 1, "(") || !symbol(i + 3, ")") {
            return None;
        }
        let hex = string_value(literal.text(sql))?;
        if hex.len() <= MAX_LITERAL_BYTES {
            return None;
        }
        return Some((
            t.start,
            tokens[i + 3].end,
            Lob::Blob(hex_bytes(&hex)?),
            i + 4,
        ));
    }
    if t.kind != TokenKind::String || t.end - t.start <= MAX_LITERAL_BYTES {
        return None;
    }
    let text = string_value(t.text(sql))?;
    if text.len() <= MAX_LITERAL_BYTES {
        return None;
    }
    // `N'…'`: the prefix goes with the literal.
    let start = match i.checked_sub(1).map(|p| tokens[p]) {
        Some(p) if p.end == t.start && p.text(sql).eq_ignore_ascii_case("N") => p.start,
        _ => t.start,
    };
    Some((start, t.end, Lob::Clob(text), i + 1))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn long_literals_become_binds() {
        let long = "x".repeat(4999) + "'!";
        let quoted = long.replace('\'', "''");
        let sql = format!("UPDATE T SET C = '{quoted}' WHERE ID = 1");
        let (out, lobs) = lob_binds(&sql);
        assert_eq!(out, "UPDATE T SET C = :platypus_lob_1 WHERE ID = 1");
        assert_eq!(lobs, [("platypus_lob_1".to_owned(), Lob::Clob(long))]);

        let hex = "0a".repeat(2001);
        let sql = format!(
            "INSERT INTO T (B, C) VALUES (HEXTORAW('{hex}'), N'{}')",
            "é".repeat(2001)
        );
        let (out, lobs) = lob_binds(&sql);
        assert_eq!(
            out,
            "INSERT INTO T (B, C) VALUES (:platypus_lob_1, :platypus_lob_2)"
        );
        assert_eq!(lobs[0].1, Lob::Blob(vec![10; 2001]));
        assert_eq!(lobs[1].1, Lob::Clob("é".repeat(2001)));
    }

    #[test]
    fn short_literals_and_ddl_stay() {
        let sql = format!(
            "INSERT INTO T VALUES ('{}', '{}')",
            "a".repeat(4000),
            "b".repeat(10)
        );
        assert!(matches!(lob_binds(&sql), (Cow::Borrowed(_), ref l) if l.is_empty()));
        let ddl = format!("COMMENT ON TABLE T IS '{}'", "a".repeat(5000));
        assert_eq!(lob_binds(&ddl).1, Vec::new());
        let block = format!("BEGIN p('{}'); END;", "a".repeat(5000));
        assert_eq!(lob_binds(&block).1, Vec::new());
    }
}
