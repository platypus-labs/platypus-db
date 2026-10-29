//! The sessions of the server and their locks, the costliest statements of its plan cache,
//! and the use of the tables and indexes of the database, from the dynamic management views
//! (they need `VIEW SERVER STATE` to show what other logins do); and the tables' upkeep.

use platypus_core::{
    Connection, EndSession, Error, ExecOptions, Maintenance, ObjectStatistics, Result, ServerLock,
    ServerQuery, ServerSession, StatisticsKind, TOP_QUERIES, TextRowExt, query_locks, query_text,
    refuse_own_session, run_maintenance, seconds_of, session_number, sort_statistics,
};
use platypus_sql::Dialect;

/// The user sessions, the one asking first. An idle session shows its last statement, as
/// PostgreSQL does.
const SESSIONS: &str = "SELECT CAST(s.session_id AS varchar(10)), s.login_name, \
     DB_NAME(s.database_id), \
     c.client_net_address + ISNULL(':' + CAST(c.client_tcp_port AS varchar(10)), ''), \
     NULLIF(s.program_name, ''), COALESCE(r.status, s.status), \
     CONVERT(varchar(19), s.login_time, 120), q.text, \
     CASE WHEN r.session_id IS NOT NULL THEN r.total_elapsed_time / 1000.0 END, r.wait_type, \
     CASE WHEN r.blocking_session_id > 0 THEN CAST(r.blocking_session_id AS varchar(10)) END, \
     CASE WHEN s.session_id = @@SPID THEN 1 ELSE 0 END \
     FROM sys.dm_exec_sessions s \
     OUTER APPLY (SELECT TOP 1 client_net_address, client_tcp_port, most_recent_sql_handle \
                  FROM sys.dm_exec_connections WHERE session_id = s.session_id) c \
     OUTER APPLY (SELECT TOP 1 * FROM sys.dm_exec_requests \
                  WHERE session_id = s.session_id ORDER BY request_id) r \
     OUTER APPLY sys.dm_exec_sql_text(COALESCE(r.sql_handle, c.most_recent_sql_handle)) q \
     WHERE s.is_user_process = 1 \
     ORDER BY CASE WHEN s.session_id = @@SPID THEN 0 ELSE 1 END, s.session_id";

/// The locks of the other sessions, but the shared lock every session holds on its
/// database. Keys, pages and rows name their table in the current database only.
const LOCKS: &str = "SELECT CAST(l.request_session_id AS varchar(10)), l.resource_type, \
     CASE WHEN l.resource_type = 'OBJECT' \
          THEN OBJECT_SCHEMA_NAME(l.resource_associated_entity_id, l.resource_database_id) \
               + '.' + OBJECT_NAME(l.resource_associated_entity_id, l.resource_database_id) \
          WHEN l.resource_type IN ('KEY', 'PAGE', 'RID', 'HOBT') \
               AND l.resource_database_id = DB_ID() \
          THEN (SELECT OBJECT_SCHEMA_NAME(p.object_id) + '.' + OBJECT_NAME(p.object_id) \
                FROM sys.partitions p WHERE p.hobt_id = l.resource_associated_entity_id) END, \
     l.request_mode, CASE WHEN l.request_status = 'GRANT' THEN 1 ELSE 0 END \
     FROM sys.dm_tran_locks l \
     WHERE l.request_session_id <> @@SPID AND l.resource_type <> 'DATABASE' \
     ORDER BY CASE WHEN l.request_status = 'GRANT' THEN 1 ELSE 0 END, l.request_session_id";

pub(crate) async fn sessions(conn: &dyn Connection) -> Result<Vec<ServerSession>> {
    Ok(query_text(conn, SESSIONS)
        .await?
        .into_iter()
        .map(|r| ServerSession {
            id: r.text(0),
            user: r.opt(1),
            database: r.opt(2),
            client: r.opt(3),
            application: r.opt(4),
            state: r.opt(5),
            started: r.opt(6),
            query: r.opt(7),
            query_seconds: r.opt(8).and_then(|s| s.parse().ok()),
            wait: r.opt(9),
            blocked_by: r.opt(10).into_iter().collect(),
            current: r.flag(11),
        })
        .collect())
}

pub(crate) async fn locks(conn: &dyn Connection) -> Result<Vec<ServerLock>> {
    query_locks(conn, LOCKS).await
}

/// `KILL`: SQL Server cannot cancel the statement of another session and keep it.
pub(crate) async fn end(conn: &dyn Connection, id: &str, how: EndSession) -> Result<()> {
    let id = session_number(id)?;
    refuse_own_session(conn, "SELECT @@SPID", id).await?;
    if how == EndSession::Cancel {
        return Err(Error::Unsupported(
            "SQL Server cannot cancel the statement of another session: end the session".to_owned(),
        ));
    }
    conn.execute(&format!("KILL {id}"), &ExecOptions::default())
        .await?;
    Ok(())
}

/// What [`maintain`] runs: no `VACUUM` or `OPTIMIZE` in SQL Server.
pub(crate) const MAINTENANCE: &[Maintenance] = &[
    Maintenance::Analyze,
    Maintenance::Reindex,
    Maintenance::Check,
];

/// The statements of the plan cache, each cut out of its batch; times are in microseconds.
fn top_queries_sql() -> String {
    format!(
        "SELECT TOP {TOP_QUERIES} SUBSTRING(t.text, s.statement_start_offset / 2 + 1, \
         (CASE WHEN s.statement_end_offset = -1 THEN DATALENGTH(t.text) \
               ELSE s.statement_end_offset END - s.statement_start_offset) / 2 + 1), \
         DB_NAME(t.dbid), CAST(s.execution_count AS varchar(20)), \
         s.total_elapsed_time / 1e6, s.total_elapsed_time / 1e6 / s.execution_count, \
         s.max_elapsed_time / 1e6, CAST(s.total_rows AS varchar(20)) \
         FROM sys.dm_exec_query_stats s CROSS APPLY sys.dm_exec_sql_text(s.sql_handle) t \
         ORDER BY s.total_elapsed_time DESC"
    )
}

