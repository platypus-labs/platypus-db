//! Large results in pages: `run_query` cut by `max_rows` (or `max_bytes`) returns a `cursor`,
//! and `next_page` reads the next rows of the same result instead of running the statement
//! again.
//!
//! The first read keeps the rows after the page, up to [`REST_ROWS`] rows, [`REST_BYTES`]
//! bytes or [`REST_TIME`] after the page is full, whichever comes first, inside the
//! statement's read-only transaction, which ends as it always does; the cursor then serves
//! its pages from memory and holds nothing on the connection. When a cap ended the result the
//! answers say so: the last page tells that the rest was not kept, never an empty page. The columns to mask are decided once, with the
//! first page: later pages hold the rows as masked then.
//!
//! A cursor belongs to the session that made it and to its connection: another session's, a
//! guessed one, an expired one and one whose connection agents may no longer use all read
//! the same, as unknown. They are kept [`KEEP`] after their last use, at most [`PER_SESSION`]
//! per session and [`TOTAL_BYTES`] altogether (the oldest go first), and closed with their
//! session.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use platypus_store::{AgentValues, SavedConnection};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::agent::{Agent, QueryArgs, locked};
use crate::error::{AgentError, Result};
use crate::guard::{self, Asked};
use crate::output::{Format, QueryResult};
use crate::query::{Cell, Column, Cut, Outcome, REST_BYTES, REST_ROWS, REST_TIME, Table};

/// How long a cursor is kept after its last use.
pub const KEEP: Duration = Duration::from_mins(5);
/// The environment variable that changes [`KEEP`], in milliseconds: for tests.
pub const KEEP_ENV: &str = "PLATYPUS_MCP_CURSOR_KEEP_MS";
/// The most cursors one session keeps.
pub const PER_SESSION: usize = 8;
/// The most memory all cursors take together, in the bytes of their values.
pub const TOTAL_BYTES: usize = 64 * 1024 * 1024;

/// The arguments of `next_page`.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct PageArgs {
    /// The `cursor` a `run_query` or an earlier `next_page` returned.
    pub cursor: String,
    /// Rows returned at most (default 100, up to 10000).
    pub max_rows: Option<u64>,
    /// Bytes of output at most (default 32768, up to 1048576).
    pub max_bytes: Option<u64>,
    /// `text` (an aligned table, the default) or `json`.
    pub format: Option<Format>,
}

/// What a cursor still has to give.
struct Held {
    id: String,
    session: String,
    connection: Uuid,
    /// Read while the connection showed sensitive values: its rows go when it stops.
    in_clear: bool,
    columns: Vec<Column>,
    rows: VecDeque<Vec<Cell>>,
    /// The cursor holds the end of the result; else the buffer's cap ended it.
    complete: bool,
    masked: Vec<String>,
    values_cut: bool,
    used: Instant,
}

impl Held {
    fn bytes(&self) -> usize {
        self.rows
            .iter()
            .flatten()
            .map(|c| c.text().len() + 24)
            .sum()
    }
}

/// The open cursors of an [`Agent`].
pub(crate) struct Cursors {
    held: Mutex<Vec<Held>>,
    keep: Duration,
}

impl Default for Cursors {
    fn default() -> Self {
        let keep = std::env::var(KEEP_ENV)
            .ok()
            .and_then(|v| v.parse().ok())
            .map_or(KEEP, Duration::from_millis);
        Self {
            held: Mutex::default(),
            keep,
        }
    }
}

impl Cursors {
    fn purge(&self, held: &mut Vec<Held>) {
        held.retain(|h| h.used.elapsed() < self.keep);
    }

    /// The connection of cursor `id` of `session`, if it is open.
    fn connection_of(&self, id: &str, session: &str) -> Option<Uuid> {
        let mut held = locked(&self.held);
        self.purge(&mut held);
        held.iter()
            .find(|h| h.id == id && h.session == session)
            .map(|h| h.connection)
    }

    /// Takes cursor `id` of `session` out: [`store`](Self::store) puts it back.
    fn take(&self, id: &str, session: &str) -> Option<Held> {
        let mut held = locked(&self.held);
        self.purge(&mut held);
        let at = held
            .iter()
            .position(|h| h.id == id && h.session == session)?;
        Some(held.remove(at))
    }

    fn forget(&self, id: &str) {
        locked(&self.held).retain(|h| h.id != id);
    }

    fn store(&self, mut cursor: Held) {
        cursor.used = Instant::now();
        let mut held = locked(&self.held);
        self.purge(&mut held);
        // The oldest of the session go first, then the oldest of all.
        while held.iter().filter(|h| h.session == cursor.session).count() >= PER_SESSION {
            let Some(at) = held.iter().position(|h| h.session == cursor.session) else {
                break;
            };
            held.remove(at);
        }
        let mut total = cursor.bytes() + held.iter().map(Held::bytes).sum::<usize>();
        while total > TOTAL_BYTES && !held.is_empty() {
            total -= held.remove(0).bytes();
        }
        held.push(cursor);
    }

