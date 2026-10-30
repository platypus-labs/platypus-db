//! The server's variables, with where each comes from, and its load, from the status
//! counters. MySQL 8 tells the source of a variable (`performance_schema.variables_info`),
//! MariaDB its default (`information_schema.SYSTEM_VARIABLES`); older MySQL only the value.

use std::collections::HashMap;

use platypus_core::{
    Connection, Metric, NamedRow, ReplicationRow, Result, ServerMetrics, ServerParameter,
    TextRowExt, differs_from, query_named, query_row, query_sizes, query_text, set_value,
    sort_parameters,
};
use platypus_sql::Dialect;

use crate::admin::mariadb;

/// MySQL 8: the value, and how the server was given it. `COMPILED` is the default.
const MYSQL_VARIABLES: &str = "SELECT v.VARIABLE_NAME, v.VARIABLE_VALUE, i.VARIABLE_SOURCE, \
     i.MIN_VALUE, i.MAX_VALUE FROM performance_schema.global_variables v \
     LEFT JOIN performance_schema.variables_info i ON i.VARIABLE_NAME = v.VARIABLE_NAME";

/// MariaDB: the value, the default, where it was set, whether it is read-only, its limits
/// and its description.
const MARIADB_VARIABLES: &str = "SELECT LOWER(VARIABLE_NAME), GLOBAL_VALUE, DEFAULT_VALUE, \
     GLOBAL_VALUE_ORIGIN, READ_ONLY, NUMERIC_MIN_VALUE, NUMERIC_MAX_VALUE, VARIABLE_COMMENT \
     FROM information_schema.SYSTEM_VARIABLES";

/// What every version has: the value alone.
const PLAIN_VARIABLES: &str = "SHOW GLOBAL VARIABLES";

pub(crate) async fn parameters(conn: &dyn Connection) -> Result<Vec<ServerParameter>> {
    let mut out = if mariadb(conn) {
        mariadb_parameters(conn).await?
    } else {
        match mysql_parameters(conn).await {
            Ok(list) => list,
            // `performance_schema` is off, or older than 8.0.
            Err(_) => plain_parameters(conn).await?,
        }
    };
    sort_parameters(&mut out);
    Ok(out)
}

/// `SET GLOBAL name = value;`, which the server refuses for a variable it only reads at
/// start.
fn set_global(name: &str, value: Option<&str>) -> Option<String> {
    Some(format!(
        "SET GLOBAL {name} = {};",
        set_value(value?, |v| Dialect::MySql.quote_literal(v))
    ))
}

/// A limit worth showing: the server writes `0` for both limits of a variable that has none.
fn limit(text: Option<String>) -> Option<String> {
    text.filter(|t| t != "NULL")
}

async fn mysql_parameters(conn: &dyn Connection) -> Result<Vec<ServerParameter>> {
    Ok(query_text(conn, MYSQL_VARIABLES)
        .await?
        .into_iter()
        .map(|r| {
            let source = r.opt(2);
            let unbounded = r.opt(3).as_deref() == Some("0") && r.opt(4).as_deref() == Some("0");
            ServerParameter {
                changed: source.as_deref().is_some_and(|s| s != "COMPILED"),
                statement: set_global(&r.text(0), r.opt(1).as_deref()),
                name: r.text(0),
                value: r.opt(1),
                source,
                min: r.opt(3).filter(|_| !unbounded),
                max: r.opt(4).filter(|_| !unbounded),
                ..ServerParameter::default()
            }
        })
        .collect())
}

async fn mariadb_parameters(conn: &dyn Connection) -> Result<Vec<ServerParameter>> {
    Ok(query_text(conn, MARIADB_VARIABLES)
        .await?
        .into_iter()
        .map(|r| {
            let (value, default, origin) = (r.opt(1), r.opt(2), r.opt(3));
            let read_only = r.opt(4).as_deref() == Some("YES");
            ServerParameter {
                changed: origin.as_deref() != Some("COMPILE-TIME")
                    && differs_from(value.as_deref(), default.as_deref()),
                statement: if read_only {
                    None
                } else {
                    set_global(&r.text(0), value.as_deref())
                },
                name: r.text(0),
                context: Some(if read_only { "read only" } else { "dynamic" }.to_owned()),
                value,
                default,
                source: origin,
                min: limit(r.opt(5)),
                max: limit(r.opt(6)),
                description: r.opt(7).filter(|d| !d.is_empty()),
                ..ServerParameter::default()
            }
        })
        .collect())
}

