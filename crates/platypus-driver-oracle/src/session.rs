//! The thread that owns an Oracle connection: the client library's calls block, so each
//! connection gets one, and the async side sends it commands.

use std::sync::mpsc as std_mpsc;
use std::sync::{Arc, Mutex, PoisonError};

use arrow_array::RecordBatch;
use arrow_schema::SchemaRef;
use oracle::sql_type::{OracleType, ToSql};
use oracle::{Connection, Connector, Privilege, ResultSet, Row, Statement};
use platypus_core::{Error, Result, TxControl};
use platypus_sql::{Dialect, Lexer, TokenKind};
use tokio::sync::{mpsc, oneshot};

use crate::binds::{Lob, lob_binds};
use crate::convert::Batcher;
use crate::oracle_error;
use crate::stop::Busy;

pub enum Command {
    Execute {
        sql: String,
        batch_size: usize,
        reply: oneshot::Sender<Result<Started>>,
    },
    Autocommit {
        on: bool,
        reply: oneshot::Sender<Result<()>>,
    },
    End {
        commit: bool,
        reply: oneshot::Sender<Result<()>>,
    },
}

pub enum Started {
    Affected(u64),
    Rows(SchemaRef, mpsc::Receiver<Result<RecordBatch>>),
}

/// How to open the connection.
#[derive(Clone)]
pub struct Login {
    pub user: String,
    pub password: String,
    pub connect_string: String,
    /// An administrative privilege to connect with (`SYSDBA`, `SYSOPER`…).
    pub privilege: Option<Privilege>,
}

/// What the server printed with `DBMS_OUTPUT`, until the connection takes it.
pub type Messages = Arc<Mutex<Vec<String>>>;

/// What the thread reports once the connection is open.
pub struct Opened {
    /// The same connection, to cancel its statement from another thread.
    pub cancel: Arc<Connection>,
    pub version: String,
    pub encrypted: bool,
    /// The statement the session waits on the server for.
    pub busy: Busy,
    /// The session's `SID` and `SERIAL#`, to stop its statement from another session.
    pub ids: Option<(u64, u64)>,
}

fn open(login: &Login, busy: Busy) -> Result<(Arc<Connection>, Opened)> {
    let mut connector = Connector::new(&login.user, &login.password, &login.connect_string);
    // No user: the operating system's account, or a wallet, signs in.
    connector.external_auth(login.user.is_empty());
    if let Some(privilege) = login.privilege {
        connector.privilege(privilege);
    }
    let connection = connector
        .connect()
        .map_err(|e| Error::Connect(oracle_error(&e).to_string()))?;
    let _ = connection.set_module("PlatypusDB");
    // What PL/SQL prints is kept for the messages, as SQL*Plus' `SET SERVEROUTPUT ON` does.
    let _ = connection.execute("BEGIN DBMS_OUTPUT.ENABLE(NULL); END;", &[]);
    let version = connection
        .server_version()
        .map(|(v, _)| v.to_string())
        .unwrap_or_default();
    let encrypted = connection
        .query_row_as::<String>(
            "SELECT SYS_CONTEXT('USERENV', 'NETWORK_PROTOCOL') FROM DUAL",
            &[],
        )
        .is_ok_and(|p| p.eq_ignore_ascii_case("tcps"));
    // `DBMS_DEBUG_JDWP` tells every session its own ids; `V$SESSION` needs a grant.
    let ids = connection
        .query_row_as::<(u64, u64)>(
            "SELECT DBMS_DEBUG_JDWP.CURRENT_SESSION_ID, DBMS_DEBUG_JDWP.CURRENT_SESSION_SERIAL \
             FROM DUAL",
            &[],
        )
        .ok();
    let connection = Arc::new(connection);
    let opened = Opened {
        cancel: Arc::clone(&connection),
        version,
        encrypted,
        busy,
        ids,
    };
    Ok((connection, opened))
}

