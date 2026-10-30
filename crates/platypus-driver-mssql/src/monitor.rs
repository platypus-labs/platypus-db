//! The server's configuration options, from `sys.configurations`, and its load, from the
//! dynamic management views (`VIEW SERVER STATE` shows the sessions of other logins).

use platypus_core::{
    Connection, Metric, ReplicationRow, Result, ServerMetrics, ServerParameter, TextRowExt,
    differs_from, float_of, query_row, query_sizes, query_text, set_value, sort_parameters,
    string_literal,
};

/// What each option is when nobody changed it: read from a server that never was
/// configured, and in the documentation of `sp_configure`. The catalog has no such column.
/// Options a service switches on by itself (`Agent XPs`, `Database Mail XPs`) are left out.
const DEFAULTS: &[(&str, &str)] = &[
    ("access check cache bucket count", "0"),
    ("access check cache quota", "0"),
    ("Ad Hoc Distributed Queries", "0"),
    ("ADR cleaner retry timeout (min)", "15"),
    ("ADR Cleaner Thread Count", "1"),
    ("ADR Preallocation Factor", "4"),
    ("affinity I/O mask", "0"),
    ("affinity mask", "0"),
    ("affinity64 I/O mask", "0"),
    ("affinity64 mask", "0"),
    ("allow filesystem enumeration", "1"),
    ("allow polybase export", "0"),
    ("allow updates", "0"),
    ("automatic soft-NUMA disabled", "0"),
    ("backup checksum default", "0"),
    ("backup compression algorithm", "0"),
    ("backup compression default", "0"),
    ("blocked process threshold (s)", "0"),
    ("c2 audit mode", "0"),
    ("clr enabled", "0"),
    ("clr strict security", "1"),
    ("column encryption enclave type", "0"),
    ("common criteria compliance enabled", "0"),
    ("contained database authentication", "0"),
    ("cost threshold for parallelism", "5"),
    ("cross db ownership chaining", "0"),
    ("cursor threshold", "-1"),
    ("Data processed daily limit in TB", "2147483647"),
    ("Data processed monthly limit in TB", "2147483647"),
    ("Data processed weekly limit in TB", "2147483647"),
    ("default full-text language", "1033"),
    ("default language", "0"),
    ("default trace enabled", "1"),
    ("disallow results from triggers", "0"),
    ("EKM provider enabled", "0"),
    ("external scripts enabled", "0"),
    ("external xtp dll gen util enabled", "0"),
    ("filestream access level", "0"),
    ("fill factor (%)", "0"),
    ("ft crawl bandwidth (max)", "100"),
    ("ft crawl bandwidth (min)", "0"),
    ("ft notify bandwidth (max)", "100"),
    ("ft notify bandwidth (min)", "0"),
    ("hadoop connectivity", "0"),
    ("hardware offload config", "0"),
    ("hardware offload enabled", "0"),
    ("hardware offload mode", "0"),
    ("index create memory (KB)", "0"),
    ("in-doubt xact resolution", "0"),
    ("lightweight pooling", "0"),
    ("locks", "0"),
    ("max degree of parallelism", "0"),
    ("max full-text crawl range", "4"),
    ("max RPC request params (KB)", "0"),
    ("max server memory (MB)", "2147483647"),
    ("max text repl size (B)", "65536"),
    ("max worker threads", "0"),
    ("media retention", "0"),
    ("min memory per query (KB)", "1024"),
    ("min server memory (MB)", "0"),
    ("nested triggers", "1"),
    ("network packet size (B)", "4096"),
    ("Ole Automation Procedures", "0"),
    ("open objects", "0"),
    ("openrowset auto_create_statistics", "1"),
    ("optimize for ad hoc workloads", "0"),
    ("PH timeout (s)", "60"),
    ("polybase enabled", "0"),
    ("polybase network encryption", "1"),
    ("precompute rank", "0"),
    ("priority boost", "0"),
    ("query governor cost limit", "0"),
    ("query wait (s)", "-1"),
    ("recovery interval (min)", "0"),
    ("remote access", "1"),
    ("remote admin connections", "0"),
    ("remote data archive", "0"),
    ("remote login timeout (s)", "10"),
    ("remote proc trans", "0"),
    ("remote query timeout (s)", "600"),
    ("scan for startup procs", "0"),
    ("server trigger recursion", "1"),
    ("set working set size", "0"),
    ("show advanced options", "0"),
    ("SMO and DMO XPs", "1"),
    ("suppress recovery model errors", "0"),
    ("tempdb metadata memory-optimized", "0"),
    ("transform noise words", "0"),
    ("two digit year cutoff", "2049"),
    ("user connections", "0"),
    ("user options", "0"),
    ("xp_cmdshell", "0"),
];

