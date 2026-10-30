//! The server's parameters, from `pg_settings`, and its load, from `pg_stat_activity`,
//! `pg_stat_database` and the replication views.

use platypus_core::{
    Connection, Metric, ReplicationRow, Result, ServerMetrics, ServerParameter, TextRowExt,
    differs_from, float_of, query_row, query_sizes, query_text, set_value, sort_parameters,
};

/// Every setting; the unit and the boot value are the ones `setting` is written in.
const SETTINGS: &str = "SELECT name, setting, unit, boot_val, source, context, short_desc, \
     min_val, max_val, pending_restart::text FROM pg_settings";

pub(crate) async fn parameters(conn: &dyn Connection) -> Result<Vec<ServerParameter>> {
    let mut out: Vec<ServerParameter> = query_text(conn, SETTINGS)
        .await?
        .into_iter()
        .map(|r| {
            let (value, default, source) = (r.opt(1), r.opt(3), r.opt(4));
            let context = r.opt(5);
            ServerParameter {
                changed: source.as_deref() != Some("default")
                    && differs_from(value.as_deref(), default.as_deref()),
                statement: alter_system(&r.text(0), value.as_deref(), context.as_deref()),
                name: r.text(0),
                unit: r.opt(2),
                value,
                default,
                source,
                context,
                description: r.opt(6),
                min: r.opt(7),
                max: r.opt(8),
                pending_restart: r.flag(9),
            }
        })
        .collect();
    sort_parameters(&mut out);
    Ok(out)
}

/// `ALTER SYSTEM SET` that keeps the value: the setting is in the parameter's own unit, which
/// the server reads a bare number in. Settings of the `internal` context cannot be set.
fn alter_system(name: &str, value: Option<&str>, context: Option<&str>) -> Option<String> {
    let plain = !name.is_empty()
        && name
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' || c == '.');
    if context == Some("internal") || !plain {
        return None;
    }
    Some(format!(
        "ALTER SYSTEM SET {name} = {};",
        set_value(value?, false)
    ))
}

pub(crate) async fn metrics(conn: &dyn Connection) -> Result<ServerMetrics> {
    let mut out = ServerMetrics::none("PostgreSQL does not count it");
    connections(conn, &mut out).await;
    out.connections_max = Metric::from_result(
        query_row(conn, "SELECT current_setting('max_connections')")
            .await
            .map(|r| float_of(r.first().and_then(Option::as_ref))),
    );
    out.uptime_seconds = Metric::from_result(
        query_row(
            conn,
            "SELECT extract(epoch FROM clock_timestamp() - pg_postmaster_start_time())::float8::text",
        )
        .await
        .map(|r| float_of(r.first().and_then(Option::as_ref))),
    );
    counters(conn, &mut out).await;
    match query_sizes(
        conn,
        // The databases the session may not connect to cannot be measured.
        "SELECT datname, pg_database_size(oid)::text FROM pg_database \
         WHERE NOT datistemplate AND has_database_privilege(oid, 'CONNECT')",
    )
    .await
    {
        Ok(sizes) => out.databases = sizes,
        Err(e) => out.databases_unavailable = Some(e.to_string()),
    }
    match replication(conn).await {
        Ok(rows) => out.replication = rows,
        Err(e) => out.replication_unavailable = Some(e.to_string()),
    }
    Ok(out)
}

/// The other sessions running a statement, and those waiting for the client (open
/// transactions included). The session asking is left out: it is always running.
async fn connections(conn: &dyn Connection, out: &mut ServerMetrics) {
    let counted = query_row(
        conn,
        "SELECT (count(*) FILTER (WHERE state = 'active'))::text, \
         (count(*) FILTER (WHERE state LIKE 'idle%'))::text \
         FROM pg_stat_activity \
         WHERE backend_type = 'client backend' AND pid <> pg_backend_pid()",
    )
    .await;
    match counted {
        Ok(row) => {
            out.connections_active = Metric::from_text(row.opt(0).as_deref());
            out.connections_idle = Metric::from_text(row.opt(1).as_deref());
        }
        Err(e) => {
            out.connections_active = Metric::missing(e.to_string());
            out.connections_idle = Metric::missing(e.to_string());
        }
    }
}

/// Transactions and page reads since the statistics were reset, of all databases.
async fn counters(conn: &dyn Connection, out: &mut ServerMetrics) {
    let read = query_row(
        conn,
        "SELECT (sum(xact_commit + xact_rollback))::text, (sum(blks_hit))::text, \
         (sum(blks_read))::text FROM pg_stat_database",
    )
    .await;
    match read {
        Ok(row) => {
            out.transactions = Metric::from_text(row.opt(0).as_deref());
            out.cache_hits = Metric::from_text(row.opt(1).as_deref());
            out.cache_reads = Metric::from_text(row.opt(2).as_deref());
        }
        Err(e) => {
            out.transactions = Metric::missing(e.to_string());
            out.cache_hits = Metric::missing(e.to_string());
            out.cache_reads = Metric::missing(e.to_string());
        }
    }
}

/// The standbys this server feeds, and the primary it follows when it is a standby.
async fn replication(conn: &dyn Connection) -> Result<Vec<ReplicationRow>> {
    // Lag in bytes is what was sent and not yet replayed. The views list the other sessions'
    // rows only to a member of `pg_monitor`; the rest see their own, so none.
    let standbys = query_text(
        conn,
        "SELECT application_name, client_addr::text, state, sync_state, \
         extract(epoch FROM replay_lag)::float8::text, \
         pg_wal_lsn_diff(sent_lsn, replay_lsn)::text FROM pg_stat_replication",
    )
    .await?;
    let mut out: Vec<ReplicationRow> = standbys
        .into_iter()
        .map(|r| ReplicationRow {
            role: "standby".to_owned(),
            name: r.opt(0).map(|n| match r.opt(1) {
                Some(addr) => format!("{n} ({addr})"),
                None => n,
            }),
            state: r.opt(2),
            lag_seconds: float_of(r.get(4).and_then(Option::as_ref)),
            lag_bytes: r.number(5),
            detail: r.opt(3).map(|s| format!("sync_state: {s}")),
        })
        .collect();
    let receiver = query_text(
        conn,
        "SELECT sender_host, status, \
         extract(epoch FROM now() - pg_last_xact_replay_timestamp())::float8::text \
         FROM pg_stat_wal_receiver",
    )
    .await?;
    out.extend(receiver.into_iter().map(|r| ReplicationRow {
        role: "primary".to_owned(),
        name: r.opt(0),
        state: r.opt(1),
        lag_seconds: float_of(r.get(2).and_then(Option::as_ref)),
        ..ReplicationRow::default()
    }));
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn alter_system_keeps_the_value_in_the_settings_unit() {
        assert_eq!(
            alter_system("shared_buffers", Some("16384"), Some("postmaster")).unwrap(),
            "ALTER SYSTEM SET shared_buffers = 16384;"
        );
        assert_eq!(
            alter_system("search_path", Some("\"$user\", public"), Some("user")).unwrap(),
            "ALTER SYSTEM SET search_path = '\"$user\", public';"
        );
        assert_eq!(
            alter_system("log_line_prefix", Some("it's"), Some("sighup")).unwrap(),
            "ALTER SYSTEM SET log_line_prefix = 'it''s';"
        );
    }

    #[test]
    fn alter_system_skips_what_cannot_be_set() {
        assert!(alter_system("block_size", Some("8192"), Some("internal")).is_none());
        assert!(alter_system("odd name", Some("1"), Some("user")).is_none());
        assert!(alter_system("work_mem", None, Some("user")).is_none());
    }
}
