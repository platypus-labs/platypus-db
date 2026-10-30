//! Sensitive columns in SQL text: the values a statement compares with, sets or inserts into a
//! sensitive column (an email, a tax code) masked, so a script or a statement of the history can
//! be shown to an agent without the personal data it carries.
//!
//! The columns are told by name: [`NameRules`] (the patterns of the app's settings, `*` for
//! any characters, case ignored) or any test the caller gives. The values are found where the
//! text ties them to a column: `col = 'x'` (any comparison, `LIKE`, either side), `col IN
//! ('x', 'y')`, `SET col = 'x'` and the positions of `INSERT INTO t (cols) VALUES (…)`.

use crate::{Dialect, Lexer, Token, TokenKind};

/// What a masked value is written as: a string literal, the same for every value.
pub const MASKED_LITERAL: &str = "'••••••'";

/// The default names of sensitive columns, as the app's settings have them.
pub const DEFAULT_NAMES: &str = "*password*, *passwd*, *secret*, *token*, *api_key*, *email*, *e_mail*, *phone*, *mobile*, *ssn*, *iban*, *card_number*, *cvv*, *tax_code*, *fiscal*, *salary*, *birth*";

/// Patterns of column names: a comma-separated list, `*` for any characters, case ignored.
#[derive(Debug, Clone, Default)]
pub struct NameRules {
    /// Each pattern as its literal pieces between the `*`, and whether it starts or ends with one.
    patterns: Vec<(Vec<String>, bool, bool)>,
}

impl NameRules {
    #[must_use]
    pub fn new(list: &str) -> Self {
        let patterns = list
            .split(',')
            .map(|p| p.trim().to_lowercase())
            .filter(|p| !p.is_empty())
            .map(|p| {
                let pieces = p
                    .split('*')
                    .filter(|s| !s.is_empty())
                    .map(str::to_owned)
                    .collect();
                (pieces, p.starts_with('*'), p.ends_with('*'))
            })
            .collect();
        Self { patterns }
    }

    /// Whether `name` is one of the patterns.
    #[must_use]
    pub fn matches(&self, name: &str) -> bool {
        let name = name.to_lowercase();
        self.patterns
            .iter()
            .any(|(pieces, open_start, open_end)| glob(&name, pieces, *open_start, *open_end))
    }
}

/// Whether `name` is `pieces` joined by any characters, with any before (`open_start`) and
/// after (`open_end`).
fn glob(name: &str, pieces: &[String], open_start: bool, open_end: bool) -> bool {
    let Some((first, rest)) = pieces.split_first() else {
        // `*` alone matches all; an empty pattern nothing.
        return open_start || open_end;
    };
    let mut at = if open_start {
        match name.find(first.as_str()) {
            Some(i) => i + first.len(),
            None => return false,
        }
    } else if name.starts_with(first.as_str()) {
        first.len()
    } else {
        return false;
    };
    for (k, piece) in rest.iter().enumerate() {
        // The last piece of a pattern that does not end with `*` must end the name.
        let last = k + 1 == rest.len() && !open_end;
        let found = if last {
            name[at..].rfind(piece.as_str())
        } else {
            name[at..].find(piece.as_str())
        };
        match found {
            Some(i) => at += i + piece.len(),
            None => return false,
        }
    }
    open_end || at == name.len()
}

/// A token of the text that is not whitespace or a comment.
struct Sig<'a> {
    token: Token,
    text: &'a str,
    /// A number: the lexer reads each digit as a symbol, they are joined here.
    number: bool,
}

impl Sig<'_> {
    fn is(&self, word: &str) -> bool {
        self.token.kind == TokenKind::Word && self.text.eq_ignore_ascii_case(word)
    }
    fn symbol(&self, s: &str) -> bool {
        self.token.kind == TokenKind::Symbol && self.text == s
    }
    /// A character of a comparison operator: `=`, `<`, `>` or `!`.
    fn is_comparison(&self) -> bool {
        ["=", "<", ">", "!"].iter().any(|s| self.symbol(s))
    }
    /// A string or a number.
    fn is_literal(&self) -> bool {
        self.number || self.token.kind == TokenKind::String
    }
    /// The name of a column it may be: a word or a quoted name.
    fn name(&self) -> Option<String> {
        match self.token.kind {
            TokenKind::Word => Some(self.text.to_owned()),
            TokenKind::QuotedIdent => crate::lexer::unquote_ident(self.text),
            _ => None,
        }
    }
}

