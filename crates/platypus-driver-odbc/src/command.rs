//! What the async side of a connection asks of its worker, and what the worker answers: the
//! worker is the thread that owns an ODBC connection (`session`) or the one that talks to a
//! JDBC bridge process (`jdbc`).

use arrow_array::RecordBatch;
use arrow_schema::SchemaRef;
use platypus_core::Result;
use tokio::sync::{mpsc, oneshot};

use crate::meta::{ColumnRow, ForeignKeyRow, KeyRow, TableRow};

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
