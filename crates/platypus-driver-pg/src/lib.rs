//! PostgreSQL driver built on `tokio-postgres`.
//!
//! Rows are fetched in the binary protocol and decoded straight into Arrow builders, one
//! batch at a time: the next batch is only read from the socket when the consumer polls for
//! it, so memory stays flat regardless of result size.
//!
//! TLS follows libpq `sslmode` semantics, see [`tls`].

mod admin;
#[doc(hidden)]
pub mod columns;
mod cursor;
mod ddl;
#[doc(hidden)]
pub mod decode;
mod explain;
pub use explain::read_plan;
mod meta;
mod tls;

use std::pin::Pin;
use std::sync::Arc;
use std::sync::PoisonError;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::Duration;

use arrow_array::{RecordBatch, RecordBatchOptions};
use arrow_schema::Schema;
use async_trait::async_trait;
use futures::StreamExt;
use platypus_core::{
    CancelHandle, Canceller, CatalogColumn, Connection, ConnectionConfig, DbError, Driver,
    DriverKind, Error, ExecOptions, ExecResult, Executed, ObjectMeta, RecordBatchStream,
    ReferencingKey, Result, SchemaMeta, ServerInfo, TableDetails, TransactionState, TxEffect,
    TxTracker, classify, execute_each,
};
use tokio::sync::{Mutex, OwnedMutexGuard};
use tokio_postgres::error::{ErrorPosition, SqlState};
use tokio_postgres::tls::MakeTlsConnect;
use tokio_postgres::types::{FromSql, ToSql, Type};
use tokio_postgres::{
    AsyncMessage, CancelToken, Client, NoTls, RowStream, SimpleQueryMessage, Socket,
};
use tokio_postgres_rustls::MakeRustlsConnect;

use crate::columns::{ColumnBuilder, column_builder};

#[derive(Debug, Default, Clone, Copy)]
pub struct PgDriver;

#[async_trait]
impl Driver for PgDriver {
    fn kind(&self) -> DriverKind {
        DriverKind::Postgres
    }

    async fn connect(&self, config: &ConnectionConfig) -> Result<Box<dyn Connection>> {
        let conn: Box<dyn Connection> = Box::new(PgConnection::connect(config).await?);
        platypus_core::run_init(&*conn, &config.init, self.kind()).await?;
        Ok(conn)
    }
}

#[derive(Debug)]
pub struct PgConnection {
    client: Arc<Client>,
    canceller: PgCanceller,
    server: ServerInfo,
    /// Held for the lifetime of a statement, including its result stream, so a new
    /// statement never starts while an abandoned one is still being cancelled.
    busy: Arc<Mutex<()>>,
    tx: TxTracker,
    /// Names cursors, which carry results inside transactions.
    next_cursor: AtomicU64,
    /// A statement's [`SAVEPOINT`] is still set (its rows were on their way).
    savepoint: AtomicBool,
    /// The notices the server sent, until `take_messages`.
    notices: Notices,
    /// Behind a pooler in transaction mode (`PgBouncer`, Supavisor, Neon's), where each
    /// transaction may get another server session: statements run unnamed, prepared and
    /// run in one round trip, outside a transaction of the session's own.
    unnamed_statements: bool,
}

/// Whether `sql` is a `COPY` to or from the client (`STDOUT`, `STDIN`), a protocol of its own.
fn copies_through_client(sql: &str) -> bool {
    let words: Vec<String> = platypus_sql::Lexer::new(sql, platypus_sql::Dialect::Postgres)
        .filter(|t| t.kind == platypus_sql::TokenKind::Word)
        .map(|t| t.text(sql).to_ascii_uppercase())
        .collect();
    words.first().is_some_and(|w| w == "COPY")
        && words.iter().any(|w| w == "STDOUT" || w == "STDIN")
}

/// What manual-commit mode sets before each statement.
const SAVEPOINT: &str = "SAVEPOINT platypus_statement";

/// The notices (`RAISE NOTICE`, warnings) of a session, as its connection task reads them.
type Notices = Arc<std::sync::Mutex<Vec<String>>>;

/// Whether `sql` sets, releases or rolls back to a savepoint of the user's, which a
/// savepoint of the app's around it would disturb.
fn names_savepoints(sql: &str) -> bool {
    matches!(
        platypus_core::leading_words(sql, 1)
            .first()
            .map(String::as_str),
        Some("SAVEPOINT" | "RELEASE" | "ROLLBACK")
    )
}

