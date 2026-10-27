//! Oracle Database through Oracle's client library (`oracle`, on ODPI-C), which is loaded at
//! run time: the Instant Client the app downloads (`platypus-drivers`), or one the system
//! has. Each connection runs on a thread of its own; a running statement is cancelled with
//! a break on the same connection, and metadata comes from the `ALL_*` views.

use std::fmt::Write as _;
use std::path::PathBuf;
use std::sync::mpsc as std_mpsc;
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use futures::StreamExt as _;
use platypus_core::{
    CATALOG_LIMIT, CancelHandle, Canceller, Capabilities, CatalogColumn, CatalogForeignKey,
    Connection, ConnectionConfig, DbError, Driver, DriverKind, Error, ExecOptions, ExecResult,
    ObjectMeta, ObjectRef, RecordBatchStream, ReferencingKey, Result, SchemaMeta, ServerInfo,
    SslMode, TableDetails, TextRowExt as _, TransactionState, TxControl, TxTracker, query_text,
};
use tokio::sync::oneshot;

mod binds;
mod client;
mod convert;
mod explain;
mod meta;
mod plsql;
mod read_as;
mod session;

use session::{Command, Login, Messages, Started};

/// Opens Oracle connections with the client library of the app's data folder, else the
/// system's.
#[derive(Debug, Clone)]
pub struct OracleDriver {
    pub data_dir: PathBuf,
}

impl OracleDriver {
    /// With the drivers of the app's data folder (`platypus_drivers::set_data_dir`).
    pub fn shared() -> Result<Self> {
        platypus_drivers::data_dir()
            .map(|dir| Self {
                data_dir: dir.to_owned(),
            })
            .ok_or_else(|| Error::Config("the drivers folder is not set".to_owned()))
    }
}

/// An error of the client library as the app's: database errors keep their `ORA-` code and
/// the position Oracle points at; a break is a cancellation.
pub(crate) fn oracle_error(e: &oracle::Error) -> Error {
    let Some(db) = e.db_error() else {
        return Error::Other(e.to_string());
    };
    if db.code() == 1013 {
        return Error::Cancelled;
    }
    let message = db.message().to_owned();
    let code = message
        .split_once(':')
        .map(|(c, _)| c.trim().to_owned())
        .filter(|c| c.starts_with("ORA-") || c.starts_with("DPI-") || c.starts_with("PLS-"));
    Error::Database(DbError {
        message: code
            .as_ref()
            .and_then(|c| message.strip_prefix(c.as_str()))
            .map_or(message.clone(), |m| {
                m.trim_start_matches(':').trim().to_owned()
            }),
        code,
        detail: None,
        hint: None,
        // Oracle counts from 0; the app from 1.
        position: (db.offset() > 0).then(|| db.offset() + 1),
    })
}

/// `e` with its position counted in characters of `sql`, as the editor counts: Oracle
/// counts bytes.
fn in_characters(mut e: Error, sql: &str) -> Error {
    if let Error::Database(db) = &mut e
        && let Some(position) = db.position
    {
        let byte = usize::try_from(position - 1)
            .unwrap_or(usize::MAX)
            .min(sql.len());
        let chars = sql.char_indices().take_while(|(i, _)| *i < byte).count();
        db.position = u32::try_from(chars + 1).ok();
    }
    e
}

/// The connect string of `config`: Easy Connect (`host:port/service`, `tcps://` with TLS,
/// the local end of the SSH tunnel when there is one) when a host is given, else the
/// database field as written (a net service name or a connect descriptor).
pub fn connect_string(config: &ConnectionConfig) -> Result<String> {
    let host = config.host.trim();
    let database = config.database.trim();
    if host.is_empty() {
        return if database.is_empty() {
            Err(Error::Config(
                "the Oracle host, or a net service name, is required".to_owned(),
            ))
        } else {
            Ok(database.to_owned())
        };
    }
    let tls = !matches!(config.ssl_mode, SslMode::Disable | SslMode::Prefer);
    let mut out = String::new();
    if tls {
        out.push_str("tcps://");
    }
    // Through an SSH tunnel: its local end.
    match config.connect_addr {
        Some(addr) => {
            let _ = write!(out, "{addr}");
        }
        None if host.contains(':') => {
            let _ = write!(out, "[{host}]:{}", config.port.unwrap_or(1521));
        }
        None => {
            let _ = write!(out, "{host}:{}", config.port.unwrap_or(1521));
        }
    }
    if !database.is_empty() {
        out.push('/');
        out.push_str(database);
    }
    let mut params = Vec::new();
    if let Some(timeout) = config.connect_timeout {
        params.push(format!("connect_timeout={}", timeout.as_secs().max(1)));
    }
    if tls {
        // The system's certificates, as the other drivers trust them.
        params.push("wallet_location=system".to_owned());
        if config.ssl_mode == SslMode::Require {
            params.push("ssl_server_dn_match=false".to_owned());
        }
    }
    if !params.is_empty() {
        out.push('?');
        out.push_str(&params.join("&"));
    }
    Ok(out)
}

