//! Rewrites a query for the data viewer without touching the rest of its text: its sort
//! replaces the top-level `ORDER BY`, a quick filter adds a condition to the top-level
//! `WHERE`, and "Count rows" turns it into a `COUNT(*)`. Only the top level counts:
//! parentheses (subqueries, window definitions) are skipped, and strings and comments never
//! match.
//!
//! Every function works on one statement and returns `None` when it cannot place the change
//! with certainty (several statements, `INTO`, `ORDER SIBLINGS BY`, `FOR SYSTEM_TIME`,
//! trailing clauses it does not know…): the caller then leaves the query alone.

use crate::{Dialect, Lexer, Token, TokenKind};

/// Aggregate functions: a select list with one needs its own row, not one per input row.
const AGGREGATES: &[&str] = &[
    "COUNT",
    "COUNT_BIG",
    "SUM",
    "AVG",
    "MIN",
    "MAX",
    "TOTAL",
    "MEDIAN",
    "ANY_VALUE",
    "EVERY",
    "STDDEV",
    "STDDEV_POP",
    "STDDEV_SAMP",
    "VARIANCE",
    "VAR_POP",
    "VAR_SAMP",
    "STRING_AGG",
    "GROUP_CONCAT",
    "LISTAGG",
    "ARRAY_AGG",
    "JSON_AGG",
    "JSONB_AGG",
    "JSON_ARRAYAGG",
    "JSON_OBJECTAGG",
    "XMLAGG",
    "BOOL_AND",
    "BOOL_OR",
    "BIT_AND",
    "BIT_OR",
    "BIT_XOR",
];

/// Words in the select head that make the select list impossible to replace.
const HEAD_MODIFIERS: &[&str] = &[
    "DISTINCT",
    "DISTINCTROW",
    "AS",
    "HIGH_PRIORITY",
    "STRAIGHT_JOIN",
    "SQL_SMALL_RESULT",
    "SQL_BIG_RESULT",
    "SQL_BUFFER_RESULT",
    "SQL_NO_CACHE",
    "SQL_CACHE",
    "SQL_CALC_FOUND_ROWS",
];

/// Flags of a scanned query.
mod flag {
    pub const DISTINCT: u16 = 1;
    pub const SET_OPERATION: u16 = 1 << 1;
    pub const GROUP_BY: u16 = 1 << 2;
    /// `HAVING` or `QUALIFY`: they may read the select list's aliases.
    pub const HAVING: u16 = 1 << 3;
    pub const AGGREGATE: u16 = 1 << 4;
    pub const WINDOW: u16 = 1 << 5;
    /// `LIMIT`, `TOP`, `OFFSET` or `FETCH`.
    pub const PAGED: u16 = 1 << 6;
    /// `OFFSET` or `FETCH`, which Transact-SQL accepts only after an `ORDER BY`.
    pub const OFFSET: u16 = 1 << 7;
    pub const WHERE_OR: u16 = 1 << 8;
    /// `WITH TIES` (`TOP`, `FETCH`), which needs an `ORDER BY` everywhere.
    pub const TIES: u16 = 1 << 9;
    /// `DISTINCT ON (…)`, whose `ORDER BY` must start with the same expressions.
    pub const DISTINCT_ON: u16 = 1 << 10;
}

/// Words after which an operand comes: a clause word there is a name (`WHERE limit > 1`).
const OPERAND_AFTER: &[&str] = &[
    "SELECT", "DISTINCT", "WHERE", "AND", "OR", "NOT", "ON", "BY", "AS", "CASE", "WHEN", "THEN",
    "ELSE", "HAVING", "QUALIFY", "SET", "LIKE", "BETWEEN",
];

