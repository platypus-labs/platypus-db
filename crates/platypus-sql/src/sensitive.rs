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

/// What a masked value of a result reads: the same for every value, so that not even its length
/// shows. NULL stays NULL.
pub const MASKED_VALUE: &str = "••••••";

/// The default names of sensitive columns, as the app's settings have them.
pub const DEFAULT_NAMES: &str = "*password*, *passwd*, *secret*, *token*, *api_key*, *email*, *e_mail*, *phone*, *mobile*, *ssn*, *iban*, *card_number*, *cvv*, *tax_code*, *fiscal*, *salary*, *birth*";

/// Patterns of column names: a comma-separated list, `*` for any characters, case ignored.
#[derive(Debug, Clone, Default)]
pub struct NameRules {
    /// The patterns, lower case.
    patterns: Vec<Vec<char>>,
}

impl NameRules {
    #[must_use]
    pub fn new(list: &str) -> Self {
        let patterns = list
            .split(',')
            .map(|p| p.trim().to_lowercase())
            .filter(|p| !p.is_empty())
            .map(|p| p.chars().collect())
            .collect();
        Self { patterns }
    }

    /// Whether there is no pattern: nothing matches.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.patterns.is_empty()
    }

    /// Whether `name` is one of the patterns, as a whole.
    #[must_use]
    pub fn matches(&self, name: &str) -> bool {
        let name: Vec<char> = name.to_lowercase().chars().collect();
        self.patterns.iter().any(|p| wildcard(p, &name))
    }
}

