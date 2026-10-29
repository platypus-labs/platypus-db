//! The rows an `UPDATE` or `DELETE` is about to change, read before it runs so that they can
//! be put back: one table, a `WHERE` (or none), nothing that picks rows otherwise.

use crate::Dialect;
use crate::analysis::{MODIFIERS, TableName, Words};

/// What a change touches.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Touched {
    /// `UPDATE` or `DELETE`.
    pub verb: &'static str,
    pub table: TableName,
    /// What a `SELECT` of the rows it changes reads after `FROM`: the table and its alias.
    pub from: String,
    /// Its condition, after `WHERE`; `None` for every row.
    pub filter: Option<String>,
    /// The columns an `UPDATE` sets, as written (without a table or alias before them).
    pub assigned: Vec<String>,
}

impl Touched {
    /// The `SELECT` of `list` from the rows it changes, as they are before.
    #[must_use]
    pub fn select(&self, list: &str) -> String {
        match &self.filter {
            Some(filter) => format!("SELECT {list} FROM {} WHERE {filter}", self.from),
            None => format!("SELECT {list} FROM {}", self.from),
        }
    }
}

/// Words that make the rows changed depend on more than the table and its `WHERE`: joins,
/// other tables, an order and a limit, Transact-SQL's `OUTPUT` and `TOP`.
const UNSUPPORTED: &[&str] = &[
    "FROM", "JOIN", "USING", "ORDER", "LIMIT", "OUTPUT", "TOP", "OPTION", "WITH",
];

/// The rows `sql`, one `UPDATE` or `DELETE` of one table, changes; `None` for any other
/// statement, and for one whose rows a `SELECT` of the table cannot tell.
#[must_use]
pub fn touched_rows(sql: &str, dialect: Dialect) -> Option<Touched> {
    let w = Words::new(sql, dialect);
    let mut end = w.tokens.len();
    while end > 0 && w.symbol(end - 1, ";") {
        end -= 1;
    }
    // One statement: no `;` inside. A Transact-SQL variable lives in its batch alone, which
    // the `SELECT` would run outside of.
    let variable = dialect == Dialect::SqlServer && (0..end).any(|i| w.symbol(i, "@"));
    if variable || w.top(0, end).any(|i| w.symbol(i, ";")) {
        return None;
    }
    let verb = match w.word(0)?.as_str() {
        "UPDATE" => "UPDATE",
        "DELETE" => "DELETE",
        _ => return None,
    };
    let mut i = 1;
    while w.is(i, MODIFIERS) {
        i += 1;
    }
    // `DELETE FROM t`, or Transact-SQL's `DELETE t`.
    if verb == "DELETE" && w.is(i, &["FROM"]) {
        i += 1;
    }
    let source_start = i;
    while w.is(i, &["ONLY"]) {
        i += 1;
    }
    let table = w.table(i, dialect)?;
    let set = if verb == "UPDATE" {
        Some(w.find_top(i, end, &["SET"])?)
    } else {
        None
    };
    // Clauses after the `WHERE` that return rows change none.
    let clauses_end = w.find_top(i, end, &["RETURNING"]).unwrap_or(end);
    if w.find_top(i, clauses_end, UNSUPPORTED).is_some() {
        return None;
    }
    let filter = w.find_top(set.unwrap_or(i), clauses_end, &["WHERE"]);
    let assigned = match set {
        Some(set) => assigned(&w, set + 1, filter.unwrap_or(clauses_end), dialect)?,
        None => Vec::new(),
    };
    let source_end = set.or(filter).unwrap_or(clauses_end);
    // `UPDATE a, b SET …` changes two tables.
    if w.top(source_start, source_end).any(|k| w.symbol(k, ",")) || source_end <= source_start {
        return None;
    }
    let filter = match filter {
        Some(at) if at + 1 >= clauses_end => return None,
        Some(at) => Some(w.text(at + 1, clauses_end).to_owned()),
        None => None,
    };
    Some(Touched {
        verb,
        table,
        from: w.text(source_start, source_end).to_owned(),
        filter,
        assigned,
    })
}

