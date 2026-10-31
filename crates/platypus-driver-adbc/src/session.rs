//! The thread that owns an ADBC connection: ADBC calls block, so each connection gets one,
//! and the async side sends it commands.

use std::collections::HashSet;
use std::path::Path;
use std::sync::Arc;
use std::sync::mpsc as std_mpsc;

use adbc_core::options::{
    AdbcVersion, InfoCode, ObjectDepth, OptionConnection, OptionDatabase, OptionValue,
};
use adbc_core::{Connection as _, Database as _, Driver as _, Optionable as _, Statement as _};
use adbc_driver_manager::{ManagedConnection, ManagedDriver};
use arrow_array::RecordBatch;
use arrow_array_59::Array as _;
use arrow_array_59::cast::AsArray as _;
use arrow_schema::SchemaRef;
use platypus_core::{DbError, Error, Result};
use platypus_sql::{Dialect, Lexer, TokenKind};
use tokio::sync::{mpsc, oneshot};

use crate::{ipc, profile, values};

pub enum Command {
    Execute {
        sql: String,
        batch_size: usize,
        reply: oneshot::Sender<Result<Started>>,
    },
    Objects {
        depth: ObjectDepth,
        catalog: Option<String>,
        schema: Option<String>,
        table: Option<String>,
        reply: oneshot::Sender<Result<Vec<RecordBatch>>>,
    },
}

pub enum Started {
    Affected(u64),
    Rows(SchemaRef, mpsc::Receiver<Result<RecordBatch>>),
}

/// What the thread reports once the connection is open.
pub struct Opened {
    /// A handle on the same connection, to cancel from another thread.
    pub cancel: ManagedConnection,
    pub vendor: Option<String>,
    pub version: Option<String>,
    /// The catalog the session starts in, if the driver tells.
    pub catalog: Option<String>,
    pub schema: Option<String>,
}

/// A batch from the driver, in the app's arrow version.
fn convert(
    batch: std::result::Result<arrow_array_59::RecordBatch, impl std::fmt::Display>,
) -> Result<RecordBatch> {
    batch
        .map_err(|e| Error::Unsupported(e.to_string()))
        .and_then(|b| ipc::batch(&b))
}

pub fn adbc_error(e: adbc_core::error::Error) -> Error {
    let state: String = e
        .sqlstate
        .iter()
        .map(|c| u8::try_from(*c).unwrap_or(b'0') as char)
        .collect();
    let code = (state != "00000" && !state.contains('\0')).then_some(state);
    Error::Database(DbError {
        code,
        message: e.message,
        detail: None,
        hint: None,
        position: None,
    })
}

fn database_option(key: &str) -> OptionDatabase {
    match key {
        profile::URI => OptionDatabase::Uri,
        profile::USERNAME => OptionDatabase::Username,
        profile::PASSWORD => OptionDatabase::Password,
        other => OptionDatabase::Other(other.to_owned()),
    }
}

/// Loads the driver and opens a connection with `options`.
fn open(
    library: &Path,
    entrypoint: Option<&str>,
    options: Vec<(String, String)>,
) -> Result<(ManagedConnection, Opened)> {
    let load = |version| {
        ManagedDriver::load_dynamic_from_filename(library, entrypoint.map(str::as_bytes), version)
    };
    // Drivers written for ADBC 1.0 refuse to initialise as 1.1.
    let mut driver = load(AdbcVersion::V110)
        .or_else(|_| load(AdbcVersion::V100))
        .map_err(|e| Error::Connect(format!("cannot load {}: {}", library.display(), e.message)))?;
    let options = options
        .into_iter()
        .map(|(k, v)| (database_option(&k), OptionValue::String(v)));
    let database = driver
        .new_database_with_opts(options)
        .map_err(|e| Error::Connect(e.message))?;
    let connection = database
        .new_connection()
        .map_err(|e| Error::Connect(e.message))?;
    let (vendor, version) = info(&connection);
    let current = |key| {
        connection
            .get_option_string(key)
            .ok()
            .filter(|s| !s.is_empty())
    };
    let opened = Opened {
        cancel: connection.clone(),
        vendor,
        version,
        catalog: current(OptionConnection::CurrentCatalog),
        schema: current(OptionConnection::CurrentSchema),
    };
    Ok((connection, opened))
}