/// Where the parts of a statement's top level are, as byte offsets, and what it holds.
#[derive(Debug, Default)]
struct Body {
    flags: u16,
    /// Start of the first token, and of the `SELECT` of the query proper (after a `WITH`).
    first: usize,
    main: usize,
    /// Start of the select list (after `DISTINCT`, `TOP n`…), and of `FROM`.
    list: usize,
    from: Option<usize>,
    /// End of the last token of the statement, without a trailing `;`.
    end: usize,
    /// The condition of `WHERE`: from the end of the keyword to the end of its last token.
    condition: Option<(usize, usize)>,
    /// End of the last token before the first clause that follows `WHERE`'s place.
    before_clauses: usize,
    /// The `ORDER BY` clause, and where the whitespace before it starts.
    order: Option<(usize, usize)>,
    order_gap: Option<usize>,
    /// End of the last token before a trailing `LIMIT`, `OFFSET`, `FETCH` or lock clause.
    before_tail: usize,
    /// Start of `FOR UPDATE`, `LOCK IN SHARE MODE` or `OPTION (…)`: they run to the end.
    lock: Option<usize>,
}

impl Body {
    fn has(&self, f: u16) -> bool {
        self.flags & f != 0
    }

    /// Whether the query is invalid without its `ORDER BY`.
    fn needs_order(&self, dialect: Dialect) -> bool {
        self.has(flag::TIES) || (dialect == Dialect::SqlServer && self.has(flag::OFFSET))
    }
}

/// The tokens of a statement without whitespace and comments, with their paren depth.
struct Tokens<'a> {
    sql: &'a str,
    list: Vec<Token>,
    depth: Vec<usize>,
}

impl<'a> Tokens<'a> {
    /// `None` when `sql` has more than one statement or unbalanced parentheses.
    fn new(sql: &'a str, dialect: Dialect) -> Option<Self> {
        let mut list = Vec::new();
        let mut depth = Vec::new();
        let mut level = 0usize;
        let mut ended = false;
        for token in Lexer::new(sql, dialect).filter(|t| !t.is_trivia()) {
            if ended {
                return None;
            }
            let text = token.text(sql);
            if token.kind == TokenKind::Symbol {
                match text {
                    "(" => {
                        depth.push(level);
                        list.push(token);
                        level += 1;
                        continue;
                    }
                    ")" => level = level.checked_sub(1)?,
                    ";" if level == 0 => {
                        ended = true;
                        continue;
                    }
                    _ => {}
                }
            }
            depth.push(level);
            list.push(token);
        }
        (level == 0 && !list.is_empty()).then_some(Self { sql, list, depth })
    }

    /// The upper-cased word at `i`, at any depth.
    fn word(&self, i: usize) -> Option<String> {
        let t = self.list.get(i)?;
        (t.kind == TokenKind::Word).then(|| t.text(self.sql).to_ascii_uppercase())
    }

    /// The upper-cased word at `i` when it is at the top level.
    fn top(&self, i: usize) -> Option<String> {
        if self.depth.get(i) == Some(&0) {
            self.word(i)
        } else {
            None
        }
    }

    fn is_word(&self, i: usize, word: &str) -> bool {
        self.word(i).is_some_and(|w| w == word)
    }

    fn is_symbol(&self, i: usize, symbol: &str) -> bool {
        self.list
            .get(i)
            .is_some_and(|t| t.kind == TokenKind::Symbol && t.text(self.sql) == symbol)
    }

    /// Whether the token at `i` stands where an operand goes: after an operator, a comma, a
    /// dot or a word such as `SELECT` or `WHERE`. A word there is a name, never a clause.
    fn after_operator(&self, i: usize) -> bool {
        let Some(prev) = i.checked_sub(1).and_then(|p| self.list.get(p)) else {
            return false;
        };
        let text = prev.text(self.sql);
        match prev.kind {
            TokenKind::Symbol => ",(=<>+-*/%|&^!~.:".contains(text),
            TokenKind::Word => OPERAND_AFTER.contains(&text.to_ascii_uppercase().as_str()),
            _ => false,
        }
    }

    /// The index after the parenthesised group that opens at `i`.
    fn after_group(&self, i: usize) -> usize {
        let mut j = i + 1;
        while j < self.list.len() && self.depth[j] > 0 {
            j += 1;
        }
        j + 1
    }

    /// The index of the query's `SELECT`: the first token, or after a `WITH` list.
    fn main(&self) -> Option<usize> {
        match self.word(0)?.as_str() {
            "SELECT" => Some(0),
            "WITH" => (1..self.list.len()).find_map(|i| match self.top(i)?.as_str() {
                "SELECT" => Some(Some(i)),
                "INSERT" | "UPDATE" | "DELETE" | "MERGE" => Some(None),
                _ => None,
            })?,
            _ => None,
        }
    }
}