/// The statement as Oracle takes it: a final `;` is refused after plain SQL and needed after
/// PL/SQL.
#[must_use]
pub fn statement_text(sql: &str) -> &str {
    if platypus_sql::is_plsql(sql) {
        sql
    } else {
        sql.trim_end().trim_end_matches(';').trim_end()
    }
}

/// The administrative privileges a session can connect with, as SQL*Plus names them.
const PRIVILEGES: &[(&str, oracle::Privilege)] = &[
    ("SYSDBA", oracle::Privilege::Sysdba),
    ("SYSOPER", oracle::Privilege::Sysoper),
    ("SYSASM", oracle::Privilege::Sysasm),
    ("SYSBACKUP", oracle::Privilege::Sysbackup),
    ("SYSDG", oracle::Privilege::Sysdg),
    ("SYSKM", oracle::Privilege::Syskm),
    ("SYSRAC", oracle::Privilege::Sysrac),
];

/// The privilege of a role name; none for an empty one or `NORMAL`.
fn privilege(name: &str) -> Result<Option<oracle::Privilege>> {
    let name = name.trim();
    if name.is_empty() || name.eq_ignore_ascii_case("normal") {
        return Ok(None);
    }
    PRIVILEGES
        .iter()
        .find(|(n, _)| n.eq_ignore_ascii_case(name))
        .map(|(_, p)| Some(*p))
        .ok_or_else(|| Error::Config(format!("unknown Oracle role {name}: SYSDBA, SYSOPER…")))
}

/// The user and the administrative privilege of a login: the user written as SQL*Plus takes
/// it (`sys as sysdba`), else the connection's `role` option.
fn login_role(user: &str, role: Option<&String>) -> Result<(String, Option<oracle::Privilege>)> {
    let words: Vec<&str> = user.split_whitespace().collect();
    if let [name, by, role] = words.as_slice()
        && by.eq_ignore_ascii_case("as")
    {
        return Ok(((*name).to_owned(), privilege(role)?));
    }
    Ok((
        user.trim().to_owned(),
        privilege(role.map_or("", String::as_str))?,
    ))
}

#[async_trait]
impl Driver for OracleDriver {
    fn kind(&self) -> DriverKind {
        DriverKind::Oracle
    }

    async fn connect(&self, config: &ConnectionConfig) -> Result<Box<dyn Connection>> {
        let dir = self.data_dir.clone();
        tokio::task::spawn_blocking(move || client::load(&dir))
            .await
            .map_err(|e| Error::Connect(e.to_string()))??;
        let conn: Box<dyn Connection> = Box::new(OracleConnection::open(config).await?);
        platypus_core::run_init(&*conn, &config.init, config.driver).await?;
        Ok(conn)
    }
}

pub struct OracleConnection {
    commands: Mutex<std_mpsc::Sender<Command>>,
    /// What PL/SQL printed (`DBMS_OUTPUT`), until [`Connection::take_messages`].
    messages: Messages,
    canceller: OracleCanceller,
    server: ServerInfo,
    dictionary: meta::Dictionary,
    tx: TxTracker,
}

impl std::fmt::Debug for OracleConnection {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OracleConnection")
            .field("server", &self.server)
            .finish_non_exhaustive()
    }
}

impl OracleConnection {
    pub async fn open(config: &ConnectionConfig) -> Result<Self> {
        let (user, privilege) = login_role(&config.user, config.options.get("role"))?;
        let login = Login {
            user,
            password: config
                .password
                .as_ref()
                .map(|p| p.expose().to_owned())
                .unwrap_or_default(),
            connect_string: connect_string(config)?,
            privilege,
        };
        let messages = Messages::default();
        let (commands, opened) = session::spawn(login, Arc::clone(&messages))?;
        let opened = opened.await.map_err(|_| Error::Closed)??;
        Ok(Self {
            dictionary: meta::Dictionary::of(&opened.version),
            commands: Mutex::new(commands),
            messages,
            canceller: OracleCanceller {
                connection: opened.cancel,
            },
            server: ServerInfo {
                product: "Oracle".to_owned(),
                version: opened.version,
                encrypted: opened.encrypted,
            },
            tx: TxTracker::default(),
        })
    }