/// Whether `name` matches `pattern`, where `*` is any characters (none included): the usual
/// greedy match, going back to the last `*` when a piece does not fit.
fn wildcard(pattern: &[char], name: &[char]) -> bool {
    let (mut p, mut n) = (0, 0);
    let mut star: Option<(usize, usize)> = None;
    while n < name.len() {
        if p < pattern.len() && pattern[p] == '*' {
            star = Some((p, n));
            p += 1;
        } else if p < pattern.len() && pattern[p] == name[n] {
            p += 1;
            n += 1;
        } else if let Some((sp, sn)) = star {
            p = sp + 1;
            n = sn + 1;
            star = Some((sp, sn + 1));
        } else {
            return false;
        }
    }
    pattern[p..].iter().all(|c| *c == '*')
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

/// The names in the operand that ends at `sig[k]`: a column (`t.col` names it last), a cast
/// of one (`col::text`), or a call (`lower(col)`, `CAST(col AS text)`) and every name in it.
fn operand_before(sig: &[Sig<'_>], mut k: usize) -> Vec<String> {
    // `::type` after the operand.
    while k >= 3 && sig[k].name().is_some() && sig[k - 1].symbol(":") && sig[k - 2].symbol(":") {
        k -= 3;
    }
    if sig[k].symbol(")") {
        let mut depth = 0usize;
        let mut names = Vec::new();
        for j in (0..k).rev() {
            if sig[j].symbol(")") {
                depth += 1;
            } else if sig[j].symbol("(") {
                if depth == 0 {
                    return names;
                }
                depth -= 1;
            } else if let Some(name) = sig[j].name() {
                names.push(name);
            }
        }
        return Vec::new();
    }
    sig[k].name().into_iter().collect()
}

/// The names in the operand that starts at `sig[k]`, as `operand_before` reads them.
fn operand_after(sig: &[Sig<'_>], mut k: usize) -> Vec<String> {
    // `t.col`: the last name of the path.
    while sig.get(k + 1).is_some_and(|s| s.symbol(".")) && k + 2 < sig.len() {
        k += 2;
    }
    if sig.get(k + 1).is_some_and(|s| s.symbol("(")) {
        let mut depth = 0usize;
        let mut names = Vec::new();
        for s in &sig[k + 2..] {
            if s.symbol("(") {
                depth += 1;
            } else if s.symbol(")") {
                if depth == 0 {
                    return names;
                }
                depth -= 1;
            } else if let Some(name) = s.name() {
                names.push(name);
            }
        }
        return Vec::new();
    }
    sig[k].name().into_iter().collect()
}

/// The names of what `sig[i]` (a literal) is compared with or set to, when the text says.
fn column_of(sig: &[Sig<'_>], i: usize) -> Vec<String> {
    // `col <op> literal`.
    if let Some(op) = operator_before(sig, i)
        && op > 0
    {
        return operand_before(sig, op - 1);
    }
    // `literal <op> col`.
    let mut j = i + 1;
    while j < sig.len() && sig[j].is_comparison() {
        j += 1;
    }
    if j > i + 1 && j < sig.len() {
        return operand_after(sig, j);
    }
    // `col BETWEEN literal AND literal`.
    if i > 1 && sig[i - 1].is("between") {
        return operand_before(sig, i - 2);
    }
    if i > 3 && sig[i - 1].is("and") && sig[i - 2].is_literal() && sig[i - 3].is("between") {
        return operand_before(sig, i - 4);
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
        return at
            .checked_sub(1)
            .map(|k| operand_before(sig, k))
            .unwrap_or_default();
    }
    Vec::new()
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
            .take_while(|&k| !sig[k].is("values") && !sig[k].is("select"))
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
        // `INSERT INTO t (cols) SELECT a, 'x', …`: a position for each top-level comma of the
        // select list.
        if sig.get(k + 1).is_some_and(|s| s.is("select")) {
            let (mut depth, mut position) = (0usize, 0usize);
            k += 2;
            while k < sig.len() && !(depth == 0 && (sig[k].is("from") || sig[k].symbol(";"))) {
                let s = &sig[k];
                if s.symbol("(") {
                    depth += 1;
                } else if s.symbol(")") {
                    depth = depth.saturating_sub(1);
                } else if s.symbol(",") && depth == 0 {
                    position += 1;
                } else if s.is_literal()
                    && let Some(column) = columns.get(position)
                {
                    out.push((k, column.clone()));
                }
                k += 1;
            }
            i = k;
            continue;
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

/// The tokens of `sql` but whitespace and comments, the pieces of a number joined: its
/// digits, a sign that cannot be a subtraction, an exponent (`1e-5`) or a hex form (`0x1F`).
fn significant(sql: &str, dialect: Dialect) -> Vec<Sig<'_>> {
    let mut out: Vec<Sig<'_>> = Vec::new();
    for token in Lexer::new(sql, dialect).filter(|t| !t.is_trivia()) {
        let text = token.text(sql);
        let digit =
            token.kind == TokenKind::Symbol && text.starts_with(|c: char| c.is_ascii_digit());
        if let Some(last) = out.last_mut()
            && last.number
            && last.token.end == token.start
            && (digit
                || text == "."
                || token.kind == TokenKind::Word
                || (matches!(text, "+" | "-") && last.text.ends_with(['e', 'E'])))
        {
            last.token.end = token.end;
            last.text = &sql[last.token.start..token.end];
            continue;
        }
        // `= -5`, `(+5`, `, -5`: a sign after an operator or a separator belongs to the number.
        let len = out.len();
        if digit
            && len >= 2
            && matches!(out[len - 1].text, "-" | "+")
            && out[len - 1].token.end == token.start
            && (out[len - 2].is_comparison()
                || out[len - 2].symbol("(")
                || out[len - 2].symbol(",")
                || out[len - 2].token.kind == TokenKind::Word && !out[len - 2].number)
        {
            let sign = &mut out[len - 1];
            sign.token.end = token.end;
            sign.text = &sql[sign.token.start..token.end];
            sign.number = true;
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

/// An item of the outermost select list of a statement.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SelectOutput {
    /// Its expression names a sensitive column.
    pub sensitive: bool,
    /// The name an alias gives it.
    pub alias: Option<String>,
    /// `*` or `t.*`: it yields any number of columns.
    pub star: bool,
}

/// Past the parenthesis that opens at `sig[open]`.
fn after_parens(sig: &[Sig<'_>], open: usize) -> usize {
    let mut depth = 0usize;
    for (k, s) in sig.iter().enumerate().skip(open) {
        if s.symbol("(") {
            depth += 1;
        } else if s.symbol(")") {
            depth = depth.saturating_sub(1);
            if depth == 0 {
                return k + 1;
            }
        }
    }
    sig.len()
}

/// The first token of the select list that starts at `sig[i]`, past `DISTINCT [ON (…)]`, `ALL`
/// and `TOP n`.
fn select_list_start(sig: &[Sig<'_>], mut i: usize) -> usize {
    if sig.get(i).is_some_and(|s| s.is("distinct") || s.is("all")) {
        i += 1;
        if sig.get(i).is_some_and(|s| s.is("on")) && sig.get(i + 1).is_some_and(|s| s.symbol("(")) {
            i = after_parens(sig, i + 1);
        }
    }
    if sig.get(i).is_some_and(|s| s.is("top")) {
        i += 1;
        i = if sig.get(i).is_some_and(|s| s.symbol("(")) {
            after_parens(sig, i)
        } else {
            i + 1
        };
    }
    i
}

/// What the select list item `item` yields.
fn select_output(item: &[Sig<'_>], sensitive: &dyn Fn(&str) -> bool) -> SelectOutput {
    // A name that a `.` or a `(` follows is a table or a function, not a column.
    let touches = item.iter().enumerate().any(|(k, s)| {
        !item
            .get(k + 1)
            .is_some_and(|next| next.symbol(".") || next.symbol("("))
            && s.name().is_some_and(|name| sensitive(&name))
    });
    let last = item.last();
    let before = item.len().checked_sub(2).map(|k| &item[k]);
    let star = last.is_some_and(|s| s.symbol("*")) && before.is_none_or(|s| s.symbol("."));
    let named = before.is_some_and(|s| s.is("as") || s.symbol(")") || s.name().is_some());
    SelectOutput {
        sensitive: touches,
        alias: last.filter(|_| named).and_then(Sig::name),
        star,
    }
}

/// Whether `sql` has more than one `SELECT`.
#[must_use]
pub fn has_subquery(sql: &str, dialect: Dialect) -> bool {
    significant(sql, dialect)
        .iter()
        .filter(|s| s.is("select"))
        .nth(1)
        .is_some()
}

/// The items of the `RETURNING` list of `sql`, else of the select list of its outermost `SELECT`,
/// in order; none when it has neither. A `WITH` before the `SELECT` is passed over, and only the
/// first branch of a `UNION` is read. Whether a name is sensitive is `sensitive`'s to say.
#[must_use]
pub fn select_outputs(
    sql: &str,
    dialect: Dialect,
    sensitive: &dyn Fn(&str) -> bool,
) -> Vec<SelectOutput> {
    const ENDS: [&str; 11] = [
        "from",
        "into",
        "where",
        "union",
        "intersect",
        "except",
        "minus",
        "group",
        "order",
        "having",
        "limit",
    ];
    let sig = significant(sql, dialect);
    let mut depth = 0usize;
    let (mut select, mut returning) = (None, None);
    for (i, s) in sig.iter().enumerate() {
        if s.symbol("(") {
            depth += 1;
        } else if s.symbol(")") {
            depth = depth.saturating_sub(1);
        } else if depth == 0 && s.is("returning") {
            returning = Some(i + 1);
        } else if depth == 0 && select.is_none() && s.is("select") {
            select = Some(i + 1);
        }
    }
    let start = match (returning, select) {
        (Some(list), _) => list,
        (None, Some(list)) => select_list_start(&sig, list),
        (None, None) => return Vec::new(),
    };
    let (mut outputs, mut item, mut depth, mut k) = (Vec::new(), start, 0usize, start);
    while k < sig.len() {
        let s = &sig[k];
        if s.symbol("(") {
            depth += 1;
        } else if s.symbol(")") {
            depth = depth.saturating_sub(1);
        } else if depth == 0 && (s.symbol(";") || ENDS.iter().any(|w| s.is(w))) {
            break;
        } else if depth == 0 && s.symbol(",") {
            outputs.push(select_output(&sig[item..k], sensitive));
            item = k + 1;
        }
        k += 1;
    }
    if item < k {
        outputs.push(select_output(&sig[item..k], sensitive));
    }
    outputs
}

/// `sql` with every string literal written as [`MASKED_LITERAL`]: for a statement of someone
/// else's, where a value may be sensitive with no column naming it (`INSERT INTO t VALUES ('…')`,
/// `f('…')`).
#[must_use]
pub fn mask_strings(sql: &str, dialect: Dialect) -> String {
    let mut out = String::with_capacity(sql.len());
    let mut at = 0;
    for token in Lexer::new(sql, dialect).filter(|t| t.kind == TokenKind::String) {
        out.push_str(&sql[at..token.start]);
        out.push_str(MASKED_LITERAL);
        at = token.end;
    }
    out.push_str(&sql[at..]);
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
            .filter(|&i| sig[i].is_literal() && column_of(&sig, i).iter().any(|c| sensitive(c))),
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

/// A database error with the values it quotes of sensitive columns masked: PostgreSQL's
/// `Key (email)=(a@b) already exists`, MySQL's `Duplicate entry 'a@b' for key 'users.email'`,
/// SQL Server's `… unique index 'IX_users_email'. The duplicate key value is (a@b).`. A key or
/// index counts as sensitive when `sensitive` holds for its name or a part of it.
#[must_use]
pub fn mask_error(text: &str, sensitive: &dyn Fn(&str) -> bool) -> String {
    let named = |name: &str| {
        sensitive(name)
            || name
                .split(['.', '_', ',', ' '])
                .any(|part| !part.is_empty() && sensitive(part))
    };
    let mut out = text.to_owned();
    // PostgreSQL: `(cols)=(values)`.
    let mut from = 0;
    while let Some(at) = out[from..].find(")=(").map(|i| i + from) {
        let open = out[..at].rfind('(').map_or(0, |i| i + 1);
        let columns = &out[open..at];
        let values_start = at + 3;
        let Some(close) = out[values_start..].find(')').map(|i| i + values_start) else {
            break;
        };
        if columns
            .split(',')
            .any(|c| named(c.trim().trim_matches('"')))
        {
            out.replace_range(values_start..close, MASK);
            from = values_start + MASK.len();
        } else {
            from = close;
        }
    }
    // MySQL: `Duplicate entry 'value' for key 'key'`.
    if let Some(entry) = out.find("Duplicate entry '")
        && let Some(end) = out[entry + 17..]
            .find("' for key '")
            .map(|i| i + entry + 17)
    {
        let key = out[end + 11..].split('\'').next().unwrap_or("");
        if named(key) {
            out.replace_range(entry + 17..end, MASK);
        }
    }
    // SQL Server: `… index 'name'. The duplicate key value is (value).`.
    if let Some(at) = out.find("The duplicate key value is (") {
        let index = out[..at].rsplit('\'').nth(1).unwrap_or("");
        let start = at + 28;
        if named(index)
            && let Some(close) = out[start..].rfind(')').map(|i| i + start)
        {
            out.replace_range(start..close, MASK);
        }
    }
    out
}

/// What a masked value of an error is written as.
const MASK: &str = "••••••";

#[cfg(test)]
mod tests {
    use super::*;

    fn outputs(sql: &str) -> Vec<SelectOutput> {
        select_outputs(sql, Dialect::Postgres, &|c| {
            c.eq_ignore_ascii_case("email") || c.eq_ignore_ascii_case("phone")
        })
    }

    fn touching(sql: &str) -> Vec<bool> {
        outputs(sql).iter().map(|o| o.sensitive).collect()
    }

    #[test]
    fn select_outputs_tell_the_items_that_read_a_sensitive_column() {
        assert_eq!(
            touching("SELECT id, email, lower(u.phone) p, 1 FROM users u WHERE email = 'x'"),
            [false, true, true, false]
        );
        assert_eq!(
            touching("SELECT DISTINCT ON (id) id, email AS e FROM t"),
            [false, true]
        );
        assert_eq!(touching("SELECT TOP 3 email FROM t"), [true]);
        assert_eq!(touching("SELECT TOP (3) id FROM t"), [false]);
        assert_eq!(
            touching("WITH c AS (SELECT email FROM t) SELECT id, (SELECT email FROM c) FROM c"),
            [false, true]
        );
        assert_eq!(
            touching("SELECT a, b FROM t UNION SELECT email, 1 FROM u"),
            [false, false]
        );
        // A table or a function of that name is not the column.
        assert_eq!(
            touching("SELECT email.id, phone(x) FROM email"),
            [false, false]
        );
        assert_eq!(touching("SELECT 'email' FROM t"), [false]);
        assert_eq!(
            touching("UPDATE t SET a = 1 RETURNING id, upper(email)"),
            [false, true]
        );
        assert_eq!(
            touching("INSERT INTO t SELECT email FROM u RETURNING id"),
            [false]
        );
        assert!(has_subquery("SELECT (SELECT 1) FROM t", Dialect::Postgres));
        assert!(!has_subquery("SELECT 'select' FROM t", Dialect::Postgres));
        assert_eq!(outputs("UPDATE t SET email = 'x'"), []);
        assert_eq!(outputs("db.users.find({})"), []);
    }

    #[test]
    fn every_string_is_masked_wherever_it_is() {
        assert_eq!(
            mask_strings(
                "INSERT INTO t VALUES (1, 'a@b.c', f('it''s')) -- 'kept'",
                Dialect::Postgres
            ),
            "INSERT INTO t VALUES (1, '••••••', f('••••••')) -- 'kept'"
        );
        assert_eq!(mask_strings("SELECT 1", Dialect::Postgres), "SELECT 1");
        // Cut by the server inside a value: what is left of it goes too.
        assert_eq!(
            mask_strings("SELECT 'a@b", Dialect::Postgres),
            "SELECT '••••••'"
        );
    }

    #[test]
    fn select_outputs_name_aliases_and_stars() {
        let items = outputs("SELECT *, t.*, email AS e, phone p, count(*), lower(email) l FROM t;");
        let aliases: Vec<_> = items.iter().map(|o| o.alias.as_deref()).collect();
        assert_eq!(aliases, [None, None, Some("e"), Some("p"), None, Some("l")]);
        let stars: Vec<_> = items.iter().map(|o| o.star).collect();
        assert_eq!(stars, [true, true, false, false, false, false]);
    }

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
        // The whole name: a piece found earlier does not stop a match at the end.
        assert!(NameRules::new("*_id").matches("parent_id_or_id"));
        assert!(NameRules::new("*secret").matches("secret_secret"));
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
        // Casts, calls, BETWEEN, signs, exponents and hex forms.
        assert_eq!(
            mask("WHERE email::text = 'a@b'"),
            "WHERE email::text = '••••••'"
        );
        assert_eq!(
            mask("WHERE lower(u.email) = 'a@b'"),
            "WHERE lower(u.email) = '••••••'"
        );
        assert_eq!(
            mask("WHERE 'a@b' = CAST(email AS text)"),
            "WHERE '••••••' = CAST(email AS text)"
        );
        assert_eq!(
            mask("WHERE birth BETWEEN '1990-01-01' AND '1990-12-31' AND id BETWEEN 1 AND 2"),
            "WHERE birth BETWEEN '••••••' AND '••••••' AND id BETWEEN 1 AND 2"
        );
        assert_eq!(
            mask("WHERE salary = -50000 OR salary < 1e-5"),
            "WHERE salary = '••••••' OR salary < '••••••'"
        );
        assert_eq!(
            mask("WHERE card_number = 0x1F2A"),
            "WHERE card_number = '••••••'"
        );
        // A subtraction is no sign.
        assert_eq!(mask("WHERE id = 5-1"), "WHERE id = 5-1");
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
        assert_eq!(
            mask("INSERT INTO users (id, email) SELECT 1, 'a@b' FROM dual WHERE x = 'y'"),
            "INSERT INTO users (id, email) SELECT 1, '••••••' FROM dual WHERE x = 'y'"
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

    #[test]
    fn masks_the_values_errors_quote() {
        let rules = NameRules::new(DEFAULT_NAMES);
        let mask = |t: &str| mask_error(t, &|c| rules.matches(c));
        assert_eq!(
            mask(
                "duplicate key value violates unique constraint \"users_email_key\"\nDETAIL: Key (email)=(giulia@x.it) already exists."
            ),
            "duplicate key value violates unique constraint \"users_email_key\"\nDETAIL: Key (email)=(••••••) already exists."
        );
        assert_eq!(
            mask("Key (id)=(7) is not present"),
            "Key (id)=(7) is not present"
        );
        assert_eq!(
            mask("Duplicate entry 'giulia@x.it' for key 'users.email'"),
            "Duplicate entry '••••••' for key 'users.email'"
        );
        assert_eq!(
            mask("Duplicate entry '7' for key 'PRIMARY'"),
            "Duplicate entry '7' for key 'PRIMARY'"
        );
        assert_eq!(
            mask(
                "Cannot insert duplicate key row in object 'dbo.users' with unique index 'IX_users_email'. The duplicate key value is (giulia@x.it)."
            ),
            "Cannot insert duplicate key row in object 'dbo.users' with unique index 'IX_users_email'. The duplicate key value is (••••••)."
        );
    }
}
