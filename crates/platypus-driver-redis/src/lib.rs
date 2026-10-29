//! Redis driver on the `redis` crate (RESP3 when the server speaks it, rustls for TLS).
//!
//! Statements are Redis commands, one per line ([`command`]), plus `BROWSE`, which reads the
//! keys of one type as the rows of a virtual table ([`browse`]). Schemas are the numbered
//! databases (`db0`, `db1`…), tables the key types ([`command::KeyType`]). There are no
//! transactions to commit: `MULTI` queues commands on the client until `EXEC`.

pub mod browse;
pub mod command;
pub mod edit;
mod error;
pub mod meta;
mod net;
pub mod value;

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};
use std::sync::{Arc, Mutex, PoisonError};

use async_trait::async_trait;
use platypus_core::{
    CancelHandle, Canceller, Capabilities, CatalogColumn, Connection, ConnectionConfig, Driver,
    DriverKind, Error, ExecOptions, ExecResult, Executed, ObjectMeta, ReferencingKey, Result,
    SchemaMeta, ServerInfo, TableDetails, TransactionState, unsupported,
};
use redis::aio::MultiplexedConnection;
use redis::{Cmd, RedisError, Value};
use tokio::sync::watch;

use crate::command::{Command, KeyType};
use crate::net::Opener;

#[derive(Debug, Clone, Copy, Default)]
pub struct RedisDriver;

#[async_trait]
impl Driver for RedisDriver {
    fn kind(&self) -> DriverKind {
        DriverKind::Redis
    }

    async fn connect(&self, config: &ConnectionConfig) -> Result<Box<dyn Connection>> {
        let opener = Arc::new(Opener::new(config)?);
        let db = match config.database.trim() {
            "" => 0,
            name => meta::database(name).map_err(|_| {
                Error::Config(format!(
                    "invalid Redis database `{name}`: a number, such as 0"
                ))
            })?,
        };
        let mut conn = opener
            .open(db)
            .await
            .map_err(|e| Error::Connect(connect_message(&e)))?;
        let info: String = redis::cmd("INFO")
            .arg("server")
            .query_async(&mut conn)
            .await
            .map_err(|e| Error::Connect(connect_message(&e)))?;
        let version = info
            .lines()
            .find_map(|l| l.trim().strip_prefix("redis_version:"))
            .unwrap_or_default()
            .to_owned();
        let session = RedisConnection {
            info: ServerInfo {
                product: "Redis".to_owned(),
                version,
                encrypted: opener.encrypted(),
            },
            opener,
            conns: tokio::sync::Mutex::new(HashMap::new()),
            current: AtomicI64::new(db),
            closed: Arc::new(AtomicBool::new(false)),
            cancel: Arc::new(watch::channel(0u64).0),
            ids: Arc::new(Mutex::new(Vec::new())),
            queue: Mutex::new(None),
        };
        session.adopt(db, conn).await;
        for statement in config.init.sql(DriverKind::Redis)? {
            session.execute(&statement, &ExecOptions::default()).await?;
        }
        Ok(Box::new(session))
    }
}

/// A connection error as the user can act on it.
fn connect_message(e: &RedisError) -> String {
    match e.code() {
        Some(code) => format!("{code} {}", e.detail().unwrap_or_default()),
        None => e.to_string(),
    }
}

/// A session: a connection per database it uses, opened on first use, and the database
/// that unqualified commands go to (`SELECT`).
pub struct RedisConnection {
    opener: Arc<Opener>,
    conns: tokio::sync::Mutex<HashMap<i64, MultiplexedConnection>>,
    current: AtomicI64,
    info: ServerInfo,
    /// The last command failed because the server could not be reached.
    closed: Arc<AtomicBool>,
    /// Bumped by the cancel handle; running commands watch it.
    cancel: Arc<watch::Sender<u64>>,
    /// The `CLIENT ID`s of the connections, which the cancel handle unblocks.
    ids: Arc<Mutex<Vec<i64>>>,
    /// The commands queued since `MULTI`.
    queue: Mutex<Option<Vec<Vec<Vec<u8>>>>>,
}

impl std::fmt::Debug for RedisConnection {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RedisConnection")
            .field("opener", &self.opener)
            .field("current", &self.current)
            .field("info", &self.info)
            .finish_non_exhaustive()
    }
}