/// The server speaking the protocol, from `version()`: CockroachDB and YugabyteDB name
/// themselves there. CockroachDB's `server_version` is the PostgreSQL release it mimics,
/// so its own release replaces it.
fn product(full: Option<&str>, server_version: String) -> (&'static str, String) {
    let full = full.unwrap_or_default();
    if full.starts_with("CockroachDB") {
        let own = full
            .split(' ')
            .find(|w| w.starts_with('v'))
            .map(str::to_owned);
        ("CockroachDB", own.unwrap_or(server_version))
    } else if server_version.contains("-YB-") {
        ("YugabyteDB", server_version)
    } else {
        ("PostgreSQL", server_version)
    }
}

/// Whether `conn` is CockroachDB, which lacks some of PostgreSQL's catalog functions.
pub(crate) fn is_cockroach(conn: &dyn Connection) -> bool {
    conn.server_info().product == "CockroachDB"
}

/// Whether `conn` is YugabyteDB, whose keys and tables are sharded.
pub(crate) fn is_yugabyte(conn: &dyn Connection) -> bool {
    conn.server_info().product == "YugabyteDB"
}

impl PgConnection {
    pub async fn connect(config: &ConnectionConfig) -> Result<Self> {
        if config.driver != DriverKind::Postgres {
            return Err(Error::Config(format!(
                "{:?} is not a PostgreSQL config",
                config.driver
            )));
        }
        let mut pg = tokio_postgres::Config::new();
        // Through a tunnel the socket goes to its local end; `host` still names the server
        // for TLS. Ports add up in tokio-postgres, so there is one call.
        let port = config
            .connect_addr
            .map_or(config.port.unwrap_or(5432), |a| a.port());
        pg.host(&config.host)
            .port(port)
            .dbname(&config.database)
            .user(&config.user);
        if let Some(addr) = config.connect_addr {
            pg.hostaddr(addr.ip());
        }
        if let Some(password) = &config.password {
            pg.password(password.expose());
        }
        if let Some(name) = &config.application_name {
            pg.application_name(name);
        }
        if let Some(timeout) = config.connect_timeout {
            pg.connect_timeout(timeout);
        }
        let (ssl_mode, tls) = tls::connector(config)?;
        pg.ssl_mode(ssl_mode);

        let notices = Notices::default();
        let client = match &tls {
            Some(tls) => spawn_connection(&pg, tls.clone(), Arc::clone(&notices)).await?,
            None => spawn_connection(&pg, NoTls, Arc::clone(&notices)).await?,
        };

        let row = client
            .simple_query(
                "SELECT current_setting('server_version'), \
                 coalesce((SELECT ssl FROM pg_stat_ssl WHERE pid = pg_backend_pid()), false), \
                 version()",
            )
            .await
            .map_err(map_error)?
            .into_iter()
            .find_map(|m| match m {
                SimpleQueryMessage::Row(row) => Some(row),
                _ => None,
            });
        let version = row
            .as_ref()
            .and_then(|r| r.get(0))
            .unwrap_or_default()
            .to_owned();
        let encrypted = row.as_ref().and_then(|r| r.get(1)) == Some("t");
        let (product, version) = product(row.as_ref().and_then(|r| r.get(2)), version);

        Ok(Self {
            canceller: PgCanceller {
                token: client.cancel_token(),
                tls,
            },
            client: Arc::new(client),
            server: ServerInfo {
                product: product.to_owned(),
                version,
                encrypted,
            },
            busy: Arc::new(Mutex::new(())),
            tx: TxTracker::default(),
            next_cursor: AtomicU64::new(1),
            savepoint: AtomicBool::new(false),
            notices,
            unnamed_statements: config
                .options
                .get(platypus_core::POOLER)
                .is_some_and(|mode| mode.eq_ignore_ascii_case("transaction")),
        })
    }
}

async fn spawn_connection<T>(
    config: &tokio_postgres::Config,
    tls: T,
    notices: Notices,
) -> Result<Client>
where
    T: MakeTlsConnect<Socket> + Send + 'static,
    T::Stream: Send + 'static,
{
    let (client, connection) = config
        .connect(tls)
        .await
        .map_err(|e| match e.as_db_error() {
            Some(_) => map_error(e),
            None => Error::Connect(error_chain(&e)),
        })?;
    // The connection drives the socket, keeping the notices it reads; it ends when the
    // client is dropped. A notice is read before the end of the statement that raised it.
    let mut connection = connection;
    tokio::spawn(async move {
        let mut messages = futures::stream::poll_fn(move |cx| connection.poll_message(cx));
        while let Some(Ok(message)) = messages.next().await {
            if let AsyncMessage::Notice(notice) = message {
                notices
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .push(format!("{}: {}", notice.severity(), notice.message()));
            }
        }
    });
    Ok(client)
}