/// Starts the thread; answers once the connection is open or has failed.
pub fn spawn(
    login: Login,
    messages: Messages,
) -> Result<(std_mpsc::Sender<Command>, oneshot::Receiver<Result<Opened>>)> {
    let (commands, inbox) = std_mpsc::channel();
    let (ready, opened) = oneshot::channel();
    let busy = Busy::default();
    std::thread::Builder::new()
        .name("platypus-oracle".to_owned())
        .spawn(move || match open(&login, busy.clone()) {
            Ok((connection, info)) => {
                let mut session = Session {
                    connection,
                    autocommit: true,
                    implicit: false,
                    messages,
                    busy,
                    executed: 0,
                };
                let _ = ready.send(Ok(info));
                // Ends when the connection, and with it the sender, is dropped.
                for command in inbox {
                    session.run(command);
                }
            }
            Err(e) => {
                let _ = ready.send(Err(e));
            }
        })
        .map_err(|e| Error::Connect(format!("cannot start the Oracle worker: {e}")))?;
    Ok((commands, opened))
}

/// The connection's own autocommit mode cannot change while the canceller shares it: the
/// session commits after each change instead, in autocommit mode.
struct Session {
    connection: Arc<Connection>,
    /// As set by the user.
    autocommit: bool,
    /// A transaction a typed `BEGIN` opened in autocommit mode, until its `COMMIT` or
    /// `ROLLBACK`.
    implicit: bool,
    messages: Messages,
    busy: Busy,
    /// Statements run so far: the number of the running one, for [`Busy`].
    executed: u64,
}

/// The binds of [`lob_binds`] as the client library takes them.
fn lob_params(lobs: &[(String, Lob)]) -> Vec<(&str, Box<dyn ToSql + '_>)> {
    lobs.iter()
        .map(|(name, lob)| -> (&str, Box<dyn ToSql + '_>) {
            match lob {
                Lob::Clob(text) => (name, Box::new((text, &OracleType::CLOB))),
                Lob::Blob(bytes) => (name, Box::new((bytes, &OracleType::BLOB))),
            }
        })
        .collect()
}

impl Session {
    fn run(&mut self, command: Command) {
        match command {
            Command::Execute {
                sql,
                batch_size,
                reply,
            } => self.execute(&sql, batch_size, reply),
            Command::Autocommit { on, reply } => {
                self.autocommit = on;
                self.implicit = false;
                let _ = reply.send(Ok(()));
            }
            Command::End { commit, reply } => {
                let _ = reply.send(self.end(commit));
            }
        }
    }

    fn end(&mut self, commit: bool) -> Result<()> {
        let result = if commit {
            self.connection.commit()
        } else {
            self.connection.rollback()
        };
        self.implicit = false;
        result.map_err(|e| oracle_error(&e))
    }

    /// Oracle has no `BEGIN` statement (it starts a PL/SQL block): a transaction starts
    /// with the first change, and commits through the client.
    fn control(&mut self, control: TxControl) -> Result<()> {
        match control {
            TxControl::Begin => {
                self.implicit = self.autocommit;
                Ok(())
            }
            TxControl::Commit => self.end(true),
            TxControl::Rollback => self.end(false),
        }
    }

    fn statement(&self, sql: &str, batch_size: usize) -> oracle::Result<Statement> {
        let mut builder = self.connection.statement(sql);
        builder.fetch_array_size(u32::try_from(batch_size).unwrap_or(u32::MAX));
        builder.build()
    }

    fn execute(&mut self, sql: &str, batch_size: usize, reply: oneshot::Sender<Result<Started>>) {
        if let Some(c) = TxControl::of(sql) {
            let _ = reply.send(self.control(c).map(|()| Started::Affected(0)));
            return;
        }
        self.executed += 1;
        let id = self.executed;
        let (text, lobs) = lob_binds(sql);
        let boxed = lob_params(&lobs);
        let params: Vec<(&str, &dyn ToSql)> = boxed.iter().map(|(n, v)| (*n, v.as_ref())).collect();
        let mut statement = match self.statement(&text, batch_size) {
            Ok(s) => s,
            Err(e) => {
                let _ = reply.send(Err(oracle_error(&e)));
                return;
            }
        };
        if statement.is_query() {
            self.stream(&text, &mut statement, &params, batch_size, id, reply);
            // The rows a `SELECT … FOR UPDATE` locked stay locked until a commit.
            if self.autocommit && !self.implicit && locks_rows(sql) {
                let _ = self.connection.commit();
            }
            return;
        }
        let committing = self.autocommit && !self.implicit;
        let result = self
            .busy
            .call(id, || statement.execute_named(&params))
            .map_err(|e| oracle_error(&e))
            .and_then(|()| self.compiled(sql))
            .and_then(|()| {
                let rows = if statement.is_dml() {
                    statement.row_count().map_err(|e| oracle_error(&e))?
                } else {
                    0
                };
                if committing {
                    self.connection.commit().map_err(|e| oracle_error(&e))?;
                }
                Ok(rows)
            })
            .map(Started::Affected);
        if statement.is_plsql() {
            self.read_output();
        }
        // Oracle commits around DDL: a transaction a `BEGIN` opened is over.
        if statement.is_ddl() {
            self.implicit = false;
        }
        let _ = reply.send(result);
    }