    pub(crate) fn close_session(&self, session: &str) {
        locked(&self.held).retain(|h| h.session != session);
    }

    /// How many cursors are open.
    #[cfg(test)]
    fn len(&self) -> usize {
        locked(&self.held).len()
    }
}

/// Closes the cursors of a server session when the session ends.
pub(crate) struct SessionCursors {
    agent: Arc<Agent>,
    session: String,
}

impl SessionCursors {
    pub(crate) fn new(agent: Arc<Agent>, session: String) -> Self {
        Self { agent, session }
    }
}

impl Drop for SessionCursors {
    fn drop(&mut self) {
        self.agent.cursors.close_session(&self.session);
    }
}

fn unknown() -> AgentError {
    AgentError::NotFound(
        "there is no such cursor: it expired, was closed with its session, or belongs to \
         another session. Run the query again."
            .to_owned(),
    )
}

fn new_id() -> String {
    format!("cur_{}", Uuid::new_v4().simple())
}

/// What a page says when a cap of the read-ahead ended the result.
fn cap_note(last: bool) -> String {
    let (rows, ms) = (REST_ROWS, REST_TIME.as_millis());
    let mb = REST_BYTES / (1024 * 1024);
    if last {
        concat!(
            "That was the last row the cursor kept, not the end of the result: the rest was ",
            "not kept. Narrow the query (WHERE, ORDER BY with a key, LIMIT) to read on."
        )
        .to_owned()
    } else {
        format!(
            "The cursor keeps only the rows read ahead past this page (up to {rows} rows, \
             {mb} MB or {ms} ms of reading): the result has more, which it does not keep."
        )
    }
}

/// Renders `result` with the cursor `id` while `later` has rows, putting back into `later`
/// the rows the byte limit left out of the page. With `at_end`, that note goes with the page
/// that takes the last row `later` had. Returns the rendering.
fn render_with_cursor(
    result: &mut QueryResult,
    later: &mut VecDeque<Vec<Cell>>,
    id: &str,
    format: Format,
    at_end: Option<&str>,
) -> String {
    loop {
        result.cursor = (!later.is_empty()).then(|| id.to_owned());
        if let Some(note) = at_end {
            result.notes.retain(|n| n != note);
            if later.is_empty() {
                result.notes.push(note.to_owned());
            }
        }
        let (text, shown) = result.render_counted(format);
        let Outcome::Rows(table) = &mut result.outcome else {
            return text;
        };
        if shown >= table.rows.len() {
            return text;
        }
        for row in table.rows.drain(shown..).rev() {
            later.push_front(row);
        }
    }
}

impl Agent {
    /// [`run_query_until`](Self::run_query_until) rendered as `args.format` says, with a
    /// cursor for the rest of a cut result.
    pub async fn run_query_page(
        &self,
        args: &QueryArgs,
        (origin, session): (&str, &str),
        stop: impl Future<Output = ()>,
    ) -> Result<String> {
        let format = args.format.unwrap_or_default();
        let result = self
            .run_query_with(args, (origin, session), true, stop)
            .await?;
        let (_, saved) = self.find(&args.connection)?;
        Ok(self.paged(result, session, &saved, format))
    }

    /// The first page of `result` as text, and the cursor for the rows after it.
    fn paged(
        &self,
        mut result: QueryResult,
        session: &str,
        saved: &SavedConnection,
        format: Format,
    ) -> String {
        let rest = match &mut result.outcome {
            Outcome::Rows(table) => table.rest.take(),
            Outcome::Affected(_) => None,
        };
        let Some(rest) = rest else {
            return result.render(format);
        };
        if !rest.complete {
            result.notes.push(cap_note(false));
        }
        let id = new_id();
        let mut later: VecDeque<_> = rest.rows.into();
        let text = render_with_cursor(&mut result, &mut later, &id, format, None);
        if let Outcome::Rows(table) = result.outcome
            && !later.is_empty()
        {
            self.cursors.store(Held {
                id,
                session: session.to_owned(),
                connection: saved.id,
                in_clear: saved.agent_values == AgentValues::Shown,
                columns: table.columns,
                rows: later,
                complete: rest.complete,
                masked: table.masked,
                values_cut: table.values_cut,
                used: Instant::now(),
            });
        }
        text
    }