#[async_trait]
impl Connection for PgConnection {
    /// In manual-commit mode each statement runs under a savepoint: one that fails is undone
    /// alone, instead of aborting the transaction and with it the changes made before it
    /// (which `COMMIT` would then roll back).
    async fn execute(&self, sql: &str, options: &ExecOptions) -> Result<ExecResult> {
        let effect = classify(sql);
        let guard = Arc::clone(&self.busy).lock_owned().await;
        let wrap =
            !self.tx.state().autocommit && effect == TxEffect::None && !names_savepoints(sql);
        let begin = self.tx.needs_begin(effect);
        let mut prelude = Vec::new();
        if begin {
            prelude.push("BEGIN");
        }
        if wrap {
            prelude.push(SAVEPOINT);
        }
        self.settle_savepoint(&prelude).await?;
        if begin {
            self.tx.set_open(true);
        }
        let result = self.execute_locked(sql, options, guard).await;
        let result = match result {
            Err(error) if wrap => Err(self.undo_statement(error).await),
            // A result set may still fail as its rows come: its savepoint stays until the
            // next statement or the end of the transaction, which release it.
            Ok(result) => {
                if wrap {
                    self.savepoint.store(true, Ordering::Release);
                }
                Ok(result)
            }
            Err(error) => Err(error),
        };
        self.tx.after(effect, false);
        result
    }

    fn take_messages(&self) -> Vec<String> {
        std::mem::take(&mut *self.notices.lock().unwrap_or_else(PoisonError::into_inner))
    }

    /// The statements in simple queries of up to [`SIMPLE_QUERY_BYTES`]: one round trip for
    /// many of them, where [`execute`](Self::execute) takes two per statement (prepare, then
    /// run). Statements with a backslash are joined only while `standard_conforming_strings`
    /// is on: with it off, `\'` in a plain literal would end the literal, and what follows
    /// would run as a statement of its own.
    async fn execute_all(&self, statements: &[&str], expected: &[u64]) -> Executed {
        // An empty query would still open a transaction in manual-commit mode.
        if statements.is_empty() || statements.iter().any(|s| classify(s) != TxEffect::None) {
            return execute_each(self, statements, expected).await;
        }
        let guard = self.busy.lock().await;
        // The data editor's own savepoints must not sit inside a statement's.
        if let Err(error) = self.settle_savepoint(&[]).await {
            return Executed {
                rows: Vec::new(),
                error: Some(error),
            };
        }
        if statements.iter().any(|s| s.contains('\\')) && !self.conforming_strings().await {
            drop(guard);
            return execute_each(self, statements, expected).await;
        }
        if self.tx.needs_begin(TxEffect::None) {
            if let Err(e) = self.client.batch_execute("BEGIN").await {
                return Executed {
                    rows: Vec::new(),
                    error: Some(map_error(e)),
                };
            }
            self.tx.set_open(true);
        }
        let mut rows = Vec::with_capacity(statements.len());
        let mut error = None;
        for group in simple_groups(statements) {
            let before = rows.len();
            let sql = statements[group.clone()].join(";\n");
            if let Err(e) = self.simple_counts(&sql, &mut rows).await {
                error = Some(map_error(e));
                break;
            }
            // A statement the server split differently would shift every count after it.
            if rows.len() != group.end {
                error = Some(Error::Unsupported(format!(
                    "the server ran {} statements instead of {}",
                    rows.len() - before,
                    group.len()
                )));
                break;
            }
            let off = rows[before..]
                .iter()
                .zip(expected.iter().skip(before))
                .any(|(n, e)| n != e);
            if off {
                break;
            }
        }
        self.tx.after(TxEffect::None, false);
        Executed { rows, error }
    }

    fn cancel_handle(&self) -> CancelHandle {
        CancelHandle::new(self.canceller.clone())
    }

    fn server_info(&self) -> &ServerInfo {
        &self.server
    }

    fn is_closed(&self) -> bool {
        self.client.is_closed()
    }

    async fn schemas(&self) -> Result<Vec<SchemaMeta>> {
        meta::schemas(self).await
    }