/// The columns of a `SET` list from `start` to `end`: `a = 1, t.b = 2`; `None` for a list it
/// cannot read, such as `(a, b) = (1, 2)`.
fn assigned(w: &Words<'_>, start: usize, end: usize, dialect: Dialect) -> Option<Vec<String>> {
    let mut columns = Vec::new();
    let mut item = start;
    loop {
        let name = if w.symbol(item + 1, ".") {
            item + 2
        } else {
            item
        };
        columns.push(w.ident(name, dialect)?);
        if !w.symbol(name + 1, "=") {
            return None;
        }
        match w.top(name + 1, end).find(|&k| w.symbol(k, ",")) {
            Some(comma) => item = comma + 1,
            None => return Some(columns),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn select(sql: &str, dialect: Dialect) -> Option<String> {
        touched_rows(sql, dialect).map(|t| t.select("*"))
    }

    #[test]
    fn reads_the_rows_a_change_touches() {
        let pg = Dialect::Postgres;
        assert_eq!(
            select(
                "UPDATE s.orders o SET total = 0 WHERE o.id > 10 AND note = 'a;b';",
                pg
            )
            .as_deref(),
            Some("SELECT * FROM s.orders o WHERE o.id > 10 AND note = 'a;b'")
        );
        let t = touched_rows(
            "DELETE FROM ONLY \"Big\" WHERE x IN (SELECT y FROM z) RETURNING *",
            pg,
        )
        .unwrap();
        assert_eq!(
            t.select("*"),
            "SELECT * FROM ONLY \"Big\" WHERE x IN (SELECT y FROM z)"
        );
        assert_eq!((t.verb, t.table.name.as_str()), ("DELETE", "Big"));
        let t = touched_rows("UPDATE t o SET o.a = f(1, 2), \"B\" = 3 WHERE id = 1", pg).unwrap();
        assert_eq!(t.assigned, ["a", "B"]);
        assert_eq!(select("UPDATE t SET (a, b) = (1, 2)", pg), None);
        assert_eq!(
            select("UPDATE t SET a = 1", pg).as_deref(),
            Some("SELECT * FROM t"),
            "every row"
        );
        assert_eq!(
            select("DELETE LOW_PRIORITY FROM `t` WHERE id = 1", Dialect::MySql).as_deref(),
            Some("SELECT * FROM `t` WHERE id = 1")
        );
        assert_eq!(
            select("DELETE t WHERE id = 1", Dialect::SqlServer).as_deref(),
            Some("SELECT * FROM t WHERE id = 1")
        );
    }

    #[test]
    fn leaves_out_what_a_select_cannot_tell() {
        let pg = Dialect::Postgres;
        for sql in [
            "SELECT 1",
            "INSERT INTO t VALUES (1)",
            "UPDATE t SET a = u.a FROM u WHERE t.id = u.id",
            "DELETE FROM t USING u WHERE t.id = u.id",
            "WITH x AS (SELECT 1) DELETE FROM t",
            "UPDATE t SET a = 1 WHERE id = 1; DELETE FROM u",
        ] {
            assert_eq!(select(sql, pg), None, "{sql}");
        }
        let my = Dialect::MySql;
        assert_eq!(select("DELETE FROM t ORDER BY id LIMIT 3", my), None);
        assert_eq!(
            select("UPDATE a, b SET a.x = b.x WHERE a.id = b.id", my),
            None
        );
        assert_eq!(
            select("UPDATE a JOIN b ON a.id = b.id SET a.x = 1", my),
            None
        );
        let ms = Dialect::SqlServer;
        assert_eq!(select("DELETE TOP (5) FROM t", ms), None);
        assert_eq!(
            select("DELETE FROM t OUTPUT deleted.* WHERE id = 1", ms),
            None
        );
        assert_eq!(
            select("UPDATE t SET a = @x WHERE id = 1", ms),
            None,
            "a batch's variable"
        );
    }
}