/// The default of option `name`, if it is one of [`DEFAULTS`].
fn default_of(name: &str) -> Option<&'static str> {
    DEFAULTS
        .iter()
        .find(|(n, _)| n.eq_ignore_ascii_case(name))
        .map(|(_, d)| *d)
}

/// The value in use, then the one configured, as numbers.
const OPTIONS: &str = "SELECT name, CAST(value_in_use AS varchar(30)), CAST(value AS varchar(30)), \
     CAST(minimum AS varchar(30)), CAST(maximum AS varchar(30)), description, \
     CAST(is_dynamic AS varchar(1)) FROM sys.configurations";

pub(crate) async fn parameters(conn: &dyn Connection) -> Result<Vec<ServerParameter>> {
    let mut out: Vec<ServerParameter> = query_text(conn, OPTIONS)
        .await?
        .into_iter()
        .map(|r| {
            let (name, in_use, configured) = (r.text(0), r.opt(1), r.opt(2));
            let default = default_of(&name).map(str::to_owned);
            let waiting = in_use != configured;
            ServerParameter {
                changed: differs_from(in_use.as_deref(), default.as_deref()),
                unit: unit_of(&name),
                statement: configure(&name, configured.as_deref()),
                context: Some(if r.flag(6) { "dynamic" } else { "restart" }.to_owned()),
                source: waiting
                    .then(|| format!("configured: {}", configured.clone().unwrap_or_default())),
                pending_restart: waiting,
                name,
                value: in_use,
                default,
                description: r.opt(5),
                min: r.opt(3),
                max: r.opt(4),
            }
        })
        .collect();
    sort_parameters(&mut out);
    Ok(out)
}

/// The unit an option's name ends with: `max server memory (MB)` counts in `MB`.
fn unit_of(name: &str) -> Option<String> {
    let inner = name.strip_suffix(')')?.rsplit_once(" (")?.1;
    (!inner.is_empty()).then(|| inner.to_owned())
}

/// The call that sets option `name` to `value`; the change waits for `RECONFIGURE`.
fn configure(name: &str, value: Option<&str>) -> Option<String> {
    Some(format!(
        "EXEC sp_configure N{}, {};\nRECONFIGURE;",
        string_literal(name, false),
        set_value(value?, false)
    ))
}

