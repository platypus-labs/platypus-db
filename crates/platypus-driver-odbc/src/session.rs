//! The thread that owns an ODBC connection: ODBC calls block, so each connection gets one,
//! and the async side sends it commands.

use std::sync::mpsc as std_mpsc;

use arrow_array::RecordBatch;
use arrow_odbc::OdbcReaderBuilder;
use arrow_odbc::odbc_api::{
    self, Connection as OdbcConnection, ConnectionOptions, ResultSetMetadata as _,
};
use arrow_schema::{ArrowError, SchemaRef};
use platypus_core::{DbError, Error, Result, TxControl};
use tokio::sync::{mpsc, oneshot};

use crate::meta::{ColumnRow, ForeignKeyRow, KeyRow, TableRow};

/// Text and binary values longer than this are an error rather than a huge buffer: columns
/// without a length (`CLOB`, `VARCHAR(MAX)`) get buffers of this size.
const MAX_VALUE: usize = 64 << 10;
/// The memory a batch's buffers may take; batches of wide rows have fewer rows.
const MAX_BATCH: usize = 256 << 20;

pub enum Command {
    Execute {
        sql: String,
        batch_size: usize,
        reply: oneshot::Sender<Result<Started>>,
    },
    Catalog {
        request: CatalogRequest,
        reply: oneshot::Sender<Result<CatalogRows>>,
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

/// What to read with the catalog functions. Names are matched exactly.
pub enum CatalogRequest {
    Schemas,
    /// The tables and views of a schema, or of every schema.
    Tables {
        schema: Option<String>,
    },
    /// The columns of a table, or of every table of a schema.
    Columns {
        schema: String,
        table: Option<String>,
    },
    PrimaryKey {
        schema: String,
        table: String,
    },
    /// The table's foreign keys.
    ForeignKeys {
        schema: String,
        table: String,
    },
    /// The foreign keys of other tables that point at the table.
    Referencing {
        schema: String,
        table: String,
    },
}

pub enum CatalogRows {
    Schemas(Vec<String>),
    Tables(Vec<TableRow>),
    Columns(Vec<ColumnRow>),
    Keys(Vec<KeyRow>),
    ForeignKeys(Vec<ForeignKeyRow>),
}

/// What the thread reports once the connection is open.
pub struct Opened {
    /// `SQL_DBMS_NAME`: `Oracle`, `DB2/NT64`, `Microsoft SQL Server`.
    pub dbms: Option<String>,
}

/// The message of an ODBC diagnostic, without the `[vendor][driver]` prefixes.
fn message(text: &str) -> String {
    let mut rest = text.trim();
    while let Some(after) = rest.strip_prefix('[').and_then(|r| r.split_once(']')) {
        rest = after.1.trim_start();
    }
    rest.to_owned()
}

pub fn odbc_error(e: odbc_api::Error) -> Error {
    match e {
        odbc_api::Error::Diagnostics { record, .. } => {
            // `State: 42S02, Native error: 942, Message: [Oracle][ODBC]…`.
            let text = record.to_string();
            let text = text.split_once("Message: ").map_or(text.as_str(), |m| m.1);
            Error::Database(DbError {
                code: Some(record.state.as_str().to_owned()),
                message: message(text),
                detail: None,
                hint: None,
                position: None,
            })
        }
        other => Error::Other(other.to_string()),
    }
}

/// An error of `arrow-odbc`, the ODBC diagnostic inside when there is one.
fn arrow_error(e: ArrowError) -> Error {
    match e {
        ArrowError::ExternalError(inner) => match inner.downcast::<odbc_api::Error>() {
            Ok(odbc) => odbc_error(*odbc),
            Err(other) => Error::Other(other.to_string()),
        },
        other => Error::Arrow(other),
    }
}

fn reader_error(e: arrow_odbc::Error) -> Error {
    match e {
        arrow_odbc::Error::UnableToRetrieveNumCols(odbc) => odbc_error(odbc),
        other => Error::Unsupported(other.to_string()),
    }
}

fn open(
    connection_string: &str,
    login_timeout: Option<u32>,
) -> Result<(OdbcConnection<'static>, Opened)> {
    let environment = odbc_api::environment()
        .map_err(|e| Error::Connect(format!("cannot start the ODBC driver manager: {e}")))?;
    let options = ConnectionOptions {
        login_timeout_sec: login_timeout,
        packet_size: None,
    };
    let connection = environment
        .connect_with_connection_string(connection_string, options)
        .map_err(|e| match odbc_error(e) {
            Error::Database(db) => Error::Connect(db.to_string()),
            other => Error::Connect(other.to_string()),
        })?;
    let dbms = connection
        .database_management_system_name()
        .ok()
        .filter(|n| !n.is_empty());
    Ok((connection, Opened { dbms }))
}

/// Starts the thread; answers once the connection is open or has failed.
pub fn spawn(
    connection_string: String,
    login_timeout: Option<u32>,
) -> Result<(std_mpsc::Sender<Command>, oneshot::Receiver<Result<Opened>>)> {
    let (commands, inbox) = std_mpsc::channel();
    let (ready, opened) = oneshot::channel();
    std::thread::Builder::new()
        .name("platypus-odbc".to_owned())
        .spawn(move || match open(&connection_string, login_timeout) {
            Ok((connection, info)) => {
                let mut session = Session {
                    catalog: connection.current_catalog().unwrap_or_default(),
                    dbms: info.dbms.clone(),
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
        .map_err(|e| Error::Connect(format!("cannot start the ODBC worker: {e}")))?;
    Ok((commands, opened))
}

struct Session {
    connection: OdbcConnection<'static>,
    /// The session's database, for drivers with catalogs (SQL Server); empty otherwise.
    catalog: String,
    dbms: Option<String>,
    /// As set by the user; a typed `BEGIN` turns the connection's off for a while.
    autocommit: bool,
    /// A transaction a typed `BEGIN` opened in autocommit mode: autocommit comes back with
    /// its `COMMIT` or `ROLLBACK`.
    implicit: bool,
}

/// Text of a catalog column; drivers return it in the system's code page, nearly always
/// ASCII for names.
fn text(bytes: Option<&[u8]>) -> Option<String> {
    bytes.map(|b| String::from_utf8_lossy(b).into_owned())
}

fn name(bytes: Option<&[u8]>) -> String {
    text(bytes).unwrap_or_default()
}

impl Session {
    fn run(&mut self, command: Command) {
        match command {
            Command::Execute {
                sql,
                batch_size,
                reply,
            } => self.execute(&sql, batch_size, reply),
            Command::Catalog { request, reply } => {
                let _ = reply.send(self.catalog(request).map_err(odbc_error));
            }
            Command::Autocommit { on, reply } => {
                let result = self.connection.set_autocommit(on).map_err(odbc_error);
                if result.is_ok() {
                    self.autocommit = on;
                    self.implicit = false;
                }
                let _ = reply.send(result);
            }
            Command::End { commit, reply } => {
                let _ = reply.send(self.end(commit));
            }
        }
    }

    /// Commits or rolls back through the API, and ends a transaction a `BEGIN` opened.
    fn end(&mut self, commit: bool) -> Result<()> {
        let result = if commit {
            self.connection.commit()
        } else {
            self.connection.rollback()
        };
        if std::mem::take(&mut self.implicit) {
            self.connection.set_autocommit(true).map_err(odbc_error)?;
        }
        result.map_err(odbc_error)
    }

    /// Transaction statements go through the API: drivers differ in how they take them.
    fn control(&mut self, control: TxControl) -> Result<()> {
        match control {
            TxControl::Begin if self.autocommit && !self.implicit => {
                self.connection.set_autocommit(false).map_err(odbc_error)?;
                self.implicit = true;
                Ok(())
            }
            TxControl::Begin => Ok(()),
            TxControl::Commit => self.end(true),
            TxControl::Rollback => self.end(false),
        }
    }

    fn execute(&mut self, sql: &str, batch_size: usize, reply: oneshot::Sender<Result<Started>>) {
        if let Some(c) = TxControl::of(sql) {
            let _ = reply.send(self.control(c).map(|()| Started::Affected(0)));
            return;
        }
        let mut statement = match self.connection.preallocate() {
            Ok(s) => s,
            Err(e) => {
                let _ = reply.send(Err(odbc_error(e)));
                return;
            }
        };
        let mut cursor = match statement.execute(sql, ()) {
            Ok(cursor) => cursor,
            Err(e) => {
                let _ = reply.send(Err(odbc_error(e)));
                return;
            }
        };
        let has_columns = cursor
            .as_mut()
            .is_some_and(|c| c.num_result_cols().unwrap_or(0) > 0);
        let Some(cursor) = cursor.filter(|_| has_columns) else {
            let rows = statement.row_count().ok().flatten().unwrap_or(0);
            let _ = reply.send(Ok(Started::Affected(rows as u64)));
            return;
        };
        let mut builder = OdbcReaderBuilder::new();
        builder
            .with_max_num_rows_per_batch(batch_size)
            .with_max_text_size(MAX_VALUE)
            .with_max_binary_size(MAX_VALUE)
            .with_max_bytes_per_batch(MAX_BATCH);
        if let Some(dbms) = &self.dbms {
            builder.with_dbms_name(dbms.clone());
        }
        let mut reader = match builder.build(cursor) {
            Ok(reader) => reader,
            Err(e) => {
                let _ = reply.send(Err(reader_error(e)));
                return;
            }
        };
        let schema = arrow_array::RecordBatchReader::schema(&reader);
        // The first batch is read before answering, so its errors fail the statement itself.
        let first = match reader.next().transpose() {
            Ok(first) => first,
            Err(e) => {
                let _ = reply.send(Err(arrow_error(e)));
                return;
            }
        };
        let (tx, rx) = mpsc::channel(1);
        if reply.send(Ok(Started::Rows(schema, rx))).is_err() {
            return;
        }
        let batches = first
            .into_iter()
            .map(Ok)
            .chain(reader.map(|b| b.map_err(arrow_error)));
        for batch in batches {
            // Stopped reading: the cursor is dropped with the reader, which closes it.
            if tx.blocking_send(batch).is_err() {
                return;
            }
        }
    }

    /// Reads the catalog. The catalog functions take patterns, where `_` matches any
    /// character: the rows are filtered to the exact names.
    fn catalog(&self, request: CatalogRequest) -> Result<CatalogRows, odbc_api::Error> {
        let c = self.catalog.as_str();
        Ok(match request {
            CatalogRequest::Schemas => {
                // `SQL_ALL_SCHEMAS`: `%` as the schema, every other argument empty.
                let mut names = Vec::new();
                for row in self.connection.tables("", "%", "", "")? {
                    names.extend(text(row?.schema.as_bytes()));
                }
                CatalogRows::Schemas(names)
            }
            CatalogRequest::Tables { schema } => {
                let mut rows = Vec::new();
                let pattern = schema.as_deref().unwrap_or("%");
                for row in self.connection.tables(c, pattern, "%", TABLE_TYPES)? {
                    let row = row?;
                    let table = TableRow {
                        schema: name(row.schema.as_bytes()),
                        name: name(row.table.as_bytes()),
                        kind: name(row.table_type.as_bytes()),
                        remarks: text(row.remarks.as_bytes()).filter(|r| !r.is_empty()),
                    };
                    if schema.as_ref().is_none_or(|s| *s == table.schema) {
                        rows.push(table);
                    }
                }
                CatalogRows::Tables(rows)
            }
            CatalogRequest::Columns { schema, table } => {
                let mut rows = Vec::new();
                let pattern = table.as_deref().unwrap_or("%");
                for row in self.connection.columns(c, &schema, pattern, "%")? {
                    let row = row?;
                    let column = ColumnRow {
                        schema: name(row.schema.as_bytes()),
                        table: name(row.table.as_bytes()),
                        name: name(row.column_name.as_bytes()),
                        sql_type: row.data_type,
                        type_name: name(row.type_name.as_bytes()),
                        size: row.column_size.as_opt().copied(),
                        digits: row.decimal_digits.as_opt().copied(),
                        // `SQL_NO_NULLS` is 0.
                        nullable: row.nullable != 0,
                        default: text(row.column_default.as_bytes()),
                        remarks: text(row.remarks.as_bytes()).filter(|r| !r.is_empty()),
                    };
                    if column.schema == schema && table.as_ref().is_none_or(|t| *t == column.table)
                    {
                        rows.push(column);
                    }
                }
                CatalogRows::Columns(rows)
            }
            CatalogRequest::PrimaryKey { schema, table } => {
                let catalog = (!c.is_empty()).then_some(c);
                let mut rows = Vec::new();
                for row in self
                    .connection
                    .primary_keys(catalog, Some(&schema), &table)?
                {
                    let row = row?;
                    rows.push(KeyRow {
                        column: name(row.column.as_bytes()),
                        seq: row.key_seq,
                        name: text(row.pk_name.as_bytes()),
                    });
                }
                CatalogRows::Keys(rows)
            }
            CatalogRequest::ForeignKeys { schema, table } => {
                CatalogRows::ForeignKeys(self.foreign_keys(("", "", ""), (c, &schema, &table))?)
            }
            CatalogRequest::Referencing { schema, table } => {
                CatalogRows::ForeignKeys(self.foreign_keys((c, &schema, &table), ("", "", ""))?)
            }
        })
    }

    /// `SQLForeignKeys`: the keys of the `fk` table, or those pointing at the `pk` table.
    fn foreign_keys(
        &self,
        pk: (&str, &str, &str),
        fk: (&str, &str, &str),
    ) -> Result<Vec<ForeignKeyRow>, odbc_api::Error> {
        let mut rows = Vec::new();
        for row in self
            .connection
            .foreign_keys(pk.0, pk.1, pk.2, fk.0, fk.1, fk.2)?
        {
            let row = row?;
            rows.push(ForeignKeyRow {
                pk_schema: name(row.pk_schema.as_bytes()),
                pk_table: name(row.pk_table.as_bytes()),
                pk_column: name(row.pk_column.as_bytes()),
                fk_schema: name(row.fk_schema.as_bytes()),
                fk_table: name(row.fk_table.as_bytes()),
                fk_column: name(row.fk_column.as_bytes()),
                seq: row.key_seq,
                name: text(row.fk_name.as_bytes()),
            });
        }
        Ok(rows)
    }
}

/// The table types the navigator lists, in the quoted form of the ODBC specification.
const TABLE_TYPES: &str = "'TABLE','VIEW'";

#[cfg(test)]
mod tests {
    #[test]
    fn strips_vendor_prefixes() {
        assert_eq!(
            super::message("[Oracle][ODBC][Ora]ORA-00942: table or view does not exist\n"),
            "ORA-00942: table or view does not exist"
        );
        assert_eq!(super::message("plain"), "plain");
    }
}
