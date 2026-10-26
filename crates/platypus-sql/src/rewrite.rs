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

mod filter;
pub use filter::{FilterColumn, filter_columns};

/// Aggregate functions: a select list with one needs its own row, not one per input row.
/// A missing name makes "Count rows" count the input rows, while a scalar function taken for
/// an aggregate only costs a derived table: the list leans wide.
const AGGREGATES: &[&str] = &[
    "COUNT",
    "COUNT_BIG",
    "SUM",
    "AVG",
    "MIN",
    "MAX",
    "TOTAL",
    "MEDIAN",
    "MODE",
    "ANY_VALUE",
    "EVERY",
    "STDDEV",
    "STDDEV_POP",
    "STDDEV_SAMP",
    "VARIANCE",
    "VAR_POP",
    "VAR_SAMP",
    "PERCENTILE_CONT",
    "PERCENTILE_DISC",
    "CORR",
    "COVAR_POP",
    "COVAR_SAMP",
    "REGR_AVGX",
    "REGR_AVGY",
    "REGR_COUNT",
    "REGR_INTERCEPT",
    "REGR_R2",
    "REGR_SLOPE",
    "REGR_SXX",
    "REGR_SXY",
    "REGR_SYY",
    "GROUPING",
    "STRING_AGG",
    "GROUP_CONCAT",
    "LISTAGG",
    "ARRAY_AGG",
    "JSON_AGG",
    "JSONB_AGG",
    "JSON_ARRAYAGG",
    "JSON_OBJECTAGG",
    "JSON_OBJECT_AGG",
    "JSONB_OBJECT_AGG",
    "XMLAGG",
    "BOOL_AND",
    "BOOL_OR",
    "BIT_AND",
    "BIT_OR",
    "BIT_XOR",
    // Transact-SQL, MySQL, Oracle, Db2.
    "STDEV",
    "STDEVP",
    "VAR",
    "VARP",
    "STD",
    "CHECKSUM_AGG",
    "GROUPING_ID",
    "APPROX_COUNT_DISTINCT",
    "COLLECT",
    "STATS_MODE",
    "CORRELATION",
    "COVARIANCE",
    "COVARIANCE_SAMP",
    "VARIANCE_SAMP",
    // SQLite, DuckDB.
    "JSON_GROUP_ARRAY",
    "JSON_GROUP_OBJECT",
    "LIST",
    "FIRST",
    "LAST",
    "ARG_MIN",
    "ARG_MAX",
    "MIN_BY",
    "MAX_BY",
    "ARBITRARY",
    "PRODUCT",
    "HISTOGRAM",
    "ENTROPY",
    "KURTOSIS",
    "SKEWNESS",
    "QUANTILE_CONT",
    "QUANTILE_DISC",
    "APPROX_QUANTILE",
    // ClickHouse (names are matched upper-cased).
    "UNIQ",
    "UNIQEXACT",
    "COUNTIF",
    "SUMIF",
    "AVGIF",
    "MINIF",
    "MAXIF",
    "GROUPARRAY",
    "GROUPUNIQARRAY",
    "ANYLAST",
    "ARGMIN",
    "ARGMAX",
    "QUANTILE",
    "QUANTILES",
    "TOPK",
    // BigQuery, Snowflake, Databricks.
    "COUNT_IF",
    "LOGICAL_AND",
    "LOGICAL_OR",
    "ARRAY_CONCAT_AGG",
    "APPROX_QUANTILES",
    "APPROX_TOP_COUNT",
    "ARRAY_UNIQUE_AGG",
    "OBJECT_AGG",
    "BOOLAND_AGG",
    "BOOLOR_AGG",
    "BITAND_AGG",
    "BITOR_AGG",
    "BITXOR_AGG",
    "HASH_AGG",
    "SKEW",
    "APPROX_PERCENTILE",
    "APPROX_TOP_K",
    "COLLECT_LIST",
    "COLLECT_SET",
    "PERCENTILE",
    "PERCENTILE_APPROX",
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
    /// An operator in the `WHERE` that binds looser than `AND`: `OR`, `XOR`, MySQL's `||`.
    pub const WHERE_OR: u16 = 1 << 8;
    /// `WITH TIES` (`TOP`, `FETCH`), which needs an `ORDER BY` everywhere.
    pub const TIES: u16 = 1 << 9;
    /// `DISTINCT ON (…)`, whose `ORDER BY` must start with the same expressions.
    pub const DISTINCT_ON: u16 = 1 << 10;
    /// `*` or `t.*` in the select list: how many columns it has is unknown.
    pub const STAR: u16 = 1 << 11;
}