    /// The next rows of the cursor `args.cursor` of `session`.
    pub fn next_page(&self, args: &PageArgs, session: &str) -> Result<String> {
        let start = Instant::now();
        let connection = self
            .cursors
            .connection_of(&args.cursor, session)
            .ok_or_else(unknown)?;
        // The connection's rules count at every page: the access may have changed.
        let (_, saved) = self
            .find(&connection.to_string())
            .inspect_err(|_| self.cursors.forget(&args.cursor))?;
        self.windows.count(&saved)?;
        let requested = Asked {
            max_rows: args.max_rows,
            max_bytes: args.max_bytes,
            timeout_seconds: None,
        };
        let (limits, notes) = guard::clamp(requested.limits()?, &requested, &saved.agent_limits);
        let mut held = self
            .cursors
            .take(&args.cursor, session)
            .ok_or_else(unknown)?;
        if held.in_clear && saved.agent_values != AgentValues::Shown {
            return Err(unknown());
        }
        let page: Vec<_> = held
            .rows
            .drain(..limits.max_rows.min(held.rows.len()))
            .collect();
        let mut result = QueryResult {
            outcome: Outcome::Rows(Table {
                columns: held.columns.clone(),
                rows: page,
                cut: (!held.rows.is_empty()).then_some(Cut::MaxRows),
                values_cut: held.values_cut,
                masked: held.masked.clone(),
                rest: None,
            }),
            elapsed: start.elapsed(),
            limits,
            notes,
            cursor: None,
        };
        let last = cap_note(true);
        let text = render_with_cursor(
            &mut result,
            &mut held.rows,
            &held.id,
            args.format.unwrap_or_default(),
            (!held.complete).then_some(last.as_str()),
        );
        if held.rows.is_empty() {
            return Ok(text);
        }
        self.cursors.store(held);
        Ok(text)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::query::Limits;

    fn held(id: &str, session: &str, rows: usize) -> Held {
        Held {
            id: id.to_owned(),
            session: session.to_owned(),
            connection: Uuid::nil(),
            in_clear: false,
            columns: vec![Column {
                name: "n".to_owned(),
                db_type: "int".to_owned(),
            }],
            rows: (0..rows)
                .map(|i| vec![Cell::Number(i.to_string())])
                .collect(),
            complete: true,
            masked: Vec::new(),
            values_cut: false,
            used: Instant::now(),
        }
    }

    #[test]
    fn a_cursor_answers_only_its_own_session() {
        let cursors = Cursors::default();
        cursors.store(held("cur_a", "s1", 3));
        assert!(cursors.connection_of("cur_a", "s1").is_some());
        assert!(cursors.connection_of("cur_a", "s2").is_none());
        assert!(cursors.connection_of("cur_b", "s1").is_none());
        assert!(cursors.take("cur_a", "s2").is_none());
        assert!(cursors.take("cur_a", "s1").is_some());
        assert!(cursors.take("cur_a", "s1").is_none());
    }

    #[test]
    fn cursors_expire_after_their_keep() {
        let cursors = Cursors {
            held: Mutex::default(),
            keep: Duration::from_millis(20),
        };
        cursors.store(held("cur_a", "s1", 1));
        assert!(cursors.connection_of("cur_a", "s1").is_some());
        std::thread::sleep(Duration::from_millis(40));
        assert!(cursors.connection_of("cur_a", "s1").is_none());
        assert_eq!(cursors.len(), 0);
    }

    #[test]
    fn a_session_keeps_a_few_cursors_and_closing_drops_them() {
        let cursors = Cursors::default();
        for i in 0..PER_SESSION + 3 {
            cursors.store(held(&format!("c{i}"), "s1", 1));
        }
        cursors.store(held("other", "s2", 1));
        assert_eq!(cursors.len(), PER_SESSION + 1);
        // The oldest of the session went.
        assert!(cursors.connection_of("c0", "s1").is_none());
        assert!(cursors.connection_of("c10", "s1").is_some());
        cursors.close_session("s1");
        assert_eq!(cursors.len(), 1);
    }

    #[test]
    fn the_oldest_cursors_go_when_memory_runs_out() {
        let cursors = Cursors::default();
        let rows = TOTAL_BYTES / 25 / 2 + 1;
        cursors.store(held("a", "s1", rows));
        cursors.store(held("b", "s2", rows));
        assert!(cursors.connection_of("a", "s1").is_none());
        assert!(cursors.connection_of("b", "s2").is_some());
    }

    #[test]
    fn the_page_puts_back_the_rows_the_byte_limit_left_out() {
        let table = Table {
            columns: held("x", "s", 0).columns,
            rows: (0..200)
                .map(|i| vec![Cell::Number(i.to_string())])
                .collect(),
            cut: Some(Cut::MaxRows),
            values_cut: false,
            masked: Vec::new(),
            rest: None,
        };
        let mut result = QueryResult {
            outcome: Outcome::Rows(table),
            elapsed: Duration::ZERO,
            limits: Limits {
                max_bytes: 300,
                ..Limits::default()
            },
            notes: Vec::new(),
            cursor: None,
        };
        let mut later = VecDeque::new();
        let text = render_with_cursor(&mut result, &mut later, "cur_x", Format::Text, None);
        assert!(text.len() <= 300, "{}: {text}", text.len());
        assert!(text.contains("cursor \"cur_x\""), "{text}");
        let Outcome::Rows(table) = &result.outcome else {
            unreachable!()
        };
        assert_eq!(table.rows.len() + later.len(), 200);
        assert_eq!(
            later.front(),
            Some(&vec![Cell::Number(table.rows.len().to_string())])
        );
    }
}