    fn send(&self, command: Command) -> Result<()> {
        self.commands
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .send(command)
            .map_err(|_| Error::Closed)
    }

    /// Sends the command made with `reply` and waits for the worker's answer.
    async fn request<T>(
        &self,
        command: impl FnOnce(oneshot::Sender<Result<T>>) -> Command,
    ) -> Result<T> {
        let (reply, answer) = oneshot::channel();
        self.send(command(reply))?;
        answer.await.map_err(|_| Error::Closed)?
    }

    /// Commits or rolls back through the client.
    async fn end(&self, commit: bool) -> Result<()> {
        self.request(|reply| Command::End { commit, reply }).await?;
        self.tx.set_open(false);
        Ok(())
    }
}

#[async_trait]
impl Connection for OracleConnection {
    async fn execute(&self, sql: &str, options: &ExecOptions) -> Result<ExecResult> {
        let command = plsql::command(sql);
        let sql = command.as_deref().unwrap_or_else(|| statement_text(sql));
        let (reply, started) = oneshot::channel();
        self.send(Command::Execute {
            sql: sql.to_owned(),
            batch_size: options.batch_size.max(1),
            reply,
        })?;
        let started = started.await.map_err(|_| Error::Closed)?;
        // Oracle commits before and after DDL, in either mode.
        self.tx.after(TxControl::effect(sql), true);
        match started.map_err(|e| in_characters(e, sql))? {
            Started::Affected(rows) => Ok(ExecResult::Affected { rows }),
            Started::Rows(schema, mut rx) => {
                let stream = futures::stream::poll_fn(move |cx| rx.poll_recv(cx));
                Ok(ExecResult::Rows(RecordBatchStream::new(
                    schema,
                    stream.boxed(),
                )))
            }
        }
    }

    fn cancel_handle(&self) -> CancelHandle {
        CancelHandle::new(self.canceller.clone())
    }

    fn server_info(&self) -> &ServerInfo {
        &self.server
    }

    fn take_messages(&self) -> Vec<String> {
        std::mem::take(
            &mut *self
                .messages
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner),
        )
    }

    /// `EXPLAIN PLAN`, read from the plan table. Oracle cannot run the statement for its
    /// actual rows and times here: with `analyze` the plan comes without them.
    async fn explain(&self, sql: &str, _analyze: bool) -> Result<platypus_core::Explain> {
        explain::explain(self, statement_text(sql)).await
    }

    fn is_closed(&self) -> bool {
        !matches!(
            self.canceller.connection.status(),
            Ok(oracle::ConnStatus::Normal)
        )
    }

    async fn schemas(&self) -> Result<Vec<SchemaMeta>> {
        Ok(meta::schemas(
            &query_text(self, &self.dictionary.schemas_sql()).await?,
        ))
    }

    async fn objects(&self, schema: &str) -> Result<Vec<ObjectMeta>> {
        Ok(meta::objects(
            &query_text(self, &meta::objects_sql(schema)).await?,
        ))
    }

    async fn table_details(&self, schema: &str, table: &str) -> Result<TableDetails> {
        let columns = query_text(self, &meta::columns_sql(schema, table)).await?;
        let constraints = query_text(self, &meta::constraints_sql(schema, table)).await?;
        let indexes = query_text(self, &meta::indexes_sql(schema, table)).await?;
        let referencing = query_text(self, &meta::referencing_sql(schema, table)).await?;
        Ok(meta::table_details(
            &columns,
            &constraints,
            &indexes,
            &referencing,
        ))
    }

    async fn referencing_keys(&self, schema: &str, table: &str) -> Result<Vec<ReferencingKey>> {
        Ok(meta::referencing_keys(
            &query_text(self, &meta::referencing_sql(schema, table)).await?,
        ))
    }

    async fn catalog_columns(&self) -> Result<Vec<CatalogColumn>> {
        Ok(meta::catalog_columns(
            &query_text(self, &self.dictionary.catalog_columns_sql(CATALOG_LIMIT)).await?,
        ))
    }

    async fn catalog_foreign_keys(&self) -> Result<Vec<CatalogForeignKey>> {
        Ok(meta::catalog_foreign_keys(
            &query_text(self, &self.dictionary.catalog_foreign_keys_sql()).await?,
        ))
    }

    async fn object_ddl(&self, object: &ObjectRef<'_>) -> Result<String> {
        let sql = meta::ddl_sql(object, self.dictionary.legacy)
            .ok_or_else(|| platypus_core::unsupported("DDL of this kind of object"))?;
        let rows = query_text(self, &sql).await?;
        Ok(rows
            .first()
            .map(|r| r.text(0).trim().to_owned())
            .unwrap_or_default())
    }

    fn transaction(&self) -> TransactionState {
        self.tx.state()
    }

    async fn set_autocommit(&self, on: bool) -> Result<()> {
        self.tx.ensure_closed()?;
        self.request(|reply| Command::Autocommit { on, reply })
            .await?;
        self.tx.set_autocommit(on);
        Ok(())
    }

    async fn commit(&self) -> Result<()> {
        self.end(true).await
    }

    async fn rollback(&self) -> Result<()> {
        self.end(false).await
    }

    fn capabilities(&self) -> Capabilities {
        Capabilities {
            transactions: true,
            cancel: true,
            object_ddl: true,
            explain: true,
            ..Capabilities::default()
        }
    }
}

