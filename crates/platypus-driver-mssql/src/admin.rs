//! The sessions of the server and their locks, from the dynamic management views (they
//! need `VIEW SERVER STATE` to show the sessions of other logins).

use platypus_core::{
    Connection, EndSession, Error, ExecOptions, Result, ServerLock, ServerSession, TextRowExt,
    query_locks, query_text, refuse_own_session, session_number,
};

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