/// Waits until the cancel handle is used after the subscription.
async fn cancelled(rx: &mut watch::Receiver<u64>) {
    if rx.changed().await.is_err() {
        std::future::pending::<()>().await;
    }
}

fn raw(args: &[Vec<u8>]) -> Cmd {
    let mut cmd = Cmd::new();
    for arg in args {
        cmd.arg(arg.as_slice());
    }
    cmd
}

/// A one-row result of `text` in a `value` column.
fn text_result(text: &str) -> Result<ExecResult> {
    value::reply(Value::SimpleString(text.to_owned()), false, 1)
}

impl RedisConnection {
    fn current(&self) -> i64 {
        self.current.load(Ordering::Relaxed)
    }

    /// Keeps `conn` as the connection to `db`, with its id for the cancel handle.
    async fn adopt(&self, db: i64, mut conn: MultiplexedConnection) {
        if let Ok(id) = redis::cmd("CLIENT")
            .arg("ID")
            .query_async::<i64>(&mut conn)
            .await
        {
            self.ids
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .push(id);
        }
        self.conns.lock().await.insert(db, conn);
    }

    /// The connection to `db`, opened when the session has none.
    async fn conn(&self, db: i64) -> Result<MultiplexedConnection> {
        if let Some(conn) = self.conns.lock().await.get(&db) {
            return Ok(conn.clone());
        }
        let conn = self
            .opener
            .open(db)
            .await
            .map_err(|e| self.failed(db, &e))?;
        self.adopt(db, conn.clone()).await;
        Ok(conn)
    }

    /// Records a failure: a lost connection is dropped, to be opened again on next use.
    fn failed(&self, db: i64, e: &RedisError) -> Error {
        if error::is_network(e) {
            self.closed.store(true, Ordering::Relaxed);
            if let Ok(mut conns) = self.conns.try_lock() {
                conns.remove(&db);
            }
        }
        error::map(e)
    }

    /// Sends `cmd` to `db`, until the cancel handle is used.
    async fn query(&self, db: i64, cmd: &Cmd) -> Result<Value> {
        let mut conn = self.conn(db).await?;
        let mut rx = self.cancel.subscribe();
        let reply = tokio::select! {
            reply = cmd.query_async::<Value>(&mut conn) => reply,
            () = cancelled(&mut rx) => return Err(Error::Cancelled),
        };
        match reply {
            Ok(Value::ServerError(e)) => Err(error::server(&e)),
            Ok(value) => {
                self.closed.store(false, Ordering::Relaxed);
                Ok(value)
            }
            Err(e) => Err(self.failed(db, &e)),
        }
    }

    /// Sends `commands` to `db` as one `MULTI`/`EXEC`; the reply of each, errors included.
    async fn transaction(&self, db: i64, commands: &[Vec<Vec<u8>>]) -> Result<Vec<Value>> {
        let mut pipe = redis::pipe();
        pipe.atomic().ignore_errors();
        for args in commands {
            pipe.add_command(raw(args));
        }
        let mut conn = self.conn(db).await?;
        let mut rx = self.cancel.subscribe();
        let reply = tokio::select! {
            reply = pipe.query_async::<Vec<Value>>(&mut conn) => reply,
            () = cancelled(&mut rx) => return Err(Error::Cancelled),
        };
        reply.map_err(|e| self.failed(db, &e))
    }