/// Words after which an operand comes: a clause word there is a name (`WHERE limit > 1`,
/// `FROM settings`).
const OPERAND_AFTER: &[&str] = &[
    "SELECT", "DISTINCT", "FROM", "JOIN", "WHERE", "AND", "OR", "XOR", "NOT", "ON", "BY", "AS",
    "CASE", "WHEN", "THEN", "ELSE", "HAVING", "QUALIFY", "SET", "LIKE", "BETWEEN",
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
    /// How many items the select list has, counting `*` as one.
    columns: usize,
    /// End of the last token of the statement, without a trailing `;`.
    end: usize,
    /// The condition of `WHERE`: from the end of the keyword to the end of its last token.
    condition: Option<(usize, usize)>,
    /// End of the last token before the first clause that follows `WHERE`'s place.
    before_clauses: usize,
    /// The `ORDER BY` clause, and where the whitespace before it starts.
    order: Option<(usize, usize)>,
    order_gap: Option<usize>,
    /// End of the last token before a trailing `LIMIT`, `OFFSET`, `FETCH`, lock clause or
    /// `OPTION (…)`.
    before_tail: usize,
    /// `FOR UPDATE` or `LOCK IN SHARE MODE` with the whitespace before them, up to a
    /// `LIMIT` that follows or the end.
    lock: Option<(usize, usize)>,
    /// Start of Transact-SQL's `OPTION (…)`, the last clause.
    hint: Option<usize>,
}

impl Body {
    fn has(&self, f: u16) -> bool {
        self.flags & f != 0
    }

    /// Whether the query is invalid without its `ORDER BY`.
    fn needs_order(&self, dialect: Dialect) -> bool {
        self.has(flag::TIES) || (dialect == Dialect::SqlServer && self.has(flag::OFFSET))
    }

    /// Notes what the select list's token at `i` holds; `start` is the list's first token.
    fn list_token(&mut self, t: &Tokens, i: usize, start: usize) {
        if t.word(i).is_some_and(|w| AGGREGATES.contains(&w.as_str())) && t.is_symbol(i + 1, "(") {
            self.flags |= flag::AGGREGATE;
        }
        if t.is_word(i, "OVER") {
            self.flags |= flag::WINDOW;
        }
        if t.depth[i] > 0 {
            return;
        }
        if t.is_symbol(i, ",") {
            self.columns += 1;
        }
        // `*` and `t.*`, not a product.
        if t.is_symbol(i, "*") && (i == start || t.is_symbol(i - 1, ",") || t.is_symbol(i - 1, "."))
        {
            self.flags |= flag::STAR;
        }
    }