pub(crate) async fn top_queries(conn: &dyn Connection) -> Result<Vec<ServerQuery>> {
    Ok(query_text(conn, &top_queries_sql())
        .await?
        .into_iter()
        .map(|r| ServerQuery {
            query: r.text(0).trim().to_owned(),
            sample: None,
            database: r.opt(1),
            user: None,
            calls: r.number(2).unwrap_or(0),
            total_seconds: seconds_of(r.opt(3)).unwrap_or(0.0),
            mean_seconds: seconds_of(r.opt(4)).unwrap_or(0.0),
            max_seconds: seconds_of(r.opt(5)),
            rows: r.number(6),
        })
        .collect())
}

/// The tables of the database: space with their indexes, rows, scans of the whole table
/// (of its heap or clustered index), seeks and lookups through its indexes, and the last
/// update of its statistics. Uses count since the server started.
const TABLES: &str = "SELECT s.name, t.name, \
     CAST(SUM(ps.reserved_page_count) * 8192 AS varchar(20)), \
     CAST(SUM(CASE WHEN ps.index_id IN (0, 1) THEN ps.row_count END) AS varchar(20)), \
     CAST(ISNULL((SELECT SUM(u.user_scans) FROM sys.dm_db_index_usage_stats u \
                  WHERE u.database_id = DB_ID() AND u.object_id = t.object_id \
                    AND u.index_id IN (0, 1)), 0) AS varchar(20)), \
     CAST(ISNULL((SELECT SUM(u.user_seeks + u.user_lookups) FROM sys.dm_db_index_usage_stats u \
                  WHERE u.database_id = DB_ID() AND u.object_id = t.object_id), 0) AS varchar(20)), \
     CONVERT(varchar(19), (SELECT MAX(STATS_DATE(st.object_id, st.stats_id)) FROM sys.stats st \
                           WHERE st.object_id = t.object_id), 120) \
     FROM sys.tables t JOIN sys.schemas s ON s.schema_id = t.schema_id \
     JOIN sys.dm_db_partition_stats ps ON ps.object_id = t.object_id \
     WHERE t.is_ms_shipped = 0 GROUP BY s.name, t.name, t.object_id";

/// Their indexes but heaps; a clustered index is the table itself, needed as a unique one.
const INDEXES: &str = "SELECT s.name, i.name, t.name, \
     CAST((SELECT SUM(ps.used_page_count) * 8192 FROM sys.dm_db_partition_stats ps \
           WHERE ps.object_id = i.object_id AND ps.index_id = i.index_id) AS varchar(20)), \
     CAST(ISNULL((SELECT SUM(u.user_seeks + u.user_scans + u.user_lookups) \
                  FROM sys.dm_db_index_usage_stats u WHERE u.database_id = DB_ID() \
                    AND u.object_id = i.object_id AND u.index_id = i.index_id), 0) AS varchar(20)), \
     CONVERT(varchar(19), STATS_DATE(i.object_id, i.index_id), 120), \
     CASE WHEN i.is_unique = 1 OR i.is_primary_key = 1 OR i.type = 1 THEN 1 ELSE 0 END \
     FROM sys.indexes i JOIN sys.tables t ON t.object_id = i.object_id \
     JOIN sys.schemas s ON s.schema_id = t.schema_id \
     WHERE t.is_ms_shipped = 0 AND i.type > 0 AND i.is_hypothetical = 0";

pub(crate) async fn object_statistics(conn: &dyn Connection) -> Result<Vec<ObjectStatistics>> {
    let mut out: Vec<ObjectStatistics> = query_text(conn, TABLES)
        .await?
        .into_iter()
        .map(|r| ObjectStatistics {
            schema: r.text(0),
            name: r.text(1),
            size_bytes: r.number(2),
            rows: r.number(3),
            full_scans: r.number(4),
            index_scans: r.number(5),
            last_analyze: r.opt(6),
            ..ObjectStatistics::default()
        })
        .collect();
    out.extend(
        query_text(conn, INDEXES)
            .await?
            .into_iter()
            .map(|r| ObjectStatistics {
                schema: r.text(0),
                name: r.text(1),
                kind: StatisticsKind::Index,
                table: r.opt(2),
                size_bytes: r.number(3),
                index_scans: r.number(4),
                last_analyze: r.opt(5),
                unique: r.flag(6),
                ..ObjectStatistics::default()
            }),
    );
    sort_statistics(&mut out);
    Ok(out)
}

pub(crate) async fn maintain(
    conn: &dyn Connection,
    schema: &str,
    table: &str,
    operation: Maintenance,
) -> Result<Vec<String>> {
    let name = format!(
        "{}.{}",
        Dialect::SqlServer.quote_ident(schema),
        Dialect::SqlServer.quote_ident(table)
    );
    let sql = match operation {
        Maintenance::Analyze => format!("UPDATE STATISTICS {name}"),
        Maintenance::Reindex => format!("ALTER INDEX ALL ON {name} REBUILD"),
        Maintenance::Check => format!("DBCC CHECKTABLE (N'{}')", name.replace('\'', "''")),
        Maintenance::Vacuum | Maintenance::Optimize => {
            return Err(platypus_core::unsupported("this maintenance in SQL Server"));
        }
    };
    run_maintenance(conn, &sql).await
}
