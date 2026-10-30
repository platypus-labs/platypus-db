//! The sessions of the server and their locks: the process list, and `InnoDB`'s lock waits
//! from `performance_schema` (MySQL 8) or `information_schema` (MariaDB). The costliest
//! statements and the use of the indexes, from `performance_schema`; the tables, from
//! `information_schema`; and their upkeep.

use std::collections::HashMap;

use platypus_core::{
    Connection, EndSession, Error, ExecOptions, Maintenance, ObjectStatistics, Result, ServerLock,
    ServerQuery, ServerSession, StatisticsKind, TOP_QUERIES, TextRowExt, query_locks,
    query_statements, query_text, refuse_own_session, run_maintenance, session_number,
    sort_statistics,
};
use platypus_sql::Dialect;

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

pub(crate) fn is_mariadb(conn: &dyn Connection) -> bool {
    mariadb(conn)
}

fn mariadb(conn: &dyn Connection) -> bool {
    conn.server_info().product == "MariaDB"
}

pub(crate) async fn sessions(conn: &dyn Connection) -> Result<Vec<ServerSession>> {
    let rows = query_text(conn, SESSIONS).await?;
    // MySQL 5.7 has MariaDB's table, not `data_lock_waits`. Seeing other sessions' locks
    // needs PROCESS (and `performance_schema`): without it the list has no waits, and the
    // Locks view tells why.
    let (first, second) = if mariadb(conn) {
        (MARIADB_WAITS, MYSQL_WAITS)
    } else {
        (MYSQL_WAITS, MARIADB_WAITS)
    };
    let waits = match query_text(conn, first).await {
        Ok(waits) => waits,
        Err(_) => query_text(conn, second).await.unwrap_or_default(),
    };
    let mut blockers: HashMap<String, Vec<String>> = HashMap::new();
    for wait in waits {
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
    let (first, second) = if mariadb(conn) {
        (MARIADB_LOCKS, MYSQL_LOCKS)
    } else {
        (MYSQL_LOCKS, MARIADB_LOCKS)
    };
    // MySQL 5.7 has MariaDB's tables; the error told is that of the server's own.
    let mut locks = match query_locks(conn, first).await {
        Ok(locks) => locks,
        Err(e) => query_locks(conn, second).await.map_err(|_| e)?,
    };
    // By id as a number, as the other servers sort theirs.
    let number = |l: &ServerLock| l.session.parse::<u64>().unwrap_or(u64::MAX);
    locks.sort_by_key(|l| (l.granted, number(l)));
    Ok(locks)
}

pub(crate) async fn end(conn: &dyn Connection, id: &str, how: EndSession) -> Result<()> {
    let id = session_number(id)?;
    refuse_own_session(conn, "SELECT CONNECTION_ID()", id).await?;
    let sql = match how {
        EndSession::Cancel => format!("KILL QUERY {id}"),
        EndSession::Terminate => format!("KILL CONNECTION {id}"),
    };
    conn.execute(&sql, &ExecOptions::default()).await?;
    Ok(())
}

/// What [`maintain`] runs: no `VACUUM` or `REINDEX` in MySQL.
pub(crate) const MAINTENANCE: &[Maintenance] = &[
    Maintenance::Analyze,
    Maintenance::Optimize,
    Maintenance::Check,
];

/// The schemas of the server itself.
const SYSTEM_SCHEMAS: &str = "('mysql', 'information_schema', 'performance_schema', 'sys')";

/// The statements by their digest, with one run's text where the server keeps it (MySQL 8);
/// timers are in picoseconds.
fn digests(sample: &str) -> String {
    format!(
        "SELECT DIGEST_TEXT, {sample}, SCHEMA_NAME, NULL, COUNT_STAR, \
         SUM_TIMER_WAIT / 1e12, AVG_TIMER_WAIT / 1e12, MAX_TIMER_WAIT / 1e12, \
         SUM_ROWS_SENT + SUM_ROWS_AFFECTED \
         FROM performance_schema.events_statements_summary_by_digest \
         WHERE DIGEST_TEXT IS NOT NULL \
         ORDER BY SUM_TIMER_WAIT DESC LIMIT {TOP_QUERIES}"
    )
}

pub(crate) async fn top_queries(conn: &dyn Connection) -> Result<Vec<ServerQuery>> {
    let on = query_text(conn, "SELECT @@performance_schema").await?;
    if !on.first().is_some_and(|r| r.flag(0)) {
        return Err(Error::Other(
            "performance_schema is off: start the server with performance_schema=ON \
             (MariaDB keeps it off by default)"
                .to_owned(),
        ));
    }
    // MySQL 5.7 and MariaDB keep no sample of a statement's runs.
    match query_statements(conn, &digests("QUERY_SAMPLE_TEXT")).await {
        Ok(queries) => Ok(queries),
        Err(e) => query_statements(conn, &digests("NULL"))
            .await
            .map_err(|_| e),
    }
}

/// The tables of the user schemas: size with the indexes, estimated rows, unused space.
fn tables_sql() -> String {
    format!(
        "SELECT TABLE_SCHEMA, TABLE_NAME, CAST(DATA_LENGTH + INDEX_LENGTH AS UNSIGNED), \
         CAST(TABLE_ROWS AS UNSIGNED), CAST(DATA_FREE AS UNSIGNED) \
         FROM information_schema.TABLES \
         WHERE TABLE_TYPE = 'BASE TABLE' AND TABLE_SCHEMA NOT IN {SYSTEM_SCHEMAS}"
    )
}

/// Their indexes, unique or not.
fn indexes_sql() -> String {
    format!(
        "SELECT TABLE_SCHEMA, TABLE_NAME, INDEX_NAME, MIN(NON_UNIQUE) = 0 \
         FROM information_schema.STATISTICS WHERE TABLE_SCHEMA NOT IN {SYSTEM_SCHEMAS} \
         GROUP BY TABLE_SCHEMA, TABLE_NAME, INDEX_NAME"
    )
}

/// The reads and writes through each index since the server started; those of no index
/// are the rows read by full scans.
fn usage_sql() -> String {
    format!(
        "SELECT OBJECT_SCHEMA, OBJECT_NAME, COALESCE(INDEX_NAME, ''), COUNT_STAR, COUNT_READ \
         FROM performance_schema.table_io_waits_summary_by_index_usage \
         WHERE OBJECT_TYPE = 'TABLE' AND OBJECT_SCHEMA NOT IN {SYSTEM_SCHEMAS}"
    )
}

pub(crate) async fn object_statistics(conn: &dyn Connection) -> Result<Vec<ObjectStatistics>> {
    let tables = query_text(conn, &tables_sql()).await?;
    let indexes = query_text(conn, &indexes_sql()).await?;
    // Without performance_schema, or the right to read it, the uses are unknown.
    let mut usage: HashMap<(String, String, String), (u64, u64)> = HashMap::new();
    for r in query_text(conn, &usage_sql()).await.unwrap_or_default() {
        usage.insert(
            (r.text(0), r.text(1), r.text(2)),
            (r.number(3).unwrap_or(0), r.number(4).unwrap_or(0)),
        );
    }
    let known = !usage.is_empty();
    let used = |schema: &str, table: &str, index: &str| {
        usage
            .get(&(schema.to_owned(), table.to_owned(), index.to_owned()))
            .copied()
            .or(known.then_some((0, 0)))
    };
    let mut out: Vec<ObjectStatistics> = tables
        .iter()
        .map(|r| {
            let (schema, name) = (r.text(0), r.text(1));
            let index_scans = known.then(|| {
                indexes
                    .iter()
                    .filter(|i| i.text(0) == schema && i.text(1) == name)
                    .filter_map(|i| used(&schema, &name, &i.text(2)).map(|(_, read)| read))
                    .sum()
            });
            ObjectStatistics {
                full_scans: used(&schema, &name, "").map(|(_, read)| read),
                index_scans,
                size_bytes: r.number(2),
                rows: r.number(3),
                free_bytes: r.number(4),
                schema,
                name,
                ..ObjectStatistics::default()
            }
        })
        .collect();
    out.extend(indexes.iter().map(|r| {
        let (schema, table, name) = (r.text(0), r.text(1), r.text(2));
        ObjectStatistics {
            index_scans: used(&schema, &table, &name).map(|(all, _)| all),
            kind: StatisticsKind::Index,
            unique: r.flag(3),
            schema,
            table: Some(table),
            name,
            ..ObjectStatistics::default()
        }
    }));
    sort_statistics(&mut out);
    Ok(out)
}

pub(crate) async fn maintain(
    conn: &dyn Connection,
    schema: &str,
    table: &str,
    operation: Maintenance,
) -> Result<Vec<String>> {
    let name = Dialect::MySql.qualified(Some(schema), table);
    let sql = match operation {
        Maintenance::Analyze => format!("ANALYZE TABLE {name}"),
        Maintenance::Optimize => format!("OPTIMIZE TABLE {name}"),
        Maintenance::Check => format!("CHECK TABLE {name}"),
        Maintenance::Vacuum | Maintenance::Reindex => {
            return Err(platypus_core::unsupported("this maintenance in MySQL"));
        }
    };
    run_maintenance(conn, &sql).await
}