/// Breaks the statement running on the connection, from another thread: it fails with
/// ORA-01013 and the session stays usable.
#[derive(Clone)]
struct OracleCanceller {
    connection: Arc<oracle::Connection>,
}

#[async_trait]
impl Canceller for OracleCanceller {
    async fn cancel(&self) -> Result<()> {
        let connection = Arc::clone(&self.connection);
        tokio::task::spawn_blocking(move || connection.break_execution())
            .await
            .map_err(|e| Error::Other(e.to_string()))?
            .map_err(|e| oracle_error(&e))
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    #[test]
    fn easy_connect_strings() {
        let mut c = ConnectionConfig::new(DriverKind::Oracle);
        c.database = "FREEPDB1".to_owned();
        c.ssl_mode = SslMode::Prefer;
        c.connect_timeout = Some(Duration::from_secs(10));
        assert_eq!(
            connect_string(&c).unwrap(),
            "localhost:1521/FREEPDB1?connect_timeout=10"
        );
        c.host = "::1".to_owned();
        c.port = Some(51521);
        c.connect_timeout = None;
        c.ssl_mode = SslMode::Require;
        assert_eq!(
            connect_string(&c).unwrap(),
            "tcps://[::1]:51521/FREEPDB1?wallet_location=system&ssl_server_dn_match=false"
        );
        c.ssl_mode = SslMode::VerifyFull;
        assert!(
            connect_string(&c)
                .unwrap()
                .ends_with("?wallet_location=system")
        );
        // Without a host, the database field is a net service name or a descriptor.
        c.host.clear();
        c.database = "SALES_HIGH".to_owned();
        assert_eq!(connect_string(&c).unwrap(), "SALES_HIGH");
        c.database.clear();
        assert!(matches!(connect_string(&c), Err(Error::Config(_))));
        // Through a tunnel.
        c.host = "db.internal".to_owned();
        c.database = "ORCL".to_owned();
        c.ssl_mode = SslMode::Disable;
        c.connect_addr = Some("127.0.0.1:40001".parse().unwrap());
        assert_eq!(connect_string(&c).unwrap(), "127.0.0.1:40001/ORCL");
    }

    #[test]
    fn positions_in_characters() {
        let db = |position| {
            Error::Database(DbError {
                position: Some(position),
                ..DbError::default()
            })
        };
        // `à` takes two bytes: byte 15 (1-based) is the 14th character.
        let Error::Database(e) = in_characters(db(15), "SELECT 'à' FROM t") else {
            unreachable!()
        };
        assert_eq!(e.position, Some(14));
        let Error::Database(e) = in_characters(db(3), "abc") else {
            unreachable!()
        };
        assert_eq!(e.position, Some(3));
    }

    #[test]
    fn statements_lose_their_final_semicolon() {
        assert_eq!(
            statement_text("select 1 from dual; \n"),
            "select 1 from dual"
        );
        assert_eq!(statement_text("BEGIN NULL; END;"), "BEGIN NULL; END;");
        assert_eq!(
            statement_text("create or replace procedure p is begin null; end;"),
            "create or replace procedure p is begin null; end;"
        );
    }

    #[test]
    fn missing_client_offers_the_download() {
        // No Instant Client in an empty folder, nor (on a clean machine) in the system.
        let dir = std::env::temp_dir().join("platypus-oracle-none");
        if let Err(e) = client::load(&dir) {
            assert!(
                matches!(
                    e,
                    Error::DriverMissing {
                        driver: DriverKind::Oracle
                    }
                ),
                "{e}"
            );
        }
    }
}
