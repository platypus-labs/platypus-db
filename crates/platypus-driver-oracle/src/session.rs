//! The thread that owns an Oracle connection: the client library's calls block, so each
//! connection gets one, and the async side sends it commands.

use std::sync::Arc;
use std::sync::mpsc as std_mpsc;

use arrow_array::RecordBatch;
use arrow_schema::SchemaRef;
use oracle::{Connection, Connector};
use platypus_core::{Error, Result, TxControl};
use tokio::sync::{mpsc, oneshot};

use crate::convert::Batcher;
use crate::oracle_error;

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
pub struct Login {
    pub user: String,
    pub password: String,
    pub connect_string: String,
}

/// What the thread reports once the connection is open.
pub struct Opened {
    /// The same connection, to cancel its statement from another thread.
    pub cancel: Arc<Connection>,
    pub version: String,
    pub encrypted: bool,
}

fn open(login: &Login) -> Result<(Arc<Connection>, Opened)> {
    let mut connector = Connector::new(&login.user, &login.password, &login.connect_string);
    // No user: the operating system's account, or a wallet, signs in.
    connector.external_auth(login.user.is_empty());
    let connection = connector
        .connect()
        .map_err(|e| Error::Connect(oracle_error(&e).to_string()))?;
    let _ = connection.set_module("PlatypusDB");
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
    let connection = Arc::new(connection);
    let opened = Opened {
        cancel: Arc::clone(&connection),
        version,
        encrypted,
    };
    Ok((connection, opened))
}

/// Starts the thread; answers once the connection is open or has failed.
pub fn spawn(
    login: Login,
) -> Result<(std_mpsc::Sender<Command>, oneshot::Receiver<Result<Opened>>)> {
    let (commands, inbox) = std_mpsc::channel();
    let (ready, opened) = oneshot::channel();
    std::thread::Builder::new()
        .name("platypus-oracle".to_owned())
        .spawn(move || match open(&login) {
            Ok((connection, info)) => {
                let mut session = Session {
                    connection,
                    autocommit: true,
                    implicit: false,
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

    fn execute(&mut self, sql: &str, batch_size: usize, reply: oneshot::Sender<Result<Started>>) {
        if let Some(c) = TxControl::of(sql) {
            let _ = reply.send(self.control(c).map(|()| Started::Affected(0)));
            return;
        }
        let fetch = u32::try_from(batch_size).unwrap_or(u32::MAX);
        let mut statement = match self
            .connection
            .statement(sql)
            .fetch_array_size(fetch)
            .build()
        {
            Ok(s) => s,
            Err(e) => {
                let _ = reply.send(Err(oracle_error(&e)));
                return;
            }
        };
        if !statement.is_query() {
            let committing = self.autocommit && !self.implicit;
            let result = statement
                .execute(&[])
                .and_then(|()| {
                    let rows = if statement.is_dml() {
                        statement.row_count()?
                    } else {
                        0
                    };
                    if committing {
                        self.connection.commit()?;
                    }
                    Ok(rows)
                })
                .map(Started::Affected)
                .map_err(|e| oracle_error(&e));
            let _ = reply.send(result);
            return;
        }
        let mut rows = match statement.query(&[]) {
            Ok(rows) => rows,
            Err(e) => {
                let _ = reply.send(Err(oracle_error(&e)));
                return;
            }
        };
        let mut batcher = match Batcher::new(rows.column_info(), batch_size) {
            Ok(b) => b,
            Err(e) => {
                let _ = reply.send(Err(e));
                return;
            }
        };
        let schema = batcher.schema();
        let mut next_batch = || -> Result<Option<RecordBatch>> {
            while batcher.rows() < batch_size {
                match rows.next() {
                    Some(row) => batcher.push(row.map_err(|e| oracle_error(&e))?.sql_values())?,
                    None => break,
                }
            }
            if batcher.rows() == 0 {
                return Ok(None);
            }
            batcher.finish().map(Some)
        };
        // The first batch is read before answering, so its errors fail the statement itself.
        let first = match next_batch() {
            Ok(first) => first,
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
            batch = match next_batch() {
                Ok(next) => next,
                Err(e) => {
                    let _ = tx.blocking_send(Err(e));
                    return;
                }
            };
        }
    }
}