    async fn objects(&self, schema: &str) -> Result<Vec<ObjectMeta>> {
        meta::objects(self, schema).await
    }

    async fn table_details(&self, schema: &str, table: &str) -> Result<TableDetails> {
        meta::table_details(self, schema, table).await
    }

    async fn referencing_keys(&self, schema: &str, table: &str) -> Result<Vec<ReferencingKey>> {
        meta::referencing_keys(self, schema, table).await
    }

    async fn catalog_columns(&self) -> Result<Vec<CatalogColumn>> {
        meta::catalog_columns(self).await
    }

    async fn catalog_foreign_keys(&self) -> Result<Vec<platypus_core::CatalogForeignKey>> {
        meta::catalog_foreign_keys(self).await
    }

    async fn explain(&self, sql: &str, analyze: bool) -> Result<platypus_core::Explain> {
        if is_cockroach(self) {
            return explain::cockroach(self, sql, analyze).await;
        }
        explain::explain(self, sql, analyze).await
    }

    async fn object_ddl(&self, object: &platypus_core::ObjectRef<'_>) -> Result<String> {
        ddl::object_ddl(self, object).await
    }

    async fn object_properties(
        &self,
        object: &platypus_core::ObjectRef<'_>,
    ) -> Result<Vec<platypus_core::Property>> {
        ddl::object_properties(self, object).await
    }

    async fn databases(&self) -> Result<Vec<String>> {
        ddl::databases(self).await
    }

    async fn search_metadata(&self, needle: &str) -> Result<Vec<platypus_core::MetadataMatch>> {
        meta::search_metadata(self, needle).await
    }

    async fn server_sessions(&self) -> Result<Vec<platypus_core::ServerSession>> {
        self.server_admin()?;
        admin::sessions(self).await
    }

    async fn server_locks(&self) -> Result<Vec<platypus_core::ServerLock>> {
        self.server_admin()?;
        admin::locks(self).await
    }

    async fn end_server_session(&self, id: &str, how: platypus_core::EndSession) -> Result<()> {
        self.server_admin()?;
        admin::end(self, id, how).await
    }

    fn transaction(&self) -> TransactionState {
        self.tx.state()
    }

    async fn set_autocommit(&self, on: bool) -> Result<()> {
        self.tx.ensure_closed()?;
        self.tx.set_autocommit(on);
        Ok(())
    }

    async fn commit(&self) -> Result<()> {
        self.end_transaction("COMMIT").await
    }

    async fn rollback(&self) -> Result<()> {
        self.end_transaction("ROLLBACK").await
    }

    fn capabilities(&self) -> platypus_core::Capabilities {
        platypus_core::Capabilities {
            transactions: true,
            cancel: true,
            // CockroachDB's plans are text trees of their own (`explain::cockroach`).
            explain: true,
            object_ddl: true,
            metadata_search: true,
            multiple_databases: true,
            server_sessions: self.has_server_admin(),
            cancel_server_queries: self.has_server_admin(),
            // CockroachDB's COPY has limits of its own in a transaction: rows go as INSERTs.
            bulk_load: !is_cockroach(self),
        }
    }

    /// `COPY … FROM STDIN` in CSV, the rows sent in chunks, in the session's transaction (one
    /// opened in manual-commit mode, as a statement would).
    /// `COPY … FROM STDIN`, in the session's transaction; in manual-commit mode under a
    /// savepoint, as a statement, so that a refused load leaves the transaction usable.
    async fn bulk_load(&self, rows: &platypus_core::BulkRows<'_>) -> Result<u64> {
        if !self.capabilities().bulk_load {
            return Err(platypus_core::unsupported("bulk loading"));
        }
        let _guard = self.busy.lock().await;
        let wrap = !self.tx.state().autocommit;
        let begin = self.tx.needs_begin(TxEffect::None);
        let mut prelude = Vec::new();
        if begin {
            prelude.push("BEGIN");
        }
        if wrap {
            prelude.push(SAVEPOINT);
        }
        self.settle_savepoint(&prelude).await?;
        if begin {
            self.tx.set_open(true);
        }
        match self.copy_rows(rows).await {
            Err(error) if wrap => Err(self.undo_statement(error).await),
            Ok(loaded) => {
                if wrap {
                    self.savepoint.store(true, Ordering::Release);
                }
                Ok(loaded)
            }
            Err(error) => Err(error),
        }
    }
}