async fn plain_parameters(conn: &dyn Connection) -> Result<Vec<ServerParameter>> {
    Ok(query_text(conn, PLAIN_VARIABLES)
        .await?
        .into_iter()
        .map(|r| ServerParameter {
            statement: set_global(&r.text(0), r.opt(1).as_deref()),
            name: r.text(0),
            value: r.opt(1),
            ..ServerParameter::default()
        })
        .collect())
}

/// The status counters the snapshot is made of.
const STATUS: &str = "SHOW GLOBAL STATUS WHERE Variable_name IN ('Threads_connected', \
     'Threads_running', 'Uptime', 'Com_commit', 'Com_rollback', 'Questions', \
     'Innodb_buffer_pool_reads', 'Innodb_buffer_pool_read_requests')";

pub(crate) async fn metrics(conn: &dyn Connection) -> Result<ServerMetrics> {
    let mut out = ServerMetrics::none("the server does not report it");
    match status(conn).await {
        Ok(status) => fill(&mut out, &status),
        Err(e) => out = ServerMetrics::none(&e.to_string()),
    }
    out.connections_max = Metric::from_result(
        query_row(conn, "SELECT @@max_connections")
            .await
            .map(|r| platypus_core::float_of(r.first().and_then(Option::as_ref))),
    );
    out.set_databases(
        query_sizes(
            conn,
            "SELECT table_schema, CAST(SUM(data_length + index_length) AS UNSIGNED) \
         FROM information_schema.tables GROUP BY table_schema",
        )
        .await,
    );
    out.set_replication(replication(conn).await);
    Ok(out)
}

async fn status(conn: &dyn Connection) -> Result<HashMap<String, f64>> {
    Ok(query_text(conn, STATUS)
        .await?
        .into_iter()
        .filter_map(|r| Some((r.text(0), r.opt(1)?.trim().parse().ok()?)))
        .collect())
}

/// Reads the snapshot out of the counters. The session asking runs a statement, so it is
/// taken off the active ones. Explicit `COMMIT`/`ROLLBACK` are the transactions the server
/// counts: statements run under autocommit are not among them, `Questions` has all.
fn fill(out: &mut ServerMetrics, status: &HashMap<String, f64>) {
    let get = |name: &str| status.get(name).copied();
    let metric = |value: Option<f64>, name: &str| {
        value.map_or_else(
            || Metric::missing(format!("{name} is not in the status")),
            Metric::of,
        )
    };
    let (connected, running) = (get("Threads_connected"), get("Threads_running"));
    out.connections_active = metric(running.map(|r| (r - 1.0).max(0.0)), "Threads_running");
    out.connections_idle = metric(
        connected.zip(running).map(|(c, r)| (c - r).max(0.0)),
        "Threads_connected",
    );
    out.uptime_seconds = metric(get("Uptime"), "Uptime");
    out.transactions = metric(
        get("Com_commit").map(|c| c + get("Com_rollback").unwrap_or(0.0)),
        "Com_commit",
    );
    out.statements = metric(get("Questions"), "Questions");
    // A read request that is not a disk read found its page in the buffer pool.
    let (requests, reads) = (
        get("Innodb_buffer_pool_read_requests"),
        get("Innodb_buffer_pool_reads"),
    );
    out.cache_hits = metric(
        requests.zip(reads).map(|(q, r)| (q - r).max(0.0)),
        "Innodb_buffer_pool_reads",
    );
    out.cache_reads = metric(reads, "Innodb_buffer_pool_reads");
}

/// What this server replicates from. `SHOW REPLICA STATUS` since MySQL 8.0.22 and MariaDB
/// 10.5; the older servers only know `SHOW SLAVE STATUS`.
async fn replication(conn: &dyn Connection) -> Result<Vec<ReplicationRow>> {
    let rows = match query_named(conn, "SHOW REPLICA STATUS").await {
        Ok(rows) => rows,
        Err(first) => query_named(conn, "SHOW SLAVE STATUS")
            .await
            .map_err(|_| first)?,
    };
    Ok(rows.iter().map(replication_row).collect())
}

/// The first of `names` the status has, as MySQL names it since 8.0.22 or before.
fn field<'a>(row: &'a NamedRow, names: &[&str]) -> Option<&'a str> {
    names
        .iter()
        .find_map(|n| row.get(*n).and_then(Option::as_deref))
        .filter(|v| !v.is_empty())
}