    /// The compilation errors of the routine, package, trigger or type `sql` created, when
    /// Oracle created it invalid ("success with compilation error", ORA-24344, a warning).
    fn compiled(&self, sql: &str) -> Result<()> {
        let Some(warning) = self.connection.last_warning() else {
            return Ok(());
        };
        if warning.db_error().is_none_or(|e| e.code() != 24344) {
            return Ok(());
        }
        let object = crate::plsql::created_object(sql);
        let errors = object
            .as_ref()
            .and_then(|o| self.connection.query(&o.errors_sql(), &[]).ok())
            .map(|rows| {
                rows.filter_map(std::result::Result::ok)
                    .filter_map(|r| {
                        Some(crate::plsql::CompileError {
                            line: r.get(0).ok()?,
                            column: r.get(1).ok()?,
                            text: r.get(2).ok()?,
                            error: r.get::<_, String>(3).ok()? == "ERROR",
                        })
                    })
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        if let Some(e) = crate::plsql::compile_error(sql, object.as_ref(), &errors) {
            return Err(e);
        }
        // Warnings only (`PLW-`): kept with the messages.
        let mut messages = self.messages.lock().unwrap_or_else(PoisonError::into_inner);
        messages.extend(errors.iter().map(crate::plsql::CompileError::line_text));
        Ok(())
    }

    /// Moves what the statement printed with `DBMS_OUTPUT` to the messages.
    fn read_output(&self) {
        const SQL: &str = "DECLARE l VARCHAR2(32767); s INTEGER; r CLOB; BEGIN LOOP \
             DBMS_OUTPUT.GET_LINE(l, s); EXIT WHEN s <> 0; r := r || l || CHR(10); END LOOP; \
             :r := r; END;";
        let Ok(mut statement) = self.connection.statement(SQL).build() else {
            return;
        };
        if statement.bind(1, &OracleType::CLOB).is_err() || statement.execute(&[]).is_err() {
            return;
        }
        let Ok(Some(text)) = statement.bind_value::<_, Option<String>>(1) else {
            return;
        };
        let mut messages = self.messages.lock().unwrap_or_else(PoisonError::into_inner);
        messages.extend(
            text.strip_suffix('\n')
                .unwrap_or(&text)
                .split('\n')
                .map(str::to_owned),
        );
    }

    /// Answers with the rows of a query, read in batches while the receiver takes them.
    fn stream(
        &self,
        sql: &str,
        statement: &mut Statement,
        params: &[(&str, &dyn ToSql)],
        batch_size: usize,
        id: u64,
        reply: oneshot::Sender<Result<Started>>,
    ) {
        let reading = Reading {
            busy: &self.busy,
            id,
            batch_size,
        };
        // A file locator fails only when its first row is read: the query runs again first.
        let failed = match self.busy.call(id, || statement.query_named(params)) {
            Ok(rows)
                if !rows
                    .column_info()
                    .iter()
                    .any(|c| *c.oracle_type() == OracleType::BFILE) =>
            {
                return reading.send_rows(rows, &[], reply);
            }
            Ok(_) => None,
            Err(e) => Some(e),
        };
        let Some(retry) = crate::read_as::retry(sql, failed.as_ref(), |q| self.describe(q)) else {
            let error = failed.map_or_else(
                || Error::Other("cannot read the query's columns".to_owned()),
                |e| oracle_error(&e),
            );
            let _ = reply.send(Err(error));
            return;
        };
        let mut again = match self.statement(&retry.sql, batch_size) {
            Ok(s) => s,
            Err(e) => {
                let _ = reply.send(Err(oracle_error(&e)));
                return;
            }
        };
        match self.busy.call(id, || again.query_named(params)) {
            Ok(rows) => reading.send_rows(rows, &retry.labels, reply),
            Err(e) => {
                let _ = reply.send(Err(oracle_error(&e)));
            }
        }
    }

    /// The columns of query `sql` as `DBMS_SQL` describes them: type code and name. It
    /// describes the types the client library cannot read (`JSON`, `VECTOR`).
    fn describe(&self, sql: &str) -> Option<Vec<(i32, String)>> {
        let mut statement = self
            .connection
            .statement(crate::read_as::DESCRIBE)
            .build()
            .ok()?;
        statement.bind("q", &(&sql, &OracleType::CLOB)).ok()?;
        statement.bind("r", &OracleType::CLOB).ok()?;
        statement.execute(&[]).ok()?;
        let text: Option<String> = statement.bind_value("r").ok()?;
        Some(crate::read_as::described(&text.unwrap_or_default()))
    }
}

/// How the rows of statement `id` are read: in batches of `batch_size`, each fetch a call
/// to the server ([`Busy`]).
struct Reading<'a> {
    busy: &'a Busy,
    id: u64,
    batch_size: usize,
}

impl Reading<'_> {
    /// Answers with `rows`, read in batches while the receiver takes them; the columns of
    /// `labels` read through a conversion.
    fn send_rows(
        &self,
        mut rows: ResultSet<'_, Row>,
        labels: &[(usize, &str)],
        reply: oneshot::Sender<Result<Started>>,
    ) {
        let batch_size = self.batch_size;
        let mut batcher = match Batcher::new(rows.column_info(), batch_size) {
            Ok(b) => b,
            Err(e) => {
                let _ = reply.send(Err(e));
                return;
            }
        };
        for &(index, label) in labels {
            batcher.label(index, label);
        }
        // The first batch is read before answering, so its errors fail the statement itself,
        // and its rows settle the types left to them.
        let first = self.next_batch(&mut rows, &mut batcher);
        let started = first.and_then(|first| Ok((first, batcher.schema()?)));
        let (first, schema) = match started {
            Ok(started) => started,
            Err(e) => {
                let _ = reply.send(Err(e));
                return;
            }
        };
        let (tx, rx) = mpsc::channel(1);
        if reply.send(Ok(Started::Rows(schema, rx))).is_err() {
            return;
        }
        let mut batch = first;
        while let Some(b) = batch {
            // Stopped reading: the statement is dropped with the result, which ends it.
            if tx.blocking_send(Ok(b)).is_err() {
                return;
            }
            batch = match self.next_batch(&mut rows, &mut batcher) {
                Ok(next) => next,
                Err(e) => {
                    let _ = tx.blocking_send(Err(e));
                    return;
                }
            };
        }
    }