impl PgConnection {
    /// Sends `rows` with `COPY`, a chunk at a time. Called with the session's lock held.
    async fn copy_rows(&self, rows: &platypus_core::BulkRows<'_>) -> Result<u64> {
        use futures::SinkExt;

        let sql = format!(
            "COPY {} ({}) FROM STDIN WITH (FORMAT csv)",
            rows.table,
            rows.columns.join(", ")
        );
        let sink = self.client.copy_in(sql.as_str()).await.map_err(map_error)?;
        futures::pin_mut!(sink);
        let mut chunk = String::new();
        for row in rows.rows {
            copy_csv_row(&mut chunk, row, rows.hex);
            if chunk.len() >= COPY_CHUNK_BYTES {
                let data = bytes::Bytes::from(std::mem::take(&mut chunk));
                sink.send(data).await.map_err(map_error)?;
            }
        }
        if !chunk.is_empty() {
            sink.send(bytes::Bytes::from(chunk))
                .await
                .map_err(map_error)?;
        }
        sink.finish().await.map_err(map_error)
    }
}

/// How much CSV a `COPY` sends at a time.
const COPY_CHUNK_BYTES: usize = 1 << 20;

/// One CSV line of `COPY`: every value quoted, so that an empty text is `""` and NULL the
/// empty field; bytes as `\x` hex.
fn copy_csv_row(out: &mut String, row: &[Option<String>], hex: &[bool]) {
    for (i, value) in row.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        let Some(value) = value else { continue };
        out.push('"');
        if hex.get(i).copied().unwrap_or(false) {
            out.push_str("\\x");
        }
        for c in value.chars() {
            if c == '"' {
                out.push('"');
            }
            out.push(c);
        }
        out.push('"');
    }
    out.push('\n');
}

impl PgConnection {
    /// The columns of `sql`, from a statement prepared in a transaction of its own: behind a
    /// pooler in transaction mode, a name prepared outside one may be gone (or on another
    /// server session) by the time it runs. Rolled back whatever happens.
    async fn describe(&self, sql: &str) -> Result<tokio_postgres::Statement> {
        let mut retried = false;
        loop {
            self.client
                .batch_execute("BEGIN")
                .await
                .map_err(map_error)?;
            let described = self.client.prepare(sql).await;
            let ended = self.client.batch_execute("ROLLBACK").await;
            match described {
                // The client looks types up with statements it prepared once, in an earlier
                // transaction: on another server session, or a reset one, they are gone.
                // Prepared again, in this one.
                Err(e) if !retried && e.code() == Some(&SqlState::INVALID_SQL_STATEMENT_NAME) => {
                    retried = true;
                    self.client.clear_type_cache();
                }
                described => {
                    let statement = described.map_err(map_error)?;
                    ended.map_err(map_error)?;
                    return Ok(statement);
                }
            }
        }
    }

    /// Whether the server lists its sessions and locks as PostgreSQL does: CockroachDB has
    /// no `pg_blocking_pids` nor `pg_terminate_backend`, YugabyteDB keeps its locks
    /// elsewhere.
    fn has_server_admin(&self) -> bool {
        !is_cockroach(self) && !is_yugabyte(self)
    }

    fn server_admin(&self) -> Result<()> {
        if self.has_server_admin() {
            Ok(())
        } else {
            Err(platypus_core::unsupported("server sessions"))
        }
    }

    /// Whether `standard_conforming_strings` is on: backslashes in plain literals are
    /// themselves. Called with the session's lock held.
    async fn conforming_strings(&self) -> bool {
        self.client
            .simple_query("SHOW standard_conforming_strings")
            .await
            .ok()
            .and_then(|messages| {
                messages.into_iter().find_map(|m| match m {
                    SimpleQueryMessage::Row(row) => row.get(0).map(|v| v == "on"),
                    _ => None,
                })
            })
            .unwrap_or(false)
    }

    /// Runs `sql` as a simple query, adding the rows each of its statements changed to
    /// `rows`; on an error, `rows` holds the counts of the statements before it.
    async fn simple_counts(
        &self,
        sql: &str,
        rows: &mut Vec<u64>,
    ) -> std::result::Result<(), tokio_postgres::Error> {
        let stream = self.client.simple_query_raw(sql).await?;
        futures::pin_mut!(stream);
        while let Some(message) = stream.next().await.transpose()? {
            if let SimpleQueryMessage::CommandComplete(n) = message {
                rows.push(n);
            }
        }
        Ok(())
    }