/// How a clause word at the top level ends what came before it.
enum Clause {
    Group,
    Having,
    SetOperation,
    Order,
    /// `LIMIT`, `OFFSET` or `FETCH`: these are the query's tail.
    Page,
    /// `FOR UPDATE`, `LOCK IN SHARE MODE` or `OPTION (…)`: ignored by a count.
    Lock,
    /// `START WITH`, `CONNECT BY`, `WINDOW`: end the `WHERE`, nothing else.
    Other,
}

/// The clause that starts at `i`, `Err` when the query has something that cannot be
/// placed around.
fn clause_at(t: &Tokens, i: usize, dialect: Dialect) -> Result<Option<Clause>, ()> {
    let Some(word) = t.top(i) else {
        return Ok(None);
    };
    let next = |w: &str| t.is_word(i + 1, w);
    let clause = match word.as_str() {
        "SETTINGS" | "FORMAT" if !t.is_symbol(i + 1, "(") => return Err(()),
        "GROUP" if next("BY") => Some(Clause::Group),
        "HAVING" | "QUALIFY" => Some(Clause::Having),
        "UNION" | "INTERSECT" | "EXCEPT" | "MINUS" => Some(Clause::SetOperation),
        "ORDER" if next("SIBLINGS") => return Err(()),
        "ORDER" if next("BY") => Some(Clause::Order),
        "LIMIT" | "OFFSET" | "FETCH" => Some(Clause::Page),
        "FOR" if ["UPDATE", "SHARE", "NO", "KEY"].iter().any(|w| next(w)) => Some(Clause::Lock),
        "INTO" | "PROCEDURE" | "FOR" => return Err(()),
        "LOCK" if next("IN") => Some(Clause::Lock),
        "OPTION" if dialect == Dialect::SqlServer && t.is_symbol(i + 1, "(") => Some(Clause::Lock),
        "WINDOW" => Some(Clause::Other),
        "START" if next("WITH") => Some(Clause::Other),
        "CONNECT" if next("BY") => Some(Clause::Other),
        _ => None,
    };
    // `SELECT offset FROM t`, `WHERE limit > 1`: a column named like a clause.
    Ok(clause.filter(|_| !t.after_operator(i)))
}

/// Skips what follows `SELECT` before the select list; returns the index of its first token.
fn skip_head(t: &Tokens, mut i: usize, body: &mut Body) -> usize {
    loop {
        match t.top(i).as_deref() {
            Some("ALL") => i += 1,
            Some(w) if HEAD_MODIFIERS.contains(&w) => {
                body.flags |= flag::DISTINCT;
                if w == "DISTINCT" && t.is_word(i + 1, "ON") {
                    body.flags |= flag::DISTINCT_ON;
                }
                i += 1;
            }
            Some("TOP") => {
                body.flags |= flag::PAGED;
                i = if t.is_symbol(i + 1, "(") {
                    t.after_group(i + 1)
                } else {
                    i + 2
                };
                if t.is_word(i, "PERCENT") {
                    i += 1;
                }
                if t.is_word(i, "WITH") && t.is_word(i + 1, "TIES") {
                    body.flags |= flag::TIES;
                    i += 2;
                }
            }
            _ => return i,
        }
    }
}

