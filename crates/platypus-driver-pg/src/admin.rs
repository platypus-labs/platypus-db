//! The sessions of the server and their locks, from `pg_stat_activity` and `pg_locks`.

use platypus_core::{
    Connection, EndSession, Error, Result, ServerLock, ServerSession, TextRowExt, query_text,
    session_number,
};

/// The client sessions, the one asking first; background workers left out.
const SESSIONS: &str = "SELECT pid::text, usename, datname, \
     CASE WHEN client_addr IS NULL THEN NULL ELSE host(client_addr) || ':' || client_port END, \
     application_name, state, to_char(backend_start, 'YYYY-MM-DD HH24:MI:SS'), query, \
     CASE WHEN state = 'active' \
          THEN extract(epoch FROM clock_timestamp() - query_start)::float8 END, \
     wait_event_type || ':' || wait_event, \
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
    Ok(query_text(conn, LOCKS)
        .await?
        .into_iter()
        .map(|r| ServerLock {
            session: r.text(0),
            kind: r.text(1),
            object: r.opt(2),
            mode: r.text(3),
            granted: r.flag(4),
        })
        .collect())
}

pub(crate) async fn end(conn: &dyn Connection, id: &str, how: EndSession) -> Result<()> {
    let pid = session_number(id)?;
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