    /// The next batch of `rows`; `None` at their end.
    fn next_batch(
        &self,
        rows: &mut ResultSet<'_, Row>,
        batcher: &mut Batcher,
    ) -> Result<Option<RecordBatch>> {
        self.busy.call(self.id, || {
            while batcher.rows() < self.batch_size {
                match rows.next() {
                    Some(row) => batcher.push(row.map_err(|e| oracle_error(&e))?.sql_values())?,
                    None => break,
                }
            }
            Ok::<_, Error>(())
        })?;
        if batcher.rows() == 0 {
            return Ok(None);
        }
        batcher.finish().map(Some)
    }
}

/// Whether a query locks the rows it reads (`FOR UPDATE`).
fn locks_rows(sql: &str) -> bool {
    let words: Vec<String> = Lexer::new(sql, Dialect::Oracle)
        .filter(|t| t.kind == TokenKind::Word)
        .map(|t| t.text(sql).to_ascii_uppercase())
        .collect();
    words.windows(2).any(|w| w[0] == "FOR" && w[1] == "UPDATE")
}

#[cfg(test)]
mod tests {
    #[test]
    fn queries_that_lock_rows() {
        assert!(super::locks_rows(
            "select * from emp where id = 1 for update nowait"
        ));
        assert!(!super::locks_rows("select 'for update' from dual"));
        assert!(!super::locks_rows("select * from emp"));
    }
}