    async fn run(&self, command: Command, options: &ExecOptions) -> Result<ExecResult> {
        let queued = self
            .queue
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .is_some();
        match command {
            Command::Multi if queued => {
                Err(Error::Other("MULTI calls can not be nested".to_owned()))
            }
            Command::Multi => {
                *self.queue.lock().unwrap_or_else(PoisonError::into_inner) = Some(Vec::new());
                text_result("OK")
            }
            Command::Discard | Command::Exec if !queued => {
                Err(Error::Other(format!("{} without MULTI", command.name())))
            }
            Command::Discard => {
                *self.queue.lock().unwrap_or_else(PoisonError::into_inner) = None;
                text_result("OK")
            }
            Command::Exec => {
                let commands = self
                    .queue
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .take()
                    .unwrap_or_default();
                let replies = self.transaction(self.current(), &commands).await?;
                value::reply(Value::Array(replies), false, options.batch_size)
            }
            Command::Raw(args) if queued => {
                self.queue
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .get_or_insert_with(Vec::new)
                    .push(args);
                text_result("QUEUED")
            }
            Command::Select(_) | Command::Browse(_) if queued => Err(Error::Other(format!(
                "{} cannot run inside MULTI",
                command.name()
            ))),
            Command::Select(db) => {
                self.conn(db).await?;
                self.current.store(db, Ordering::Relaxed);
                Ok(ExecResult::Affected { rows: 0 })
            }
            Command::Browse(browse) => {
                let conn = self
                    .conn(browse.db.unwrap_or_else(|| self.current()))
                    .await?;
                Ok(browse::run(conn, browse, options.batch_size))
            }
            Command::Raw(args) => {
                let upper = String::from_utf8_lossy(&args[0]).to_ascii_uppercase();
                let json = matches!(upper.as_str(), "JSON.GET" | "JSON.MGET");
                let reply = self.query(self.current(), &raw(&args)).await?;
                value::reply(reply, json, options.batch_size)
            }
        }
    }

    /// The keys of `db` by type, when a scan of at most [`meta::COUNT_CAP`] keys sees them
    /// all.
    async fn count_types(&self, db: i64) -> Result<Option<Vec<(KeyType, u64)>>> {
        let mut cursor = 0u64;
        let mut types = Vec::new();
        loop {
            let mut scan = redis::cmd("SCAN");
            scan.arg(cursor).arg("COUNT").arg(1000);
            let reply = self.query(db, &scan).await?;
            let Value::Array(parts) = reply else {
                return Err(value::unexpected("SCAN", &reply));
            };
            let [next, keys] = parts.as_slice() else {
                return Ok(None);
            };
            let keys = match keys {
                Value::Array(keys) | Value::Set(keys) => keys.clone(),
                _ => Vec::new(),
            };
            if !keys.is_empty() {
                let mut pipe = redis::pipe();
                for key in &keys {
                    if let Value::BulkString(key) = key {
                        pipe.cmd("TYPE").arg(key.as_slice());
                    }
                }
                let mut conn = self.conn(db).await?;
                let replies: Vec<Value> = pipe
                    .ignore_errors()
                    .query_async(&mut conn)
                    .await
                    .map_err(|e| self.failed(db, &e))?;
                types.extend(replies);
            }
            cursor = value::text(next).parse().unwrap_or(0);
            if cursor == 0 {
                return Ok(Some(meta::count_types(&types)));
            }
            if types.len() >= meta::COUNT_CAP {
                return Ok(None);
            }
        }
    }

    /// Whether the server has `RedisJSON`.
    async fn has_json(&self, db: i64) -> bool {
        let mut cmd = redis::cmd("COMMAND");
        cmd.arg("INFO").arg("JSON.GET");
        match self.query(db, &cmd).await {
            Ok(Value::Array(items)) => items.iter().any(|i| !matches!(i, Value::Nil)),
            Ok(Value::Map(items)) => !items.is_empty(),
            _ => false,
        }
    }
}

struct RedisCanceller {
    opener: Arc<Opener>,
    cancel: Arc<watch::Sender<u64>>,
    ids: Arc<Mutex<Vec<i64>>>,
}

#[async_trait]
impl Canceller for RedisCanceller {
    async fn cancel(&self) -> Result<()> {
        self.cancel.send_modify(|n| *n += 1);
        // A blocking command (`BLPOP`, `XREAD BLOCK`, `WAIT`) holds its connection on the
        // server: unblock it from another one.
        let ids = self
            .ids
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone();
        if let Ok(mut conn) = self.opener.open(0).await {
            for id in ids {
                let _ = redis::cmd("CLIENT")
                    .arg("UNBLOCK")
                    .arg(id)
                    .arg("ERROR")
                    .query_async::<Value>(&mut conn)
                    .await;
            }
        }
        Ok(())
    }
}

#[async_trait]
impl Connection for RedisConnection {
    async fn execute(&self, sql: &str, options: &ExecOptions) -> Result<ExecResult> {
        let lines = command::split(sql);
        if lines.is_empty() {
            return Err(error::parse(sql, "no command"));
        }
        let mut last = None;
        for range in lines {
            let line = &sql[range];
            let parsed = command::parse(line).map_err(|e| error::parse(line, &e))?;
            last = Some(self.run(parsed, options).await?);
        }
        last.ok_or_else(|| Error::Other("no command".to_owned()))
    }