/// Reads the top level of one statement; `None` when it is not a plain query that can be
/// rewritten with certainty.
fn scan(sql: &str, dialect: Dialect) -> Option<Body> {
    let t = Tokens::new(sql, dialect)?;
    let main = t.main()?;
    let mut body = Body {
        first: t.list[0].start,
        main: t.list[main].start,
        end: t.list.last()?.end,
        ..Body::default()
    };
    let start = skip_head(&t, main + 1, &mut body);
    body.list = t.list.get(start)?.start;
    body.before_clauses = body.end;
    body.before_tail = body.end;

    let (mut in_list, mut in_order, mut in_where, mut clauses, mut tail) =
        (true, false, false, false, false);
    for i in start..t.list.len() {
        let before = t.list[i - 1].end;
        if in_list {
            if t.word(i).is_some_and(|w| AGGREGATES.contains(&w.as_str()))
                && t.is_symbol(i + 1, "(")
            {
                body.flags |= flag::AGGREGATE;
            }
            if t.is_word(i, "OVER") {
                body.flags |= flag::WINDOW;
            }
            if t.top(i).as_deref() == Some("FROM") {
                (body.from, in_list) = (Some(t.list[i].start), false);
                continue;
            }
        }
        if t.top(i).as_deref() == Some("WHERE")
            && body.condition.is_none()
            && body.from.is_some()
            && !clauses
        {
            body.condition = Some((t.list[i].end, body.end));
            in_where = true;
        }
        if in_where && t.top(i).as_deref() == Some("OR") {
            body.flags |= flag::WHERE_OR;
        }
        // `FETCH FIRST n ROWS WITH TIES`.
        if tail && t.top(i).as_deref() == Some("TIES") {
            body.flags |= flag::TIES;
        }
        let Ok(clause) = clause_at(&t, i, dialect) else {
            return None;
        };
        let Some(clause) = clause else { continue };
        if !clauses {
            (clauses, body.before_clauses) = (true, before);
        }
        if in_where {
            in_where = false;
            body.condition = body.condition.map(|(s, _)| (s, before));
        }
        match clause {
            Clause::Group => body.flags |= flag::GROUP_BY,
            Clause::Having => body.flags |= flag::HAVING,
            Clause::SetOperation => body.flags |= flag::SET_OPERATION,
            Clause::Order if body.order.is_none() => {
                body.order = Some((t.list[i].start, body.end));
                body.order_gap = gap_before(sql, &t.list, i);
                in_order = true;
            }
            Clause::Page | Clause::Lock => {
                if in_order {
                    in_order = false;
                    body.order = body.order.map(|(s, _)| (s, before));
                }
                if !tail {
                    (tail, body.before_tail) = (true, before);
                }
                if matches!(clause, Clause::Lock) {
                    body.lock.get_or_insert(t.list[i].start);
                } else {
                    body.flags |= flag::PAGED;
                    if !t.is_word(i, "LIMIT") {
                        body.flags |= flag::OFFSET;
                    }
                }
            }
            Clause::Order | Clause::Other => {}
        }
    }
    Some(body)
}

/// The start of the whitespace before `tokens[i]`, when only whitespace separates it from
/// the token before: it can go with the token. Comments in between stay.
fn gap_before(sql: &str, tokens: &[Token], i: usize) -> Option<usize> {
    let from = tokens[i - 1].end;
    sql[from..tokens[i].start].trim().is_empty().then_some(from)
}

/// `sql` with its top-level `ORDER BY` replaced by one on `column` (`true`: descending), or
/// removed with `None`. Without one, a new clause goes before a trailing `LIMIT`, `OFFSET`,
/// `FETCH` or lock clause, otherwise at the end of the statement. Where the query needs an
/// `ORDER BY` (`OFFSET` on Transact-SQL, `WITH TIES`), removing it leaves
/// `ORDER BY (SELECT NULL)` on Transact-SQL and gives up elsewhere; so does a `DISTINCT ON`.
#[must_use]
pub fn set_order_by(sql: &str, dialect: Dialect, order: Option<(&str, bool)>) -> Option<String> {
    let body = scan(sql, dialect)?;
    if body.has(flag::DISTINCT_ON) {
        return None;
    }
    let clause = order.map(|(column, desc)| {
        let direction = if desc { "DESC" } else { "ASC" };
        format!("ORDER BY {} {direction}", dialect.quote_ident(column))
    });
    let (from, to, text) = match (body.order, clause) {
        (Some((start, end)), Some(clause)) => (start, end, clause),
        (Some((start, end)), None) if body.needs_order(dialect) => {
            if dialect != Dialect::SqlServer {
                return None;
            }
            (start, end, "ORDER BY (SELECT NULL)".to_owned())
        }
        (Some((start, end)), None) => (body.order_gap.unwrap_or(start), end, String::new()),
        (None, Some(clause)) => (body.before_tail, body.before_tail, format!("\n{clause}")),
        (None, None) => return Some(sql.to_owned()),
    };
    Some(format!("{}{text}{}", &sql[..from], &sql[to..]))
}