fn replication_row(row: &NamedRow) -> ReplicationRow {
    let io = field(row, &["replica_io_running", "slave_io_running"]).unwrap_or("?");
    let sql = field(row, &["replica_sql_running", "slave_sql_running"]).unwrap_or("?");
    let host = field(row, &["source_host", "master_host"]);
    let port = field(row, &["source_port", "master_port"]);
    ReplicationRow {
        role: "primary".to_owned(),
        name: host.map(|h| port.map_or_else(|| h.to_owned(), |p| format!("{h}:{p}"))),
        state: Some(format!("IO: {io}, SQL: {sql}")),
        lag_seconds: field(row, &["seconds_behind_source", "seconds_behind_master"])
            .and_then(|s| s.parse().ok()),
        lag_bytes: None,
        detail: field(row, &["last_error"]).map(str::to_owned),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn status() -> HashMap<String, f64> {
        [
            ("Threads_connected", 5.0),
            ("Threads_running", 2.0),
            ("Uptime", 90.0),
            ("Com_commit", 10.0),
            ("Com_rollback", 2.0),
            ("Questions", 500.0),
            ("Innodb_buffer_pool_reads", 40.0),
            ("Innodb_buffer_pool_read_requests", 1040.0),
        ]
        .into_iter()
        .map(|(k, v)| (k.to_owned(), v))
        .collect()
    }

    #[test]
    fn counters_become_a_snapshot() {
        let mut out = ServerMetrics::none("x");
        fill(&mut out, &status());
        assert_eq!(out.connections_active.value, Some(1.0));
        assert_eq!(out.connections_idle.value, Some(3.0));
        assert_eq!(out.transactions.value, Some(12.0));
        assert_eq!(out.statements.value, Some(500.0));
        assert_eq!(out.cache_hits.value, Some(1000.0));
        assert_eq!(out.cache_reads.value, Some(40.0));
        assert_eq!(out.uptime_seconds.value, Some(90.0));
    }

    #[test]
    fn a_missing_counter_is_told() {
        let mut out = ServerMetrics::none("x");
        let mut partial = status();
        partial.remove("Com_commit");
        fill(&mut out, &partial);
        assert!(out.transactions.unavailable.unwrap().contains("Com_commit"));
        assert!(out.statements.value.is_some());
    }

    #[test]
    fn replication_reads_both_namings() {
        let row = |pairs: &[(&str, &str)]| -> NamedRow {
            pairs
                .iter()
                .map(|(k, v)| ((*k).to_owned(), Some((*v).to_owned())))
                .collect()
        };
        let new = replication_row(&row(&[
            ("replica_io_running", "Yes"),
            ("replica_sql_running", "No"),
            ("source_host", "db1"),
            ("source_port", "3306"),
            ("seconds_behind_source", "7"),
            ("last_error", "boom"),
        ]));
        assert_eq!(new.state.as_deref(), Some("IO: Yes, SQL: No"));
        assert_eq!(new.name.as_deref(), Some("db1:3306"));
        assert_eq!(new.lag_seconds, Some(7.0));
        assert_eq!(new.detail.as_deref(), Some("boom"));
        let old = replication_row(&row(&[
            ("slave_io_running", "Yes"),
            ("slave_sql_running", "Yes"),
            ("master_host", "db0"),
            ("seconds_behind_master", ""),
            ("last_error", ""),
        ]));
        assert_eq!(old.name.as_deref(), Some("db0"));
        assert_eq!(old.lag_seconds, None);
        assert_eq!(old.detail, None);
    }

    #[test]
    fn set_statements_quote_for_mysql() {
        assert_eq!(
            set_global("max_connections", Some("151")).unwrap(),
            "SET GLOBAL max_connections = 151;"
        );
        assert_eq!(
            set_global("sql_mode", Some("A,B")).unwrap(),
            "SET GLOBAL sql_mode = 'A,B';"
        );
        assert_eq!(
            set_global("x", Some(r"a\b")).unwrap(),
            // A backslash goes as hex: `NO_BACKSLASH_ESCAPES` would read it otherwise.
            "SET GLOBAL x = _utf8mb4 X'615c62';"
        );
        assert!(set_global("x", None).is_none());
    }
}