/// The vendor's name and version, when the driver reports them.
fn info(connection: &ManagedConnection) -> (Option<String>, Option<String>) {
    let codes = HashSet::from([InfoCode::VendorName, InfoCode::VendorVersion]);
    let Ok(reader) = connection.get_info(Some(codes)) else {
        return (None, None);
    };
    let (mut vendor, mut version) = (None, None);
    for batch in reader.flatten() {
        let (Some(names), Some(values)) = (
            batch
                .column(0)
                .as_primitive_opt::<arrow_array_59::types::UInt32Type>(),
            batch.column(1).as_union_opt(),
        ) else {
            continue;
        };
        for i in 0..batch.num_rows() {
            let value = values.value(i);
            let Some(text) = value.as_string_opt::<i32>().filter(|t| !t.is_null(0)) else {
                continue;
            };
            let text = text.value(0).to_owned();
            // 0: vendor name, 1: vendor version (ADBC_INFO_VENDOR_*).
            match names.value(i) {
                0 => vendor = Some(text),
                1 => version = Some(text),
                _ => {}
            }
        }
    }
    (vendor, version)
}

/// Starts the thread; answers once the connection is open or has failed.
pub fn spawn(
    library: &Path,
    entrypoint: Option<String>,
    options: Vec<(String, String)>,
) -> Result<(std_mpsc::Sender<Command>, oneshot::Receiver<Result<Opened>>)> {
    let (commands, inbox) = std_mpsc::channel();
    let (ready, opened) = oneshot::channel();
    let library = library.to_owned();
    std::thread::Builder::new()
        .name("platypus-adbc".to_owned())
        .spawn(
            move || match open(&library, entrypoint.as_deref(), options) {
                Ok((mut connection, info)) => {
                    let _ = ready.send(Ok(info));
                    // Ends when the connection, and with it the sender, is dropped.
                    for command in inbox {
                        run(&mut connection, command);
                    }
                }
                Err(e) => {
                    let _ = ready.send(Err(e));
                }
            },
        )
        .map_err(|e| Error::Connect(format!("cannot start the ADBC worker: {e}")))?;
    Ok((commands, opened))
}

fn run(connection: &mut ManagedConnection, command: Command) {
    match command {
        Command::Execute {
            sql,
            batch_size,
            reply,
        } => execute(connection, &sql, batch_size, reply),
        Command::Objects {
            depth,
            catalog,
            schema,
            table,
            reply,
        } => {
            let result = connection
                .get_objects(
                    depth,
                    catalog.as_deref(),
                    schema.as_deref(),
                    table.as_deref(),
                    None,
                    None,
                )
                .map_err(adbc_error)
                .and_then(|reader| reader.map(convert).collect());
            let _ = reply.send(result);
        }
    }
}

/// First words of statements that answer with rows; others report a row count.
const QUERIES: &[&str] = &[
    "SELECT", "WITH", "SHOW", "DESCRIBE", "DESC", "EXPLAIN", "VALUES", "TABLE", "CALL", "LIST",
    "FROM",
];

/// Whether `sql` answers with rows.
pub fn returns_rows(sql: &str) -> bool {
    Lexer::new(sql, Dialect::Ansi)
        .find(|t| t.kind == TokenKind::Word || t.kind == TokenKind::Symbol)
        .is_none_or(|t| {
            // `(SELECT …)`.
            t.kind == TokenKind::Symbol
                || QUERIES.contains(&t.text(sql).to_ascii_uppercase().as_str())
        })
}