/// Whether `sig[i]` ends a comparison operator, and where the operator starts.
fn operator_before(sig: &[Sig<'_>], i: usize) -> Option<usize> {
    let mut j = i;
    while j > 0 && sig[j - 1].is_comparison() {
        j -= 1;
    }
    if j < i {
        return Some(j);
    }
    // `LIKE`, `ILIKE`, `NOT LIKE`.
    if j > 0 && (sig[j - 1].is("like") || sig[j - 1].is("ilike")) {
        j -= 1;
        if j > 0 && sig[j - 1].is("not") {
            j -= 1;
        }
        return Some(j);
    }
    None
}

/// The column `sig[i]` (a literal) is compared with or set to, when the text says.
fn column_of(sig: &[Sig<'_>], i: usize) -> Option<String> {
    // `col <op> literal`.
    if let Some(op) = operator_before(sig, i)
        && op > 0
        && let Some(name) = sig[op - 1].name()
    {
        return Some(name);
    }
    // `literal <op> col`.
    let mut j = i + 1;
    while j < sig.len() && sig[j].is_comparison() {
        j += 1;
    }
    if j > i + 1 && j < sig.len() {
        // `t.col` names the column last.
        while sig.get(j + 1).is_some_and(|s| s.symbol(".")) && j + 2 < sig.len() {
            j += 2;
        }
        // A function's result is no column.
        if !sig.get(j + 1).is_some_and(|s| s.symbol("("))
            && let Some(name) = sig[j].name()
        {
            return Some(name);
        }
    }
    // `col [NOT] IN (literal, literal…)`.
    let mut j = i;
    while j > 0 && (sig[j - 1].symbol(",") || sig[j - 1].is_literal()) {
        j -= 1;
    }
    if j > 1 && sig[j - 1].symbol("(") && sig[j - 2].is("in") {
        let at = if j > 2 && sig[j - 3].is("not") {
            j - 3
        } else {
            j - 2
        };
        return at.checked_sub(1).and_then(|k| sig[k].name());
    }
    None
}

/// For each literal of `INSERT INTO t (c1, c2…) VALUES (v1, v2…), (…)`, the column of its
/// position, by token index.
fn inserted(sig: &[Sig<'_>]) -> Vec<(usize, String)> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < sig.len() {
        if !sig[i].is("insert") {
            i += 1;
            continue;
        }
        // The column list: the first parenthesis after INSERT, before VALUES.
        let Some(open) = (i..sig.len())
            .take_while(|&k| !sig[k].is("values"))
            .find(|&k| sig[k].symbol("("))
        else {
            i += 1;
            continue;
        };
        let mut columns = Vec::new();
        let mut k = open + 1;
        while k < sig.len() && !sig[k].symbol(")") {
            if let Some(name) = sig[k].name() {
                // `schema.t.col` is not in a column list; the last name before a comma is.
                if sig
                    .get(k + 1)
                    .is_some_and(|s| s.symbol(",") || s.symbol(")"))
                {
                    columns.push(name);
                }
            }
            k += 1;
        }
        if !sig.get(k + 1).is_some_and(|s| s.is("values")) {
            i = k + 1;
            continue;
        }
        // Each tuple: a position for each top-level comma.
        k += 2;
        while k < sig.len() && sig[k].symbol("(") {
            let (mut depth, mut position) = (0usize, 0usize);
            k += 1;
            while k < sig.len() {
                let s = &sig[k];
                if s.symbol("(") {
                    depth += 1;
                } else if s.symbol(")") {
                    if depth == 0 {
                        break;
                    }
                    depth -= 1;
                } else if s.symbol(",") && depth == 0 {
                    position += 1;
                } else if s.is_literal()
                    && let Some(column) = columns.get(position)
                {
                    out.push((k, column.clone()));
                }
                k += 1;
            }
            k += 1;
            if !sig.get(k).is_some_and(|s| s.symbol(",")) {
                break;
            }
            k += 1;
        }
        i = k;
    }
    out
}

/// The tokens of `sql` but whitespace and comments, the digits of a number joined.
fn significant(sql: &str, dialect: Dialect) -> Vec<Sig<'_>> {
    let mut out: Vec<Sig<'_>> = Vec::new();
    for token in Lexer::new(sql, dialect).filter(|t| !t.is_trivia()) {
        let text = token.text(sql);
        let digit =
            token.kind == TokenKind::Symbol && text.starts_with(|c: char| c.is_ascii_digit());
        if let Some(last) = out.last_mut()
            && last.number
            && last.token.end == token.start
            && (digit || text == ".")
        {
            last.token.end = token.end;
            last.text = &sql[last.token.start..token.end];
            continue;
        }
        out.push(Sig {
            token,
            text,
            number: digit,
        });
    }
    out
}

/// `sql` with the values tied to a column for which `sensitive` holds written as
/// [`MASKED_LITERAL`].
#[must_use]
pub fn mask_literals(sql: &str, dialect: Dialect, sensitive: &dyn Fn(&str) -> bool) -> String {
    let sig = significant(sql, dialect);
    let mut masked: Vec<usize> = inserted(&sig)
        .into_iter()
        .filter(|(_, column)| sensitive(column))
        .map(|(i, _)| i)
        .collect();
    masked.extend(
        (0..sig.len())
            .filter(|&i| sig[i].is_literal() && column_of(&sig, i).is_some_and(|c| sensitive(&c))),
    );
    masked.sort_unstable();
    masked.dedup();
    let mut out = String::with_capacity(sql.len());
    let mut at = 0;
    for i in masked {
        let Token { start, end, .. } = sig[i].token;
        out.push_str(&sql[at..start]);
        out.push_str(MASKED_LITERAL);
        at = end;
    }
    out.push_str(&sql[at..]);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mask(sql: &str) -> String {
        let rules = NameRules::new(DEFAULT_NAMES);
        mask_literals(sql, Dialect::Postgres, &|c| rules.matches(c))
    }

    #[test]
    fn rules_match_names_as_the_app_does() {
        let rules = NameRules::new(DEFAULT_NAMES);
        for name in [
            "email",
            "customer_email",
            "Password_Hash",
            "api_key",
            "PHONE",
            "iban",
            "date_of_birth",
        ] {
            assert!(rules.matches(name), "{name}");
        }
        for name in ["id", "name", "created_at", "amount"] {
            assert!(!rules.matches(name), "{name}");
        }
        let exact = NameRules::new("ssn, card*, *code");
        assert!(exact.matches("SSN") && !exact.matches("ssn2"));
        assert!(exact.matches("card_number") && !exact.matches("my_card"));
        assert!(exact.matches("tax_code") && !exact.matches("code_x"));
        assert!(
            NameRules::new("a*b*c").matches("aXbYc") && !NameRules::new("a*b*c").matches("aXcYb")
        );
        assert!(!NameRules::new("").matches("anything"));
    }

    #[test]
    fn masks_values_compared_with_a_sensitive_column() {
        assert_eq!(
            mask("SELECT id FROM users WHERE email = 'giulia@example.com' AND name = 'Giulia'"),
            "SELECT id FROM users WHERE email = '••••••' AND name = 'Giulia'"
        );
        assert_eq!(
            mask("WHERE u.phone<>'+39 333' OR 'x@y' = u.email"),
            "WHERE u.phone<>'••••••' OR '••••••' = u.email"
        );
        assert_eq!(
            mask("WHERE email NOT LIKE '%@corp.it'"),
            "WHERE email NOT LIKE '••••••'"
        );
        assert_eq!(
            mask("WHERE \"Email\" IN ('a@b', 'c@d') AND id IN (1, 2)"),
            "WHERE \"Email\" IN ('••••••', '••••••') AND id IN (1, 2)"
        );
        assert_eq!(mask("WHERE salary > 50000"), "WHERE salary > '••••••'");
    }

    #[test]
    fn masks_values_set_or_inserted() {
        assert_eq!(
            mask("UPDATE users SET email = 'new@x.it', name = 'N' WHERE id = 3"),
            "UPDATE users SET email = '••••••', name = 'N' WHERE id = 3"
        );
        assert_eq!(
            mask(
                "INSERT INTO users (id, email, phone) VALUES (1, 'a@b', lower('X')), (2, 'c@d', NULL)"
            ),
            "INSERT INTO users (id, email, phone) VALUES (1, '••••••', lower('••••••')), (2, '••••••', NULL)"
        );
    }

    #[test]
    fn leaves_what_is_not_tied_to_a_sensitive_column() {
        let sql = "-- email = 'a@b'\nSELECT 'email' AS label, count(*) FROM users WHERE id = 1";
        assert_eq!(mask(sql), sql);
        // A function's result is no column.
        assert_eq!(mask("WHERE lower(name) = 'x'"), "WHERE lower(name) = 'x'");
        assert_eq!(
            mask("INSERT INTO t VALUES ('a@b')"),
            "INSERT INTO t VALUES ('a@b')"
        );
    }

    #[test]
    fn default_names_are_the_apps() {
        let ts = std::fs::read_to_string(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../apps/desktop/src/guards/maskRules.ts"),
        )
        .unwrap();
        assert!(
            ts.contains(&format!(
                "export const DEFAULT_NAMES = \"{DEFAULT_NAMES}\";"
            )),
            "DEFAULT_NAMES differs from apps/desktop/src/guards/maskRules.ts"
        );
    }
}