    /// Runs `statements` as one `MULTI`/`EXEC` on the database a first `SELECT n` names
    /// (the session's otherwise, which stays). Each counts one row; a command the server
    /// refuses stops the count there (those after it ran: `EXEC` does not roll back).
    async fn execute_all(&self, statements: &[&str], _expected: &[u64]) -> Executed {
        let mut rows = Vec::new();
        let mut db = self.current();
        let mut commands = Vec::new();
        for (i, statement) in statements.iter().enumerate() {
            match command::parse(statement) {
                Ok(Command::Select(n)) if i == 0 => {
                    db = n;
                    rows.push(1);
                }
                Ok(Command::Raw(args)) => commands.push(args),
                Ok(other) => {
                    return Executed {
                        rows,
                        error: Some(Error::Other(format!(
                            "{} cannot run in a batch",
                            other.name()
                        ))),
                    };
                }
                Err(e) => {
                    return Executed {
                        rows,
                        error: Some(error::parse(statement, &e)),
                    };
                }
            }
        }
        let replies = match self.transaction(db, &commands).await {
            Ok(replies) => replies,
            Err(error) => {
                return Executed {
                    rows,
                    error: Some(error),
                };
            }
        };
        for reply in replies {
            if let Value::ServerError(e) = reply {
                return Executed {
                    rows,
                    error: Some(error::server(&e)),
                };
            }
            rows.push(1);
        }
        Executed { rows, error: None }
    }

    fn cancel_handle(&self) -> CancelHandle {
        CancelHandle::new(RedisCanceller {
            opener: Arc::clone(&self.opener),
            cancel: Arc::clone(&self.cancel),
            ids: Arc::clone(&self.ids),
        })
    }

    fn server_info(&self) -> &ServerInfo {
        &self.info
    }

    fn is_closed(&self) -> bool {
        self.closed.load(Ordering::Relaxed)
    }

    async fn check_alive(&self) -> bool {
        self.query(self.current(), &redis::cmd("PING"))
            .await
            .is_ok()
    }

    async fn schemas(&self) -> Result<Vec<SchemaMeta>> {
        let mut cmd = redis::cmd("INFO");
        cmd.arg("keyspace");
        let info = value::text(&self.query(self.current(), &cmd).await?);
        let with_keys = meta::keyspace(&info)
            .into_iter()
            .filter(|&(_, n)| n > 0)
            .map(|(db, _)| db)
            .collect();
        Ok(meta::schemas(with_keys, self.current()))
    }

    async fn objects(&self, schema: &str) -> Result<Vec<ObjectMeta>> {
        let db = meta::database(schema)?;
        let counts = self.count_types(db).await?;
        let json = self.has_json(db).await;
        Ok(meta::objects(counts.as_deref(), json))
    }

    async fn table_details(&self, schema: &str, table: &str) -> Result<TableDetails> {
        meta::database(schema)?;
        meta::table_details(table)
    }

    async fn referencing_keys(&self, _schema: &str, _table: &str) -> Result<Vec<ReferencingKey>> {
        Ok(Vec::new())
    }

    async fn catalog_columns(&self) -> Result<Vec<CatalogColumn>> {
        let schema = meta::schema_name(self.current());
        Ok(KeyType::ALL
            .into_iter()
            .flat_map(|kind| {
                let schema = schema.clone();
                kind.columns()
                    .iter()
                    .map(move |&(column, _)| CatalogColumn {
                        schema: schema.clone(),
                        table: kind.table().to_owned(),
                        column: column.to_owned(),
                    })
            })
            .collect())
    }

    fn transaction(&self) -> TransactionState {
        TransactionState {
            autocommit: true,
            open: false,
        }
    }

    async fn set_autocommit(&self, on: bool) -> Result<()> {
        if on {
            Ok(())
        } else {
            Err(unsupported(
                "transactions on Redis (MULTI and EXEC run in the editor)",
            ))
        }
    }

    async fn commit(&self) -> Result<()> {
        Ok(())
    }

    async fn rollback(&self) -> Result<()> {
        Ok(())
    }

    fn capabilities(&self) -> Capabilities {
        Capabilities {
            cancel: true,
            ..Capabilities::default()
        }
    }
}