/// `sql` without the `;` that ends it, and what follows it: Trino's driver sends the text
/// as one statement, and Trino refuses a terminator ("mismatched input ';'").
pub fn without_terminator(sql: &str) -> &str {
    let last = Lexer::new(sql, Dialect::Ansi)
        .filter(|t| !t.is_trivia())
        .last();
    match last {
        Some(t) if t.kind == TokenKind::Symbol && t.text(sql) == ";" => &sql[..t.start],
        _ => sql,
    }
}

fn execute(
    connection: &mut ManagedConnection,
    sql: &str,
    batch_size: usize,
    reply: oneshot::Sender<Result<Started>>,
) {
    let sql = without_terminator(sql);
    let statement = connection.new_statement().and_then(|mut s| {
        s.set_sql_query(sql)?;
        Ok(s)
    });
    let mut statement = match statement {
        Ok(s) => s,
        Err(e) => {
            let _ = reply.send(Err(adbc_error(e)));
            return;
        }
    };
    if !returns_rows(sql) {
        let rows = statement
            .execute_update()
            .map(|n| Started::Affected(n.and_then(|n| u64::try_from(n).ok()).unwrap_or(0)))
            .map_err(adbc_error);
        let _ = reply.send(rows);
        return;
    }
    let opened = statement
        .execute()
        .map_err(adbc_error)
        .and_then(|mut reader| {
            let schema = values::schema(&ipc::schema(&reader.schema())?);
            // The first batch is read before answering, so its errors fail the statement itself.
            let first = reader
                .next()
                .map(|b| convert(b).and_then(|b| values::batch(b, &schema)))
                .transpose()?;
            Ok((reader, schema, first))
        });
    let (reader, schema, first) = match opened {
        Ok(opened) => opened,
        Err(e) => {
            let _ = reply.send(Err(e));
            return;
        }
    };
    let (tx, rx) = mpsc::channel(1);
    if reply
        .send(Ok(Started::Rows(Arc::clone(&schema), rx)))
        .is_err()
    {
        return;
    }
    let rest = reader.map(|b| convert(b).and_then(|b| values::batch(b, &schema)));
    let batches = first.into_iter().map(Ok).chain(rest);
    for batch in batches {
        let sent = match batch {
            Ok(batch) => send_sliced(&tx, &batch, batch_size),
            Err(e) => {
                let _ = tx.blocking_send(Err(e));
                false
            }
        };
        // Stopped reading: the statement is dropped with the reader, which ends it.
        if !sent {
            return;
        }
    }
}

/// Sends `batch` in pieces of at most `batch_size` rows; `false` once nobody reads.
fn send_sliced(
    tx: &mpsc::Sender<Result<RecordBatch>>,
    batch: &RecordBatch,
    batch_size: usize,
) -> bool {
    let rows = batch.num_rows();
    if rows <= batch_size {
        return tx.blocking_send(Ok(batch.clone())).is_ok();
    }
    (0..rows).step_by(batch_size).all(|offset| {
        tx.blocking_send(Ok(batch.slice(offset, batch_size.min(rows - offset))))
            .is_ok()
    })
}

#[cfg(test)]
mod tests {
    #[test]
    fn statements_with_rows() {
        for sql in [
            "SELECT 1",
            "  with x as (select 1) select * from x",
            "(SELECT 1)",
            "SHOW TABLES",
            "-- c\nselect 1",
        ] {
            assert!(super::returns_rows(sql), "{sql}");
        }
        for sql in [
            "CREATE TABLE t (a int)",
            "insert into t values (1)",
            "DROP TABLE t",
        ] {
            assert!(!super::returns_rows(sql), "{sql}");
        }
    }

    #[test]
    fn terminators_are_left_out() {
        use super::without_terminator as w;
        assert_eq!(w("DROP TABLE \"g\";"), "DROP TABLE \"g\"");
        assert_eq!(w("SELECT 1 ;  -- done\n"), "SELECT 1 ");
        assert_eq!(w("SELECT ';'"), "SELECT ';'");
        assert_eq!(w("SELECT 1"), "SELECT 1");
    }
}