    /// Goes from `part` into `clause`, which starts at token `i`, and returns the part it
    /// opens. `None` when the clause cannot come there: `ORDER BY` or `GROUP BY` after
    /// `LIMIT` or a lock clause, a second `ORDER BY`, anything after `OPTION (…)`. Such a
    /// query is invalid, and the ranges recorded so far would overlap.
    fn enter(&mut self, t: &Tokens, part: Part, clause: Clause, i: usize) -> Option<Part> {
        let (start, before) = (t.list[i].start, t.list[i - 1].end);
        let tail = matches!(part, Part::Page | Part::Lock | Part::Hint);
        let next = match clause {
            _ if part == Part::Hint => return None,
            Clause::Order if tail || self.order.is_some() => return None,
            Clause::Group | Clause::Having | Clause::SetOperation | Clause::Other
                if tail || part == Part::Order =>
            {
                return None;
            }
            Clause::Lock if self.lock.is_some() && part != Part::Lock => return None,
            Clause::Group => {
                self.flags |= flag::GROUP_BY;
                Part::Clauses
            }
            Clause::Having => {
                self.flags |= flag::HAVING;
                Part::Clauses
            }
            Clause::SetOperation => {
                self.flags |= flag::SET_OPERATION;
                Part::Clauses
            }
            Clause::Other => Part::Clauses,
            Clause::Order => {
                self.order = Some((start, self.end));
                self.order_gap = gap_before(t.sql, &t.list, i);
                Part::Order
            }
            Clause::Page => {
                self.flags |= flag::PAGED;
                if !t.is_word(i, "LIMIT") {
                    self.flags |= flag::OFFSET;
                }
                Part::Page
            }
            Clause::Lock => {
                let from = gap_before(t.sql, &t.list, i).unwrap_or(start);
                self.lock.get_or_insert((from, self.end));
                Part::Lock
            }
            Clause::Hint => {
                self.hint = Some(start);
                Part::Hint
            }
        };
        // The clause ends what was open before it.
        match part {
            Part::Where => self.condition = self.condition.map(|(s, _)| (s, before)),
            Part::Order => self.order = self.order.map(|(s, _)| (s, before)),
            Part::Lock if next != Part::Lock => self.lock = self.lock.map(|(s, _)| (s, before)),
            _ => {}
        }
        if matches!(part, Part::List | Part::From | Part::Where) {
            self.before_clauses = before;
        }
        if !tail && matches!(next, Part::Page | Part::Lock | Part::Hint) {
            self.before_tail = before;
        }
        Some(next)
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
    fn operand_at(&self, i: usize) -> bool {
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

    /// The index after the number that starts at `i` (`TOP 10`): the lexer reads each digit
    /// as a symbol of its own, so the digits right after it belong to it.
    fn after_number(&self, i: usize) -> usize {
        let mut j = i + 1;
        while self.list.get(j).is_some_and(|t| {
            t.kind == TokenKind::Symbol
                && t.start == self.list[j - 1].end
                && t.text(self.sql)
                    .bytes()
                    .all(|b| b.is_ascii_digit() || b == b'.')
        }) {
            j += 1;
        }
        j
    }

    /// Whether the `FROM` at `i` belongs to `IS [NOT] DISTINCT FROM`.
    fn is_distinct_from(&self, i: usize) -> bool {
        i.checked_sub(1)
            .is_some_and(|p| self.is_word(p, "DISTINCT"))
            && i.checked_sub(2)
                .is_some_and(|p| self.is_word(p, "IS") || self.is_word(p, "NOT"))
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
#[derive(Debug, Clone, Copy)]
enum Clause {
    Group,
    Having,
    SetOperation,
    Order,
    /// `LIMIT`, `OFFSET` or `FETCH`: these are the query's tail.
    Page,
    /// `FOR UPDATE` or `LOCK IN SHARE MODE`: ignored by a count.
    Lock,
    /// Transact-SQL's `OPTION (…)`: it ends the whole statement, so a count keeps it last.
    Hint,
    /// `START WITH`, `CONNECT BY`, `WINDOW`: end the `WHERE`, nothing else.
    Other,
}

/// Where the scan of a statement's top level is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Part {
    /// The select list, up to `FROM` or the first clause.
    List,
    /// From `FROM` to `WHERE` or the first clause.
    From,
    Where,
    /// After `GROUP BY`, `HAVING`, a set operation or another clause before `ORDER BY`.
    Clauses,
    Order,
    /// After `LIMIT`, `OFFSET` or `FETCH`.
    Page,
    Lock,
    Hint,
}

/// The clause that starts at `i`, `Err` when the query has something that cannot be
/// placed around.
fn clause_at(t: &Tokens, i: usize, dialect: Dialect) -> Result<Option<Clause>, ()> {
    let Some(word) = t.top(i) else {
        return Ok(None);
    };
    let next = |w: &str| t.is_word(i + 1, w);
    let clause = match word.as_str() {
        // Reserved words, never a name.
        "INTO" => return Err(()),
        "ORDER" if next("SIBLINGS") => return Err(()),
        "FOR" if !["UPDATE", "SHARE", "NO", "KEY"].iter().any(|w| next(w)) => return Err(()),
        // `SELECT offset FROM t`, `FROM settings`, `WHERE format = 'pdf'`: a name.
        _ if t.operand_at(i) => return Ok(None),
        // MySQL's `PROCEDURE ANALYSE()`, ClickHouse's `SETTINGS …` and `FORMAT …`.
        "PROCEDURE" => return Err(()),
        "SETTINGS" | "FORMAT" if dialect == Dialect::ClickHouse && !t.is_symbol(i + 1, "(") => {
            return Err(());
        }
        "GROUP" if next("BY") => Clause::Group,
        "HAVING" | "QUALIFY" => Clause::Having,
        "UNION" | "INTERSECT" | "EXCEPT" | "MINUS" => Clause::SetOperation,
        "ORDER" if next("BY") => Clause::Order,
        "LIMIT" | "OFFSET" | "FETCH" => Clause::Page,
        "FOR" => Clause::Lock,
        "LOCK" if next("IN") => Clause::Lock,
        "OPTION" if dialect == Dialect::SqlServer && t.is_symbol(i + 1, "(") => Clause::Hint,
        "WINDOW" => Clause::Other,
        "START" if next("WITH") => Clause::Other,
        "CONNECT" if next("BY") => Clause::Other,
        _ => return Ok(None),
    };
    Ok(Some(clause))
}

/// Skips what follows `SELECT` before the select list; returns the index of its first token.
fn skip_head(t: &Tokens, mut i: usize, body: &mut Body, dialect: Dialect) -> usize {
    loop {
        match t.top(i).as_deref() {
            Some("ALL") => i += 1,
            // Oracle's `UNIQUE` is its `DISTINCT`.
            Some(w)
                if HEAD_MODIFIERS.contains(&w) || (w == "UNIQUE" && dialect == Dialect::Oracle) =>
            {
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
                    t.after_number(i + 1)
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
    scan_tokens(sql, dialect).map(|(_, body)| body)
}

/// `scan`, with the statement's tokens.
fn scan_tokens(sql: &str, dialect: Dialect) -> Option<(Tokens<'_>, Body)> {
    let t = Tokens::new(sql, dialect)?;
    let main = t.main()?;
    let mut body = Body {
        first: t.list[0].start,
        main: t.list[main].start,
        end: t.list.last()?.end,
        columns: 1,
        ..Body::default()
    };
    let start = skip_head(&t, main + 1, &mut body, dialect);
    body.list = t.list.get(start)?.start;
    body.before_clauses = body.end;
    body.before_tail = body.end;

    let mut part = Part::List;
    for i in start..t.list.len() {
        match (part, t.top(i).as_deref()) {
            (Part::List, Some("FROM")) if !t.is_distinct_from(i) => {
                (body.from, part) = (Some(t.list[i].start), Part::From);
                continue;
            }
            (Part::List, _) => body.list_token(&t, i, start),
            (Part::From, Some("WHERE")) => {
                (body.condition, part) = (Some((t.list[i].end, body.end)), Part::Where);
                continue;
            }
            (Part::Where, Some("OR" | "XOR")) => body.flags |= flag::WHERE_OR,
            (Part::Where, None)
                if dialect == Dialect::MySql && t.depth[i] == 0 && t.is_symbol(i, "|") =>
            {
                body.flags |= flag::WHERE_OR;
            }
            // `FETCH FIRST n ROWS WITH TIES`.
            (Part::Page, Some("TIES")) => body.flags |= flag::TIES,
            _ => {}
        }
        if let Some(clause) = clause_at(&t, i, dialect).ok()? {
            part = body.enter(&t, part, clause, i)?;
        }
    }
    Some((t, body))
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
    let clause = order.map(|(column, desc)| {
        let direction = if desc { "DESC" } else { "ASC" };
        format!("ORDER BY {} {direction}", dialect.quote_ident(column))
    });
    put_order_by(sql, dialect, clause)
}

/// The top-level `ORDER BY` of `sql` as written, from `ORDER` to its last token; `None` when
/// it has none or cannot be read with certainty. What `restore_order_by` puts back.
#[must_use]
pub fn order_by_clause(sql: &str, dialect: Dialect) -> Option<String> {
    let (start, end) = scan(sql, dialect)?.order?;
    Some(sql[start..end].to_owned())
}

/// `sql` with `clause`, an `ORDER BY` read by `order_by_clause`, in place of its top-level
/// one, placed as `set_order_by` places a sort: the statement gets back the sort it had before
/// the viewer changed it. `None` when the result would not be one statement ordered by
/// `clause`.
#[must_use]
pub fn restore_order_by(sql: &str, dialect: Dialect, clause: &str) -> Option<String> {
    let clause = clause.trim();
    let restored = put_order_by(sql, dialect, Some(clause.to_owned()))?;
    (order_by_clause(&restored, dialect)? == clause).then_some(restored)
}

/// `sql` with its top-level `ORDER BY` replaced by `clause`, or removed with `None`; see
/// `set_order_by`.
fn put_order_by(sql: &str, dialect: Dialect, clause: Option<String>) -> Option<String> {
    let body = scan(sql, dialect)?;
    if body.has(flag::DISTINCT_ON) {
        return None;
    }
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
/// and the other clauses that follow. Not for set operations or queries without `FROM`. A
/// quick filter on a result column writes its condition on what `filter_columns` gives.
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
/// need: the `ORDER BY` (unless `keep_order`), the lock clauses and `OPTION (…)`.
fn counted(sql: &str, body: &Body, start: usize, keep_order: bool) -> String {
    let order = body
        .order
        .filter(|_| !keep_order)
        .map(|(from, to)| (body.order_gap.unwrap_or(from), to));
    let mut cuts: Vec<_> = order.into_iter().chain(body.lock).collect();
    cuts.sort_unstable();
    let mut text = String::new();
    let mut at = start;
    for (from, to) in cuts {
        // A cut before `at` is skipped: `get` never panics.
        if let Some(kept) = sql.get(at..from) {
            text.push_str(kept);
            at = to;
        }
    }
    text.push_str(
        sql.get(at..body.hint.unwrap_or(body.end))
            .unwrap_or_default(),
    );
    text
}

/// The column names a Transact-SQL derived table needs, as ` (c1, c2)`: there every column
/// must have a name, and a different one. Empty elsewhere, and with `*`, which hides how
/// many columns there are.
fn column_names(body: &Body, dialect: Dialect) -> String {
    if dialect != Dialect::SqlServer || body.has(flag::STAR) {
        return String::new();
    }
    let names: Vec<String> = (1..=body.columns).map(|n| format!("c{n}")).collect();
    format!(" ({})", names.join(", "))
}

/// A query counting the rows of `sql`. A plain query (no `DISTINCT`, grouping, set
/// operation, aggregate, window function or row cap) keeps its `FROM` and the rest and
/// selects `COUNT(*)`: no derived table, so duplicate column names and CTEs cannot get in
/// the way. Anything else is wrapped as `SELECT COUNT(*) FROM (…) t`, selecting `1 AS x`
/// where the select list does not decide the rows (a row cap, a window function); a leading
/// `WITH` stays outside, and on Transact-SQL a column list names the columns. The alias has
/// no `AS`, which Oracle rejects. The `ORDER BY` is dropped, except where the query needs it
/// (`OFFSET … FETCH` on Transact-SQL, `WITH TIES`); `OPTION (…)` moves to the end.
#[must_use]
pub fn count_query(sql: &str, dialect: Dialect) -> Option<String> {
    let body = scan(sql, dialect)?;
    let keep_order = body.needs_order(dialect);
    let prefix = &sql[body.first..body.main];
    // Where the select list decides the rows; `GROUP BY 1` and `HAVING` may also name it.
    let by_list =
        flag::DISTINCT | flag::SET_OPERATION | flag::GROUP_BY | flag::HAVING | flag::AGGREGATE;
    let count = match body.from {
        Some(from) if !body.has(by_list | flag::WINDOW | flag::PAGED) => {
            let rest = counted(sql, &body, from, false);
            format!("{prefix}SELECT COUNT(*) {}", rest.trim_end())
        }
        Some(from) if !body.has(by_list) && !keep_order => {
            let head = &sql[body.main..body.list];
            let inner = format!("{head}1 AS x {}", counted(sql, &body, from, false));
            format!("{prefix}SELECT COUNT(*) FROM (\n{}\n) t", inner.trim_end())
        }
        _ => format!(
            "{prefix}SELECT COUNT(*) FROM (\n{}\n) t{}",
            counted(sql, &body, body.main, keep_order).trim_end(),
            column_names(&body, dialect)
        ),
    };
    Some(match body.hint {
        Some(hint) => format!("{count}\n{}", &sql[hint..body.end]),
        None => count,
    })
}

#[cfg(test)]
mod tests;
