//! The server's settings, from `system.settings` and `system.server_settings`, and its load,
//! from `system.metrics`, `system.events`, `system.parts` and `system.replicas`.

use platypus_core::{
    Connection, Metric, ReplicationRow, Result, ServerMetrics, ServerParameter, TextRowExt,
    float_of, query_row, query_sizes, query_text, set_value, sort_parameters,
};

/// The settings of the session: their value, their default and whether they differ. The
/// server writes descriptions over several lines.
const SETTINGS: &str = "SELECT name, value, default, toString(changed), description, min, max, \
     toString(readonly), type FROM system.settings";

/// The settings of the server itself, which only the configuration files change. Not before
/// ClickHouse 23.
const SERVER_SETTINGS: &str = "SELECT name, value, default, toString(changed), description, \
     toString(changeable_without_restart), type FROM system.server_settings";

pub(crate) async fn parameters(conn: &dyn Connection) -> Result<Vec<ServerParameter>> {
    let mut out: Vec<ServerParameter> = query_text(conn, SETTINGS)
        .await?
        .into_iter()
        .map(|r| {
            let value = r.opt(1);
            // 0 can be changed, 1 only read, 2 only by the one who wrote the constraint.
            let writable = r.opt(7).as_deref() == Some("0");
            ServerParameter {
                changed: r.flag(3),
                statement: value
                    .as_deref()
                    .filter(|_| writable)
                    .map(|v| format!("SET {} = {};", r.text(0), set_value(v, true))),
                name: r.text(0),
                value,
                default: r.opt(2),
                source: Some("setting".to_owned()),
                context: r.opt(8),
                description: r.opt(4).map(|d| one_line(&d)),
                min: r.opt(5),
                max: r.opt(6),
                ..ServerParameter::default()
            }
        })
        .collect();
    // Servers before 23 have no such table; their settings are all there is to show.
    if let Ok(rows) = query_text(conn, SERVER_SETTINGS).await {
        out.extend(rows.into_iter().map(|r| ServerParameter {
            changed: r.flag(3),
            name: r.text(0),
            value: r.opt(1),
            default: r.opt(2),
            source: Some("server setting".to_owned()),
            context: Some(if r.flag(5) { "dynamic" } else { "restart" }.to_owned()),
            description: r.opt(4).map(|d| one_line(&d)),
            ..ServerParameter::default()
        }));
    }
    sort_parameters(&mut out);
    Ok(out)
}

/// `text` on one line: the descriptions break at the width of the source file they come from.
fn one_line(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

pub(crate) async fn metrics(conn: &dyn Connection) -> Result<ServerMetrics> {
    let mut out = ServerMetrics::none("ClickHouse does not count it");
    connections(conn, &mut out).await;
    out.connections_max = scalar(
        conn,
        "SELECT value FROM system.server_settings WHERE name = 'max_connections'",
    )
    .await;
    out.uptime_seconds = scalar(conn, "SELECT toString(uptime())").await;
    out.statements = scalar(
        conn,
        "SELECT toString(value) FROM system.events WHERE event = 'Query'",
    )
    .await;
    out.transactions = Metric::missing("ClickHouse has no transactions to count");
    cache(conn, &mut out).await;
    match query_sizes(
        conn,
        "SELECT database, toString(sum(bytes_on_disk)) FROM system.parts \
         WHERE active GROUP BY database",
    )
    .await
    {
        Ok(sizes) => out.databases = sizes,
        Err(e) => out.databases_unavailable = Some(e.to_string()),
    }
    match replicas(conn).await {
        Ok(rows) => out.replication = rows,
        Err(e) => out.replication_unavailable = Some(e.to_string()),
    }
    Ok(out)
}

/// One number out of the first column of `sql`.
async fn scalar(conn: &dyn Connection, sql: &str) -> Metric {
    Metric::from_result(
        query_row(conn, sql)
            .await
            .map(|r| float_of(r.first().and_then(Option::as_ref))),
    )
}

/// The queries running are the active ones, the session asking among them; the other open
/// connections are idle.
async fn connections(conn: &dyn Connection, out: &mut ServerMetrics) {
    let read = query_row(
        conn,
        "SELECT toString(sumIf(value, metric = 'Query')), \
         toString(sumIf(value, metric IN ('TCPConnection', 'HTTPConnection', \
         'MySQLConnection', 'PostgreSQLConnection'))) FROM system.metrics",
    )
    .await;
    match read {
        Ok(row) => {
            let queries = float_of(row.first().and_then(Option::as_ref));
            let open = float_of(row.get(1).and_then(Option::as_ref));
            out.connections_active = Metric::from_result(Ok(queries.map(|q| (q - 1.0).max(0.0))));
            out.connections_idle =
                Metric::from_result(Ok(queries.zip(open).map(|(q, o)| (o - q).max(0.0))));
        }
        Err(e) => {
            out.connections_active = Metric::missing(e.to_string());
            out.connections_idle = Metric::missing(e.to_string());
        }
    }
}

/// The mark cache, which every read of a part goes through, is the cache the server counts.
async fn cache(conn: &dyn Connection, out: &mut ServerMetrics) {
    let read = query_row(
        conn,
        "SELECT toString(sumIf(value, event = 'MarkCacheHits')), \
         toString(sumIf(value, event = 'MarkCacheMisses')) FROM system.events",
    )
    .await;
    match read {
        Ok(row) => {
            out.cache_hits = Metric::from_text(row.opt(0).as_deref());
            out.cache_reads = Metric::from_text(row.opt(1).as_deref());
        }
        Err(e) => {
            out.cache_hits = Metric::missing(e.to_string());
            out.cache_reads = Metric::missing(e.to_string());
        }
    }
}

/// The replicated tables and how far each is behind.
async fn replicas(conn: &dyn Connection) -> Result<Vec<ReplicationRow>> {
    Ok(query_text(
        conn,
        "SELECT concat(database, '.', table), \
         multiIf(is_readonly, 'readonly', is_session_expired, 'session expired', 'online'), \
         toString(absolute_delay), last_queue_update_exception \
         FROM system.replicas ORDER BY database, table",
    )
    .await?
    .into_iter()
    .map(|r| ReplicationRow {
        role: "table".to_owned(),
        name: r.opt(0),
        state: r.opt(1),
        lag_seconds: float_of(r.get(2).and_then(Option::as_ref)),
        detail: r.opt(3).filter(|d| !d.is_empty()),
        ..ReplicationRow::default()
    })
    .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn descriptions_are_one_line() {
        assert_eq!(one_line("\n   a  b\n  c "), "a b c");
        assert_eq!(one_line(""), "");
    }
}
