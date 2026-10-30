//! A snapshot of how busy a database server is: connections, transaction and cache
//! counters, uptime, the size of each database and its replication. Read by
//! [`Connection::server_metrics`]; the counters only grow, so a rate comes from two
//! snapshots, which the dashboard of the admin tab takes some seconds apart.
//!
//! [`Connection::server_metrics`]: crate::Connection::server_metrics

use serde::Serialize;

use crate::{Connection, Result, TextRow, TextRowExt, query_text};

/// One number of a snapshot, or the reason the server did not give it (a missing privilege,
/// a statistic the server does not keep).
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct Metric {
    pub value: Option<f64>,
    /// Why there is no value; `None` when there is one.
    pub unavailable: Option<String>,
}

impl Metric {
    /// A value read.
    #[must_use]
    pub fn of(value: f64) -> Self {
        Self {
            value: Some(value),
            unavailable: None,
        }
    }

    /// No value, for `reason`.
    #[must_use]
    pub fn missing(reason: impl Into<String>) -> Self {
        Self {
            value: None,
            unavailable: Some(reason.into()),
        }
    }

    /// The outcome of reading a metric: the number, or what went wrong as the reason. A
    /// query that returns NULL says the server keeps no such figure.
    #[must_use]
    pub fn from_result(result: Result<Option<f64>>) -> Self {
        match result {
            Ok(Some(v)) if v.is_finite() => Self::of(v),
            Ok(_) => Self::missing("the server does not report it"),
            Err(e) => Self::missing(e.to_string()),
        }
    }

    /// A number the server wrote as text.
    #[must_use]
    pub fn from_text(text: Option<&str>) -> Self {
        Self::from_result(Ok(text.and_then(|t| t.trim().parse().ok())))
    }
}

/// The first row of `sql`, or an empty row when it returns none: the figures of a snapshot
/// come a query each, so that one the session may not read leaves the others.
///
/// # Errors
/// When the query fails.
pub async fn query_row(conn: &dyn Connection, sql: &str) -> Result<TextRow> {
    Ok(query_text(conn, sql)
        .await?
        .into_iter()
        .next()
        .unwrap_or_default())
}

/// The sizes `sql` reads, one a row: database name, bytes; the biggest first.
///
/// # Errors
/// When the query fails.
pub async fn query_sizes(conn: &dyn Connection, sql: &str) -> Result<Vec<DatabaseSize>> {
    let mut sizes: Vec<DatabaseSize> = query_text(conn, sql)
        .await?
        .into_iter()
        .map(|r| DatabaseSize {
            name: r.text(0),
            bytes: r.number(1).unwrap_or(0),
        })
        .collect();
    sizes.sort_by(|a, b| b.bytes.cmp(&a.bytes).then_with(|| a.name.cmp(&b.name)));
    Ok(sizes)
}

/// `text` as a number, if it is one.
#[must_use]
pub fn float_of(text: Option<&String>) -> Option<f64> {
    text?.trim().parse().ok().filter(|v: &f64| v.is_finite())
}

/// The size on disk of one database of the server.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct DatabaseSize {
    pub name: String,
    pub bytes: u64,
}

/// One link of the server's replication: a standby it feeds, or the primary it follows.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct ReplicationRow {
    /// `standby` for a replica this server feeds, `primary` for the server it follows,
    /// `group` for a member of an availability group, `table` for a replicated table.
    pub role: String,
    /// The replica, source, group or table.
    pub name: Option<String>,
    /// As the server names it: `streaming`, `Yes`, `SYNCHRONIZED`, …
    pub state: Option<String>,
    /// How far behind it is, in seconds.
    pub lag_seconds: Option<f64>,
    /// How far behind it is, in bytes of log.
    pub lag_bytes: Option<u64>,
    /// An error or a note the server gives.
    pub detail: Option<String>,
}

/// What [`Connection::server_metrics`](crate::Connection::server_metrics) reads.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct ServerMetrics {
    /// Client connections running a statement.
    pub connections_active: Metric,
    /// Client connections waiting for the client.
    pub connections_idle: Metric,
    /// How many the server accepts.
    pub connections_max: Metric,
    pub uptime_seconds: Metric,
    /// Transactions committed and rolled back since the start (a counter).
    pub transactions: Metric,
    /// Statements or batches run since the start (a counter).
    pub statements: Metric,
    /// Reads of a page found in the cache, since the start (a counter).
    pub cache_hits: Metric,
    /// Reads of a page that went to disk, since the start (a counter).
    pub cache_reads: Metric,
    pub databases: Vec<DatabaseSize>,
    /// Why `databases` is empty, where the server refused.
    pub databases_unavailable: Option<String>,
    pub replication: Vec<ReplicationRow>,
    /// Why the replication could not be read; not set when the server replicates nothing.
    pub replication_unavailable: Option<String>,
}

impl ServerMetrics {
    /// A snapshot with every metric missing for `reason`, to fill in what is read.
    #[must_use]
    pub fn none(reason: &str) -> Self {
        let missing = || Metric::missing(reason);
        Self {
            connections_active: missing(),
            connections_idle: missing(),
            connections_max: missing(),
            uptime_seconds: missing(),
            transactions: missing(),
            statements: missing(),
            cache_hits: missing(),
            cache_reads: missing(),
            ..Self::default()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Error;

    #[test]
    fn a_metric_is_a_number_or_a_reason() {
        assert_eq!(Metric::from_text(Some(" 12.5 ")), Metric::of(12.5));
        assert!(Metric::from_text(None).unavailable.is_some());
        assert!(Metric::from_text(Some("abc")).value.is_none());
        let denied = Metric::from_result(Err(Error::Other("permission denied".to_owned())));
        assert!(denied.unavailable.unwrap().contains("permission denied"));
        assert!(Metric::from_result(Ok(Some(f64::NAN))).value.is_none());
    }

    #[test]
    fn floats_come_from_text() {
        assert_eq!(float_of(Some(&"3".to_owned())), Some(3.0));
        assert_eq!(float_of(Some(&"inf".to_owned())), None);
        assert_eq!(float_of(None), None);
    }

    #[test]
    fn a_snapshot_without_data_says_why() {
        let none = ServerMetrics::none("not kept");
        assert_eq!(none.transactions.unavailable.as_deref(), Some("not kept"));
        assert_eq!(none.databases.len(), 0);
    }
}
