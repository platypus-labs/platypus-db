//! The sessions of the server and their locks, from `pg_stat_activity` and `pg_locks`; the
//! costliest statements, from the `pg_stat_statements` extension; the use of tables and
//! indexes, from `pg_stat_user_tables` and `pg_stat_user_indexes`; and their upkeep.

use platypus_core::{
    Connection, EndSession, Error, Maintenance, ObjectStatistics, Result, ServerLock, ServerQuery,
    ServerSession, StatisticsKind, TOP_QUERIES, TextRowExt, query_locks, query_statements,
    query_text, refuse_own_session, run_maintenance, session_number, sort_statistics,
};
use platypus_sql::Dialect;

/// The client sessions, the one asking first; background workers left out.
const SESSIONS: &str = "SELECT pid::text, usename, datname, \
     CASE WHEN client_addr IS NULL THEN NULL ELSE host(client_addr) || ':' || client_port END, \
     application_name, state, to_char(backend_start, 'YYYY-MM-DD HH24:MI:SS'), query, \
     CASE WHEN state = 'active' \
          THEN extract(epoch FROM clock_timestamp() - query_start)::float8 END, \
     CASE WHEN state = 'active' THEN wait_event_type || ':' || wait_event END, \
     array_to_string(pg_blocking_pids(pid), ','), pid = pg_backend_pid() \
     FROM pg_stat_activity WHERE backend_type = 'client backend' \
     ORDER BY pid = pg_backend_pid() DESC, pid";

/// The locks of the other sessions, those on tables with the table's name. Every
/// transaction holds its own virtual id: those granted say nothing and are left out.
const LOCKS: &str = "SELECT l.pid::text, l.locktype, \
     CASE WHEN c.oid IS NULL THEN NULL ELSE n.nspname || '.' || c.relname END, \
     l.mode, l.granted \
     FROM pg_locks l \
     LEFT JOIN pg_class c ON c.oid = l.relation \
     LEFT JOIN pg_namespace n ON n.oid = c.relnamespace \
     WHERE l.pid IS NOT NULL AND l.pid <> pg_backend_pid() \
       AND NOT (l.locktype = 'virtualxid' AND l.granted) \
     ORDER BY l.granted, l.pid, l.locktype";

pub(crate) async fn sessions(conn: &dyn Connection) -> Result<Vec<ServerSession>> {
    Ok(query_text(conn, SESSIONS)
        .await?
        .into_iter()
        .map(|r| ServerSession {
            id: r.text(0),
            user: r.opt(1),
            database: r.opt(2),
            client: r.opt(3),
            application: r.opt(4).filter(|a| !a.is_empty()),
            state: r.opt(5),
            started: r.opt(6),
            query: r.opt(7).filter(|q| !q.is_empty()),
            query_seconds: r.opt(8).and_then(|s| s.parse().ok()),
            wait: r.opt(9),
            blocked_by: ids(r.opt(10)),
            current: r.flag(11),
        })
        .collect())
}

pub(crate) async fn locks(conn: &dyn Connection) -> Result<Vec<ServerLock>> {
    query_locks(conn, LOCKS).await
}

pub(crate) async fn end(conn: &dyn Connection, id: &str, how: EndSession) -> Result<()> {
    let pid = session_number(id)?;
    refuse_own_session(conn, "SELECT pg_backend_pid()", pid).await?;
    let function = match how {
        EndSession::Cancel => "pg_cancel_backend",
        EndSession::Terminate => "pg_terminate_backend",
    };
    let done = query_text(conn, &format!("SELECT {function}({pid})")).await?;
    // `false` when there is no such session; one of another user without the rights is
    // an error already.
    if done.first().is_some_and(|r| r.flag(0)) {
        Ok(())
    } else {
        Err(Error::Other(format!("there is no session {pid}")))
    }
}

/// What [`maintain`] runs: no `OPTIMIZE` or `CHECK` in PostgreSQL.
pub(crate) const MAINTENANCE: &[Maintenance] = &[
    Maintenance::Vacuum,
    Maintenance::Analyze,
    Maintenance::Reindex,
];

/// The schema of `pg_stat_statements` and whether its times are `*_exec_time` (version 1.8,
/// PostgreSQL 13) rather than `*_time`.
const STATEMENTS_EXTENSION: &str = "SELECT n.nspname, \
     current_setting('server_version_num')::int >= 130000 \
     FROM pg_extension e JOIN pg_namespace n ON n.oid = e.extnamespace \
     WHERE e.extname = 'pg_stat_statements'";