    /// Releases the savepoint a statement left set, then runs `prelude`. A statement whose
    /// rows failed after the first ones aborted the transaction: it is rolled back to its
    /// savepoint, so the changes before it stay. Called with the session's lock held.
    async fn settle_savepoint(&self, prelude: &[&str]) -> Result<()> {
        let set = self.savepoint.swap(false, Ordering::AcqRel);
        if !set {
            if prelude.is_empty() {
                return Ok(());
            }
            return self
                .client
                .batch_execute(&prelude.join("; "))
                .await
                .map_err(map_error);
        }
        let release = "RELEASE SAVEPOINT platypus_statement";
        let sql = std::iter::once(release)
            .chain(prelude.iter().copied())
            .collect::<Vec<_>>()
            .join("; ");
        if self.client.batch_execute(&sql).await.is_ok() {
            return Ok(());
        }
        self.client
            .batch_execute(
                "ROLLBACK TO SAVEPOINT platypus_statement; RELEASE SAVEPOINT platypus_statement",
            )
            .await
            .map_err(map_error)?;
        if prelude.is_empty() {
            return Ok(());
        }
        self.client
            .batch_execute(&prelude.join("; "))
            .await
            .map_err(map_error)
    }

    /// Rolls back the statement that failed with `error` to its savepoint, and says so.
    async fn undo_statement(&self, error: Error) -> Error {
        let undone = self
            .client
            .batch_execute(
                "ROLLBACK TO SAVEPOINT platypus_statement; RELEASE SAVEPOINT platypus_statement",
            )
            .await;
        match (error, undone) {
            (Error::Database(mut db), Ok(())) => {
                let note = "Only this statement was undone: the transaction stays open with the changes made before it.";
                db.hint = Some(match db.hint.take() {
                    Some(hint) => format!("{hint}\n{note}"),
                    None => note.to_owned(),
                });
                Error::Database(db)
            }
            (error, _) => error,
        }
    }

    async fn end_transaction(&self, sql: &str) -> Result<()> {
        let _guard = self.busy.lock().await;
        if !self.tx.state().open {
            return Ok(());
        }
        // A statement that failed after its first rows is undone before the commit, which
        // would otherwise roll everything back.
        if sql == "COMMIT" {
            self.settle_savepoint(&[]).await?;
        }
        self.savepoint.store(false, Ordering::Release);
        let result = self.client.batch_execute(sql).await.map_err(map_error);
        // Even a failed COMMIT ends the transaction in PostgreSQL.
        self.tx.set_open(false);
        result
    }

    async fn execute_locked(
        &self,
        sql: &str,
        options: &ExecOptions,
        guard: OwnedMutexGuard<()>,
    ) -> Result<ExecResult> {
        let open = self.tx.state().open;
        if (open || options.paged) && cursor::eligible(sql) {
            let name = format!(
                "platypus_cursor_{}",
                self.next_cursor.fetch_add(1, Ordering::Relaxed)
            );
            return cursor::open(Arc::clone(&self.client), &name, sql, options, guard, !open).await;
        }
        if copies_through_client(sql) {
            return Err(Error::Unsupported(
                "COPY ... TO STDOUT and FROM STDIN, which stream through the client: export \
                 the query's rows, import a file, or COPY to or from a file on the server"
                    .to_owned(),
            ));
        }
        // A pooler in transaction mode keeps one server session for a transaction only: the
        // statement is described in a transaction of its own, then run unnamed.
        let unnamed = self.unnamed_statements && !open;
        let statement = if unnamed {
            self.describe(sql).await?
        } else {
            self.client.prepare(sql).await.map_err(map_error)?
        };

        // `SELECT` without columns has rows all the same: no columns is no result only
        // for the other statements.
        let query = matches!(
            platypus_core::leading_words(sql, 1)
                .first()
                .map(String::as_str),
            Some("SELECT" | "VALUES" | "TABLE")
        );
        if statement.columns().is_empty() && !query {
            let rows = if unnamed {
                self.client.execute_typed(sql, &[]).await
            } else {
                self.client.execute_raw(&statement, no_params()).await
            }
            .map_err(map_error)?;
            return Ok(ExecResult::Affected { rows });
        }

        let (fields, mut builders): (Vec<_>, Vec<_>) = statement
            .columns()
            .iter()
            .map(column_builder)
            .collect::<Result<Vec<_>, _>>()?
            .into_iter()
            .unzip();
        columns::load_names(&self.client, &mut builders).await;
        let schema = Arc::new(Schema::new(fields));
        let rows = if unnamed {
            self.client.query_typed_raw(sql, no_typed_params()).await
        } else {
            self.client.query_raw(&statement, no_params()).await
        }
        .map_err(map_error)?;

        let state = BatchState {
            rows: Some(Box::pin(rows)),
            builders,
            schema: Arc::clone(&schema),
            batch_size: options.batch_size.max(1),
            canceller: self.canceller.clone(),
            guard: Some(guard),
        };
        let stream = futures::stream::try_unfold(state, next_batch);
        Ok(ExecResult::Rows(RecordBatchStream::new(
            schema,
            stream.boxed(),
        )))
    }
}