pub(crate) async fn metrics(conn: &dyn Connection) -> Result<ServerMetrics> {
    let mut out = ServerMetrics::none("SQL Server does not count it");
    sessions(conn, &mut out).await;
    out.connections_max = scalar(
        conn,
        "SELECT CAST(CASE WHEN value_in_use = 0 THEN @@MAX_CONNECTIONS ELSE value_in_use END \
         AS varchar(20)) FROM sys.configurations WHERE name = 'user connections'",
    )
    .await;
    out.uptime_seconds = scalar(
        conn,
        "SELECT CAST(DATEDIFF(SECOND, sqlserver_start_time, SYSDATETIME()) AS varchar(20)) \
         FROM sys.dm_os_sys_info",
    )
    .await;
    out.transactions = counter(conn, "Databases", "Transactions/sec", Some("_Total")).await;
    out.statements = counter(conn, "SQL Statistics", "Batch Requests/sec", None).await;
    // The buffer cache hit ratio is a fraction the server keeps, not two counters that grow:
    // the dashboard cannot take a rate of it.
    out.cache_hits = Metric::missing("SQL Server reports a hit ratio, not counters of hits");
    out.cache_reads = out.cache_hits.clone();
    out.set_databases(
        query_sizes(
            conn,
            "SELECT d.name, CAST(SUM(CAST(f.size AS bigint)) * 8192 AS varchar(30)) \
         FROM sys.master_files f JOIN sys.databases d ON d.database_id = f.database_id \
         GROUP BY d.name",
        )
        .await,
    );
    out.set_replication(availability_groups(conn).await);
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

/// The other user sessions running a request, and those that do not.
async fn sessions(conn: &dyn Connection, out: &mut ServerMetrics) {
    let counted = query_row(
        conn,
        "SELECT CAST(COUNT(CASE WHEN status = 'running' THEN 1 END) AS varchar(20)), \
         CAST(COUNT(CASE WHEN status <> 'running' THEN 1 END) AS varchar(20)) \
         FROM sys.dm_exec_sessions WHERE is_user_process = 1 AND session_id <> @@SPID",
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

/// A performance counter of the object `SQLServer:<object>` (or the named instance's own
/// prefix), which grows since the server started.
async fn counter(
    conn: &dyn Connection,
    object: &str,
    name: &str,
    instance: Option<&str>,
) -> Metric {
    let instance = instance.map_or_else(String::new, |i| {
        format!(" AND RTRIM(instance_name) = {}", string_literal(i, false))
    });
    scalar(
        conn,
        &format!(
            "SELECT CAST(SUM(cntr_value) AS varchar(30)) FROM sys.dm_os_performance_counters \
             WHERE RTRIM(object_name) LIKE '%:{object}' AND RTRIM(counter_name) = '{name}'{instance}"
        ),
    )
    .await
}

/// The replicas of the availability groups the server takes part in; none where it has no
/// Always On.
async fn availability_groups(conn: &dyn Connection) -> Result<Vec<ReplicationRow>> {
    Ok(query_text(
        conn,
        "SELECT g.name, r.replica_server_name, s.role_desc, s.synchronization_health_desc, \
         s.connected_state_desc, s.last_connect_error_description \
         FROM sys.dm_hadr_availability_replica_states s \
         JOIN sys.availability_replicas r ON r.replica_id = s.replica_id \
         JOIN sys.availability_groups g ON g.group_id = s.group_id \
         ORDER BY g.name, r.replica_server_name",
    )
    .await?
    .into_iter()
    .map(|r| ReplicationRow {
        role: "group".to_owned(),
        name: Some(format!("{} / {}", r.text(0), r.text(1))),
        state: Some(format!("{} · {}", r.text(2), r.text(3))),
        detail: r.opt(5).filter(|d| !d.is_empty()).or_else(|| r.opt(4)),
        ..ReplicationRow::default()
    })
    .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn options_have_their_unit_in_the_name() {
        assert_eq!(unit_of("max server memory (MB)").as_deref(), Some("MB"));
        assert_eq!(unit_of("fill factor (%)").as_deref(), Some("%"));
        assert_eq!(unit_of("max degree of parallelism"), None);
    }

    #[test]
    fn defaults_ignore_case() {
        assert_eq!(default_of("MAX DEGREE OF PARALLELISM"), Some("0"));
        assert_eq!(default_of("cost threshold for parallelism"), Some("5"));
        assert_eq!(default_of("no such option"), None);
    }

    #[test]
    fn configure_quotes_the_name() {
        assert_eq!(
            configure("cost threshold for parallelism", Some("50")).unwrap(),
            "EXEC sp_configure N'cost threshold for parallelism', 50;\nRECONFIGURE;"
        );
        assert!(configure("x", None).is_none());
    }
}