pub(crate) async fn top_queries(conn: &dyn Connection) -> Result<Vec<ServerQuery>> {
    let found = query_text(conn, STATEMENTS_EXTENSION).await?;
    let Some(extension) = found.first() else {
        return Err(Error::Other(
            "the pg_stat_statements extension is not installed: add it to \
             shared_preload_libraries in postgresql.conf, restart the server, then run \
             CREATE EXTENSION pg_stat_statements"
                .to_owned(),
        ));
    };
    let schema = Dialect::Postgres
        .quote_ident(&extension.text(0))
        .into_owned();
    let time = if extension.flag(1) {
        "exec_time"
    } else {
        "time"
    };
    // Statements of other users without the rights read `<insufficient privilege>`.
    let sql = format!(
        "SELECT s.query, NULL::text, d.datname, r.rolname, s.calls::text, \
         (s.total_{time} / 1000)::float8, (s.mean_{time} / 1000)::float8, \
         (s.max_{time} / 1000)::float8, s.rows::text \
         FROM {schema}.pg_stat_statements s \
         LEFT JOIN pg_database d ON d.oid = s.dbid \
         LEFT JOIN pg_roles r ON r.oid = s.userid \
         WHERE s.query <> '<insufficient privilege>' \
         ORDER BY s.total_{time} DESC LIMIT {TOP_QUERIES}"
    );
    query_statements(conn, &sql).await
}

/// The tables, then the indexes of each, of the user schemas. Vacuums and analyzes count by
/// hand or by the autovacuum, whichever came last.
const OBJECT_STATISTICS: &str = "SELECT schemaname::text, relname::text, NULL::text, \
     pg_total_relation_size(relid)::text, n_live_tup::text, n_dead_tup::text, seq_scan::text, \
     idx_scan::text, \
     to_char(greatest(last_vacuum, last_autovacuum), 'YYYY-MM-DD HH24:MI:SS'), \
     to_char(greatest(last_analyze, last_autoanalyze), 'YYYY-MM-DD HH24:MI:SS'), false \
     FROM pg_stat_user_tables \
     UNION ALL \
     SELECT s.schemaname::text, s.indexrelname::text, s.relname::text, \
     pg_relation_size(s.indexrelid)::text, \
     NULL, NULL, NULL, s.idx_scan::text, NULL, NULL, i.indisunique OR i.indisprimary \
     FROM pg_stat_user_indexes s JOIN pg_index i ON i.indexrelid = s.indexrelid";

pub(crate) async fn object_statistics(conn: &dyn Connection) -> Result<Vec<ObjectStatistics>> {
    let mut out: Vec<ObjectStatistics> = query_text(conn, OBJECT_STATISTICS)
        .await?
        .into_iter()
        .map(|r| {
            let table = r.opt(2);
            ObjectStatistics {
                schema: r.text(0),
                name: r.text(1),
                kind: if table.is_some() {
                    StatisticsKind::Index
                } else {
                    StatisticsKind::Table
                },
                table,
                size_bytes: r.number(3),
                rows: r.number(4),
                dead_rows: r.number(5),
                free_bytes: None,
                full_scans: r.number(6),
                index_scans: r.number(7),
                last_vacuum: r.opt(8),
                last_analyze: r.opt(9),
                unique: r.flag(10),
            }
        })
        .collect();
    sort_statistics(&mut out);
    Ok(out)
}

pub(crate) async fn maintain(
    conn: &dyn Connection,
    schema: &str,
    table: &str,
    operation: Maintenance,
) -> Result<Vec<String>> {
    let name = Dialect::Postgres.qualified(Some(schema), table);
    let sql = match operation {
        Maintenance::Vacuum => format!("VACUUM (VERBOSE) {name}"),
        Maintenance::Analyze => format!("ANALYZE VERBOSE {name}"),
        Maintenance::Reindex => format!("REINDEX TABLE {name}"),
        Maintenance::Optimize | Maintenance::Check => {
            return Err(platypus_core::unsupported("this maintenance in PostgreSQL"));
        }
    };
    run_maintenance(conn, &sql).await
}

/// `1,2` as its ids.
fn ids(list: Option<String>) -> Vec<String> {
    list.unwrap_or_default()
        .split(',')
        .filter(|id| !id.is_empty())
        .map(str::to_owned)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_blocking_ids() {
        assert_eq!(ids(Some("12,7".to_owned())), ["12", "7"]);
        assert_eq!(ids(Some(String::new())), Vec::<String>::new());
        assert_eq!(ids(None), Vec::<String>::new());
    }
}