/// Most bytes of SQL in one simple query of [`PgConnection::execute_all`]: a protocol
/// message holds at most 1 GiB.
const SIMPLE_QUERY_BYTES: usize = 64 << 20;

/// `statements` in consecutive runs of up to [`SIMPLE_QUERY_BYTES`], a statement longer
/// than that in a run of its own.
fn simple_groups(statements: &[&str]) -> Vec<std::ops::Range<usize>> {
    let mut groups = Vec::new();
    let (mut start, mut bytes) = (0, 0);
    for (i, sql) in statements.iter().enumerate() {
        if i > start && bytes + sql.len() > SIMPLE_QUERY_BYTES {
            groups.push(start..i);
            (start, bytes) = (i, 0);
        }
        bytes += sql.len() + 2;
    }
    groups.push(start..statements.len());
    groups
}

fn no_params() -> std::iter::Empty<&'static (dyn ToSql + Sync)> {
    std::iter::empty()
}

fn no_typed_params() -> std::iter::Empty<(&'static (dyn ToSql + Sync), Type)> {
    std::iter::empty()
}

struct BatchState {
    /// `None` once the server has sent the whole result (or an error).
    rows: Option<Pin<Box<RowStream>>>,
    builders: Vec<ColumnBuilder>,
    schema: Arc<Schema>,
    batch_size: usize,
    canceller: PgCanceller,
    guard: Option<OwnedMutexGuard<()>>,
}

impl BatchState {
    fn finish(&mut self) {
        self.rows = None;
        self.guard = None;
    }
}

/// Accepts any column type and hands back the raw wire bytes, decoded by [`ColumnBuilder`].
struct Raw<'a>(&'a [u8]);

impl<'a> FromSql<'a> for Raw<'a> {
    fn from_sql(
        _: &Type,
        raw: &'a [u8],
    ) -> std::result::Result<Self, Box<dyn std::error::Error + Sync + Send>> {
        Ok(Raw(raw))
    }

    fn accepts(_: &Type) -> bool {
        true
    }
}

/// Decodes one row into the column builders.
pub(crate) fn append_row(
    builders: &mut [ColumnBuilder],
    schema: &Schema,
    row: &tokio_postgres::Row,
) -> Result<()> {
    for (i, builder) in builders.iter_mut().enumerate() {
        let value = row.try_get::<_, Option<Raw<'_>>>(i).map_err(map_error)?;
        builder.append(value.map(|r| r.0)).map_err(|e| {
            let field = schema.field(i);
            Error::Decode {
                column: field.name().clone(),
                db_type: field
                    .metadata()
                    .get(platypus_core::META_DB_TYPE)
                    .cloned()
                    .unwrap_or_default(),
                message: e.to_string(),
            }
        })?;
    }
    Ok(())
}

async fn next_batch(mut state: BatchState) -> Result<Option<(RecordBatch, BatchState)>> {
    let Some(rows) = state.rows.as_mut() else {
        return Ok(None);
    };
    let mut len = 0;
    while len < state.batch_size {
        let row = match rows.next().await {
            Some(Ok(row)) => row,
            Some(Err(e)) => {
                state.finish();
                return Err(map_error(e));
            }
            None => {
                state.finish();
                break;
            }
        };
        append_row(&mut state.builders, &state.schema, &row)?;
        len += 1;
    }
    if len == 0 {
        return Ok(None);
    }
    let columns = state
        .builders
        .iter_mut()
        .map(ColumnBuilder::finish)
        .collect();
    // A result without columns (`SELECT FROM t`) still has its rows.
    let options = RecordBatchOptions::new().with_row_count(Some(len));
    let batch = RecordBatch::try_new_with_options(Arc::clone(&state.schema), columns, &options)?;
    Ok(Some((batch, state)))
}

