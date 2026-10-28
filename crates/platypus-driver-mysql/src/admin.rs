//! The sessions of the server and their locks: the process list, and `InnoDB`'s lock waits
//! from `performance_schema` (MySQL 8) or `information_schema` (MariaDB).

use std::collections::HashMap;

use platypus_core::{
    Connection, EndSession, ExecOptions, Result, ServerLock, ServerSession, TextRowExt, query_text,
    session_number,
};

/// The client sessions, the one asking first; the server's own threads left out.
const SESSIONS: &str = "SELECT ID, USER, DB, HOST, COMMAND, INFO, \
     CASE WHEN COMMAND IN ('Query', 'Execute') THEN TIME END, NULLIF(STATE, ''), \
     ID = CONNECTION_ID() \
     FROM information_schema.PROCESSLIST \
     WHERE COMMAND NOT IN ('Daemon', 'Binlog Dump') AND USER NOT IN ('system user', 'event_scheduler') \
     ORDER BY ID = CONNECTION_ID() DESC, ID";

/// Who waits for whom on MySQL 8: (waiting, blocking) session ids.
const MYSQL_WAITS: &str = "SELECT r.trx_mysql_thread_id, b.trx_mysql_thread_id \
     FROM performance_schema.data_lock_waits w \
     JOIN information_schema.INNODB_TRX r ON r.trx_id = w.REQUESTING_ENGINE_TRANSACTION_ID \
     JOIN information_schema.INNODB_TRX b ON b.trx_id = w.BLOCKING_ENGINE_TRANSACTION_ID";

/// The same on MariaDB, which kept `InnoDB`'s tables in `information_schema`.
const MARIADB_WAITS: &str = "SELECT r.trx_mysql_thread_id, b.trx_mysql_thread_id \
     FROM information_schema.INNODB_LOCK_WAITS w \
     JOIN information_schema.INNODB_TRX r ON r.trx_id = w.requesting_trx_id \
     JOIN information_schema.INNODB_TRX b ON b.trx_id = w.blocking_trx_id";

/// MySQL 8's row and table locks, then its metadata locks (those a DDL waits for).
const MYSQL_LOCKS: &str = "SELECT t.PROCESSLIST_ID, l.LOCK_TYPE, \
     CONCAT(l.OBJECT_SCHEMA, '.', l.OBJECT_NAME), l.LOCK_MODE, l.LOCK_STATUS = 'GRANTED' \
     FROM performance_schema.data_locks l \
     JOIN performance_schema.threads t ON t.THREAD_ID = l.THREAD_ID \
     WHERE t.PROCESSLIST_ID <> CONNECTION_ID() \
     UNION ALL \
     SELECT t.PROCESSLIST_ID, 'METADATA', CONCAT(m.OBJECT_SCHEMA, '.', m.OBJECT_NAME), \
     m.LOCK_TYPE, m.LOCK_STATUS = 'GRANTED' \
     FROM performance_schema.metadata_locks m \
     JOIN performance_schema.threads t ON t.THREAD_ID = m.OWNER_THREAD_ID \
     WHERE m.OBJECT_TYPE = 'TABLE' AND t.PROCESSLIST_ID <> CONNECTION_ID() \
       AND m.OBJECT_SCHEMA NOT IN ('performance_schema', 'information_schema')";

/// MariaDB's: only the locks that some transaction waits for, or that one is waiting.
const MARIADB_LOCKS: &str = "SELECT x.trx_mysql_thread_id, l.lock_type, \
     REPLACE(l.lock_table, '`', ''), l.lock_mode, \
     l.lock_id <> COALESCE(x.trx_requested_lock_id, '') \
     FROM information_schema.INNODB_LOCKS l \
     JOIN information_schema.INNODB_TRX x ON x.trx_id = l.lock_trx_id \
     WHERE x.trx_mysql_thread_id <> CONNECTION_ID()";

fn mariadb(conn: &dyn Connection) -> bool {
    conn.server_info().product == "MariaDB"
}

pub(crate) async fn sessions(conn: &dyn Connection) -> Result<Vec<ServerSession>> {
    let rows = query_text(conn, SESSIONS).await?;
    // Seeing other sessions' locks needs the PROCESS privilege: without it, no waits.
    let waits = if mariadb(conn) {
        MARIADB_WAITS
    } else {
        MYSQL_WAITS
    };
    let mut blockers: HashMap<String, Vec<String>> = HashMap::new();
    for wait in query_text(conn, waits).await.unwrap_or_default() {
        let blocking = blockers.entry(wait.text(0)).or_default();
        if !blocking.contains(&wait.text(1)) {
            blocking.push(wait.text(1));
        }
    }
    Ok(rows
        .into_iter()
        .map(|r| ServerSession {
            blocked_by: blockers.remove(&r.text(0)).unwrap_or_default(),
            id: r.text(0),
            user: r.opt(1),
            database: r.opt(2),
            client: r.opt(3),
            application: None,
            state: r.opt(4),
            started: None,
            query: r.opt(5),
            query_seconds: r.opt(6).and_then(|s| s.parse().ok()),
            wait: r.opt(7),
            current: r.flag(8),
        })
        .collect())
}

pub(crate) async fn locks(conn: &dyn Connection) -> Result<Vec<ServerLock>> {
    let sql = if mariadb(conn) {
        MARIADB_LOCKS
    } else {
        MYSQL_LOCKS
    };
    let mut locks: Vec<ServerLock> = query_text(conn, sql)
        .await?
        .into_iter()
        .map(|r| ServerLock {
            session: r.text(0),
            kind: r.text(1),
            object: r.opt(2),
            mode: r.text(3),
            granted: r.flag(4),
        })
        .collect();
    locks.sort_by(|a, b| (a.granted, &a.session).cmp(&(b.granted, &b.session)));
    Ok(locks)
}

pub(crate) async fn end(conn: &dyn Connection, id: &str, how: EndSession) -> Result<()> {
    let id = session_number(id)?;
    let sql = match how {
        EndSession::Cancel => format!("KILL QUERY {id}"),
        EndSession::Terminate => format!("KILL CONNECTION {id}"),
    };
    conn.execute(&sql, &ExecOptions::default()).await?;
    Ok(())
}