/// `sql` with `condition` added to its top-level `WHERE` with `AND` (the existing condition
/// goes in parentheses when it has an `OR`), or a new `WHERE` before `GROUP BY`, `ORDER BY`
/// and the other clauses that follow. Not for set operations or queries without `FROM`.
#[must_use]
pub fn add_condition(sql: &str, dialect: Dialect, condition: &str) -> Option<String> {
    let body = scan(sql, dialect)?;
    let condition = condition.trim();
    if condition.is_empty() || body.from.is_none() || body.has(flag::SET_OPERATION) {
        return None;
    }
    let (from, to, text) = match body.condition {
        Some((start, end)) => {
            let old = sql[start..end].trim();
            let old = if body.has(flag::WHERE_OR) {
                format!("({old})")
            } else {
                old.to_owned()
            };
            (start, end, format!(" {old} AND {condition}"))
        }
        None => (
            body.before_clauses,
            body.before_clauses,
            format!("\nWHERE {condition}"),
        ),
    };
    Some(format!("{}{text}{}", &sql[..from], &sql[to..]))
}

/// The text of `sql` from `start` to the end of its statement, without what a count does not
/// need: the `ORDER BY` (unless `keep_order`) and the lock or `OPTION` clause.
fn without_order(sql: &str, body: &Body, start: usize, keep_order: bool) -> String {
    let mut cuts = Vec::new();
    if let (Some((from, to)), false) = (body.order, keep_order) {
        cuts.push((body.order_gap.unwrap_or(from), to));
    }
    if let Some(lock) = body.lock {
        cuts.push((lock, body.end));
    }
    cuts.sort_unstable();
    let mut text = String::new();
    let mut at = start;
    for (from, to) in cuts.into_iter().filter(|c| c.0 >= start) {
        text.push_str(&sql[at..from]);
        at = to.max(at);
    }
    text.push_str(&sql[at..body.end]);
    text
}

/// A query counting the rows of `sql`. A plain query (no `DISTINCT`, grouping, set
/// operation, aggregate, window function or row cap) keeps its `FROM` and the rest and
/// selects `COUNT(*)`: no derived table, so duplicate column names and CTEs cannot get in
/// the way. Anything else is wrapped as `SELECT COUNT(*) FROM (…) t`, selecting `1` where
/// that is safe; a leading `WITH` stays outside. The alias has no `AS`, which Oracle
/// rejects. The `ORDER BY` is dropped, except where the query needs it (`OFFSET … FETCH` on
/// Transact-SQL, `WITH TIES`).
#[must_use]
pub fn count_query(sql: &str, dialect: Dialect) -> Option<String> {
    let body = scan(sql, dialect)?;
    let keep_order = body.needs_order(dialect);
    let prefix = &sql[body.first..body.main];
    let plain = !body.has(
        flag::DISTINCT
            | flag::SET_OPERATION
            | flag::GROUP_BY
            | flag::HAVING
            | flag::AGGREGATE
            | flag::WINDOW
            | flag::PAGED,
    );
    if let (true, Some(from)) = (plain, body.from) {
        let rest = without_order(sql, &body, from, false);
        return Some(format!("{prefix}SELECT COUNT(*) {}", rest.trim_end()));
    }
    let one = !body.has(flag::DISTINCT | flag::SET_OPERATION | flag::HAVING)
        && (body.has(flag::GROUP_BY) || !body.has(flag::AGGREGATE))
        && !keep_order;
    let inner = match body.from {
        Some(from) if one => {
            let head = &sql[body.main..body.list];
            format!("{head}1 {}", without_order(sql, &body, from, keep_order))
        }
        _ => without_order(sql, &body, body.main, keep_order),
    };
    Some(format!(
        "{prefix}SELECT COUNT(*) FROM (\n{}\n) t",
        inner.trim_end()
    ))
}

#[cfg(test)]
mod tests;