/// How long an abandoned result may drain on its own before it is cancelled.
const DRAIN_BEFORE_CANCEL: Duration = Duration::from_millis(100);

impl Drop for BatchState {
    /// An abandoned result is drained in the background, and cancelled server-side only
    /// if it is still running after a moment; the connection stays busy until it is done.
    ///
    /// A cancel request reaches the backend as a signal, asynchronously: one sent for a
    /// query that had already finished could land on the next statement and cancel it. Most
    /// abandoned results (a paused page of rows) have finished, so they are only drained.
    fn drop(&mut self) {
        let (Some(mut rows), Some(guard)) = (self.rows.take(), self.guard.take()) else {
            return;
        };
        let Ok(runtime) = tokio::runtime::Handle::try_current() else {
            return;
        };
        let canceller = self.canceller.clone();
        runtime.spawn(async move {
            let drained = tokio::time::timeout(DRAIN_BEFORE_CANCEL, async {
                while rows.next().await.is_some() {}
            })
            .await;
            if drained.is_err() {
                let _ = canceller.cancel().await;
                while rows.next().await.is_some() {}
            }
            drop(guard);
        });
    }
}

/// Cancel requests open a new connection, so they need the same TLS setup as the session.
#[derive(Clone)]
struct PgCanceller {
    token: CancelToken,
    tls: Option<MakeRustlsConnect>,
}

impl std::fmt::Debug for PgCanceller {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PgCanceller")
            .field("tls", &self.tls.is_some())
            .finish_non_exhaustive()
    }
}

#[async_trait]
impl Canceller for PgCanceller {
    async fn cancel(&self) -> Result<()> {
        let result = match &self.tls {
            Some(tls) => self.token.cancel_query(tls.clone()).await,
            None => self.token.cancel_query(NoTls).await,
        };
        result.map_err(|e| Error::Connect(format!("cancel request failed: {e}")))
    }
}

#[expect(
    clippy::needless_pass_by_value,
    reason = "used directly as a `map_err` callback"
)]
fn map_error(e: tokio_postgres::Error) -> Error {
    if let Some(db) = e.as_db_error() {
        if *db.code() == SqlState::QUERY_CANCELED {
            return Error::Cancelled;
        }
        return Error::Database(DbError {
            code: Some(db.code().code().to_owned()),
            message: db.message().to_owned(),
            detail: db.detail().map(str::to_owned),
            hint: db.hint().map(str::to_owned),
            position: match db.position() {
                Some(ErrorPosition::Original(p)) => Some(*p),
                _ => None,
            },
        });
    }
    if e.is_closed() {
        return Error::Closed;
    }
    Error::Other(error_chain(&e))
}

/// `tokio-postgres` errors keep the useful part (e.g. the TLS failure) in their sources.
fn error_chain(e: &dyn std::error::Error) -> String {
    let mut message = e.to_string();
    let mut source = e.source();
    while let Some(cause) = source {
        message.push_str(": ");
        message.push_str(&cause.to_string());
        source = cause.source();
    }
    message
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn copies_rows_as_csv() {
        let mut out = String::new();
        let rows = [
            vec![Some("1".to_owned()), None, Some(String::new())],
            vec![
                Some("a \"b\",\nc".to_owned()),
                Some("ff00".to_owned()),
                Some("\\".to_owned()),
            ],
        ];
        for row in &rows {
            copy_csv_row(&mut out, row, &[false, true, false]);
        }
        assert_eq!(out, "\"1\",,\"\"\n\"a \"\"b\"\",\nc\",\"\\xff00\",\"\\\"\n");
    }

    #[test]
    fn tells_statements_the_savepoints_must_leave_alone() {
        assert!(names_savepoints("savepoint a"));
        assert!(names_savepoints("RELEASE SAVEPOINT a"));
        assert!(names_savepoints("rollback to a"));
        assert!(!names_savepoints("insert into t values (1)"));
        assert!(!names_savepoints("select 'savepoint'"));
    }

    #[test]
    fn finds_copies_through_the_client() {
        assert!(copies_through_client("COPY (SELECT 1) TO STDOUT"));
        assert!(copies_through_client("copy t from stdin with csv"));
        assert!(!copies_through_client("COPY t TO '/tmp/t.csv'"));
        assert!(!copies_through_client("SELECT 'stdout'"));
    }
}
