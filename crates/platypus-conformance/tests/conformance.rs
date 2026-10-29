//! The same scenarios against every driver: PostgreSQL, MySQL, MariaDB, SQL Server,
//! ClickHouse, SQLite, DuckDB and libSQL.
//!
//! Server databases come from `docker/compose.yml` (SQL Server and libSQL from their
//! `sqlserver` and `libsql` profiles). Setting `PLATYPUS_TEST_PG_URL`,
//! `PLATYPUS_TEST_MYSQL_URL`, `PLATYPUS_TEST_MARIADB_URL`, `PLATYPUS_TEST_MSSQL_URL`,
//! `PLATYPUS_TEST_CLICKHOUSE_URL` or `PLATYPUS_TEST_LIBSQL_URL` makes that database required
//! (CI does); otherwise an unreachable default database is skipped. SQLite and DuckDB always
//! run, on temporary files. `PLATYPUS_TEST_ONLY` (a backend's name, e.g. `mysql`) leaves out
//! the others.

use std::time::Duration;

use arrow_array::RecordBatch;
use arrow_array::cast::AsArray;
use arrow_cast::display::array_value_to_string;
use futures::{StreamExt, TryStreamExt};
use platypus_core::geometry;
use platypus_core::{
    BulkRows, Connection, ConnectionConfig, ConstraintKind, Driver, DriverKind, EndSession, Error,
    ExecOptions, ExecResult, Maintenance, MatchKind, ObjectKind, ObjectRef, PlanNode,
    ServerSession, ServerVersion, SessionInit, StatisticsKind,
};
use platypus_driver_clickhouse::ClickHouseDriver;
use platypus_driver_duckdb::DuckDbDriver;
use platypus_driver_libsql::LibsqlDriver;
use platypus_driver_mongo::MongoDriver;
use platypus_driver_mssql::MssqlDriver;
use platypus_driver_mysql::MySqlDriver;
use platypus_driver_pg::PgDriver;
use platypus_driver_redis::RedisDriver;
use platypus_driver_sqlite::SqliteDriver;
use platypus_mcp::query::{Limits, Outcome};
use platypus_sql::generated::Generated;
use tempfile::TempDir;

const SERIES_ROWS: usize = 100_000;

struct Backend {
    name: &'static str,
    product: &'static str,
    config: ConnectionConfig,
    /// Schema holding the demo tables `customers`, `orders` and the view `customer_totals`.
    schema: String,
    /// `SERIES_ROWS` rows with columns `id` (1..) and `label`.
    series_sql: String,
    /// A statement that runs for far longer than the tests wait.
    endless_sql: &'static str,
    /// `(session id query, kill statement prefix)`; `None` for SQLite and DuckDB, which
    /// have no server.
    kill: Option<(&'static str, &'static str)>,
    /// Keeps the SQLite or DuckDB file alive.
    _dir: Option<TempDir>,
}

impl Backend {
    /// SQLite, or libSQL over HTTP: the same SQL and catalog.
    fn speaks_sqlite(&self) -> bool {
        matches!(self.config.driver, DriverKind::Sqlite | DriverKind::Libsql)
    }

    async fn connect(&self) -> Box<dyn Connection> {
        driver(self.config.driver)
            .connect(&self.config)
            .await
            .unwrap_or_else(|e| panic!("{}: cannot connect: {e}", self.name))
    }

    /// `TEMPORARY ` where the server has temporary tables: a libSQL server takes none, and
    /// gets plain tables, dropped when the tests start.
    fn temporary(&self) -> &'static str {
        if self.config.driver == DriverKind::Libsql {
            ""
        } else {
            "TEMPORARY "
        }
    }

    /// The name of the temporary table `name`: SQL Server's start with `#`.
    fn temp(&self, name: &str) -> String {
        if self.config.driver == DriverKind::SqlServer {
            format!("#{name}")
        } else {
            name.to_owned()
        }
    }

    /// Whether the database has foreign keys and unique constraints: ClickHouse has
    /// neither, its primary key only orders the data.
    fn keys(&self) -> bool {
        self.config.driver != DriverKind::ClickHouse
    }

    /// What follows the column list of a `CREATE TABLE`: ClickHouse needs an engine.
    fn table_options(&self, order_by: &str) -> String {
        if self.config.driver == DriverKind::ClickHouse {
            format!(" ENGINE = MergeTree ORDER BY {order_by}")
        } else {
            String::new()
        }
    }

    /// Creates the temporary table `name`, visible to the session only.
    fn create_temp(&self, name: &str, columns: &str) -> String {
        if self.config.driver == DriverKind::SqlServer {
            format!("CREATE TABLE {} ({columns})", self.temp(name))
        } else {
            format!("CREATE {}TABLE {name} ({columns})", self.temporary())
        }
    }
}

/// The drivers reached through a bridge, as a pattern: they have their own tests.
macro_rules! bridged {
    () => {
        DriverKind::Snowflake
            | DriverKind::BigQuery
            | DriverKind::Databricks
            | DriverKind::Trino
            | DriverKind::Adbc
            | DriverKind::Oracle
            | DriverKind::Db2
            | DriverKind::Odbc
    };
}

const BRIDGED: &str = "drivers with a bridge or a vendor client have their own tests in      bridges.rs, oracle.rs and odbc.rs";

fn driver(kind: DriverKind) -> &'static dyn Driver {
    match kind {
        DriverKind::Postgres => &PgDriver,
        DriverKind::MySql => &MySqlDriver,
        DriverKind::Sqlite => &SqliteDriver,
        DriverKind::Libsql => &LibsqlDriver,
        DriverKind::DuckDb => &DuckDbDriver,
        DriverKind::SqlServer => &MssqlDriver,
        DriverKind::ClickHouse => &ClickHouseDriver,
        DriverKind::Mongo => &MongoDriver,
        DriverKind::Redis => &RedisDriver,
        bridged!() => unreachable!("{BRIDGED}"),
    }
}

/// The config for a server from `env`, or `default` when unset; `None` when the default
/// server is unreachable.
async fn server_config(env: &str, default: &str, name: &str) -> Option<ConnectionConfig> {
    let (url, required) = match std::env::var(env) {
        Ok(url) => (url, true),
        Err(_) => (default.to_owned(), false),
    };
    let config = ConnectionConfig::from_url(&url).unwrap();
    match driver(config.driver).connect(&config).await {
        Ok(_) => Some(config),
        Err(e) if !required => {
            eprintln!("skipping {name}: unreachable at {default}: {e}");
            None
        }
        Err(e) => panic!("cannot connect to {env}: {e}"),
    }
}

const MYSQL_SERIES: &str = "SELECT 1 + a.d + 10 * b.d + 100 * c.d + 1000 * e.d + 10000 * f.d AS id,
       CONCAT('row ', a.d) AS label
FROM digits a, digits b, digits c, digits e, digits f";

// `COUNT(*)` of a cross join is computed without scanning by MySQL 8; `SUM` is not.
const MYSQL_ENDLESS: &str = "SELECT SUM(a.d + j.d) FROM digits a, digits b, digits c, digits d, \
     digits e, digits f, digits g, digits h, digits i, digits j";

const MSSQL_SERIES: &str = "WITH digits (d) AS (
    SELECT d FROM (VALUES (0), (1), (2), (3), (4), (5), (6), (7), (8), (9)) v (d)
)
SELECT 1 + a.d + 10 * b.d + 100 * c.d + 1000 * e.d + 10000 * f.d AS id,
       CONCAT('row ', a.d) AS label
FROM digits a, digits b, digits c, digits e, digits f";

/// The demo schema of `docker/postgres` and `docker/mysql` for SQL Server, whose image
/// runs no seed scripts. Created once per database; the application lock, held by the
/// session, keeps concurrent tests from creating it twice.
const MSSQL_FIXTURE: &[&str] = &[
    "EXEC sp_getapplock @Resource = 'platypus_conformance', @LockMode = 'Exclusive', \
     @LockOwner = 'Session'",
    "IF SCHEMA_ID('demo') IS NULL EXEC('CREATE SCHEMA demo')",
    "IF OBJECT_ID('demo.customers') IS NULL
     BEGIN
         CREATE TABLE demo.customers (
             id INT IDENTITY PRIMARY KEY,
             email NVARCHAR(255) NOT NULL UNIQUE,
             full_name NVARCHAR(255) NOT NULL
         );
         CREATE TABLE demo.orders (
             id INT IDENTITY PRIMARY KEY,
             customer_id INT NOT NULL REFERENCES demo.customers (id),
             total DECIMAL(12, 2) NOT NULL
         );
         CREATE INDEX orders_customer_id_idx ON demo.orders (customer_id);
         EXEC('CREATE VIEW demo.customer_totals AS
               SELECT c.id, COUNT(o.id) AS orders
               FROM demo.customers c LEFT JOIN demo.orders o ON o.customer_id = c.id
               GROUP BY c.id');
         INSERT INTO demo.customers (email, full_name)
         VALUES (N'ada@example.com', N'Ada'), (N'bob@example.com', N'Bob');
         INSERT INTO demo.orders (customer_id, total) VALUES (1, 10.50), (1, 4.25), (2, 7.00);
     END",
    "EXEC sp_releaseapplock @Resource = 'platypus_conformance', @LockOwner = 'Session'",
];

async fn mssql_backend() -> Option<Backend> {
    let config = server_config(
        "PLATYPUS_TEST_MSSQL_URL",
        "sqlserver://sa:Platypus-2026@localhost:51433/platypus?sslmode=require",
        "SQL Server",
    )
    .await?;
    let conn = MssqlDriver.connect(&config).await.unwrap();
    for sql in MSSQL_FIXTURE {
        run(&*conn, sql)
            .await
            .unwrap_or_else(|e| panic!("sqlserver: {sql}: {e}"));
    }
    Some(Backend {
        name: "sqlserver",
        product: "SQL Server",
        config,
        schema: "demo".to_owned(),
        series_sql: MSSQL_SERIES.to_owned(),
        endless_sql: "WAITFOR DELAY '00:10:00'",
        kill: Some(("SELECT @@SPID", "KILL")),
        _dir: None,
    })
}

/// The demo tables in ClickHouse, whose image runs no seed scripts. A skipping index
/// stands in for the index on `customer_id`; there are no foreign keys or unique keys.
const CLICKHOUSE_FIXTURE: &[&str] = &[
    "CREATE TABLE IF NOT EXISTS customers (id Int32, email String, full_name String)
     ENGINE = MergeTree ORDER BY id",
    "CREATE TABLE IF NOT EXISTS orders (
        id Int32,
        customer_id Int32,
        total Decimal(12, 2),
        INDEX orders_customer_id_idx customer_id TYPE minmax GRANULARITY 1
     ) ENGINE = MergeTree ORDER BY id",
    "CREATE VIEW IF NOT EXISTS customer_totals AS
     SELECT c.id, count(o.id) AS orders
     FROM customers c LEFT JOIN orders o ON o.customer_id = c.id
     GROUP BY c.id",
    "INSERT INTO customers
     SELECT * FROM values('id Int32, email String, full_name String',
                          (1, 'ada@example.com', 'Ada'), (2, 'bob@example.com', 'Bob'))
     WHERE (SELECT count() FROM customers) = 0",
    "INSERT INTO orders
     SELECT * FROM values('id Int32, customer_id Int32, total Decimal(12, 2)',
                          (1, 1, 10.50), (2, 1, 4.25), (3, 2, 7.00))
     WHERE (SELECT count() FROM orders) = 0",
];

async fn clickhouse_backend() -> Option<Backend> {
    let config = server_config(
        "PLATYPUS_TEST_CLICKHOUSE_URL",
        "clickhouse://platypus:platypus@localhost:58123/platypus",
        "ClickHouse",
    )
    .await?;
    {
        let _seeding = SEEDING.lock().await;
        let conn = ClickHouseDriver.connect(&config).await.unwrap();
        for sql in CLICKHOUSE_FIXTURE {
            run(&*conn, sql)
                .await
                .unwrap_or_else(|e| panic!("clickhouse: {sql}: {e}"));
        }
    }
    Some(Backend {
        name: "clickhouse",
        product: "ClickHouse",
        schema: config.database.clone(),
        config,
        series_sql: format!(
            "SELECT number + 1 AS id, concat('row ', toString(number + 1)) AS label \
             FROM numbers({SERIES_ROWS})"
        ),
        endless_sql: "SELECT count() FROM system.numbers",
        // An HTTP session cannot be ended from another client.
        kill: None,
        _dir: None,
    })
}

const SQLITE_FIXTURE: &[&str] = &[
    "CREATE TABLE customers (
        id INTEGER PRIMARY KEY,
        email TEXT NOT NULL UNIQUE,
        full_name TEXT NOT NULL
    )",
    "CREATE TABLE orders (
        id INTEGER PRIMARY KEY,
        customer_id INTEGER NOT NULL REFERENCES customers (id),
        total NUMERIC(12, 2) NOT NULL
    )",
    "CREATE INDEX orders_customer_id_idx ON orders (customer_id)",
    "CREATE VIEW customer_totals AS
     SELECT c.id, count(o.id) AS orders
     FROM customers c LEFT JOIN orders o ON o.customer_id = c.id
     GROUP BY c.id",
];

fn sqlite_series() -> String {
    format!(
        "WITH RECURSIVE s(id) AS (SELECT 1 UNION ALL SELECT id + 1 FROM s WHERE id < {SERIES_ROWS})
         SELECT id, 'row ' || id AS label FROM s"
    )
}

const SQLITE_ENDLESS: &str =
    "WITH RECURSIVE s(n) AS (SELECT 1 UNION ALL SELECT n + 1 FROM s) SELECT count(*) FROM s";

async fn sqlite_backend() -> Backend {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("conformance.db");
    std::fs::File::create(&path).unwrap();
    let mut config = ConnectionConfig::new(DriverKind::Sqlite);
    config.database = path.to_string_lossy().into_owned();
    let conn = SqliteDriver.connect(&config).await.unwrap();
    for sql in SQLITE_FIXTURE {
        conn.execute(sql, &ExecOptions::default()).await.unwrap();
    }
    Backend {
        name: "sqlite",
        product: "SQLite",
        config,
        schema: "main".to_owned(),
        series_sql: sqlite_series(),
        endless_sql: SQLITE_ENDLESS,
        kill: None,
        _dir: Some(dir),
    }
}

const DUCKDB_FIXTURE: &[&str] = &[
    "CREATE TABLE customers (
        id INTEGER PRIMARY KEY,
        email VARCHAR NOT NULL UNIQUE,
        full_name VARCHAR NOT NULL
    )",
    "CREATE TABLE orders (
        id INTEGER PRIMARY KEY,
        customer_id INTEGER NOT NULL REFERENCES customers (id),
        total DECIMAL(12, 2) NOT NULL
    )",
    "CREATE INDEX orders_customer_id_idx ON orders (customer_id)",
    "CREATE VIEW customer_totals AS
     SELECT c.id, count(o.id) AS orders
     FROM customers c LEFT JOIN orders o ON o.customer_id = c.id
     GROUP BY c.id",
];

/// The test-only token of the `libsql` service in `docker/compose.yml`.
const LIBSQL_TOKEN: &str = "eyJhbGciOiJFZERTQSIsInR5cCI6IkpXVCJ9.eyJhIjoicnciLCJpYXQiOjE3NjcyMjU2MDB9.\
     gcA5ick0W16rnB4ROBvkn896i2bNi3Z80USgUN9a5tEtdq5kigej16LpaCi_sVzxdzVIGUQ1n9eUtayjb2GdAg";

const LIBSQL_SCRATCH: &[&str] = &[
    "conf_t",
    "conf_tx",
    "conf_plan",
    "conf_big",
    "conf_edit",
    "conf_many",
    "conf_keys",
    "init_probe",
];

static LIBSQL_CLEANED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// A libSQL server, shared by the tests: the fixture is created once.
async fn libsql_backend() -> Option<Backend> {
    let default = format!("libsql://:{LIBSQL_TOKEN}@localhost:58080?sslmode=disable");
    let config = server_config("PLATYPUS_TEST_LIBSQL_URL", &default, "libSQL").await?;
    {
        let _seeding = SEEDING.lock().await;
        let conn = LibsqlDriver.connect(&config).await.unwrap();
        // The tables the scenarios create as temporary elsewhere, left by an earlier run.
        if !LIBSQL_CLEANED.swap(true, std::sync::atomic::Ordering::Relaxed) {
            for table in LIBSQL_SCRATCH {
                run(&*conn, &format!("DROP TABLE IF EXISTS {table}"))
                    .await
                    .unwrap_or_else(|e| panic!("libsql: {e}"));
            }
        }
        for sql in SQLITE_FIXTURE {
            let sql = sql
                .replacen("CREATE TABLE", "CREATE TABLE IF NOT EXISTS", 1)
                .replacen("CREATE INDEX", "CREATE INDEX IF NOT EXISTS", 1)
                .replacen("CREATE VIEW", "CREATE VIEW IF NOT EXISTS", 1);
            run(&*conn, &sql)
                .await
                .unwrap_or_else(|e| panic!("libsql: {sql}: {e}"));
        }
    }
    Some(Backend {
        name: "libsql",
        product: "libSQL",
        config,
        schema: "main".to_owned(),
        series_sql: sqlite_series(),
        endless_sql: SQLITE_ENDLESS,
        kill: None,
        _dir: None,
    })
}

async fn duckdb_backend() -> Backend {
    platypus_driver_duckdb::testing::load();
    let dir = tempfile::tempdir().unwrap();
    // DuckDB creates the file; an empty one is not a database.
    let path = dir.path().join("conformance.duckdb");
    let mut config = ConnectionConfig::new(DriverKind::DuckDb);
    config.database = path.to_string_lossy().into_owned();
    let conn = DuckDbDriver.connect(&config).await.unwrap();
    for sql in DUCKDB_FIXTURE {
        conn.execute(sql, &ExecOptions::default()).await.unwrap();
    }
    // One process at a time may open a DuckDB file for writing.
    drop(conn);
    Backend {
        name: "duckdb",
        product: "DuckDB",
        config,
        schema: "main".to_owned(),
        series_sql: format!(
            "SELECT range + 1 AS id, 'row ' || (range + 1) AS label FROM range({SERIES_ROWS})"
        ),
        endless_sql: "SELECT max(hash(range)) FROM range(1000000000000000)",
        kill: None,
        _dir: Some(dir),
    }
}

const PG_SEED: &str = include_str!("../../../docker/postgres/01-seed.sql");

/// Serializes seeding: the tests run in parallel and each lists the backends.
static SEEDING: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

/// Held by every test: YugabyteDB fails DDL that races other sessions' DDL with `40001`
/// (`Restart read required`) inside transactions, where it cannot be run again.
static SERIAL: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

/// Runs the PostgreSQL seed on a server that ran none (CockroachDB, YugabyteDB), once.
async fn seed_pg_wire(config: &ConnectionConfig, name: &str) {
    let _seeding = SEEDING.lock().await;
    let conn = PgDriver.connect(config).await.unwrap();
    // The orders are seeded last: a seed cut short is run again.
    let seeded = platypus_core::query_text(&*conn, "SELECT count(*) FROM demo.orders").await;
    if seeded.is_ok_and(|rows| rows == [[Some("20000".to_owned())]]) {
        return;
    }
    run(&*conn, "DROP SCHEMA IF EXISTS demo CASCADE")
        .await
        .unwrap_or_else(|e| panic!("{name}: {e}"));
    for statement in platypus_sql::split(PG_SEED, platypus_sql::Dialect::Postgres) {
        let sql = statement.text(PG_SEED);
        // The type zoo and the large table are for the PostgreSQL driver's own tests.
        if ["mood", "type_zoo", "big_table"]
            .iter()
            .any(|t| sql.contains(t))
        {
            continue;
        }
        run(&*conn, sql)
            .await
            .unwrap_or_else(|e| panic!("{name}: seed: {sql}: {e}"));
    }
}

/// The servers that speak the PostgreSQL protocol, run with the PostgreSQL driver.
async fn pg_wire_backends() -> Vec<Backend> {
    let mut all = Vec::new();
    for (name, product, env, default, seed) in [
        (
            "timescale",
            "PostgreSQL",
            "PLATYPUS_TEST_TIMESCALE_URL",
            "postgres://platypus:platypus@localhost:55433/platypus?sslmode=disable",
            false,
        ),
        (
            "cockroach",
            "CockroachDB",
            "PLATYPUS_TEST_COCKROACH_URL",
            "postgres://root@localhost:56257/platypus?sslmode=disable",
            true,
        ),
        (
            "yugabyte",
            "YugabyteDB",
            "PLATYPUS_TEST_YUGABYTE_URL",
            "postgres://yugabyte:yugabyte@localhost:55434/yugabyte?sslmode=disable",
            true,
        ),
    ] {
        let Some(config) = server_config(env, default, product).await else {
            continue;
        };
        if seed {
            seed_pg_wire(&config, name).await;
        }
        all.push(Backend {
            name,
            product,
            config,
            schema: "demo".to_owned(),
            series_sql: format!(
                "SELECT g AS id, 'row ' || g AS label FROM generate_series(1, {SERIES_ROWS}) g"
            ),
            endless_sql: "SELECT pg_sleep(600)",
            kill: Some(if name == "cockroach" {
                (
                    "SELECT quote_literal(current_setting('session_id'))",
                    "CANCEL SESSION",
                )
            } else {
                ("SELECT pg_backend_pid()", "SELECT pg_terminate_backend")
            }),
            _dir: None,
        });
    }
    all
}

async fn backends() -> Vec<Backend> {
    // `PLATYPUS_TEST_ONLY=mysql` runs on that database alone, as the run on each older
    // version does (scripts/conformance-versions.mjs).
    let only = std::env::var("PLATYPUS_TEST_ONLY").ok();
    let wanted = |name: &str| only.as_deref().is_none_or(|o| o == name);
    let mut all = Vec::new();
    if !wanted("postgres") {
    } else if let Some(config) = server_config(
        "PLATYPUS_TEST_PG_URL",
        "postgres://platypus:platypus@localhost:55432/platypus?sslmode=disable",
        "PostgreSQL",
    )
    .await
    {
        all.push(Backend {
            name: "postgres",
            product: "PostgreSQL",
            config,
            schema: "demo".to_owned(),
            series_sql: format!(
                "SELECT g AS id, 'row ' || g AS label FROM generate_series(1, {SERIES_ROWS}) g"
            ),
            endless_sql: "SELECT pg_sleep(600)",
            kill: Some(("SELECT pg_backend_pid()", "SELECT pg_terminate_backend")),
            _dir: None,
        });
    }
    for (name, product, env, default) in [
        (
            "mysql",
            "MySQL",
            "PLATYPUS_TEST_MYSQL_URL",
            "mysql://platypus:platypus@localhost:53306/platypus",
        ),
        (
            "mariadb",
            "MariaDB",
            "PLATYPUS_TEST_MARIADB_URL",
            "mysql://platypus:platypus@localhost:53307/platypus",
        ),
    ] {
        if !wanted(name) {
            continue;
        }
        if let Some(config) = server_config(env, default, product).await {
            all.push(Backend {
                name,
                product,
                config,
                schema: "platypus".to_owned(),
                series_sql: MYSQL_SERIES.to_owned(),
                endless_sql: MYSQL_ENDLESS,
                kill: Some(("SELECT CONNECTION_ID()", "KILL CONNECTION")),
                _dir: None,
            });
        }
    }
    if only.is_none() {
        all.extend(pg_wire_backends().await);
    }
    if wanted("sqlserver") {
        all.extend(mssql_backend().await);
    }
    if wanted("clickhouse") {
        all.extend(clickhouse_backend().await);
    }
    if wanted("sqlite") {
        all.push(sqlite_backend().await);
    }
    if wanted("duckdb") {
        all.push(duckdb_backend().await);
    }
    if wanted("libsql") {
        all.extend(libsql_backend().await);
    }
    all
}

async fn collect(b: &Backend, conn: &dyn Connection, sql: &str) -> Vec<RecordBatch> {
    match conn.execute(sql, &ExecOptions::default()).await {
        Ok(ExecResult::Rows(stream)) => stream
            .try_collect()
            .await
            .unwrap_or_else(|e| panic!("{}: {sql}: {e}", b.name)),
        other => panic!("{}: expected rows from {sql}, got {other:?}", b.name),
    }
}

/// Runs `sql`, again after a serialization failure outside a transaction: YugabyteDB
/// fails DDL that races the other tests' DDL with `40001`, which clients are to retry.
async fn execute(conn: &dyn Connection, sql: &str) -> Result<ExecResult, Error> {
    for _ in 0..20 {
        match conn.execute(sql, &ExecOptions::default()).await {
            Err(Error::Database(e))
                if e.code.as_deref() == Some("40001") && !conn.transaction().open =>
            {
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
            other => return other,
        }
    }
    conn.execute(sql, &ExecOptions::default()).await
}

async fn affected(b: &Backend, conn: &dyn Connection, sql: &str) -> u64 {
    match execute(conn, sql).await {
        Ok(ExecResult::Affected { rows }) => rows,
        other => panic!("{}: expected a row count from {sql}, got {other:?}", b.name),
    }
}

fn cell(batch: &RecordBatch, column: usize, row: usize) -> Option<String> {
    let array = batch.column(column);
    (!array.is_null(row)).then(|| array_value_to_string(array, row).unwrap())
}

/// Runs `sql` to completion, returning the first error from either phase.
async fn run(conn: &dyn Connection, sql: &str) -> Result<(), Error> {
    match execute(conn, sql).await? {
        ExecResult::Rows(stream) => stream.try_for_each(|_| async { Ok(()) }).await,
        ExecResult::Affected { .. } => Ok(()),
    }
}

#[tokio::test]
async fn connects_and_reports_the_server() {
    let _serial = SERIAL.lock().await;

    for b in backends().await {
        let conn = b.connect().await;
        let server = conn.server_info();
        assert_eq!(server.product, b.product, "{}", b.name);
        assert!(!server.version.is_empty(), "{}: empty version", b.name);
    }
}

#[tokio::test]
async fn selects_scalars() {
    let _serial = SERIAL.lock().await;

    for b in backends().await {
        let conn = b.connect().await;
        let batches = collect(
            &b,
            &*conn,
            "SELECT 42 AS answer, 'platypus' AS name, NULL AS missing",
        )
        .await;
        let batch = &batches[0];
        assert_eq!(batch.num_rows(), 1, "{}", b.name);
        let names: Vec<_> = batch
            .schema()
            .fields()
            .iter()
            .map(|f| f.name().clone())
            .collect();
        assert_eq!(names, ["answer", "name", "missing"], "{}", b.name);
        assert_eq!(cell(batch, 0, 0).as_deref(), Some("42"), "{}", b.name);
        assert_eq!(cell(batch, 1, 0).as_deref(), Some("platypus"), "{}", b.name);
        assert_eq!(cell(batch, 2, 0), None, "{}", b.name);
    }
}

#[tokio::test]
async fn streams_large_results_in_batches() {
    let _serial = SERIAL.lock().await;

    for b in backends().await {
        let conn = b.connect().await;
        let options = ExecOptions {
            batch_size: 1000,
            ..ExecOptions::default()
        };
        let ExecResult::Rows(stream) = conn.execute(&b.series_sql, &options).await.unwrap() else {
            panic!("{}: expected rows", b.name);
        };
        let batches: Vec<RecordBatch> = stream.try_collect().await.unwrap();
        let total: usize = batches.iter().map(RecordBatch::num_rows).sum();
        assert_eq!(total, SERIES_ROWS, "{}", b.name);
        assert!(batches.len() >= SERIES_ROWS / 1000, "{}", b.name);
        assert!(
            batches.iter().all(|batch| batch.num_rows() <= 1000),
            "{}: oversized batch",
            b.name
        );
    }
}

#[tokio::test]
async fn reports_affected_rows() {
    let _serial = SERIAL.lock().await;

    for b in backends().await {
        let conn = b.connect().await;
        let conn = &*conn;
        let ddl = affected(&b, conn, &b.create_temp("conf_t", "id INT, v VARCHAR(10)")).await;
        assert_eq!(ddl, 0, "{}: DDL", b.name);
        let t = b.temp("conf_t");
        let sql = format!("INSERT INTO {t} VALUES (1, 'a'), (2, 'b'), (3, 'c')");
        assert_eq!(affected(&b, conn, &sql).await, 3, "{}: insert", b.name);
        // ClickHouse updates and deletes are mutations: they report no row counts and do
        // not apply to temporary tables.
        if b.config.driver == DriverKind::ClickHouse {
            affected(&b, conn, &format!("DROP TABLE {t}")).await;
            continue;
        }
        let sql = format!("UPDATE {t} SET v = 'z' WHERE id >= 2");
        assert_eq!(affected(&b, conn, &sql).await, 2, "{}: update", b.name);
        let rows = collect(&b, conn, &format!("SELECT count(*) FROM {t} WHERE v = 'z'")).await;
        assert_eq!(cell(&rows[0], 0, 0).as_deref(), Some("2"), "{}", b.name);
        assert_eq!(
            affected(&b, conn, &format!("DELETE FROM {t}")).await,
            3,
            "{}: delete",
            b.name
        );
        assert_eq!(
            affected(&b, conn, &format!("DROP TABLE {t}")).await,
            0,
            "{}: drop",
            b.name
        );
    }
}

#[tokio::test]
async fn maps_errors_with_their_position() {
    let _serial = SERIAL.lock().await;

    for b in backends().await {
        let conn = b.connect().await;
        match run(&*conn, "SELECT * FROM no_such_table").await {
            Err(Error::Database(e)) => {
                assert!(e.code.is_some(), "{}: no error code", b.name);
                assert!(
                    e.message.contains("no_such_table"),
                    "{}: {}",
                    b.name,
                    e.message
                );
            }
            other => panic!("{}: expected a database error, got {other:?}", b.name),
        }
        // DuckDB and ClickHouse read `FROMM` as an alias and stop at `t`; CockroachDB gives
        // no position.
        let at = match (b.config.driver, b.name) {
            (DriverKind::DuckDb | DriverKind::ClickHouse, _) => Some(16),
            (_, "cockroach") => None,
            _ => Some(10),
        };
        match run(&*conn, "SELECT * FROMM t").await {
            Err(Error::Database(e)) => assert_eq!(e.position, at, "{}: {e}", b.name),
            other => panic!("{}: expected a syntax error, got {other:?}", b.name),
        }
        // The session is still usable after errors.
        let rows = collect(&b, &*conn, "SELECT 1").await;
        assert_eq!(cell(&rows[0], 0, 0).as_deref(), Some("1"), "{}", b.name);
    }
}

#[tokio::test]
async fn cancels_running_queries() {
    let _serial = SERIAL.lock().await;

    for b in backends().await {
        let conn = b.connect().await;
        let handle = conn.cancel_handle();
        // libSQL over HTTP has no way to stop a statement.
        if !conn.capabilities().cancel {
            assert!(
                matches!(handle.cancel().await, Err(Error::Unsupported(_))),
                "{}",
                b.name
            );
            continue;
        }
        // Again until the query stops: a cancel that arrives before the query starts (a busy
        // YugabyteDB) is lost.
        let cancel = tokio::spawn(async move {
            loop {
                tokio::time::sleep(Duration::from_millis(500)).await;
                if let Err(e) = handle.cancel().await {
                    return e;
                }
            }
        });
        let result = tokio::time::timeout(Duration::from_secs(30), run(&*conn, b.endless_sql))
            .await
            .unwrap_or_else(|_| panic!("{}: cancel did not stop the query", b.name));
        cancel.abort();
        if let Ok(e) = cancel.await {
            panic!("{}: cancel failed: {e}", b.name);
        }
        assert!(
            matches!(result, Err(Error::Cancelled)),
            "{}: got {result:?}",
            b.name
        );
        let rows = collect(&b, &*conn, "SELECT 1").await;
        assert_eq!(cell(&rows[0], 0, 0).as_deref(), Some("1"), "{}", b.name);
    }
}

#[tokio::test]
async fn abandoned_results_free_the_connection() {
    let _serial = SERIAL.lock().await;

    for b in backends().await {
        let conn = b.connect().await;
        // Paged: read a page at a time, as the result grid does.
        for paged in [false, true] {
            let options = ExecOptions {
                batch_size: 100,
                paged,
            };
            let ExecResult::Rows(mut stream) = conn.execute(&b.series_sql, &options).await.unwrap()
            else {
                panic!("{}: expected rows", b.name);
            };
            let first = stream.next().await.unwrap().unwrap();
            assert_eq!(first.num_rows(), 100, "{} (paged {paged})", b.name);
            drop(stream);
            let rows =
                tokio::time::timeout(Duration::from_secs(10), collect(&b, &*conn, "SELECT 7"))
                    .await
                    .unwrap_or_else(|_| panic!("{}: connection stuck after drop", b.name));
            assert_eq!(cell(&rows[0], 0, 0).as_deref(), Some("7"), "{}", b.name);
            assert!(!conn.transaction().open, "{} (paged {paged})", b.name);
        }
    }
}

#[tokio::test]
async fn navigates_metadata() {
    let _serial = SERIAL.lock().await;

    for b in backends().await {
        let conn = b.connect().await;
        let schemas = conn.schemas().await.unwrap();
        assert!(
            schemas.iter().any(|s| s.name == b.schema),
            "{}: {schemas:?}",
            b.name
        );
        assert!(
            schemas.iter().any(|s| s.current),
            "{}: no current schema",
            b.name
        );

        let objects = conn.objects(&b.schema).await.unwrap();
        let kind = |name: &str| objects.iter().find(|o| o.name == name).map(|o| o.kind);
        assert_eq!(kind("customers"), Some(ObjectKind::Table), "{}", b.name);
        assert_eq!(kind("orders"), Some(ObjectKind::Table), "{}", b.name);
        assert_eq!(
            kind("customer_totals"),
            Some(ObjectKind::View),
            "{}",
            b.name
        );

        let orders = conn.table_details(&b.schema, "orders").await.unwrap();
        if b.config.driver == DriverKind::DuckDb {
            // A name spelled in another case, as a bare name in a query can be.
            let upper = conn.table_details(&b.schema, "ORDERS").await.unwrap();
            assert!(!upper.constraints.is_empty(), "{}", b.name);
            assert_eq!(
                upper.constraints.len(),
                orders.constraints.len(),
                "{}",
                b.name
            );
        }
        let column = |name: &str| {
            orders
                .columns
                .iter()
                .find(|c| c.name == name)
                .unwrap_or_else(|| panic!("{}: no column {name}", b.name))
        };
        assert!(column("id").primary_key, "{}", b.name);
        assert!(!column("customer_id").nullable, "{}", b.name);
        assert!(!column("total").primary_key, "{}", b.name);
        assert_eq!(orders.columns[0].name, "id", "{}: column order", b.name);

        let primary: Vec<_> = orders.indexes.iter().filter(|i| i.primary).collect();
        assert_eq!(primary.len(), 1, "{}: {:?}", b.name, orders.indexes);
        assert_eq!(primary[0].columns, ["id"], "{}", b.name);
        let by_customer = orders
            .indexes
            .iter()
            .find(|i| i.name == "orders_customer_id_idx")
            .unwrap_or_else(|| panic!("{}: {:?}", b.name, orders.indexes));
        assert_eq!(by_customer.columns, ["customer_id"], "{}", b.name);
        assert!(!by_customer.unique, "{}", b.name);

        let view = conn
            .table_details(&b.schema, "customer_totals")
            .await
            .unwrap();
        assert!(view.columns.len() >= 2, "{}: view columns", b.name);
        if !b.keys() {
            assert_eq!(orders.foreign_keys, [], "{}", b.name);
            continue;
        }

        assert_eq!(orders.foreign_keys.len(), 1, "{}", b.name);
        let fk = &orders.foreign_keys[0];
        assert_eq!(fk.columns, ["customer_id"], "{}", b.name);
        assert_eq!(fk.ref_table, "customers", "{}", b.name);
        assert_eq!(fk.ref_columns, ["id"], "{}", b.name);

        let customers = conn.table_details(&b.schema, "customers").await.unwrap();
        assert!(
            customers
                .indexes
                .iter()
                .any(|i| i.unique && !i.primary && i.columns == ["email"]),
            "{}: {:?}",
            b.name,
            customers.indexes
        );

        let referencing = conn.referencing_keys(&b.schema, "customers").await.unwrap();
        assert_eq!(referencing.len(), 1, "{}: {referencing:?}", b.name);
        let key = &referencing[0];
        assert_eq!(
            (key.schema.as_str(), key.table.as_str()),
            (b.schema.as_str(), "orders"),
            "{}",
            b.name
        );
        assert_eq!(key.columns, ["customer_id"], "{}", b.name);
        assert_eq!(key.ref_columns, ["id"], "{}", b.name);
        assert_eq!(
            conn.referencing_keys(&b.schema, "orders").await.unwrap(),
            [],
            "{}",
            b.name
        );
    }
}

async fn count(b: &Backend, conn: &dyn Connection) -> String {
    let rows = collect(
        b,
        conn,
        &format!("SELECT count(*) FROM {}", b.temp("conf_tx")),
    )
    .await;
    cell(&rows[0], 0, 0).unwrap()
}

#[tokio::test]
async fn manual_commit_and_rollback() {
    let _serial = SERIAL.lock().await;

    for b in backends().await {
        let conn = b.connect().await;
        let conn = &*conn;
        // ClickHouse has no transactions: manual commit is refused.
        if !conn.capabilities().transactions {
            assert!(conn.set_autocommit(false).await.is_err(), "{}", b.name);
            assert!(conn.transaction().autocommit, "{}", b.name);
            continue;
        }
        affected(&b, conn, &b.create_temp("conf_tx", "id INT")).await;
        let t = b.temp("conf_tx");
        let insert = |id: u32| format!("INSERT INTO {t} VALUES ({id})");
        assert!(conn.transaction().autocommit, "{}", b.name);

        conn.set_autocommit(false).await.unwrap();
        assert!(!conn.transaction().open, "{}: nothing ran yet", b.name);
        affected(&b, conn, &insert(1)).await;
        assert!(
            conn.transaction().open,
            "{}: insert opens a transaction",
            b.name
        );
        assert!(
            conn.set_autocommit(true).await.is_err(),
            "{}: switching with pending changes",
            b.name
        );
        assert_eq!(count(&b, conn).await, "1", "{}", b.name);
        conn.rollback().await.unwrap();
        assert!(!conn.transaction().open, "{}", b.name);
        assert_eq!(count(&b, conn).await, "0", "{}: rolled back", b.name);
        conn.rollback().await.unwrap();

        // A result abandoned inside a transaction must not abort it.
        affected(&b, conn, &insert(2)).await;
        let options = ExecOptions {
            batch_size: 100,
            ..ExecOptions::default()
        };
        let ExecResult::Rows(mut stream) = conn.execute(&b.series_sql, &options).await.unwrap()
        else {
            panic!("{}: expected rows", b.name);
        };
        stream.next().await.unwrap().unwrap();
        drop(stream);
        affected(&b, conn, &insert(3)).await;
        conn.commit().await.unwrap();
        assert!(!conn.transaction().open, "{}", b.name);

        conn.set_autocommit(true).await.unwrap();
        assert_eq!(
            count(&b, conn).await,
            "2",
            "{}: both inserts committed",
            b.name
        );
        assert!(!conn.transaction().open, "{}: autocommit again", b.name);

        // Transaction statements typed by the user are tracked too.
        affected(&b, conn, "BEGIN").await;
        assert!(conn.transaction().open, "{}: typed BEGIN", b.name);
        affected(&b, conn, &format!("DELETE FROM {t}")).await;
        affected(&b, conn, "ROLLBACK").await;
        assert!(!conn.transaction().open, "{}: typed ROLLBACK", b.name);
        assert_eq!(count(&b, conn).await, "2", "{}", b.name);

        if b.config.driver == DriverKind::SqlServer {
            // A nested COMMIT closes one level only: the session's transaction stays open.
            conn.set_autocommit(false).await.unwrap();
            affected(&b, conn, &insert(4)).await;
            affected(&b, conn, "BEGIN TRAN").await;
            affected(&b, conn, "COMMIT").await;
            assert!(conn.transaction().open, "{}: nested COMMIT", b.name);
            conn.rollback().await.unwrap();
            assert_eq!(count(&b, conn).await, "2", "{}: all rolled back", b.name);
            // The count opened a transaction of its own.
            conn.rollback().await.unwrap();
            conn.set_autocommit(true).await.unwrap();
        }

        // DDL as the first statement opens the transaction too, where DDL is transactional.
        let mysql = b.config.driver == DriverKind::MySql;
        let d = platypus_sql::Dialect::from(b.config.driver);
        let ddl = d.qualified(Some(&b.schema), "conf_tx_ddl");
        let _ = run(conn, &format!("DROP TABLE {ddl}")).await;
        conn.set_autocommit(false).await.unwrap();
        run(conn, &format!("CREATE TABLE {ddl} (id INT)"))
            .await
            .unwrap();
        assert_eq!(conn.transaction().open, !mysql, "{}: DDL first", b.name);
        conn.rollback().await.unwrap();
        let kept = run(conn, &format!("SELECT * FROM {ddl}")).await.is_ok();
        // CockroachDB commits the transaction before DDL (`autocommit_before_ddl`), YugabyteDB
        // runs DDL in a transaction of its own.
        let ddl_commits = mysql || matches!(b.name, "cockroach" | "yugabyte");
        assert_eq!(kept, ddl_commits, "{}: the CREATE rolled back", b.name);
        conn.rollback().await.unwrap();
        conn.set_autocommit(true).await.unwrap();

        // A read in manual-commit mode does not keep another session from writing.
        let _ = run(conn, &format!("DROP TABLE {ddl}")).await;
        run(conn, &format!("CREATE TABLE {ddl} (id INT)"))
            .await
            .unwrap();
        conn.set_autocommit(false).await.unwrap();
        run(conn, &format!("SELECT * FROM {ddl}")).await.unwrap();
        let other = b.connect().await;
        run(&*other, &format!("INSERT INTO {ddl} VALUES (1)"))
            .await
            .unwrap_or_else(|e| panic!("{}: write beside a read: {e}", b.name));
        conn.rollback().await.unwrap();
        conn.set_autocommit(true).await.unwrap();
        let _ = run(conn, &format!("DROP TABLE {ddl}")).await;
    }
}

#[tokio::test]
async fn lists_catalog_columns_for_completion() {
    let _serial = SERIAL.lock().await;

    for b in backends().await {
        let conn = b.connect().await;
        let catalog = conn.catalog_columns().await.unwrap();
        let has = |table: &str, column: &str| {
            catalog
                .iter()
                .any(|c| c.schema == b.schema && c.table == table && c.column == column)
        };
        assert!(has("orders", "customer_id"), "{}", b.name);
        assert!(has("customer_totals", "id"), "{}: views too", b.name);
        assert!(
            !catalog
                .iter()
                .any(|c| c.schema == "information_schema" || c.schema == "pg_catalog"),
            "{}: no system schemas",
            b.name
        );
        // Columns of a table stay in declaration order.
        let orders: Vec<_> = catalog
            .iter()
            .filter(|c| c.schema == b.schema && c.table == "orders")
            .map(|c| c.column.as_str())
            .collect();
        assert_eq!(orders.first(), Some(&"id"), "{}", b.name);
        if !b.keys() {
            continue;
        }

        // Foreign keys complete JOIN conditions.
        let keys = conn.catalog_foreign_keys().await.unwrap();
        let fk = keys
            .iter()
            .find(|k| k.schema == b.schema && k.table == "orders")
            .unwrap_or_else(|| panic!("{}: orders foreign key in {keys:?}", b.name));
        assert_eq!(fk.columns, ["customer_id"], "{}", b.name);
        assert_eq!(
            (fk.ref_schema.as_str(), fk.ref_table.as_str()),
            (b.schema.as_str(), "customers"),
            "{}",
            b.name
        );
        assert_eq!(fk.ref_columns, ["id"], "{}", b.name);
    }
}

#[tokio::test]
async fn reports_lost_connections() {
    let _serial = SERIAL.lock().await;

    for b in backends().await {
        let Some((id_sql, kill)) = b.kill else {
            continue;
        };
        let conn = b.connect().await;
        assert!(!conn.is_closed(), "{}", b.name);
        let rows = collect(&b, &*conn, id_sql).await;
        let id = cell(&rows[0], 0, 0).unwrap();
        let killer = b.connect().await;
        let kill_sql = if kill.starts_with("SELECT") {
            format!("{kill}({id})")
        } else {
            format!("{kill} {id}")
        };
        run(&*killer, &kill_sql).await.unwrap();

        assert!(
            run(&*conn, "SELECT 1").await.is_err(),
            "{}: query on a dead connection",
            b.name
        );
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        while !conn.is_closed() && std::time::Instant::now() < deadline {
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        assert!(
            conn.is_closed(),
            "{}: connection reported as closed",
            b.name
        );
    }
}

fn plan_mentions(node: &PlanNode, name: &str) -> bool {
    node.operation.contains(name)
        || node.detail.as_deref().is_some_and(|d| d.contains(name))
        || node.children.iter().any(|c| plan_mentions(c, name))
}

/// Some step has actual rows.
fn plan_has_actuals(node: &PlanNode) -> bool {
    node.actual_rows.is_some() || node.children.iter().any(plan_has_actuals)
}

/// What a database's plans hold, where they differ.
struct PlanSupport {
    /// The steps name the tables they read (ClickHouse names its storage engine instead).
    names_tables: bool,
    /// A plan that runs the statement: none in SQLite, ClickHouse and SQL Server here.
    analyzes: bool,
    /// Analyze inside an open transaction, rolled back to a savepoint: DuckDB has none.
    analyzes_in_transaction: bool,
}

/// Whether the plans give the rows read by full scans, to suggest indexes: PostgreSQL's and
/// MySQL's.
fn plans_suggest(b: &Backend) -> bool {
    matches!(b.config.driver, DriverKind::Postgres | DriverKind::MySql)
        && b.product != "CockroachDB"
}

/// A MySQL server older than `version`; MariaDB numbers its versions apart.
fn mysql_before(conn: &dyn Connection, version: ServerVersion) -> bool {
    let info = conn.server_info();
    info.product == "MySQL" && !info.at_least(version)
}

/// MySQL's first versions with `CHECK` constraints and with `EXPLAIN ANALYZE`.
const MYSQL_CHECKS: ServerVersion = ServerVersion::new(8, 0, 16);
const MYSQL_ANALYZE: ServerVersion = ServerVersion::new(8, 0, 18);

fn plan_support(b: &Backend, conn: &dyn Connection) -> PlanSupport {
    let driver = b.config.driver;
    let analyzes = !b.speaks_sqlite()
        && !matches!(driver, DriverKind::ClickHouse | DriverKind::SqlServer)
        && !mysql_before(conn, MYSQL_ANALYZE);
    PlanSupport {
        names_tables: driver != DriverKind::ClickHouse,
        analyzes,
        analyzes_in_transaction: analyzes && driver != DriverKind::DuckDb,
    }
}

/// Through `PgBouncer` in transaction mode (`docker compose --profile pooler`), each
/// transaction may land on another server session: the navigator's and the editor's
/// requests still work with the connection's pooler mode set. Skipped when it is not running.
#[tokio::test]
async fn works_through_a_transaction_pooler() {
    let _serial = SERIAL.lock().await;
    let url = std::env::var("PLATYPUS_TEST_PGBOUNCER_URL").unwrap_or_else(|_| {
        "postgres://platypus:platypus@localhost:56432/platypus?sslmode=disable".to_owned()
    });
    let mut config = ConnectionConfig::from_url(&url).unwrap();
    config
        .options
        .insert(platypus_core::POOLER.to_owned(), "transaction".to_owned());
    let conn = match driver(DriverKind::Postgres).connect(&config).await {
        Ok(conn) => conn,
        Err(e) => {
            eprintln!("skipping PgBouncer: unreachable at {url}: {e}");
            return;
        }
    };
    let object = ObjectRef {
        schema: "demo",
        name: "customers",
        kind: ObjectKind::Table,
        parent: None,
    };
    assert!(
        conn.object_ddl(&object)
            .await
            .unwrap()
            .contains("CREATE TABLE")
    );
    assert_ne!(
        conn.object_properties(&object).await.unwrap(),
        [] as [platypus_core::Property; 0]
    );
    assert_ne!(
        conn.search_metadata("customers").await.unwrap(),
        [] as [platypus_core::MetadataMatch; 0]
    );
    assert_ne!(
        conn.objects("demo").await.unwrap(),
        [] as [platypus_core::ObjectMeta; 0]
    );
    let plan = conn
        .explain("SELECT * FROM demo.customers WHERE id > 1", true)
        .await
        .unwrap();
    assert!(plan.analyzed);
    assert!(
        conn.server_sessions()
            .await
            .unwrap()
            .iter()
            .any(|s| s.current)
    );
    let rows = run(&*conn, "SELECT count(*) FROM demo.orders").await;
    assert!(rows.is_ok(), "{rows:?}");
}

/// Plans come back as a tree. With analyze the statement runs, and what it changes is
/// rolled back, in autocommit and inside an open transaction alike.
/// Steps in `node`'s tree.
fn plan_steps(node: &PlanNode) -> usize {
    1 + node.children.iter().map(plan_steps).sum::<usize>()
}

/// The plan document a driver returns reads back, pasted, as the same tree: the reader of
/// pasted plans knows each database's own output.
#[tokio::test]
async fn explained_plans_read_back() {
    let _serial = SERIAL.lock().await;

    for b in backends().await {
        let read: fn(&str) -> platypus_core::Result<platypus_core::Explain> = match b.config.driver
        {
            DriverKind::Postgres => platypus_driver_pg::read_plan,
            DriverKind::MySql => platypus_driver_mysql::read_plan,
            DriverKind::Sqlite => platypus_driver_sqlite::meta::read_plan,
            DriverKind::SqlServer => platypus_driver_mssql::read_plan,
            _ => continue,
        };
        let conn = b.connect().await;
        let sql = format!(
            "SELECT c.id, o.id FROM {s}.customers c JOIN {s}.orders o ON o.customer_id = c.id WHERE c.id > 1",
            s = b.schema
        );
        for analyze in [false, true] {
            let plan = match conn.explain(&sql, analyze).await {
                Ok(plan) => plan,
                // SQL Server's plans are estimated only.
                Err(Error::Unsupported(_)) if analyze => continue,
                Err(e) => panic!("{}: explain: {e}", b.name),
            };
            let pasted = read(&plan.plan)
                .unwrap_or_else(|e| panic!("{}: read back: {e}\n{}", b.name, plan.plan));
            assert_eq!(pasted.root.operation, plan.root.operation, "{}", b.name);
            assert_eq!(
                plan_steps(&pasted.root),
                plan_steps(&plan.root),
                "{} (analyze {analyze})",
                b.name
            );
            assert_eq!(
                pasted.analyzed, plan.analyzed,
                "{} (analyze {analyze})",
                b.name
            );
        }
    }
}

#[tokio::test]
async fn explains_queries() {
    let _serial = SERIAL.lock().await;

    for b in backends().await {
        let conn = b.connect().await;
        let conn = &*conn;
        // Plans are optional: `capabilities_match_optional_methods` checks the others.
        if !conn.capabilities().explain {
            continue;
        }
        let support = plan_support(&b, conn);
        let table = b.temp("conf_plan");
        run(conn, &b.create_temp("conf_plan", "id INT"))
            .await
            .unwrap_or_else(|e| panic!("{}: {e}", b.name));
        affected(
            &b,
            conn,
            &format!("INSERT INTO {table} VALUES (1), (2), (3)"),
        )
        .await;
        let count = || async {
            let rows = collect(&b, conn, &format!("SELECT count(*) FROM {table}")).await;
            cell(&rows[0], 0, 0).unwrap()
        };
        let select = format!("SELECT * FROM {table} WHERE id > 1");
        let plan = conn
            .explain(&select, false)
            .await
            .unwrap_or_else(|e| panic!("{}: {e}", b.name));
        assert!(!plan.analyzed && !plan.plan.is_empty(), "{}", b.name);
        if support.names_tables {
            assert!(
                plan_mentions(&plan.root, "conf_plan"),
                "{}: {:#?}",
                b.name,
                plan.root
            );
        }

        let analyzed = conn.explain(&select, true).await;
        if !support.analyzes {
            // A plan without its actual figures, or a plain refusal.
            if let Ok(plan) = analyzed {
                assert!(
                    !plan.analyzed && !plan_has_actuals(&plan.root),
                    "{}",
                    b.name
                );
            }
            continue;
        }
        let plan = analyzed.unwrap_or_else(|e| panic!("{}: {e}", b.name));
        assert!(plan.analyzed, "{}", b.name);
        assert!(plan_has_actuals(&plan.root), "{}: {:#?}", b.name, plan.root);

        // MySQL analyzes only the multi-table form of DELETE.
        let delete = if b.product == "MySQL" {
            "DELETE p FROM conf_plan p WHERE p.id > 1".to_owned()
        } else {
            format!("DELETE FROM {table} WHERE id > 1")
        };
        conn.explain(&delete, true)
            .await
            .unwrap_or_else(|e| panic!("{}: {e}", b.name));
        assert_eq!(count().await, "3", "{}: analyze rolled back", b.name);
        assert!(!conn.transaction().open, "{}", b.name);
        if !support.analyzes_in_transaction {
            continue;
        }

        conn.set_autocommit(false).await.unwrap();
        affected(&b, conn, &format!("INSERT INTO {table} VALUES (4)")).await;
        conn.explain(&delete, true).await.unwrap();
        assert!(conn.transaction().open, "{}: still open", b.name);
        assert_eq!(count().await, "4", "{}: only the analyze undone", b.name);
        conn.rollback().await.unwrap();
        conn.set_autocommit(true).await.unwrap();
        assert_eq!(count().await, "3", "{}", b.name);
    }
}

/// A filtered full scan of a large table is suggested an index.
#[tokio::test]
async fn suggests_indexes_for_large_scans() {
    let _serial = SERIAL.lock().await;

    for b in backends().await {
        let conn = b.connect().await;
        let conn = &*conn;
        // Plans are optional: `capabilities_match_optional_methods` checks the others.
        if !conn.capabilities().explain || !plan_support(&b, conn).analyzes {
            continue;
        }
        run(
            conn,
            &format!("CREATE {}TABLE conf_big AS {}", b.temporary(), b.series_sql),
        )
        .await
        .unwrap_or_else(|e| panic!("{}: {e}", b.name));
        let plan = conn
            .explain("SELECT * FROM conf_big WHERE id = 42", true)
            .await
            .unwrap_or_else(|e| panic!("{}: {e}", b.name));
        let tables: Vec<_> = plan.suggestions.iter().map(|s| s.table.as_str()).collect();
        // Only some plans have the row counts to judge a table by.
        let expected: &[&str] = if plans_suggest(&b) {
            &["conf_big"]
        } else {
            &[]
        };
        assert_eq!(tables, expected, "{}: {:#?}", b.name, plan.root);
    }
}
/// A schema dumped to an SQL file runs again where it came from: tables in the order of
/// their keys (a key to a row further down too), rows with their own numbers and numbering
/// going on after them, text exactly, and the views.
#[tokio::test]
async fn dumps_and_runs_sql_files() {
    use platypus_sql::Dialect;
    use platypus_transfer::{dump::dump, script::run_file};

    let _serial = SERIAL.lock().await;
    let dir = tempfile::tempdir().unwrap();

    for b in backends().await {
        let d = Dialect::from(b.config.driver);
        let (identity, text) = match d {
            Dialect::Postgres if b.name == "postgres" => {
                ("int GENERATED ALWAYS AS IDENTITY PRIMARY KEY", "text")
            }
            Dialect::MySql => ("int AUTO_INCREMENT PRIMARY KEY", "text"),
            Dialect::SqlServer => ("int IDENTITY PRIMARY KEY", "nvarchar(200)"),
            Dialect::Sqlite if b.name == "sqlite" => ("INTEGER PRIMARY KEY", "text"),
            Dialect::DuckDb => ("INTEGER PRIMARY KEY", "varchar"),
            _ => continue,
        };
        let conn = b.connect().await;
        let conn = &*conn;
        let q = |name: &str| d.qualified(Some(&b.schema), name);
        let (parent, child, view) = (
            q("conf_dump_parent"),
            q("conf_dump_child"),
            q("conf_dump_view"),
        );
        let clean = || async {
            let _ = run(conn, &format!("DROP VIEW {view}")).await;
            let _ = run(conn, &format!("DROP TABLE {child}")).await;
            let _ = run(conn, &format!("DROP TABLE {parent}")).await;
        };
        clean().await;
        let name = "it's \"q\"\nline 2";
        let statements = dump_fixture(d, (&parent, &child, &view), (identity, text), name);
        for sql in &statements {
            run(conn, sql)
                .await
                .unwrap_or_else(|e| panic!("{}: {sql}: {e}", b.name));
        }
        // A key to a row that comes after it; DuckDB checks keys as rows go in, in order.
        let forward = d != Dialect::DuckDb;
        if forward {
            run(conn, &format!("UPDATE {child} SET boss = 2 WHERE id = 1"))
                .await
                .unwrap_or_else(|e| panic!("{}: {e}", b.name));
        }

        let file = dir.path().join(format!("{}.sql", b.name));
        let only = ["conf_dump_parent", "conf_dump_child", "conf_dump_view"].map(str::to_owned);
        let mut out = std::fs::File::create(&file).unwrap();
        let dumped = dump((conn, d, &b.schema), &mut out, (true, Some(&only)), |_| {
            true
        })
        .await
        .unwrap_or_else(|e| panic!("{}: {e}", b.name));
        drop(out);
        assert_eq!(
            (dumped.tables, dumped.rows),
            (2, 4),
            "{}: {:?}",
            b.name,
            dumped.skipped
        );
        let script = std::fs::read_to_string(&file).unwrap();

        clean().await;
        let ran = run_file(conn, d, &file, true, |_| true)
            .await
            .unwrap_or_else(|e| panic!("{}: {e}\n{script}", b.name));
        assert!(ran >= 5, "{}: {ran} statements", b.name);

        let names = collect(
            &b,
            conn,
            &format!("SELECT id, name FROM {view} ORDER BY id"),
        )
        .await;
        assert_eq!(cell(&names[0], 1, 0).as_deref(), Some(name), "{}", b.name);
        assert_eq!(cell(&names[0], 1, 1), None, "{}", b.name);
        let rows = collect(
            &b,
            conn,
            &format!("SELECT boss, amount FROM {child} ORDER BY id"),
        )
        .await;
        let boss = forward.then(|| "2".to_owned());
        assert_eq!(cell(&rows[0], 0, 0), boss, "{}", b.name);
        let amount: f64 = cell(&rows[0], 1, 1).unwrap().parse().unwrap();
        assert!((amount + 3.25).abs() < 1e-9, "{}: {amount}", b.name);
        // The numbering goes on after the rows put back; DuckDB's key numbers nothing.
        if d != Dialect::DuckDb {
            run(conn, &format!("INSERT INTO {parent} (name) VALUES ('new')"))
                .await
                .unwrap_or_else(|e| panic!("{}: {e}\n{script}", b.name));
            let id = collect(&b, conn, &format!("SELECT MAX(id) FROM {parent}")).await;
            assert_eq!(
                cell(&id[0], 0, 0).as_deref(),
                Some("3"),
                "{}\n{script}",
                b.name
            );
        }
        clean().await;
        failing_file(&b, conn, d, dir.path()).await;
    }
}

/// The tables, rows and view [`dumps_and_runs_sql_files`] dumps: a parent with numbered ids
/// and `name` in a row, a child with keys to it and to itself, and a view of the parent.
fn dump_fixture(
    d: platypus_sql::Dialect,
    (parent, child, view): (&str, &str, &str),
    (identity, text): (&str, &str),
    name: &str,
) -> [String; 7] {
    use platypus_sql::Dialect;

    // SQLite names no schema in a key's REFERENCES.
    let (to_parent, to_child) = if d == Dialect::Sqlite {
        ("conf_dump_parent", "conf_dump_child")
    } else {
        (parent, child)
    };
    [
        format!("CREATE TABLE {parent} (id {identity}, name {text})"),
        format!(
            "CREATE TABLE {child} (id int PRIMARY KEY, parent_id int REFERENCES {to_parent} (id), \
             boss int REFERENCES {to_child} (id), amount decimal(10, 2))"
        ),
        // DuckDB numbers no key by itself.
        if d == Dialect::DuckDb {
            format!(
                "INSERT INTO {parent} VALUES (1, {}), (2, NULL)",
                d.quote_literal(name)
            )
        } else {
            format!(
                "INSERT INTO {parent} (name) VALUES ({})",
                d.quote_literal(name)
            )
        },
        if d == Dialect::DuckDb {
            "SELECT 1".to_owned()
        } else {
            format!("INSERT INTO {parent} (name) VALUES (NULL)")
        },
        format!("INSERT INTO {child} (id, parent_id, boss, amount) VALUES (1, 1, NULL, 1.5)"),
        format!("INSERT INTO {child} (id, parent_id, boss, amount) VALUES (2, 2, 1, -3.25)"),
        format!("CREATE VIEW {view} AS SELECT id, name FROM {parent}"),
    ]
}

/// What a dumped table needs and a routine's body come back: a PostgreSQL enum, `serial`
/// sequence and partitioned table (whose rows are written once), a MySQL procedure and
/// trigger with `;` in their body, a SQL Server or MariaDB sequence's position.
#[tokio::test]
async fn dumps_types_routines_and_sequences() {
    use platypus_sql::Dialect;

    let _serial = SERIAL.lock().await;
    let dir = tempfile::tempdir().unwrap();
    for b in backends().await {
        let d = Dialect::from(b.config.driver);
        let conn = b.connect().await;
        let conn = &*conn;
        let q = |name: &str| d.qualified(Some(&b.schema), name);
        let mariadb = d == Dialect::MySql && conn.server_info().product == "MariaDB";
        let (setup, drop, checks): DumpCase = match d {
            Dialect::Postgres if b.name == "postgres" => (
                vec![
                    format!("CREATE TYPE {} AS ENUM ('a', 'b')", q("conf_dump2_mood")),
                    format!(
                        "CREATE TABLE {} (id serial PRIMARY KEY, m {})",
                        q("conf_dump2_t"),
                        q("conf_dump2_mood")
                    ),
                    format!("INSERT INTO {} (m) VALUES ('a'), ('b')", q("conf_dump2_t")),
                    format!(
                        "CREATE TABLE {} (at int) PARTITION BY RANGE (at)",
                        q("conf_dump2_p")
                    ),
                    format!(
                        "CREATE TABLE {} PARTITION OF {} FOR VALUES FROM (0) TO (100)",
                        q("conf_dump2_p1"),
                        q("conf_dump2_p")
                    ),
                    format!("INSERT INTO {} VALUES (1), (2)", q("conf_dump2_p")),
                    format!(
                        "CREATE FUNCTION {}() RETURNS int LANGUAGE plpgsql AS $$ BEGIN RETURN 1; END $$",
                        q("conf_dump2_f")
                    ),
                ],
                vec![
                    format!("DROP FUNCTION {}()", q("conf_dump2_f")),
                    format!("DROP TABLE {}", q("conf_dump2_p")),
                    format!("DROP TABLE {}", q("conf_dump2_t")),
                    format!("DROP TYPE {}", q("conf_dump2_mood")),
                    format!("DROP SEQUENCE IF EXISTS {}", q("conf_dump2_t_id_seq")),
                    format!("DROP SEQUENCE IF EXISTS {}", q("conf_dump2_t_id_seq1")),
                ],
                vec![
                    (format!("SELECT COUNT(*) FROM {}", q("conf_dump2_p")), "2"),
                    (format!("SELECT {}()", q("conf_dump2_f")), "1"),
                    (
                        format!(
                            "INSERT INTO {} (m) VALUES ('b') RETURNING id",
                            q("conf_dump2_t")
                        ),
                        "3",
                    ),
                ],
            ),
            Dialect::MySql => mysql_dump_case(&q, mariadb),
            Dialect::SqlServer => (
                vec![
                    format!("CREATE SEQUENCE {} AS int START WITH 1", q("conf_dump2_s")),
                    format!("SELECT NEXT VALUE FOR {}", q("conf_dump2_s")),
                    format!("SELECT NEXT VALUE FOR {}", q("conf_dump2_s")),
                ],
                vec![format!("DROP SEQUENCE {}", q("conf_dump2_s"))],
                vec![(format!("SELECT NEXT VALUE FOR {}", q("conf_dump2_s")), "3")],
            ),
            _ => continue,
        };
        dump_round_trip(&b, conn, d, dir.path(), (&setup, &drop, &checks)).await;
        if d == Dialect::MySql {
            delimiter_across_chunks(&b, conn, dir.path()).await;
        }
    }
}

/// MySQL's case of [`dumps_types_routines_and_sequences`]: a procedure and a trigger with
/// `;` in their body, and on MariaDB a sequence.
fn mysql_dump_case(q: &dyn Fn(&str) -> String, mariadb: bool) -> DumpCase {
    let mut setup = vec![
        format!(
            "CREATE TABLE {} (id int PRIMARY KEY, n int)",
            q("conf_dump2_t")
        ),
        format!(
            "CREATE PROCEDURE {}() BEGIN DECLARE x int; SET x = 1; SELECT x; END",
            q("conf_dump2_f")
        ),
        format!(
            "CREATE TRIGGER {} BEFORE INSERT ON {} FOR EACH ROW BEGIN SET NEW.n = 7; SET NEW.n = NEW.n + 1; END",
            q("conf_dump2_tr"),
            q("conf_dump2_t")
        ),
    ];
    let mut drop = vec![
        format!("DROP PROCEDURE {}", q("conf_dump2_f")),
        format!("DROP TABLE {}", q("conf_dump2_t")),
    ];
    let mut checks = vec![
        (format!("CALL {}()", q("conf_dump2_f")), "1"),
        (
            format!("INSERT INTO {} (id) VALUES (1)", q("conf_dump2_t")),
            "",
        ),
        (format!("SELECT n FROM {}", q("conf_dump2_t")), "8"),
    ];
    if mariadb {
        setup.push(format!("CREATE SEQUENCE {}", q("conf_dump2_s")));
        setup.push(format!(
            "SELECT NEXTVAL({0}), NEXTVAL({0})",
            q("conf_dump2_s")
        ));
        drop.push(format!("DROP SEQUENCE {}", q("conf_dump2_s")));
        checks.push((format!("SELECT NEXTVAL({}) > 2", q("conf_dump2_s")), "1"));
    }
    (setup, drop, checks)
}

/// What a dump round trip runs first, drops, and checks after.
type DumpCase = (Vec<String>, Vec<String>, Vec<(String, &'static str)>);

/// A MySQL `DELIMITER` holds in the pieces of a file read after it: a procedure 4 MB on,
/// which the first piece cuts, keeps its body whole.
async fn delimiter_across_chunks(
    b: &Backend,
    conn: &dyn platypus_core::Connection,
    dir: &std::path::Path,
) {
    let p = platypus_sql::Dialect::MySql.qualified(Some(&b.schema), "conf_script_p");
    let file = dir.join("delimiter.sql");
    let head = format!("DROP PROCEDURE IF EXISTS {p};\nDELIMITER $$\n-- ");
    // The 4 MB piece ends inside the procedure's body, between its two statements.
    let body = format!(
        "\nCREATE PROCEDURE {p}() BEGIN SELECT 1; SELECT 2; END$$\nDELIMITER ;\nCALL {p}();\n"
    );
    let cut = body.find("SELECT 2").unwrap();
    let filler = "x".repeat((4 << 20) - head.len() - cut);
    std::fs::write(&file, format!("{head}{filler}{body}")).unwrap();
    let ran = platypus_transfer::script::run_file(
        conn,
        platypus_sql::Dialect::MySql,
        &file,
        false,
        |_| true,
    )
    .await
    .unwrap_or_else(|e| panic!("{}: {e}", b.name));
    assert_eq!(ran, 3, "{}", b.name);
    let _ = run(conn, &format!("DROP PROCEDURE {p}")).await;
}

/// Runs `setup`, dumps the `conf_dump2_` objects, runs `drop` and the dump, then each check
/// gives its first value (any value for an empty one).
async fn dump_round_trip(
    b: &Backend,
    conn: &dyn platypus_core::Connection,
    d: platypus_sql::Dialect,
    dir: &std::path::Path,
    (setup, drop, checks): (&[String], &[String], &[(String, &str)]),
) {
    use platypus_transfer::{dump::dump, script::run_file};

    for sql in drop {
        let _ = run(conn, sql).await;
    }
    for sql in setup {
        run(conn, sql)
            .await
            .unwrap_or_else(|e| panic!("{}: {sql}: {e}", b.name));
    }
    let only: Vec<String> = conn
        .objects(&b.schema)
        .await
        .unwrap()
        .into_iter()
        .filter(|o| o.name.starts_with("conf_dump2_"))
        .map(|o| o.name)
        .collect();
    let file = dir.join(format!("{}-2.sql", b.name));
    let mut out = std::fs::File::create(&file).unwrap();
    let dumped = dump((conn, d, &b.schema), &mut out, (true, Some(&only)), |_| {
        true
    })
    .await
    .unwrap_or_else(|e| panic!("{}: {e}", b.name));
    drop_all(conn, drop).await;
    let script = std::fs::read_to_string(&file).unwrap();
    assert!(
        dumped.skipped.is_empty(),
        "{}: {:?}",
        b.name,
        dumped.skipped
    );
    run_file(conn, d, &file, false, |_| true)
        .await
        .unwrap_or_else(|e| panic!("{}: {e}\n{script}", b.name));
    for (sql, want) in checks {
        if want.is_empty() {
            run(conn, sql)
                .await
                .unwrap_or_else(|e| panic!("{}: {sql}: {e}\n{script}", b.name));
            continue;
        }
        let rows = collect(b, conn, sql).await;
        let got = rows.first().and_then(|r| cell(r, 0, 0));
        assert_eq!(got.as_deref(), Some(*want), "{}: {sql}\n{script}", b.name);
    }
    drop_all(conn, drop).await;
}

async fn drop_all(conn: &dyn platypus_core::Connection, drop: &[String]) {
    for sql in drop {
        let _ = run(conn, sql).await;
    }
}

/// A file whose statement fails: its number and line are told; in one transaction what ran
/// before is rolled back, without one it stays.
async fn failing_file(
    b: &Backend,
    conn: &dyn platypus_core::Connection,
    d: platypus_sql::Dialect,
    dir: &std::path::Path,
) {
    use platypus_transfer::script::{ScriptError, run_file};

    let extra = d.qualified(Some(&b.schema), "conf_script_extra");
    let _ = run(conn, &format!("DROP TABLE {extra}")).await;
    run(conn, &format!("CREATE TABLE {extra} (id int)"))
        .await
        .unwrap_or_else(|e| panic!("{}: {e}", b.name));
    let file = dir.join("failing.sql");
    let missing = d.qualified(Some(&b.schema), "conf_script_missing");
    std::fs::write(
        &file,
        format!("INSERT INTO {extra} VALUES (1);\n\nINSERT INTO {missing} VALUES (1);\n"),
    )
    .unwrap();
    // Transact-SQL runs the two as one batch.
    let at = if d == platypus_sql::Dialect::SqlServer {
        (1, 1)
    } else {
        (2, 3)
    };
    let count = || async {
        let rows = collect(b, conn, &format!("SELECT COUNT(*) FROM {extra}")).await;
        cell(&rows[0], 0, 0)
    };
    for (transaction, kept) in [(true, "0"), (false, "1")] {
        match run_file(conn, d, &file, transaction, |_| true).await {
            Err(ScriptError::Statement { number, line, .. }) => {
                assert_eq!((number, line), at, "{}", b.name);
            }
            other => panic!("{}: {other:?}", b.name),
        }
        assert_eq!(
            count().await.as_deref(),
            Some(kept),
            "{} {transaction}",
            b.name
        );
    }
    if d == platypus_sql::Dialect::MySql {
        ddl_commits(b, conn, &file, (&extra, &missing)).await;
    }
    let _ = run(conn, &format!("DROP TABLE {extra}")).await;
}

/// DDL commits on its own in MySQL: in a transaction, what ran before it stays, and the
/// error says so.
async fn ddl_commits(
    b: &Backend,
    conn: &dyn platypus_core::Connection,
    file: &std::path::Path,
    (extra, missing): (&str, &str),
) {
    use platypus_transfer::script::{ScriptError, run_file};

    let d = platypus_sql::Dialect::MySql;
    let made = d.qualified(Some(&b.schema), "conf_script_made");
    let _ = run(conn, &format!("DROP TABLE {made}")).await;
    run(conn, &format!("DELETE FROM {extra}")).await.unwrap();
    std::fs::write(
        file,
        format!(
            "INSERT INTO {extra} VALUES (1);\nCREATE TABLE {made} (id int);\n\
             INSERT INTO {extra} VALUES (2);\nINSERT INTO {missing} VALUES (1);\n"
        ),
    )
    .unwrap();
    let error = run_file(conn, d, file, true, |_| true).await.unwrap_err();
    assert!(
        matches!(
            error,
            ScriptError::Statement {
                number: 4,
                committed: 2,
                ..
            }
        ),
        "{}: {error:?}",
        b.name
    );
    assert!(
        error.to_string().contains("Statements 1 to 2 stay"),
        "{error}"
    );
    let rows = collect(b, conn, &format!("SELECT COUNT(*) FROM {extra}")).await;
    assert_eq!(cell(&rows[0], 0, 0).as_deref(), Some("1"), "{}", b.name);
    let _ = run(conn, &format!("DROP TABLE {made}")).await;
}

/// A snapshot before an `UPDATE` or `DELETE` reads the rows it changes, and its statements put
/// them back as they were: quotes, NULL and decimals included. Too many rows, or an `UPDATE`
/// of a table without a key, make none.
#[tokio::test]
async fn snapshots_restore_changed_rows() {
    use platypus_sql::Dialect;
    use platypus_transfer::snapshot::{Taken, take};

    let _serial = SERIAL.lock().await;
    for b in backends().await {
        let d = Dialect::from(b.config.driver);
        match d {
            Dialect::Postgres if b.name == "postgres" => {}
            Dialect::MySql | Dialect::SqlServer | Dialect::DuckDb => {}
            Dialect::Sqlite if b.name == "sqlite" => {}
            _ => continue,
        }
        let conn = b.connect().await;
        let conn = &*conn;
        let t = d.qualified(Some(&b.schema), "conf_snap");
        let k = d.qualified(Some(&b.schema), "conf_snap_nokey");
        for table in [&t, &k] {
            let _ = run(conn, &format!("DROP TABLE {table}")).await;
        }
        let text = if d == Dialect::SqlServer {
            "nvarchar(50)"
        } else {
            "varchar(50)"
        };
        for sql in [
            format!("CREATE TABLE {t} (id int PRIMARY KEY, name {text}, amount decimal(10,2))"),
            format!("INSERT INTO {t} VALUES (1, 'it''s', 1.50), (2, NULL, -3.25), (3, 'c', 0)"),
            format!("CREATE TABLE {k} (id int, name {text})"),
        ] {
            run(conn, &sql)
                .await
                .unwrap_or_else(|e| panic!("{}: {sql}: {e}", b.name));
        }
        let rows = format!("SELECT id, name, amount FROM {t} ORDER BY id");
        for (verb, change) in [
            (
                "UPDATE",
                format!("UPDATE {t} SET name = 'x', amount = 9 WHERE id <= 2"),
            ),
            ("DELETE", format!("DELETE FROM {t} WHERE id <> 3")),
        ] {
            snapshot_round_trip(&b, conn, d, (verb, &change), &rows).await;
        }
        assert_eq!(
            take(conn, d, &format!("DELETE FROM {t}"), 2).await.unwrap(),
            Taken::TooMany,
            "{}",
            b.name
        );
        assert_eq!(
            take(conn, d, &format!("UPDATE {k} SET name = 'a'"), 10)
                .await
                .unwrap(),
            Taken::NoKey,
            "{}",
            b.name
        );
        assert_eq!(
            take(
                conn,
                d,
                &format!("UPDATE {t} SET id = id + 10 WHERE id = 1"),
                10
            )
            .await
            .unwrap(),
            Taken::NoKey,
            "{}: the key changes",
            b.name
        );
        for table in [&t, &k] {
            let _ = run(conn, &format!("DROP TABLE {table}")).await;
        }
    }
}

/// Takes a snapshot before `change` (two rows), runs it, then the snapshot's statements: the
/// rows `rows` reads are as before.
async fn snapshot_round_trip(
    b: &Backend,
    conn: &dyn platypus_core::Connection,
    d: platypus_sql::Dialect,
    (verb, change): (&str, &str),
    rows: &str,
) {
    use platypus_transfer::snapshot::{Taken, take};

    let before = format!("{:?}", collect(b, conn, rows).await);
    let Ok(Taken::Snapshot(snapshot)) = take(conn, d, change, 100).await else {
        panic!("{}: no snapshot of {change}", b.name)
    };
    assert_eq!((snapshot.verb, snapshot.rows), (verb, 2), "{}", b.name);
    run(conn, change)
        .await
        .unwrap_or_else(|e| panic!("{}: {e}", b.name));
    for sql in &snapshot.restore {
        run(conn, sql)
            .await
            .unwrap_or_else(|e| panic!("{}: {sql}: {e}", b.name));
    }
    let after = format!("{:?}", collect(b, conn, rows).await);
    assert_eq!(after, before, "{}: {:?}", b.name, snapshot.restore);
}

/// A bulk load writes text exactly (quotes, tabs, line breaks, backslashes, empty text apart
/// from NULL, bytes), in the session's transaction; refused rows fail it and leave the session
/// usable. MySQL without `local_infile` refuses the load itself, as `Unsupported`.
#[tokio::test]
async fn bulk_loads_rows() {
    let _serial = SERIAL.lock().await;

    for b in backends().await {
        let conn = b.connect().await;
        let conn = &*conn;
        if !conn.capabilities().bulk_load {
            continue;
        }
        let d = platypus_sql::Dialect::from(b.config.driver);
        let table = d.qualified(Some(&b.schema), "conf_bulk");
        let _ = run(conn, &format!("DROP TABLE {table}")).await;
        let (text, bytes) = match d {
            platypus_sql::Dialect::Postgres => ("text", "bytea"),
            platypus_sql::Dialect::SqlServer => ("nvarchar(max)", "varbinary(max)"),
            _ => ("text", "blob"),
        };
        run(
            conn,
            &format!("CREATE TABLE {table} (id int PRIMARY KEY, note {text}, raw {bytes})"),
        )
        .await
        .unwrap_or_else(|e| panic!("{}: {e}", b.name));
        let columns = ["id", "note", "raw"].map(|c| d.quote_ident(c).into_owned());
        let tricky = "a \"q\", 'x'\tb\nc\\d\r";
        let rows = vec![
            vec![
                Some("1".to_owned()),
                Some(tricky.to_owned()),
                Some("00ff".to_owned()),
            ],
            vec![Some("2".to_owned()), Some(String::new()), None],
            vec![Some("3".to_owned()), None, Some(String::new())],
        ];
        let bulk = BulkRows {
            table: &table,
            columns: &columns,
            hex: &[false, false, true],
            rows: &rows,
        };
        run(conn, "BEGIN").await.unwrap();
        match conn.bulk_load(&bulk).await {
            Err(Error::Unsupported(why)) if b.name == "mysql" => {
                eprintln!("{}: {why}", b.name);
                run(conn, "ROLLBACK").await.unwrap();
                run(conn, &format!("DROP TABLE {table}")).await.unwrap();
                continue;
            }
            loaded => assert_eq!(
                loaded.unwrap_or_else(|e| panic!("{}: {e}", b.name)),
                3,
                "{}",
                b.name
            ),
        }
        conn.commit().await.unwrap();
        let other = b.connect().await;
        let hex = match d {
            platypus_sql::Dialect::Postgres => "encode(raw, 'hex')",
            platypus_sql::Dialect::SqlServer => "LOWER(CONVERT(varchar(max), raw, 2))",
            _ => "LOWER(HEX(raw))",
        };
        let batches = collect(
            &b,
            &*other,
            &format!(
                "SELECT id, note, {hex}, CASE WHEN note IS NULL THEN 1 ELSE 0 END \
                 FROM {table} ORDER BY id"
            ),
        )
        .await;
        let row = |r: usize| -> Vec<_> { (0..4).map(|c| cell(&batches[0], c, r)).collect() };
        assert_eq!(row(0)[1].as_deref(), Some(tricky), "{}", b.name);
        assert_eq!(row(0)[2].as_deref(), Some("00ff"), "{}", b.name);
        assert_eq!(
            row(1)[1].as_deref(),
            Some(""),
            "{}: empty, not NULL",
            b.name
        );
        assert_eq!(row(1)[2], None, "{}", b.name);
        assert_eq!(row(2)[3].as_deref(), Some("1"), "{}: NULL", b.name);
        assert_eq!(row(2)[2].as_deref(), Some(""), "{}: no bytes", b.name);

        // A duplicate key fails the load, and the session goes on.
        let again = BulkRows {
            rows: &rows[..1],
            ..bulk
        };
        run(conn, "BEGIN").await.unwrap();
        assert!(
            conn.bulk_load(&again).await.is_err(),
            "{}: duplicate key",
            b.name
        );
        conn.rollback().await.unwrap();
        let count = collect(&b, conn, &format!("SELECT COUNT(*) FROM {table}")).await;
        assert_eq!(cell(&count[0], 0, 0).as_deref(), Some("3"), "{}", b.name);
        run(conn, &format!("DROP TABLE {table}")).await.unwrap();
    }
}

/// Every optional method agrees with its capability flag: unsupported ones fail with
/// `Error::Unsupported`, supported ones never do.
#[tokio::test]
async fn capabilities_match_optional_methods() {
    let _serial = SERIAL.lock().await;

    for b in backends().await {
        let conn = b.connect().await;
        let caps = conn.capabilities();
        // libSQL's HTTP protocol has no way to interrupt a statement.
        assert_eq!(
            caps.cancel,
            b.config.driver != DriverKind::Libsql,
            "{}",
            b.name
        );
        // ClickHouse is the one database without transactions.
        assert_eq!(
            caps.transactions,
            b.config.driver != DriverKind::ClickHouse,
            "{}",
            b.name
        );

        let explain = conn.explain("SELECT 1", false).await;
        assert_eq!(
            matches!(explain, Err(Error::Unsupported(_))),
            !caps.explain,
            "{}: explain vs capabilities.explain: {explain:?}",
            b.name
        );

        let object = ObjectRef {
            schema: &b.schema,
            name: "customers",
            kind: ObjectKind::Table,
            parent: None,
        };
        let ddl = conn.object_ddl(&object).await;
        assert_eq!(
            matches!(ddl, Err(Error::Unsupported(_))),
            !caps.object_ddl,
            "{}: object_ddl vs capabilities.object_ddl: {ddl:?}",
            b.name
        );

        let search = conn.search_metadata("customers").await;
        assert_eq!(
            matches!(search, Err(Error::Unsupported(_))),
            !caps.metadata_search,
            "{}: search_metadata vs capabilities.metadata_search: {search:?}",
            b.name
        );

        // No such table: a supported load fails, but not as unsupported.
        let missing = platypus_sql::Dialect::from(b.config.driver)
            .qualified(Some(&b.schema), "conf_no_such_bulk");
        let columns = ["id".to_owned()];
        let load = conn
            .bulk_load(&platypus_core::BulkRows {
                table: &missing,
                columns: &columns,
                hex: &[false],
                rows: &[vec![Some("1".to_owned())]],
            })
            .await;
        assert_eq!(
            matches!(load, Err(Error::Unsupported(_))),
            !caps.bulk_load,
            "{}: bulk_load vs capabilities.bulk_load: {load:?}",
            b.name
        );

        server_capabilities_match(&b, &*conn, caps).await;
    }
}

/// The server's sessions, statistics and maintenance agree with their capability flags.
async fn server_capabilities_match(
    b: &Backend,
    conn: &dyn Connection,
    caps: platypus_core::Capabilities,
) {
    let sessions = conn.server_sessions().await;
    assert_eq!(
        matches!(sessions, Err(Error::Unsupported(_))),
        !caps.server_sessions,
        "{}: server_sessions vs capabilities.server_sessions: {sessions:?}",
        b.name
    );
    let locks = conn.server_locks().await;
    assert_eq!(
        matches!(locks, Err(Error::Unsupported(_))),
        !caps.server_sessions,
        "{}: server_locks vs capabilities.server_sessions: {locks:?}",
        b.name
    );
    // No session has id 0: a supported cancel fails, but not as unsupported.
    let cancel = conn.end_server_session("0", EndSession::Cancel).await;
    assert_eq!(
        matches!(cancel, Err(Error::Unsupported(_))),
        !caps.cancel_server_queries,
        "{}: cancelling vs capabilities.cancel_server_queries: {cancel:?}",
        b.name
    );
    assert!(
        !caps.cancel_server_queries || caps.server_sessions,
        "{}",
        b.name
    );

    let queries = conn.top_queries().await;
    assert_eq!(
        matches!(queries, Err(Error::Unsupported(_))),
        !caps.server_statistics,
        "{}: top_queries vs capabilities.server_statistics: {queries:?}",
        b.name
    );
    let statistics = conn.object_statistics().await;
    assert_eq!(
        matches!(statistics, Err(Error::Unsupported(_))),
        !caps.server_statistics,
        "{}: object_statistics vs capabilities.server_statistics: {statistics:?}",
        b.name
    );
    // No such table: a supported operation fails, but not as unsupported.
    for operation in [
        Maintenance::Vacuum,
        Maintenance::Analyze,
        Maintenance::Reindex,
        Maintenance::Optimize,
        Maintenance::Check,
    ] {
        let done = conn
            .maintain(&b.schema, "conf_no_such_table", operation)
            .await;
        assert_eq!(
            matches!(done, Err(Error::Unsupported(_))),
            !caps.maintenance.contains(&operation),
            "{}: maintain {operation:?} vs capabilities.maintenance: {done:?}",
            b.name
        );
    }
}

/// Session `id` in the list of `admin` once `ready` holds for it, within 20 seconds.
async fn server_session(
    b: &Backend,
    admin: &dyn Connection,
    id: &str,
    ready: impl Fn(&ServerSession) -> bool,
) -> ServerSession {
    for _ in 0..80 {
        let sessions = admin
            .server_sessions()
            .await
            .unwrap_or_else(|e| panic!("{}: server_sessions: {e}", b.name));
        if let Some(session) = sessions.into_iter().find(|s| s.id == id && ready(s)) {
            return session;
        }
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
    panic!("{}: session {id} never got there", b.name)
}

/// The id of the session of `conn`, from its own list.
async fn own_session(b: &Backend, conn: &dyn Connection) -> String {
    let sessions = conn
        .server_sessions()
        .await
        .unwrap_or_else(|e| panic!("{}: server_sessions: {e}", b.name));
    let own: Vec<_> = sessions.iter().filter(|s| s.current).collect();
    assert_eq!(
        own.len(),
        1,
        "{}: one current session: {sessions:?}",
        b.name
    );
    assert_eq!(sessions[0].id, own[0].id, "{}: listed first", b.name);
    own[0].id.clone()
}

/// The session list shows another session running its statement, which is cancelled
/// (where the server can) and then ended from the first.
#[tokio::test]
async fn lists_and_ends_server_sessions() {
    let _serial = SERIAL.lock().await;

    for b in backends().await {
        let admin = b.connect().await;
        let caps = admin.capabilities();
        if !caps.server_sessions {
            continue;
        }
        let worker = b.connect().await;
        let id = own_session(&b, &*worker).await;
        assert_ne!(id, own_session(&b, &*admin).await, "{}", b.name);

        let endless = b.endless_sql;
        let start = |conn: Box<dyn Connection>| {
            tokio::spawn(async move {
                let result = run(&*conn, endless).await;
                (conn, result)
            })
        };
        let mut worker = worker;
        if caps.cancel_server_queries {
            let task = start(worker);
            // MySQL shows the statement a moment after the session starts it.
            let running =
                server_session(&b, &*admin, &id, |s| s.is_running() && s.query.is_some()).await;
            assert!(!running.current, "{}: {running:?}", b.name);
            assert!(running.query.is_some(), "{}: {running:?}", b.name);
            admin
                .end_server_session(&id, EndSession::Cancel)
                .await
                .unwrap_or_else(|e| panic!("{}: cancel: {e}", b.name));
            let (conn, result) = tokio::time::timeout(Duration::from_secs(20), task)
                .await
                .unwrap_or_else(|_| panic!("{}: the cancel did not stop it", b.name))
                .unwrap();
            assert!(result.is_err(), "{}", b.name);
            // The session stays.
            let rows = collect(&b, &*conn, "SELECT 1").await;
            assert_eq!(cell(&rows[0], 0, 0).as_deref(), Some("1"), "{}", b.name);
            worker = conn;
        }

        let task = start(worker);
        server_session(&b, &*admin, &id, ServerSession::is_running).await;
        admin
            .end_server_session(&id, EndSession::Terminate)
            .await
            .unwrap_or_else(|e| panic!("{}: terminate: {e}", b.name));
        let (_, result) = tokio::time::timeout(Duration::from_secs(20), task)
            .await
            .unwrap_or_else(|_| panic!("{}: ending the session did not stop it", b.name))
            .unwrap();
        assert!(result.is_err(), "{}", b.name);
        let mut gone = false;
        for _ in 0..40 {
            let sessions = admin.server_sessions().await.unwrap();
            if sessions.iter().all(|s| s.id != id) {
                gone = true;
                break;
            }
            tokio::time::sleep(Duration::from_millis(250)).await;
        }
        assert!(gone, "{}: session {id} is still listed", b.name);
        assert!(
            admin
                .end_server_session("1; SELECT 1", EndSession::Terminate)
                .await
                .is_err(),
            "{}",
            b.name
        );
        // The session asking is refused, and stays.
        let own = own_session(&b, &*admin).await;
        assert!(
            admin
                .end_server_session(&own, EndSession::Terminate)
                .await
                .is_err(),
            "{}: ended itself",
            b.name
        );
        let rows = collect(&b, &*admin, "SELECT 1").await;
        assert_eq!(cell(&rows[0], 0, 0).as_deref(), Some("1"), "{}", b.name);
    }
}

/// The statistics list a new table and its indexes, each maintenance operation of the
/// database runs on it, and the costliest statements are read where the server keeps them
/// (`pg_stat_statements`, `performance_schema`, the plan cache).
#[tokio::test]
async fn reads_server_statistics_and_maintains_tables() {
    let _serial = SERIAL.lock().await;

    for b in backends().await {
        let conn = b.connect().await;
        let caps = conn.capabilities();
        if !caps.server_statistics {
            continue;
        }
        let table = format!("{}.conf_stats", b.schema);
        run(&*conn, &format!("DROP TABLE IF EXISTS {table}"))
            .await
            .unwrap();
        run(
            &*conn,
            &format!("CREATE TABLE {table} (id INT PRIMARY KEY, v INT)"),
        )
        .await
        .unwrap_or_else(|e| panic!("{}: {e}", b.name));
        run(&*conn, &format!("CREATE INDEX conf_stats_v ON {table} (v)"))
            .await
            .unwrap_or_else(|e| panic!("{}: {e}", b.name));
        run(
            &*conn,
            &format!("INSERT INTO {table} (id, v) VALUES (1, 10), (2, 20), (3, 30)"),
        )
        .await
        .unwrap();
        for _ in 0..3 {
            collect(&b, &*conn, &format!("SELECT v FROM {table} WHERE id = 2")).await;
        }

        let objects = conn
            .object_statistics()
            .await
            .unwrap_or_else(|e| panic!("{}: object_statistics: {e}", b.name));
        let ours = |o: &&platypus_core::ObjectStatistics| {
            o.schema.eq_ignore_ascii_case(&b.schema)
                && (o.name == "conf_stats" || o.table.as_deref() == Some("conf_stats"))
        };
        let found: Vec<_> = objects.iter().filter(ours).collect();
        assert_eq!(
            found[0].kind,
            StatisticsKind::Table,
            "{}: {found:?}",
            b.name
        );
        assert_eq!(found[0].name, "conf_stats", "{}", b.name);
        let indexes = &found[1..];
        assert!(
            indexes
                .iter()
                .any(|i| i.name == "conf_stats_v" && !i.unique),
            "{}: {indexes:?}",
            b.name
        );
        assert!(
            indexes.iter().any(|i| i.unique),
            "{}: the primary key: {indexes:?}",
            b.name
        );
        assert!(
            indexes.iter().all(|i| i.kind == StatisticsKind::Index),
            "{}",
            b.name
        );

        for &operation in caps.maintenance {
            conn.maintain(&b.schema, "conf_stats", operation)
                .await
                .unwrap_or_else(|e| panic!("{}: {operation:?}: {e}", b.name));
        }
        // The table is still there and whole.
        let rows = collect(&b, &*conn, &format!("SELECT count(*) FROM {table}")).await;
        assert_eq!(cell(&rows[0], 0, 0).as_deref(), Some("3"), "{}", b.name);

        if b.config.driver == DriverKind::Postgres {
            // Needs the library preloaded (docker/compose.yml does).
            let _ = run(&*conn, "CREATE EXTENSION IF NOT EXISTS pg_stat_statements").await;
        }
        match conn.top_queries().await {
            Ok(queries) => {
                assert!(!queries.is_empty(), "{}: no statements", b.name);
                assert!(queries.len() <= platypus_core::TOP_QUERIES, "{}", b.name);
                assert!(
                    queries
                        .windows(2)
                        .all(|w| w[0].total_seconds >= w[1].total_seconds),
                    "{}: the costliest first",
                    b.name
                );
                assert!(queries.iter().all(|q| q.calls > 0 && !q.query.is_empty()));
            }
            // A server without the statistics says how to have them.
            Err(e) => assert!(
                e.to_string().contains("pg_stat_statements")
                    || e.to_string().contains("performance_schema"),
                "{}: top_queries: {e}",
                b.name
            ),
        }
        run(&*conn, &format!("DROP TABLE {table}")).await.unwrap();
    }
}

/// A session waiting for a row another one updates shows whom it waits for, and both
/// their locks are listed.
#[tokio::test]
async fn shows_blocking_chains() {
    let _serial = SERIAL.lock().await;

    for b in backends().await {
        let admin = b.connect().await;
        if !admin.capabilities().server_sessions {
            continue;
        }
        let table = format!("{}.conf_admin_lock", b.schema);
        run(&*admin, &format!("DROP TABLE IF EXISTS {table}"))
            .await
            .unwrap();
        run(
            &*admin,
            &format!("CREATE TABLE {table} (id INT PRIMARY KEY, v INT)"),
        )
        .await
        .unwrap_or_else(|e| panic!("{}: {e}", b.name));
        run(&*admin, &format!("INSERT INTO {table} VALUES (1, 0)"))
            .await
            .unwrap();

        let holder = b.connect().await;
        let holder_id = own_session(&b, &*holder).await;
        holder.set_autocommit(false).await.unwrap();
        run(&*holder, &format!("UPDATE {table} SET v = 1 WHERE id = 1"))
            .await
            .unwrap_or_else(|e| panic!("{}: {e}", b.name));
        let waiter = b.connect().await;
        let waiter_id = own_session(&b, &*waiter).await;
        let update = format!("UPDATE {table} SET v = 2 WHERE id = 1");
        let task = tokio::spawn(async move { run(&*waiter, &update).await });

        let blocked = server_session(&b, &*admin, &waiter_id, |s| {
            s.blocked_by.contains(&holder_id)
        })
        .await;
        assert!(blocked.is_running(), "{}: {blocked:?}", b.name);
        let locks = admin
            .server_locks()
            .await
            .unwrap_or_else(|e| panic!("{}: server_locks: {e}", b.name));
        assert!(
            locks.iter().any(|l| l.session == waiter_id && !l.granted),
            "{}: the waiter's lock: {locks:?}",
            b.name
        );
        assert!(
            locks.iter().any(|l| l.session == holder_id
                && l.granted
                && l.object
                    .as_deref()
                    .is_some_and(|o| o.to_lowercase().ends_with("conf_admin_lock"))),
            "{}: the holder's lock on the table: {locks:?}",
            b.name
        );
        let own = own_session(&b, &*admin).await;
        assert!(locks.iter().all(|l| l.session != own), "{}", b.name);

        holder.rollback().await.unwrap();
        tokio::time::timeout(Duration::from_secs(20), task)
            .await
            .unwrap_or_else(|_| panic!("{}: the waiter never got the row", b.name))
            .unwrap()
            .unwrap_or_else(|e| panic!("{}: {e}", b.name));
        drop(holder);
        run(&*admin, &format!("DROP TABLE {table}")).await.unwrap();
    }
}

/// Constraints, incoming references, triggers and DDL, on a probe table created for the
/// test (with a primary key, a unique column, a check and a trigger) and on the fixtures.
#[tokio::test]
async fn describes_objects_and_their_ddl() {
    let _serial = SERIAL.lock().await;

    for b in backends().await {
        let conn = b.connect().await;
        let suffix = std::process::id();
        let (table, trigger) = (format!("probe_t{suffix}"), format!("probe_trg{suffix}"));
        let (setup, teardown) = probe_sql(&b, &table, &trigger, suffix);
        for sql in teardown.iter().chain(&setup) {
            run(&*conn, sql)
                .await
                .unwrap_or_else(|e| panic!("{}: {sql}: {e}", b.name));
        }
        check_probe(&b, &*conn, &table, &trigger).await;
        check_fixture_objects(&b, &*conn).await;
        for sql in &teardown {
            run(&*conn, sql).await.unwrap();
        }
    }
}

/// `(setup, teardown)` statements for the probe table and its trigger.
fn probe_sql(b: &Backend, table: &str, trigger: &str, suffix: u32) -> (Vec<String>, Vec<String>) {
    let s = &b.schema;
    let columns = match b.config.driver {
        DriverKind::Mongo | DriverKind::Redis => {
            unreachable!("MongoDB and Redis have their own tests in mongo.rs and redis.rs")
        }
        bridged!() => unreachable!("{BRIDGED}"),
        DriverKind::Postgres => "id int PRIMARY KEY, code text UNIQUE, qty int CHECK (qty >= 0)",
        DriverKind::MySql => {
            "id INT PRIMARY KEY, code VARCHAR(10) UNIQUE, qty INT CHECK (qty >= 0)"
        }
        DriverKind::Sqlite | DriverKind::Libsql => {
            "id INTEGER PRIMARY KEY, code TEXT UNIQUE, qty INT CHECK (qty >= 0)"
        }
        DriverKind::DuckDb => {
            "id INTEGER PRIMARY KEY, code VARCHAR UNIQUE, qty INTEGER CHECK (qty >= 0)"
        }
        DriverKind::SqlServer => {
            "id INT PRIMARY KEY, code NVARCHAR(10) UNIQUE, qty INT CHECK (qty >= 0)"
        }
        DriverKind::ClickHouse => {
            "id Int32, code String, qty Int32, CONSTRAINT qty_ok CHECK qty >= 0"
        }
    };
    let create = format!(
        "CREATE TABLE {s}.{table} ({columns}){}",
        b.table_options("id")
    );
    let drop = format!("DROP TABLE IF EXISTS {s}.{table}");
    match b.config.driver {
        DriverKind::Mongo | DriverKind::Redis => {
            unreachable!("MongoDB and Redis have their own tests in mongo.rs and redis.rs")
        }
        bridged!() => unreachable!("{BRIDGED}"),
        DriverKind::Postgres => {
            let function = format!("{s}.probe_fn{suffix}()");
            (
                vec![
                    create,
                    format!(
                        "CREATE FUNCTION {function} RETURNS trigger LANGUAGE plpgsql AS $$BEGIN RETURN NEW; END$$"
                    ),
                    format!(
                        "CREATE TRIGGER {trigger} BEFORE INSERT ON {s}.{table} FOR EACH ROW EXECUTE FUNCTION {function}"
                    ),
                ],
                vec![drop, format!("DROP FUNCTION IF EXISTS {function}")],
            )
        }
        DriverKind::MySql => (
            vec![
                create,
                format!(
                    "CREATE TRIGGER {s}.{trigger} BEFORE INSERT ON {s}.{table} FOR EACH ROW SET NEW.qty = NEW.qty"
                ),
            ],
            vec![drop],
        ),
        DriverKind::Sqlite | DriverKind::Libsql => (
            vec![
                create,
                format!(
                    "CREATE TRIGGER {s}.{trigger} BEFORE INSERT ON {table} BEGIN SELECT 1; END"
                ),
            ],
            vec![drop],
        ),
        // DuckDB and ClickHouse have no triggers.
        DriverKind::DuckDb | DriverKind::ClickHouse => (vec![create], vec![drop]),
        DriverKind::SqlServer => (
            vec![
                create,
                format!(
                    "CREATE TRIGGER {s}.{trigger} ON {s}.{table} AFTER INSERT AS SET NOCOUNT ON"
                ),
            ],
            vec![drop],
        ),
    }
}

async fn check_probe(b: &Backend, conn: &dyn Connection, table: &str, trigger: &str) {
    let s = &b.schema;
    let details = conn.table_details(s, table).await.unwrap();
    let has = |kind| details.constraints.iter().any(|c| c.kind == kind);
    assert!(
        has(ConstraintKind::PrimaryKey),
        "{}: {:?}",
        b.name,
        details.constraints
    );
    assert_eq!(
        has(ConstraintKind::Unique),
        b.keys(),
        "{}: {:?}",
        b.name,
        details.constraints
    );
    // SQLite keeps checks only in the table's SQL; MySQL before 8.0.16 ignores them.
    if !b.speaks_sqlite() && !mysql_before(conn, MYSQL_CHECKS) {
        let check = details
            .constraints
            .iter()
            .find(|c| c.kind == ConstraintKind::Check);
        assert!(
            check.is_some_and(|c| c.definition.contains("qty")),
            "{}: {:?}",
            b.name,
            details.constraints
        );
    }

    let object = |name, kind, parent| ObjectRef {
        schema: s,
        name,
        kind,
        parent,
    };
    let ddl = conn
        .object_ddl(&object(table, ObjectKind::Table, None))
        .await
        .unwrap();
    for part in ["CREATE TABLE", "code", "qty"] {
        assert!(ddl.contains(part), "{}: {part} in {ddl}", b.name);
    }
    if matches!(b.config.driver, DriverKind::DuckDb | DriverKind::ClickHouse) {
        return;
    }

    let objects = conn.objects(s).await.unwrap();
    let found = objects.iter().find(|o| o.name == trigger);
    assert_eq!(
        found.map(|o| o.kind),
        Some(ObjectKind::Trigger),
        "{}",
        b.name
    );
    assert_eq!(
        found.and_then(|o| o.parent.as_deref()),
        Some(table),
        "{}",
        b.name
    );

    let ddl = conn
        .object_ddl(&object(trigger, ObjectKind::Trigger, Some(table)))
        .await
        .unwrap();
    assert!(
        ddl.to_uppercase().contains("CREATE") && ddl.contains(trigger),
        "{}: {ddl}",
        b.name
    );
}

async fn check_fixture_objects(b: &Backend, conn: &dyn Connection) {
    let object = |name, kind| ObjectRef {
        schema: &b.schema,
        name,
        kind,
        parent: None,
    };
    let ddl = conn
        .object_ddl(&object("orders", ObjectKind::Table))
        .await
        .unwrap();
    assert!(
        ddl.contains("customers") || !b.keys(),
        "{}: FK missing in {ddl}",
        b.name
    );
    let ddl = conn
        .object_ddl(&object("customer_totals", ObjectKind::View))
        .await
        .unwrap();
    assert!(ddl.contains("customer_totals"), "{}: {ddl}", b.name);

    let customers = conn.table_details(&b.schema, "customers").await.unwrap();
    let incoming = customers.referenced_by.iter().find(|r| r.table == "orders");
    assert!(
        incoming.is_some_and(|r| r.columns == ["customer_id"] && r.ref_columns == ["id"])
            || !b.keys(),
        "{}: {:?}",
        b.name,
        customers.referenced_by
    );

    let props = conn
        .object_properties(&object("customers", ObjectKind::Table))
        .await
        .unwrap();
    // SQLite has no catalog of owners or sizes.
    if !b.speaks_sqlite() {
        assert!(!props.is_empty(), "{}: no properties", b.name);
    }
}

/// Creates the temporary table `conf_edit` and returns its result schema.
async fn edit_fixture(
    b: &Backend,
    conn: &dyn Connection,
) -> platypus_core::arrow_schema::SchemaRef {
    let ddl = match b.config.driver {
        DriverKind::Mongo | DriverKind::Redis => {
            unreachable!("MongoDB and Redis have their own tests in mongo.rs and redis.rs")
        }
        bridged!() => unreachable!("{BRIDGED}"),
        DriverKind::Postgres => {
            "CREATE TEMPORARY TABLE conf_edit (id INT PRIMARY KEY, name TEXT, \
                 amount NUMERIC(10, 2), data BYTEA, at TIMESTAMPTZ, n INT DEFAULT 7)"
        }
        DriverKind::MySql => {
            "CREATE TEMPORARY TABLE conf_edit (id INT PRIMARY KEY, name TEXT, \
                 amount DECIMAL(10, 2), data BLOB, at TIMESTAMP NULL, n INT DEFAULT 7)"
        }
        DriverKind::Sqlite | DriverKind::Libsql => {
            "CREATE TEMPORARY TABLE conf_edit (id INTEGER PRIMARY KEY, name TEXT, \
                 amount NUMERIC, data BLOB, at TEXT, n INT DEFAULT 7)"
        }
        DriverKind::DuckDb => {
            "CREATE TEMPORARY TABLE conf_edit (id INTEGER PRIMARY KEY, name VARCHAR, \
                 amount DECIMAL(10, 2), data BLOB, \"at\" TIMESTAMPTZ, n INTEGER DEFAULT 7)"
        }
        DriverKind::SqlServer => {
            "CREATE TABLE #conf_edit (id INT PRIMARY KEY, name NVARCHAR(MAX), \
                 amount DECIMAL(10, 2), data VARBINARY(MAX), at DATETIMEOFFSET, n INT DEFAULT 7)"
        }
        DriverKind::ClickHouse => {
            "CREATE TEMPORARY TABLE conf_edit (id Int32, name Nullable(String),                  amount Decimal(10, 2), data Nullable(String), at Nullable(DateTime64(6, 'UTC')),                  n Int32 DEFAULT 7)"
        }
    };
    affected(b, conn, &ddl.replacen("TEMPORARY ", b.temporary(), 1)).await;
    let ExecResult::Rows(stream) = conn
        .execute(
            &format!("SELECT * FROM {}", b.temp("conf_edit")),
            &ExecOptions::default(),
        )
        .await
        .unwrap()
    else {
        panic!("{}: expected rows", b.name);
    };
    let schema = stream.schema();
    drop(stream);
    schema
}

/// Data editor statements round-trip on every driver: quoting, NULL, defaults, binary
/// values, timestamps with a time zone as row keys, and all-or-nothing saving.
#[tokio::test]
async fn edits_table_data() {
    use platypus_sql::Dialect;
    use platypus_sql::dml::{self, Column, Statement, Value, ValueKind};

    let _serial = SERIAL.lock().await;

    for b in backends().await {
        let conn = b.connect().await;
        let conn = &*conn;
        // Saving edits checks each statement's row count inside a transaction; ClickHouse
        // has no transactions and its updates report no row counts.
        if !conn.capabilities().transactions {
            continue;
        }
        let schema = edit_fixture(&b, conn).await;
        let columns: Vec<Column<'_>> = schema
            .fields()
            .iter()
            .map(|f| Column {
                name: f.name(),
                kind: ValueKind::of(f.data_type()),
                default: (f.name() == "n").then_some("7"),
            })
            .collect();
        let d = Dialect::from(b.config.driver);
        let t = &b.temp("conf_edit");
        let at = "2024-03-01 10:00:00+00:00";
        let name = r#"it's \ "q""#;

        let inserts = dml::inserts(
            d,
            t,
            &columns,
            &[
                vec![
                    (0, Value::Text("1")),
                    (1, Value::Text(name)),
                    (2, Value::Text("12.50")),
                    (3, Value::Text("00ff10")),
                    (4, Value::Text(at)),
                    (5, Value::Default),
                ],
                vec![(0, Value::Text("2"))],
            ],
        )
        .unwrap();
        dml::apply(conn, d, &inserts)
            .await
            .unwrap_or_else(|e| panic!("{}: insert: {e}", b.name));
        let rows = collect(
            &b,
            conn,
            &format!("SELECT id, name, amount, data, n FROM {t} ORDER BY id"),
        )
        .await;
        let batch = &rows[0];
        assert_eq!(batch.num_rows(), 2, "{}", b.name);
        assert_eq!(
            cell(batch, 1, 0).as_deref(),
            Some(name),
            "{}: quoting",
            b.name
        );
        let amount: f64 = cell(batch, 2, 0).unwrap().parse().unwrap();
        assert!((amount - 12.5).abs() < f64::EPSILON, "{}: {amount}", b.name);
        assert_eq!(
            cell(batch, 3, 0).as_deref(),
            Some("00ff10"),
            "{}: binary",
            b.name
        );
        assert_eq!(cell(batch, 1, 1), None, "{}: NULL", b.name);
        assert_eq!(
            cell(batch, 4, 1).as_deref(),
            Some("7"),
            "{}: default",
            b.name
        );

        // The timestamp is part of the key, so its literal must match the stored value.
        let update = dml::update(
            d,
            t,
            &columns,
            &[(1, Value::Null), (5, Value::Default)],
            &[(0, Some("1")), (4, Some(at))],
        )
        .unwrap();
        dml::apply(conn, d, &[Statement::one(update)])
            .await
            .unwrap_or_else(|e| panic!("{}: update: {e}", b.name));

        edit_rollbacks(&b, conn, &columns).await;
    }
}

/// The all-or-nothing part of `edits_table_data`, on its `conf_edit` rows 1 and 2.
async fn edit_rollbacks(
    b: &Backend,
    conn: &dyn Connection,
    columns: &[platypus_sql::dml::Column<'_>],
) {
    use platypus_sql::Dialect;
    use platypus_sql::dml::{self, ApplyError, Statement, Value};

    let (d, t) = (Dialect::from(b.config.driver), &b.temp("conf_edit"));

    // A statement that misses its row rolls back the ones before it.
    let ok = dml::update(d, t, columns, &[(1, Value::Text("x"))], &[(0, Some("2"))]).unwrap();
    let missing = dml::update(d, t, columns, &[(1, Value::Text("y"))], &[(0, Some("99"))]).unwrap();
    let missing = Statement::one(missing);
    let err = dml::apply(conn, d, &[Statement::one(ok), missing.clone()])
        .await
        .unwrap_err();
    assert!(
        matches!(
            err,
            ApplyError::RowCount {
                index: 1,
                rows: 0,
                ..
            }
        ),
        "{}: {err}",
        b.name
    );
    let sql = format!("SELECT count(*) FROM {t} WHERE name IS NULL");
    let rows = collect(b, conn, &sql).await;
    assert_eq!(
        cell(&rows[0], 0, 0).as_deref(),
        Some("2"),
        "{}: rolled back",
        b.name
    );
    assert!(!conn.transaction().open, "{}", b.name);

    // In manual-commit mode the changes join the session's transaction.
    conn.set_autocommit(false).await.unwrap();
    let delete = dml::delete(d, t, columns, &[(0, Some("2"))]).unwrap();
    dml::apply(conn, d, &[Statement::one(delete)])
        .await
        .unwrap_or_else(|e| panic!("{}: delete: {e}", b.name));
    assert!(conn.transaction().open, "{}: left uncommitted", b.name);
    // A missing row that changed nothing keeps the session's work, savepoints or not.
    let err = dml::apply(conn, d, &[missing]).await.unwrap_err();
    assert!(
        matches!(
            err,
            ApplyError::RowCount {
                index: 0,
                rows: 0,
                ..
            }
        ),
        "{}: {err}",
        b.name
    );
    assert!(conn.transaction().open, "{}: still open", b.name);
    let rows = collect(b, conn, &format!("SELECT count(*) FROM {t}")).await;
    assert_eq!(
        cell(&rows[0], 0, 0).as_deref(),
        Some("1"),
        "{}: delete kept",
        b.name
    );
    conn.rollback().await.unwrap();
    conn.set_autocommit(true).await.unwrap();
    let rows = collect(b, conn, &format!("SELECT count(*) FROM {t}")).await;
    assert_eq!(cell(&rows[0], 0, 0).as_deref(), Some("2"), "{}", b.name);
}

/// Many new and deleted rows go in batched statements on every driver: multi-row `VALUES`,
/// key lists by composite and simple keys, quoted text in the keys; a batch that finds
/// fewer rows than it names rolls back.
#[tokio::test]
async fn edits_many_rows() {
    use platypus_sql::Dialect;
    use platypus_sql::dml::{self, ApplyError, Column, Value, ValueKind};

    const ROWS: usize = 1200;
    let _serial = SERIAL.lock().await;
    for b in backends().await {
        let conn = b.connect().await;
        let conn = &*conn;
        if !conn.capabilities().transactions {
            continue;
        }
        let d = Dialect::from(b.config.driver);
        let t = &b.temp("conf_many");
        let text = if b.config.driver == DriverKind::SqlServer {
            "NVARCHAR(40)"
        } else {
            "VARCHAR(40)"
        };
        let ddl =
            format!("id INT NOT NULL, tag {text} NOT NULL, label {text}, PRIMARY KEY (id, tag)");
        affected(&b, conn, &b.create_temp("conf_many", &ddl)).await;
        let columns = [
            Column {
                name: "id",
                kind: ValueKind::Integer,
                default: None,
            },
            Column {
                name: "tag",
                kind: ValueKind::Text,
                default: None,
            },
            Column {
                name: "label",
                kind: ValueKind::Text,
                default: None,
            },
        ];
        let ids: Vec<String> = (1..=ROWS).map(|i| i.to_string()).collect();
        let tags: Vec<String> = (1..=ROWS).map(|i| format!("it's \\ {}", i % 3)).collect();
        let count = || async {
            let rows = collect(&b, conn, &format!("SELECT count(*) FROM {t}")).await;
            cell(&rows[0], 0, 0).unwrap()
        };

        let rows: Vec<Vec<(usize, Value<'_>)>> = (0..ROWS)
            .map(|i| {
                let label = if i % 7 == 0 {
                    Value::Null
                } else {
                    Value::Text(&ids[i])
                };
                vec![
                    (0, Value::Text(&ids[i])),
                    (1, Value::Text(&tags[i])),
                    (2, label),
                ]
            })
            .collect();
        let inserts = dml::inserts(d, t, &columns, &rows).unwrap();
        assert_eq!(inserts.len(), 3, "{}", b.name);
        dml::apply(conn, d, &inserts)
            .await
            .unwrap_or_else(|e| panic!("{}: insert: {e}", b.name));
        assert_eq!(count().await, ROWS.to_string(), "{}: inserted", b.name);

        let key = |i: usize| vec![(0, Some(ids[i].as_str())), (1, Some(tags[i].as_str()))];
        let deletes =
            dml::deletes(d, t, &columns, &(0..1100).map(key).collect::<Vec<_>>()).unwrap();
        assert_eq!(deletes.len(), 3, "{}", b.name);
        dml::apply(conn, d, &deletes)
            .await
            .unwrap_or_else(|e| panic!("{}: delete: {e}", b.name));
        assert_eq!(count().await, "100", "{}: deleted", b.name);

        // A row deleted meanwhile: the batch finds one row fewer than it names.
        let gone: Vec<_> = (1100..ROWS).chain([4]).map(key).collect();
        let err = dml::apply(conn, d, &dml::deletes(d, t, &columns, &gone).unwrap())
            .await
            .unwrap_err();
        assert!(
            matches!(
                err,
                ApplyError::RowCount {
                    index: 0,
                    rows: 100,
                    expected: 101
                }
            ),
            "{}: {err}",
            b.name
        );
        assert_eq!(count().await, "100", "{}: rolled back", b.name);
        assert!(!conn.transaction().open, "{}", b.name);

        // By a key of one column.
        let by_id: Vec<_> = (1100..ROWS)
            .map(|i| vec![(0, Some(ids[i].as_str()))])
            .collect();
        dml::apply(conn, d, &dml::deletes(d, t, &columns, &by_id).unwrap())
            .await
            .unwrap_or_else(|e| panic!("{}: delete by id: {e}", b.name));
        assert_eq!(count().await, "0", "{}: all deleted", b.name);
    }
}

/// The statements that switch backslash escapes in string literals off and back on:
/// `standard_conforming_strings` on PostgreSQL, `NO_BACKSLASH_ESCAPES` on MySQL.
async fn backslash_modes(b: &Backend, conn: &dyn Connection) -> Option<[String; 2]> {
    match (b.config.driver, b.name) {
        (DriverKind::Postgres, "postgres" | "timescale" | "yugabyte") => Some([
            "SET standard_conforming_strings = on".to_owned(),
            "SET standard_conforming_strings = off".to_owned(),
        ]),
        (DriverKind::MySql, _) => {
            let mode = collect(b, conn, "SELECT @@SESSION.sql_mode").await;
            let mode = cell(&mode[0], 0, 0).unwrap_or_default();
            let with = if mode.is_empty() {
                "NO_BACKSLASH_ESCAPES".to_owned()
            } else {
                format!("{mode},NO_BACKSLASH_ESCAPES")
            };
            Some([
                format!("SET SESSION sql_mode = '{with}'"),
                format!("SET SESSION sql_mode = '{mode}'"),
            ])
        }
        _ => None,
    }
}

/// Text with backslashes and quotes saves as typed, is found again by key and by the data
/// search, and never ends its literal early, whatever the session says of backslashes:
/// `standard_conforming_strings` on PostgreSQL, `NO_BACKSLASH_ESCAPES` on MySQL.
#[tokio::test]
async fn edits_text_with_backslashes() {
    use platypus_sql::Dialect;
    use platypus_sql::dml::{self, Column, Value, ValueKind};

    let _serial = SERIAL.lock().await;
    let texts = [r"x\'); DROP TABLE conf_scs; --", r"a\b", "it's", r"\\"];
    for b in backends().await {
        let conn = b.connect().await;
        let conn = &*conn;
        let Some(modes) = backslash_modes(&b, conn).await else {
            continue;
        };
        let d = Dialect::from(b.config.driver);
        let columns = [
            Column {
                name: "id",
                kind: ValueKind::Integer,
                default: None,
            },
            Column {
                name: "name",
                kind: ValueKind::Text,
                default: None,
            },
        ];
        affected(
            &b,
            conn,
            "CREATE TEMPORARY TABLE conf_scs (id INT PRIMARY KEY, name TEXT)",
        )
        .await;
        for setting in &modes {
            run(conn, setting)
                .await
                .unwrap_or_else(|e| panic!("{}: {e}", b.name));
            let ids: Vec<String> = (1..=texts.len()).map(|i| i.to_string()).collect();
            let rows: Vec<Vec<(usize, Value<'_>)>> = ids
                .iter()
                .zip(texts)
                .map(|(id, text)| vec![(0, Value::Text(id)), (1, Value::Text(text))])
                .collect();
            let inserts = dml::inserts(d, "conf_scs", &columns, &rows).unwrap();
            dml::apply(conn, d, &inserts)
                .await
                .unwrap_or_else(|e| panic!("{}: insert after {setting}: {e}", b.name));
            let saved = collect(&b, conn, "SELECT name FROM conf_scs ORDER BY id").await;
            let saved: Vec<_> = (0..texts.len())
                .map(|i| cell(&saved[0], 0, i).unwrap())
                .collect();
            assert_eq!(saved, texts, "{}: {setting}", b.name);
            let search = format!(
                "SELECT count(*) FROM conf_scs WHERE {}",
                d.contains("name", r"a\b")
            );
            let found = collect(&b, conn, &search).await;
            assert_eq!(
                cell(&found[0], 0, 0).as_deref(),
                Some("1"),
                "{}: {search}",
                b.name
            );
            // Keys with backslashes find their rows.
            let keys: Vec<_> = texts.iter().map(|t| vec![(1, Some(*t))]).collect();
            let deletes = dml::deletes(d, "conf_scs", &columns, &keys).unwrap();
            dml::apply(conn, d, &deletes)
                .await
                .unwrap_or_else(|e| panic!("{}: delete after {setting}: {e}", b.name));
            let left = collect(&b, conn, "SELECT count(*) FROM conf_scs").await;
            assert_eq!(cell(&left[0], 0, 0).as_deref(), Some("0"), "{}", b.name);
        }
    }
}

// Composite keys of timestamps with a time zone, decimals and UUIDs find their rows in a
/// batched delete, on the dialects that list them as row values or with `OR`.
#[tokio::test]
async fn edits_composite_keys() {
    use platypus_sql::Dialect;
    use platypus_sql::dml::{self, Column, Value, ValueKind};

    let _serial = SERIAL.lock().await;
    let rows = [
        [
            "2024-03-01 10:00:00+00:00",
            "12.50",
            "a0eebc99-9c0b-4ef8-bb6d-6bb9bd380a11",
        ],
        [
            "2024-03-01 10:00:00+00:00",
            "1e3",
            "a0eebc99-9c0b-4ef8-bb6d-6bb9bd380a12",
        ],
        [
            "2024-03-02 11:30:00+00:00",
            "12.50",
            "a0eebc99-9c0b-4ef8-bb6d-6bb9bd380a11",
        ],
    ];
    for b in backends().await {
        let ddl = match b.config.driver {
            DriverKind::Postgres if b.name == "postgres" => {
                "at TIMESTAMPTZ, amount NUMERIC(10, 2), u UUID, PRIMARY KEY (at, amount, u)"
            }
            DriverKind::DuckDb => {
                "\"at\" TIMESTAMPTZ, amount DECIMAL(10, 2), u UUID, PRIMARY KEY (\"at\", amount, u)"
            }
            DriverKind::Sqlite | DriverKind::Libsql => {
                "at TEXT, amount NUMERIC, u TEXT, PRIMARY KEY (at, amount, u)"
            }
            _ => continue,
        };
        let conn = b.connect().await;
        let conn = &*conn;
        let d = Dialect::from(b.config.driver);
        if b.config.driver == DriverKind::Libsql {
            run(conn, "DROP TABLE IF EXISTS conf_keys").await.unwrap();
        }
        affected(&b, conn, &b.create_temp("conf_keys", ddl)).await;
        let schema = match conn
            .execute("SELECT * FROM conf_keys", &ExecOptions::default())
            .await
            .unwrap()
        {
            ExecResult::Rows(stream) => stream.schema(),
            ExecResult::Affected { .. } => panic!("{}: expected rows", b.name),
        };
        let columns: Vec<Column<'_>> = schema
            .fields()
            .iter()
            .map(|f| Column {
                name: f.name(),
                kind: ValueKind::of(f.data_type()),
                default: None,
            })
            .collect();
        let values: Vec<Vec<(usize, Value<'_>)>> = rows
            .iter()
            .map(|r| {
                r.iter()
                    .enumerate()
                    .map(|(i, v)| (i, Value::Text(v)))
                    .collect()
            })
            .collect();
        dml::apply(
            conn,
            d,
            &dml::inserts(d, "conf_keys", &columns, &values).unwrap(),
        )
        .await
        .unwrap_or_else(|e| panic!("{}: insert: {e}", b.name));
        // The decimal as the grid shows it once saved.
        let keys: Vec<Vec<(usize, Option<&str>)>> = [rows[0], [rows[1][0], "1000.00", rows[1][2]]]
            .iter()
            .map(|r| r.iter().enumerate().map(|(i, v)| (i, Some(*v))).collect())
            .collect();
        let deletes = dml::deletes(d, "conf_keys", &columns, &keys).unwrap();
        assert_eq!(deletes.len(), 1, "{}", b.name);
        dml::apply(conn, d, &deletes)
            .await
            .unwrap_or_else(|e| panic!("{}: {}: {e}", b.name, deletes[0].sql));
        let left = collect(&b, conn, "SELECT count(*) FROM conf_keys").await;
        assert_eq!(cell(&left[0], 0, 0).as_deref(), Some("1"), "{}", b.name);
        if b.config.driver == DriverKind::Libsql {
            run(conn, "DROP TABLE conf_keys").await.unwrap();
        }
    }
}

/// Without savepoints (DuckDB), a save whose first statement finds no row stops there and
/// leaves the session's uncommitted work as it was.
#[tokio::test]
async fn edits_keep_uncommitted_work_without_savepoints() {
    use platypus_sql::Dialect;
    use platypus_sql::dml::{self, ApplyError, Column, Statement, ValueKind};

    let _serial = SERIAL.lock().await;
    let b = duckdb_backend().await;
    let conn = b.connect().await;
    let conn = &*conn;
    let d = Dialect::DuckDb;
    let columns = [Column {
        name: "id",
        kind: ValueKind::Integer,
        default: None,
    }];
    affected(&b, conn, "CREATE TABLE conf_dk (id INTEGER PRIMARY KEY)").await;
    affected(&b, conn, "INSERT INTO conf_dk VALUES (1), (2)").await;
    conn.set_autocommit(false).await.unwrap();
    affected(&b, conn, "INSERT INTO conf_dk VALUES (3)").await;
    let delete =
        |id| Statement::one(dml::delete(d, "conf_dk", &columns, &[(0, Some(id))]).unwrap());
    let err = dml::apply(conn, d, &[delete("99"), delete("1")])
        .await
        .unwrap_err();
    assert!(
        matches!(
            err,
            ApplyError::RowCount {
                index: 0,
                rows: 0,
                ..
            }
        ),
        "{err}"
    );
    assert!(conn.transaction().open, "the session's transaction goes on");
    let count = collect(&b, conn, "SELECT count(*) FROM conf_dk").await;
    assert_eq!(
        cell(&count[0], 0, 0).as_deref(),
        Some("3"),
        "row 3 kept, row 1 not deleted"
    );
    // Once a statement changed rows, only a rollback of everything undoes them, and the
    // error says so.
    let err = dml::apply(conn, d, &[delete("1"), delete("99")])
        .await
        .unwrap_err();
    assert!(
        matches!(&err, ApplyError::SessionRolledBack(cause)
            if matches!(**cause, ApplyError::RowCount { index: 1, rows: 0, .. })),
        "{err}"
    );
    assert!(err.to_string().contains("rolled back"), "{err}");
    assert!(!conn.transaction().open);
    conn.set_autocommit(true).await.unwrap();
    let count = collect(&b, conn, "SELECT count(*) FROM conf_dk").await;
    assert_eq!(
        cell(&count[0], 0, 0).as_deref(),
        Some("2"),
        "rows 1 and 2, without row 3"
    );
}

/// The table of `qa_fixgrid_generated_columns` on `b`, with its numbered and computed columns
/// as the catalog should report them; `None` where the test does not apply.
fn generated_fixture(b: &Backend) -> Option<(&'static str, Vec<(&'static str, Generated)>)> {
    use Generated::{Computed, Identity, Numbered};
    Some(match b.name {
        "postgres" => (
            "id INT GENERATED ALWAYS AS IDENTITY PRIMARY KEY, n INT GENERATED BY DEFAULT AS \
             IDENTITY, s SERIAL, a INT, total INT GENERATED ALWAYS AS (a * 2) STORED",
            vec![
                ("id", Identity),
                ("n", Numbered),
                ("s", Numbered),
                ("total", Computed),
            ],
        ),
        "mysql" | "mariadb" => (
            "id INT AUTO_INCREMENT PRIMARY KEY, a INT, total INT AS (a * 2) VIRTUAL, \
             st INT AS (a + 1) STORED, \
             at TIMESTAMP NULL DEFAULT CURRENT_TIMESTAMP ON UPDATE CURRENT_TIMESTAMP",
            vec![("id", Numbered), ("total", Computed), ("st", Computed)],
        ),
        "sqlserver" => (
            "id INT IDENTITY(1, 1) PRIMARY KEY, a INT, total AS (a * 2), rv ROWVERSION",
            vec![("id", Identity), ("total", Computed), ("rv", Computed)],
        ),
        "sqlite" | "libsql" => (
            "id INTEGER PRIMARY KEY, a INT, total INT GENERATED ALWAYS AS (a * 2) VIRTUAL",
            vec![("id", Numbered), ("total", Computed)],
        ),
        _ => return None,
    })
}

/// The data editor leaves numbered and computed columns to the database: the catalog tells
/// them apart, new rows (a duplicated row) get a fresh number, and the undo script of a
/// deleted row puts its number back, with the override the database asks for, and leaves
/// computed columns out.
#[tokio::test]
async fn qa_fixgrid_generated_columns() {
    use platypus_sql::Dialect;
    use platypus_sql::dml::{self, Column, Value, ValueKind};
    use platypus_sql::generated::generated_columns;

    let _serial = SERIAL.lock().await;
    for b in backends().await {
        let Some((columns_sql, expected)) = generated_fixture(&b) else {
            continue;
        };
        let conn = b.connect().await;
        let conn = &*conn;
        let d = Dialect::from(b.config.driver);
        let t = d.qualified(Some(&b.schema), "qa_fixgrid_gen");
        let _ = run(conn, &format!("DROP TABLE IF EXISTS {t}")).await;
        run(conn, &format!("CREATE TABLE {t} ({columns_sql})"))
            .await
            .unwrap_or_else(|e| panic!("{}: create: {e}", b.name));

        let mut found: Vec<_> = generated_columns(conn, d, &b.schema, "qa_fixgrid_gen")
            .await
            .unwrap_or_else(|e| panic!("{}: catalog: {e}", b.name))
            .into_iter()
            .map(|c| (c.name, c.generated))
            .collect();
        found.sort_by(|x, y| x.0.cmp(&y.0));
        let mut want: Vec<_> = expected.iter().map(|&(n, g)| (n.to_owned(), g)).collect();
        want.sort_by(|x, y| x.0.cmp(&y.0));
        assert_eq!(found, want, "{}: generated columns", b.name);

        let Ok(ExecResult::Rows(stream)) = execute(conn, &format!("SELECT * FROM {t}")).await
        else {
            panic!("{}: expected rows", b.name);
        };
        let fields = stream.schema();
        drop(stream);
        let columns: Vec<Column<'_>> = fields
            .fields()
            .iter()
            .map(|f| Column {
                name: f.name(),
                kind: ValueKind::of(f.data_type()),
                default: None,
            })
            .collect();
        let index = |name: &str| columns.iter().position(|c| c.name == name).unwrap();
        let a = index("a");
        // New rows set only `a`, as a duplicated row does once numbered and computed columns
        // are left out.
        let inserts = dml::inserts(
            d,
            &t,
            &columns,
            &[vec![(a, Value::Text("5"))], vec![(a, Value::Text("6"))]],
        )
        .unwrap();
        dml::apply(conn, d, &inserts)
            .await
            .unwrap_or_else(|e| panic!("{}: insert: {e}", b.name));

        // Delete the first row and put it back with the undo script's statements.
        let row = collect(&b, conn, &format!("SELECT * FROM {t} WHERE a = 5")).await;
        let texts: Vec<Option<String>> = (0..columns.len()).map(|c| cell(&row[0], c, 0)).collect();
        let id = texts[index("id")].clone().unwrap();
        affected(&b, conn, &format!("DELETE FROM {t} WHERE a = 5")).await;
        let kind = |c: &Column<'_>| expected.iter().find(|(n, _)| *n == c.name).map(|e| e.1);
        let restored: Vec<usize> = (0..columns.len())
            .filter(|&c| kind(&columns[c]) != Some(Generated::Computed))
            .collect();
        let identity: Vec<usize> = restored
            .iter()
            .copied()
            .filter(|&c| kind(&columns[c]) == Some(Generated::Identity))
            .collect();
        let values: Vec<_> = restored
            .iter()
            .map(|&c| (c, texts[c].as_deref().map_or(Value::Null, Value::Text)))
            .collect();
        let undo = dml::restore_inserts(d, &t, &columns, &[values], &identity).unwrap();
        for sql in &undo {
            run(conn, sql)
                .await
                .unwrap_or_else(|e| panic!("{}: undo {sql}: {e}", b.name));
        }
        let back = collect(&b, conn, &format!("SELECT id, total FROM {t} WHERE a = 5")).await;
        assert_eq!(cell(&back[0], 0, 0), Some(id), "{}: the same id", b.name);
        assert_eq!(
            cell(&back[0], 1, 0).as_deref(),
            Some("10"),
            "{}: computed",
            b.name
        );
        run(conn, &format!("DROP TABLE {t}")).await.unwrap();
        eprintln!("{}: generated columns checked", b.name);
    }
}

/// A partial unique index (`WHERE`) is reported as such: it tells no row apart, so the data
/// editor neither keys rows by it nor deletes them in batches by it.
#[tokio::test]
async fn reports_partial_indexes() {
    let _serial = SERIAL.lock().await;
    for b in backends().await {
        let (schema, ddl) = match (b.config.driver, b.name) {
            (DriverKind::Postgres, "postgres") | (DriverKind::SqlServer, _) => (
                "demo",
                [
                    "CREATE TABLE demo.conf_partial (id INT NOT NULL, code INT NOT NULL, live INT)",
                    "CREATE UNIQUE INDEX conf_partial_live ON demo.conf_partial (code) WHERE live = 1",
                    "CREATE UNIQUE INDEX conf_partial_id ON demo.conf_partial (id)",
                ],
            ),
            (DriverKind::Sqlite, _) => (
                "main",
                [
                    "CREATE TABLE conf_partial (id INT NOT NULL, code INT NOT NULL, live INT)",
                    "CREATE UNIQUE INDEX conf_partial_live ON conf_partial (code) WHERE live = 1",
                    "CREATE UNIQUE INDEX conf_partial_id ON conf_partial (id)",
                ],
            ),
            _ => continue,
        };
        let conn = b.connect().await;
        let conn = &*conn;
        let table = format!("{schema}.conf_partial");
        run(conn, &format!("DROP TABLE IF EXISTS {table}"))
            .await
            .unwrap_or_else(|e| panic!("{}: {e}", b.name));
        for sql in ddl {
            run(conn, sql)
                .await
                .unwrap_or_else(|e| panic!("{}: {sql}: {e}", b.name));
        }
        let details = conn.table_details(schema, "conf_partial").await.unwrap();
        let partial = |name: &str| {
            details
                .indexes
                .iter()
                .find(|i| i.name == name)
                .unwrap_or_else(|| panic!("{}: no {name} in {:?}", b.name, details.indexes))
                .partial
        };
        assert!(partial("conf_partial_live"), "{}", b.name);
        assert!(!partial("conf_partial_id"), "{}", b.name);
        run(conn, &format!("DROP TABLE {table}")).await.unwrap();
    }
}

#[tokio::test]
async fn estimates_table_rows() {
    use platypus_sql::Dialect;

    let _serial = SERIAL.lock().await;
    for b in backends().await {
        let conn = b.connect().await;
        let d = Dialect::from(b.config.driver);
        for schema in [Some(b.schema.as_str()), None] {
            // Unqualified names are the current schema's, which may not hold the seed.
            let sql = d.row_estimate_sql(schema, "customers");
            let rows = platypus_core::query_text(conn.as_ref(), &sql)
                .await
                .unwrap_or_else(|e| panic!("{}: {sql}: {e}", b.name));
            assert!(rows.len() <= 1, "{}: {rows:?}", b.name);
            if let Some(Some(n)) = rows.first().and_then(|r| r.first()) {
                assert!(n.parse::<i64>().is_ok(), "{}: {n}", b.name);
            }
            // No table of that name in the current schema is no estimate, not an error.
            d.estimate_rows(conn.as_ref(), schema, "customers")
                .await
                .unwrap_or_else(|e| panic!("{}: {e}", b.name));
        }
    }
}

/// Creates `demo.conf_arrays` and its composite type, and checks the columns' fields.
async fn arrays_fixture(conn: &dyn Connection) {
    for sql in [
        "DROP TABLE IF EXISTS demo.conf_arrays",
        "DROP TYPE IF EXISTS demo.conf_pair",
        // INT4: `INT` is 64-bit in CockroachDB.
        "CREATE TYPE demo.conf_pair AS (label TEXT, n INT4)",
        "CREATE TABLE demo.conf_arrays (id INT PRIMARY KEY, tags TEXT[], nums INT[], \
         pair demo.conf_pair, pairs demo.conf_pair[])",
        "INSERT INTO demo.conf_arrays (id) VALUES (1)",
    ] {
        run(conn, sql).await.unwrap();
    }

    let details = conn.table_details("demo", "conf_arrays").await.unwrap();
    let fields = |name: &str| -> Vec<(String, String)> {
        details
            .columns
            .iter()
            .find(|c| c.name == name)
            .unwrap()
            .fields
            .iter()
            .map(|f| (f.name.clone(), f.data_type.clone()))
            .collect()
    };
    let pair_fields = [
        ("label".to_owned(), "text".to_owned()),
        ("n".to_owned(), "integer".to_owned()),
    ];
    assert_eq!(fields("pair"), pair_fields);
    assert_eq!(fields("pairs"), pair_fields, "fields of the element type");
    assert_eq!(fields("tags"), []);
}

/// Arrays shown as PostgreSQL prints them (bounds other than 1 first, `;` between boxes),
/// and read back by the server as the same values: saving the text keeps them. CockroachDB
/// has neither boxes nor bounds.
async fn arrays_as_postgresql_prints_them(b: &Backend, conn: &dyn Connection) {
    let shown = [
        ("INT4[]", "[0:1]={1,2}"),
        ("INT4[]", "[1:1][-1:0]={{1,NULL}}"),
        ("BOX[]", "{(1,1),(0,0);(2,2),(1,1)}"),
        ("TEXT[]", r#"{a(b),"null",NULL,"",'q'}"#),
    ];
    for (ty, text) in shown {
        let quoted = text.replace('\'', "''");
        let sql = format!("SELECT '{quoted}'::{ty}, ('{quoted}'::{ty})::TEXT");
        let rows = collect(b, conn, &sql).await;
        assert_eq!(
            cell(&rows[0], 0, 0).as_deref(),
            Some(text),
            "{}: {sql}",
            b.name
        );
        assert_eq!(
            cell(&rows[0], 1, 0).as_deref(),
            Some(text),
            "{}: {sql}",
            b.name
        );
    }
}

/// PostgreSQL arrays and composite values: the fields of composite columns, and literals the
/// data editor joins that come back as the same elements.
#[tokio::test]
async fn edits_arrays_and_composites() {
    use platypus_sql::Dialect;
    use platypus_sql::dml::{self, Column, Value, ValueKind};
    use platypus_sql::pg_literal::{join_array, join_composite, split_array, split_composite};

    let _serial = SERIAL.lock().await;

    for b in backends().await {
        if b.config.driver != DriverKind::Postgres {
            continue;
        }
        let conn = b.connect().await;
        let conn = &*conn;
        arrays_fixture(conn).await;

        let text = |items: &[Option<&str>]| -> Vec<Option<String>> {
            items.iter().map(|s| s.map(str::to_owned)).collect()
        };
        let tags = text(&[
            Some("plain"),
            Some("with space"),
            Some(r#"q"x\y"#),
            Some("{b},"),
            Some("null"),
            Some(""),
            None,
        ]);
        let nums = text(&[Some("1"), None, Some("-3")]);
        let pair = text(&[Some(r#"a (b), "c" \"#), None]);
        let pairs = vec![Some(join_composite(&pair)), None];
        let values = [
            join_array(&tags, ','),
            join_array(&nums, ','),
            join_composite(&pair),
            join_array(&pairs, ','),
        ];
        let columns: Vec<Column<'_>> = ["id", "tags", "nums", "pair", "pairs"]
            .iter()
            .map(|name| Column {
                name,
                kind: if *name == "id" {
                    ValueKind::Integer
                } else {
                    ValueKind::Text
                },
                default: None,
            })
            .collect();
        let set: Vec<(usize, Value<'_>)> = values
            .iter()
            .enumerate()
            .map(|(i, v)| (i + 1, Value::Text(v)))
            .collect();
        let update = dml::update(
            Dialect::from(b.config.driver),
            "demo.conf_arrays",
            &columns,
            &set,
            &[(0, Some("1"))],
        )
        .unwrap();
        dml::apply(
            conn,
            Dialect::from(b.config.driver),
            &[dml::Statement::one(update)],
        )
        .await
        .unwrap_or_else(|e| panic!("{}: update: {e}", b.name));

        let rows = collect(
            &b,
            conn,
            "SELECT tags, nums, pair, pairs FROM demo.conf_arrays",
        )
        .await;
        let shown = |column| cell(&rows[0], column, 0).unwrap();
        assert_eq!(split_array(&shown(0), ','), Some(tags));
        assert_eq!(split_array(&shown(1), ','), Some(nums));
        assert_eq!(split_composite(&shown(2)), Some(pair.clone()));
        let read = split_array(&shown(3), ',').unwrap();
        assert_eq!(read[1], None);
        assert_eq!(split_composite(read[0].as_deref().unwrap()), Some(pair));
        if b.name != "cockroach" {
            arrays_as_postgresql_prints_them(&b, conn).await;
        }

        run(conn, "DROP TABLE demo.conf_arrays").await.unwrap();
        run(conn, "DROP TYPE demo.conf_pair").await.unwrap();
    }
}

#[tokio::test]
async fn searches_metadata() {
    let _serial = SERIAL.lock().await;

    for b in backends().await {
        let conn = b.connect().await;
        if !conn.capabilities().metadata_search {
            continue;
        }
        let hits = conn.search_metadata("CUSTOMER").await.unwrap();
        let hit = |name: &str, matched: MatchKind| {
            hits.iter()
                .any(|h| h.schema == b.schema && h.name == name && h.matched == matched)
        };
        assert!(hit("customers", MatchKind::Name), "{}: {hits:?}", b.name);
        assert!(
            hit("customer_totals", MatchKind::Name),
            "{}: {hits:?}",
            b.name
        );
        assert!(
            hits.iter().any(|h| h.name == "orders"
                && h.kind == ObjectKind::Table
                && h.column.as_deref() == Some("customer_id")),
            "{}: {hits:?}",
            b.name
        );
        // `count(` is in the view's source, not in its name or columns.
        let hits = conn.search_metadata("COUNT(").await.unwrap();
        assert!(
            hits.iter()
                .any(|h| h.name == "customer_totals" && h.matched == MatchKind::Source),
            "{}: {hits:?}",
            b.name
        );
        // Wildcards match themselves.
        assert_eq!(conn.search_metadata("c%s").await.unwrap(), [], "{}", b.name);
    }
}

#[tokio::test]
async fn searches_table_data() {
    use platypus_sql::Dialect;

    let _serial = SERIAL.lock().await;
    for b in backends().await {
        let conn = b.connect().await;
        let d = Dialect::from(b.config.driver);
        let table = format!("probe_search{}", std::process::id());
        let qualified = d.qualified(Some(&b.schema), &table);
        let _ = run(&*conn, &format!("DROP TABLE {qualified}")).await;
        run(
            &*conn,
            &format!(
                "CREATE TABLE {qualified} (id INTEGER, label {}){}",
                if b.config.driver == DriverKind::ClickHouse {
                    "Nullable(String)"
                } else {
                    "VARCHAR(20)"
                },
                b.table_options("id")
            ),
        )
        .await
        .unwrap();
        run(
            &*conn,
            &format!(
                "INSERT INTO {qualified} (id, label) VALUES (1, 'Alpha'), (2, 'beta'), (30, '100%'), (4, NULL)"
            ),
        )
        .await
        .unwrap();
        for (needle, expected) in [
            ("ALP", "1"),
            ("a", "2"),
            ("%", "1"),
            ("3", "1"),
            ("zz", "0"),
        ] {
            let sql = format!(
                "SELECT COUNT(*) FROM {qualified} WHERE {}",
                d.any_column_contains(&["id", "label"], needle)
            );
            let rows = collect(&b, &*conn, &sql).await;
            assert_eq!(
                cell(&rows[0], 0, 0).as_deref(),
                Some(expected),
                "{}: {sql}",
                b.name
            );
        }
        run(&*conn, &format!("DROP TABLE {qualified}"))
            .await
            .unwrap();
    }
}

/// Row estimates and sizes of tables, and PostgreSQL partitions under their table.
#[tokio::test]
async fn sizes_tables_and_nests_partitions() {
    let _serial = SERIAL.lock().await;

    for b in backends().await {
        let conn = b.connect().await;
        let objects = conn.objects(&b.schema).await.unwrap();
        let customers = objects.iter().find(|o| o.name == "customers").unwrap();
        // SQLite and libSQL keep no statistics, DuckDB only row estimates, CockroachDB no
        // page counts.
        assert_eq!(
            customers.size.is_some(),
            !b.config.driver.is_embedded() && !b.speaks_sqlite() && b.name != "cockroach",
            "{}: {customers:?}",
            b.name
        );
        if b.config.driver != DriverKind::Postgres || b.name == "cockroach" {
            continue;
        }
        let details = conn.table_details(&b.schema, "customers").await.unwrap();
        assert!(
            details.indexes.iter().all(|i| i.size.is_some()),
            "{}",
            b.name
        );

        let parent = format!("probe_part{}", std::process::id());
        let child = format!("{parent}_2026");
        let _ = run(
            &*conn,
            &format!("DROP TABLE IF EXISTS {}.{parent}", b.schema),
        )
        .await;
        let s = &b.schema;
        for sql in [
            format!("CREATE TABLE {s}.{parent} (at date NOT NULL) PARTITION BY RANGE (at)"),
            format!(
                "CREATE TABLE {s}.{child} PARTITION OF {s}.{parent}
                 FOR VALUES FROM ('2026-01-01') TO ('2027-01-01')"
            ),
        ] {
            run(&*conn, &sql).await.unwrap();
        }
        let objects = conn.objects(&b.schema).await.unwrap();
        let find = |name: &str| objects.iter().find(|o| o.name == name).unwrap();
        assert_eq!(
            find(&child).parent.as_deref(),
            Some(parent.as_str()),
            "{}",
            b.name
        );
        assert_eq!(find(&parent).parent, None, "{}", b.name);
        run(&*conn, &format!("DROP TABLE {}.{parent}", b.schema))
            .await
            .unwrap();
    }
}

/// The geometry in a cell: binary values as (E)WKB, others as text.
fn shape(batch: &RecordBatch, column: usize) -> Option<geometry::Shape> {
    let array = batch.column(column);
    match array.as_binary_opt::<i32>() {
        Some(a) => geometry::from_bytes(a.value(0)),
        None => geometry::from_text(&cell(batch, column, 0)?),
    }
}

#[tokio::test]
async fn reads_geometries() {
    use geometry::Shape;

    let _serial = SERIAL.lock().await;
    let point = Shape::Point { x: 1.0, y: 2.0 };
    let line = Shape::Line {
        points: vec![[0.0, 0.0], [2.0, 1.0]],
    };
    for b in backends().await {
        // CockroachDB has none of PostgreSQL's geometric types.
        if b.name == "cockroach" {
            continue;
        }
        let conn = b.connect().await;
        let (sql, expected) = match b.config.driver {
            DriverKind::Mongo | DriverKind::Redis => {
                unreachable!("MongoDB and Redis have their own tests in mongo.rs and redis.rs")
            }
            bridged!() => unreachable!("{BRIDGED}"),
            DriverKind::Postgres => (
                "SELECT point '(1,2)', box '(1,1),(0,0)', polygon '((0,0),(1,0),(1,1))', \
                 path '[(0,0),(2,1)]', lseg '[(0,0),(2,1)]', circle '<(1,2),3>'",
                vec![
                    point.clone(),
                    Shape::Polygon {
                        rings: vec![vec![
                            [1.0, 1.0],
                            [0.0, 1.0],
                            [0.0, 0.0],
                            [1.0, 0.0],
                            [1.0, 1.0],
                        ]],
                    },
                    Shape::Polygon {
                        rings: vec![vec![[0.0, 0.0], [1.0, 0.0], [1.0, 1.0]]],
                    },
                    line.clone(),
                    line.clone(),
                    Shape::Circle {
                        x: 1.0,
                        y: 2.0,
                        r: 3.0,
                    },
                ],
            ),
            DriverKind::MySql => (
                "SELECT ST_GeomFromText('POINT(1 2)'), ST_GeomFromText('LINESTRING(0 0, 2 1)')",
                vec![point.clone(), line.clone()],
            ),
            // ClickHouse geometries are tuples of coordinates: shapes come as text.
            DriverKind::Sqlite
            | DriverKind::Libsql
            | DriverKind::DuckDb
            | DriverKind::ClickHouse => ("SELECT 'POINT (1 2)'", vec![point.clone()]),
            // SQL Server's own serialization is not WKB: the values are converted.
            DriverKind::SqlServer => (
                "SELECT geometry::Point(1, 2, 0).STAsBinary(), \
                 geometry::STGeomFromText('LINESTRING(0 0, 2 1)', 0).STAsBinary()",
                vec![point.clone(), line.clone()],
            ),
        };
        let batches = collect(&b, &*conn, sql).await;
        let shapes: Vec<_> = (0..expected.len()).map(|c| shape(&batches[0], c)).collect();
        let expected: Vec<_> = expected.into_iter().map(Some).collect();
        assert_eq!(shapes, expected, "{}", b.name);
    }
}

#[tokio::test]
async fn sets_the_session_up_after_connecting() {
    let _serial = SERIAL.lock().await;

    for b in backends().await {
        let mut config = b.config.clone();
        let (check, expected) = match config.driver {
            DriverKind::Mongo | DriverKind::Redis => {
                unreachable!("MongoDB and Redis have their own tests in mongo.rs and redis.rs")
            }
            bridged!() => unreachable!("{BRIDGED}"),
            DriverKind::Postgres => (
                "SELECT current_schema(), current_setting('TimeZone')",
                vec![Some(b.schema.clone()), Some("Europe/Rome".to_owned())],
            ),
            DriverKind::DuckDb => ("SELECT current_schema()", vec![Some(b.schema.clone())]),
            DriverKind::MySql => (
                "SELECT DATABASE(), @@session.time_zone",
                vec![Some(b.schema.clone()), Some("+02:00".to_owned())],
            ),
            DriverKind::ClickHouse => (
                "SELECT currentDatabase(), timezone()",
                vec![Some(b.schema.clone()), Some("Europe/Rome".to_owned())],
            ),
            DriverKind::Sqlite | DriverKind::Libsql | DriverKind::SqlServer => {
                ("SELECT 1", vec![Some("1".to_owned())])
            }
        };
        // SQL Server names its temporary tables with a `#`.
        let probe_table = if config.driver == DriverKind::SqlServer {
            "#init_probe"
        } else {
            "init_probe"
        };
        config.init = SessionInit {
            schema: (!matches!(
                config.driver,
                DriverKind::Sqlite | DriverKind::Libsql | DriverKind::SqlServer
            ))
            .then(|| b.schema.clone()),
            time_zone: match config.driver {
                DriverKind::Mongo | DriverKind::Redis => {
                    unreachable!("MongoDB and Redis have their own tests in mongo.rs and redis.rs")
                }
                bridged!() => unreachable!("{BRIDGED}"),
                DriverKind::Postgres | DriverKind::ClickHouse => Some("Europe/Rome".to_owned()),
                DriverKind::MySql => Some("+02:00".to_owned()),
                DriverKind::Sqlite
                | DriverKind::Libsql
                | DriverKind::DuckDb
                | DriverKind::SqlServer => None,
            },
            statements: vec![if config.driver == DriverKind::SqlServer {
                "CREATE TABLE #init_probe (x int)".to_owned()
            } else if config.driver == DriverKind::Libsql {
                "CREATE TABLE IF NOT EXISTS init_probe (x int)".to_owned()
            } else {
                "CREATE TEMPORARY TABLE init_probe (x int)".to_owned()
            }],
        };
        let conn = driver(config.driver)
            .connect(&config)
            .await
            .unwrap_or_else(|e| panic!("{}: cannot connect: {e}", b.name));
        let batches = collect(&b, &*conn, check).await;
        let got: Vec<_> = (0..expected.len())
            .map(|c| cell(&batches[0], c, 0))
            .collect();
        assert_eq!(got, expected, "{}", b.name);
        // The bootstrap statement ran on this session.
        let probe = collect(&b, &*conn, &format!("SELECT count(*) FROM {probe_table}")).await;
        assert_eq!(cell(&probe[0], 0, 0).as_deref(), Some("0"), "{}", b.name);

        // A failing statement fails the connection and is named.
        config.init.statements = vec!["SELECT * FROM no_such_table_for_init".to_owned()];
        let err = match driver(config.driver).connect(&config).await {
            Ok(_) => panic!("{}: a failing bootstrap statement connected", b.name),
            Err(e) => e.to_string(),
        };
        assert!(err.contains("no_such_table_for_init"), "{}: {err}", b.name);
    }
}

/// The rows of `table` an agent's read-only query sees.
async fn agent_count(b: &Backend, conn: &dyn Connection, table: &str) -> String {
    agent_value(b, conn, &format!("SELECT count(*) FROM {table}")).await
}

/// The first value an agent's read-only query returns.
async fn agent_value(b: &Backend, conn: &dyn Connection, sql: &str) -> String {
    let read_only = Some(b.config.driver);
    match platypus_mcp::query::run(conn, sql, read_only, &Limits::default()).await {
        Ok(Outcome::Rows(t)) => t.rows[0][0].text().to_owned(),
        Ok(other) => panic!("{}: {sql} returned {other:?}", b.name),
        Err(e) => panic!("{}: {sql}: {e:?}", b.name),
    }
}

#[tokio::test]
async fn agents_read_in_read_only_transactions() {
    let _serial = SERIAL.lock().await;

    for b in backends().await {
        let conn = b.connect().await;
        let conn = &*conn;
        // A real table: READ ONLY transactions still write temporary ones.
        let table = "agent_read_only_probe";
        run(conn, &format!("DROP TABLE IF EXISTS {table}"))
            .await
            .unwrap();
        run(conn, &format!("CREATE TABLE {table} (id INT)"))
            .await
            .unwrap_or_else(|e| panic!("{}: {e}", b.name));

        assert_eq!(agent_count(&b, conn, table).await, "0", "{}", b.name);
        if b.config.driver == DriverKind::SqlServer {
            // Waits for locks briefly while read-only, then as before.
            assert_eq!(agent_value(&b, conn, "SELECT @@LOCK_TIMEOUT").await, "5000");
            let after = collect(&b, conn, "SELECT @@LOCK_TIMEOUT").await;
            assert_eq!(cell(&after[0], 0, 0).as_deref(), Some("-1"));
        }
        // A write that got past the statement check is refused, or rolled back. ClickHouse
        // has no transaction to do either: only the statement check stops it.
        let insert = format!("INSERT INTO {table} VALUES (1)");
        if conn.capabilities().transactions {
            let write =
                platypus_mcp::query::run(conn, &insert, Some(b.config.driver), &Limits::default())
                    .await;
            if platypus_mcp::query::refuses_writes_when_read_only(b.config.driver) {
                assert!(
                    write.is_err(),
                    "{}: the read-only transaction wrote",
                    b.name
                );
            }
            assert!(!conn.transaction().open, "{}: left a transaction", b.name);
            assert_eq!(agent_count(&b, conn, table).await, "0", "{}", b.name);
        }

        // Afterwards the session writes again (SQLite turns query_only off).
        assert_eq!(affected(&b, conn, &insert).await, 1, "{}", b.name);
        assert_eq!(agent_count(&b, conn, table).await, "1", "{}", b.name);
        run(conn, &format!("DROP TABLE {table}")).await.unwrap();
    }
}

/// An agent's `explain` with analyze runs in the read-only transaction where it refuses
/// writes: the plan comes back analyzed, a write inside it fails or changes nothing, and
/// no transaction is left open.
#[tokio::test]
async fn agents_explain_analyze_in_read_only_transactions() {
    let _serial = SERIAL.lock().await;

    for b in backends().await {
        let conn = b.connect().await;
        let conn = &*conn;
        if !conn.capabilities().explain
            || !platypus_mcp::query::refuses_writes_when_read_only(b.config.driver)
            || mysql_before(conn, MYSQL_ANALYZE)
        {
            continue;
        }
        let table = "agent_explain_probe";
        run(conn, &format!("DROP TABLE IF EXISTS {table}"))
            .await
            .unwrap();
        run(conn, &format!("CREATE TABLE {table} (id INT)"))
            .await
            .unwrap_or_else(|e| panic!("{}: {e}", b.name));
        let guard = Some(b.config.driver);
        let timeout = Limits::DEFAULT_TIMEOUT;
        let select = format!("SELECT * FROM {table}");
        let plan = platypus_mcp::query::guarded(
            conn,
            guard,
            timeout,
            std::future::pending(),
            conn.explain(&select, true),
        )
        .await
        .unwrap_or_else(|e| panic!("{}: {e:?}", b.name));
        assert!(!plan.root.operation.is_empty(), "{}", b.name);
        assert!(!conn.transaction().open, "{}: left a transaction", b.name);

        let insert = format!("INSERT INTO {table} VALUES (1)");
        let _ = platypus_mcp::query::guarded(
            conn,
            guard,
            timeout,
            std::future::pending(),
            conn.explain(&insert, true),
        )
        .await;
        assert!(!conn.transaction().open, "{}: left a transaction", b.name);
        assert_eq!(agent_count(&b, conn, table).await, "0", "{}", b.name);
        run(conn, &format!("DROP TABLE {table}")).await.unwrap();
    }
}

/// The open app as an agent sees it: it answers every change with `verdict`, and counts.
#[derive(Debug)]
struct AnsweringApp {
    verdict: platypus_mcp::Verdict,
    asked: std::sync::atomic::AtomicUsize,
}

#[async_trait::async_trait]
impl platypus_mcp::AppLink for AnsweringApp {
    async fn sessions(&self) -> Result<Vec<platypus_mcp::OpenSession>, String> {
        Ok(Vec::new())
    }
    async fn confirm(
        &self,
        _: platypus_mcp::WriteRequest,
    ) -> Result<platypus_mcp::Verdict, String> {
        self.asked
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        Ok(self.verdict.clone())
    }
    async fn hello(&self, _: &str, _: &str) -> Result<(), String> {
        Ok(())
    }
}

/// A function that deletes every row of `table`, in `driver`'s SQL.
fn purge_function(driver: &str, table: &str) -> String {
    if driver == "postgres" {
        format!(
            "CREATE FUNCTION agent_purge() RETURNS int LANGUAGE plpgsql AS $$ BEGIN DELETE FROM {table}; RETURN 1; END $$"
        )
    } else {
        format!(
            "CREATE FUNCTION agent_purge() RETURNS INT MODIFIES SQL DATA BEGIN DELETE FROM {table}; RETURN 1; END"
        )
    }
}

/// Saves `b` in the profile at `dir` as the connection `Db`, read-write for agents.
fn save_agent_connection(dir: &std::path::Path, b: &Backend, driver: &str) {
    let password = b.config.password.as_ref().map_or("", |p| p.expose());
    std::fs::write(
        dir.join("connections.toml"),
        format!(
            "version = 4\n\n[[connections]]\nid = \"00000000-0000-4000-8000-000000000001\"\n\
             name = \"Db\"\ndriver = \"{driver}\"\nhost = \"{}\"\nport = {}\ndatabase = \"{}\"\n\
             user = \"{}\"\npassword_command = \"echo {password}\"\nssl_mode = \"disable\"\n\
             agent_access = \"read_write\"\n",
            b.config.host,
            b.config.port.unwrap_or_default(),
            b.config.database,
            b.config.user,
        ),
    )
    .unwrap();
}

/// On a read-write connection a query that writes through a function is asked about like
/// any change: it runs first in the read-only transaction, which refuses the write.
#[tokio::test]
async fn agents_ask_before_a_query_that_writes() {
    use platypus_mcp::{Agent, Profile, QueryArgs, Verdict};

    let _serial = SERIAL.lock().await;
    for b in backends().await {
        let driver = match b.config.driver {
            DriverKind::Postgres if b.name != "cockroach" => "postgres",
            DriverKind::MySql => "mysql",
            _ => continue,
        };
        let conn = b.connect().await;
        let conn = &*conn;
        let table = "agent_purge_probe";
        // MySQL names a function without its arguments.
        let drop = if driver == "postgres" {
            "DROP FUNCTION IF EXISTS agent_purge()"
        } else {
            "DROP FUNCTION IF EXISTS agent_purge"
        };
        run(conn, drop)
            .await
            .unwrap_or_else(|e| panic!("{}: {e}", b.name));
        run(conn, &format!("DROP TABLE IF EXISTS {table}"))
            .await
            .unwrap();
        run(conn, &format!("CREATE TABLE {table} (id INT)"))
            .await
            .unwrap();
        run(conn, &format!("INSERT INTO {table} VALUES (1), (2)"))
            .await
            .unwrap();
        let function = purge_function(driver, table);
        // MySQL with binary logging wants SUPER or log_bin_trust_function_creators.
        if let Err(e) = run(conn, &function).await {
            eprintln!("skipping {}: cannot create the function: {e}", b.name);
            continue;
        }

        let dir = tempfile::tempdir().unwrap();
        save_agent_connection(dir.path(), &b, driver);
        let query = |sql: &str| QueryArgs {
            connection: "Db".to_owned(),
            sql: sql.to_owned(),
            max_rows: None,
            max_bytes: None,
            timeout_seconds: None,
            format: None,
        };
        let agent = |verdict| {
            let app = std::sync::Arc::new(AnsweringApp {
                verdict,
                asked: std::sync::atomic::AtomicUsize::default(),
            });
            (
                Agent::new(Profile::at(dir.path())).with_link(app.clone()),
                app,
            )
        };
        let left = || async {
            let rows = collect(&b, conn, &format!("SELECT count(*) FROM {table}")).await;
            cell(&rows[0], 0, 0)
        };

        // Refused: nothing deleted; a plain query is not asked about.
        let (refusing, app) = agent(Verdict::Deny { reason: None });
        refusing
            .run_query(&query(&format!("SELECT count(*) FROM {table}")), "test")
            .await
            .unwrap_or_else(|e| panic!("{}: {e}", b.name));
        assert_eq!(
            app.asked.load(std::sync::atomic::Ordering::Relaxed),
            0,
            "{}",
            b.name
        );
        let refused = refusing
            .run_query(&query("SELECT agent_purge()"), "test")
            .await
            .unwrap_err();
        assert!(
            refused.to_string().contains("refused"),
            "{}: {refused}",
            b.name
        );
        assert_eq!(
            app.asked.load(std::sync::atomic::Ordering::Relaxed),
            1,
            "{}",
            b.name
        );
        assert_eq!(left().await.as_deref(), Some("2"), "{}", b.name);

        // Allowed: it runs again, writing.
        let (allowing, _) = agent(Verdict::Once);
        allowing
            .run_query(&query("SELECT agent_purge()"), "test")
            .await
            .unwrap_or_else(|e| panic!("{}: {e}", b.name));
        assert_eq!(left().await.as_deref(), Some("0"), "{}", b.name);

        run(conn, drop)
            .await
            .unwrap_or_else(|e| panic!("{}: {e}", b.name));
        run(conn, &format!("DROP TABLE {table}")).await.unwrap();
    }
}

/// PostgreSQL decodes `U&"…"` names the way the agents' read-only check does, so a function
/// spelled with Unicode escapes is the one the check refuses.
#[tokio::test]
async fn agents_read_only_check_decodes_postgres_unicode_names() {
    use platypus_mcp::access::{AgentAccess, UnfilteredRule, check};
    use platypus_sql::Dialect;

    let _serial = SERIAL.lock().await;
    for b in backends()
        .await
        .into_iter()
        // CockroachDB has no `U&` names.
        .filter(|b| b.config.driver == DriverKind::Postgres && b.name != "cockroach")
    {
        let conn = b.connect().await;
        for sql in [
            r#"SELECT U&"\0070g_backend_pid"() = pg_backend_pid()"#,
            r#"SELECT U&"\+000070g_backend_pid"() = pg_backend_pid()"#,
            r#"SELECT U&"!0070g_backend_pid" UESCAPE '!' () = pg_backend_pid()"#,
        ] {
            let same = collect(&b, &*conn, sql).await;
            assert_eq!(cell(&same[0], 0, 0).as_deref(), Some("true"), "{sql}");
        }
        for sql in [
            r#"SELECT U&"\0070g_advisory_lock"(4243)"#,
            r#"SELECT U&"\0070g_read_file"('PG_VERSION')"#,
            r#"SELECT U&"!0070g_ls_dir" UESCAPE '!' ('.')"#,
            "SELECT pg_stat_reset()",
            "SELECT pg_stat_reset_shared('bgwriter')",
        ] {
            let checked = check(
                sql,
                Dialect::Postgres,
                AgentAccess::ReadOnly,
                UnfilteredRule::Refuse,
                "pg",
            );
            assert!(checked.is_err(), "{}: {sql} passed the check", b.name);
        }
    }
}

/// A CSV file imported into a table created from its inferred types, then a failed import
/// that leaves that table as it was.
#[tokio::test]
async fn imports_data_files() {
    use platypus_sql::Dialect;
    use platypus_transfer::import::{ImportColumn, ImportMode, ImportPlan, import};

    platypus_driver_duckdb::testing::load();
    let _serial = SERIAL.lock().await;

    let dir = tempfile::tempdir().unwrap();
    let csv = dir.path().join("conf_import.csv");
    std::fs::write(
        &csv,
        "id,name,price,active,day,at\n\
         1,it's,9.5,true,2024-03-01,2024-03-01 10:00:00\n\
         2,,0.25,false,2024-12-31,2024-12-31 23:59:59\n\
         3,ink,100,true,,\n",
    )
    .unwrap();
    let mut file = ConnectionConfig::new(DriverKind::DuckDb);
    file.database = csv.to_string_lossy().into_owned();
    let source = DuckDbDriver.connect(&file).await.unwrap();
    let ExecResult::Rows(stream) = source
        .execute("SELECT * FROM conf_import LIMIT 0", &ExecOptions::default())
        .await
        .unwrap()
    else {
        panic!("no schema");
    };
    let fields = stream.schema().fields().clone();
    drop(stream);

    // Manual commit, replace and upsert need transactions and unique keys, which ClickHouse
    // lacks; the transfers cover its imports.
    let all = backends().await;
    for b in all
        .iter()
        .filter(|b| b.config.driver != DriverKind::ClickHouse)
    {
        let conn = b.connect().await;
        let d = Dialect::from(b.config.driver);
        let table = d.qualified(Some(&b.schema), "conf_import");
        let _ = run(&*conn, &format!("DROP TABLE {table}")).await;
        // A manual-commit session, like a production connection's: the import commits anyway.
        conn.set_autocommit(false).await.unwrap();
        let mut plan = ImportPlan {
            source_table: "conf_import".to_owned(),
            source_schema: None,
            schema: Some(b.schema.clone()),
            table: "conf_import".to_owned(),
            create: true,
            mode: ImportMode::Append,
            keys: Vec::new(),
            primary_key: Vec::new(),
            identity: None,
            constraints: Vec::new(),
            columns: fields
                .iter()
                .map(|f| ImportColumn {
                    source: f.name().clone(),
                    target: f.name().clone(),
                    data_type: Some(d.column_type(f.data_type())),
                    not_null: false,
                    default: None,
                })
                .collect(),
        };
        let mut seen = 0;
        let rows = import((&*source, Dialect::DuckDb), (&*conn, d), &plan, |n| {
            seen = n;
            true
        })
        .await
        .unwrap_or_else(|e| panic!("{}: {e}", b.name));
        assert_eq!((rows, seen), (3, 3), "{}", b.name);
        let check = format!(
            "SELECT COUNT(*), SUM(id), MAX(name), SUM(price) FROM {table} WHERE name IS NOT NULL OR id = 2"
        );
        let other = b.connect().await;
        let batches = collect(b, &*other, &check).await;
        let values: Vec<_> = (0..4).map(|c| cell(&batches[0], c, 0)).collect();
        assert_eq!(values[0].as_deref(), Some("3"), "{}", b.name);
        assert_eq!(values[1].as_deref(), Some("6"), "{}", b.name);
        assert_eq!(values[2].as_deref(), Some("it's"), "{}", b.name);
        let price: f64 = values[3].as_deref().unwrap().parse().unwrap();
        assert!((price - 109.75).abs() < 1e-9, "{}: {price}", b.name);

        check_import_modes(b, &*source, &*conn, &*other, &plan, &table).await;

        // The table exists: creating it again fails, and the rows stay.
        assert!(
            import((&*source, Dialect::DuckDb), (&*conn, d), &plan, |_| true)
                .await
                .is_err(),
            "{}",
            b.name
        );
        plan.create = false;
        plan.columns[1].target = "missing".to_owned();
        assert!(
            import((&*source, Dialect::DuckDb), (&*conn, d), &plan, |_| true)
                .await
                .is_err(),
            "{}",
            b.name
        );
        let batches = collect(b, &*conn, &format!("SELECT COUNT(*) FROM {table}")).await;
        assert_eq!(cell(&batches[0], 0, 0).as_deref(), Some("3"), "{}", b.name);
        run(&*conn, &format!("DROP TABLE {table}")).await.unwrap();
        conn.commit().await.unwrap();
    }
}

/// A table copied from every database into every other, in a table created with the types
/// mapped to the target's.
#[tokio::test]
async fn transfers_tables_between_databases() {
    use platypus_sql::Dialect;
    use platypus_transfer::import::{ImportColumn, ImportMode, ImportPlan, import, target_type};

    let _serial = SERIAL.lock().await;

    let all = backends().await;
    for b in &all {
        let conn = b.connect().await;
        let d = Dialect::from(b.config.driver);
        let src = d.qualified(Some(&b.schema), "conf_xfer_src");
        let _ = run(&*conn, &format!("DROP TABLE {src}")).await;
        // ClickHouse columns are not nullable unless declared so.
        let null = if b.config.driver == DriverKind::ClickHouse {
            " NULL"
        } else {
            ""
        };
        run(
            &*conn,
            &format!(
                "CREATE TABLE {src} (id INT{null}, name VARCHAR(20){null}, price DECIMAL(10, 2){null}, day DATE{null})"
            ),
        )
        .await
        .unwrap();
        run(
            &*conn,
            &format!(
                "INSERT INTO {src} VALUES (1, 'it''s', 1.50, '2024-03-01'), (2, NULL, 2.25, '2024-12-31'), (3, 'ink', NULL, NULL)"
            ),
        )
        .await
        .unwrap_or_else(|e| panic!("{}: {e}", b.name));
    }
    for from in &all {
        let source = from.connect().await;
        let source_dialect = Dialect::from(from.config.driver);
        let ExecResult::Rows(stream) = source
            .execute(
                &format!(
                    "SELECT * FROM {} WHERE 1 = 0",
                    source_dialect.qualified(Some(&from.schema), "conf_xfer_src")
                ),
                &ExecOptions::default(),
            )
            .await
            .unwrap()
        else {
            panic!("{}: no schema", from.name);
        };
        let fields = stream.schema().fields().clone();
        drop(stream);
        for to in &all {
            let pair = format!("{} -> {}", from.name, to.name);
            let target = to.connect().await;
            let d = Dialect::from(to.config.driver);
            let dst = d.qualified(Some(&to.schema), "conf_xfer_dst");
            let _ = run(&*target, &format!("DROP TABLE {dst}")).await;
            let plan = ImportPlan {
                source_table: "conf_xfer_src".to_owned(),
                source_schema: Some(from.schema.clone()),
                schema: Some(to.schema.clone()),
                table: "conf_xfer_dst".to_owned(),
                create: true,
                mode: ImportMode::Append,
                keys: Vec::new(),
                primary_key: Vec::new(),
                identity: None,
                constraints: Vec::new(),
                columns: fields
                    .iter()
                    .map(|f| ImportColumn {
                        source: f.name().clone(),
                        target: f.name().to_lowercase(),
                        data_type: Some(target_type(f, d)),
                        not_null: false,
                        default: None,
                    })
                    .collect(),
            };
            let rows = import((&*source, source_dialect), (&*target, d), &plan, |_| true)
                .await
                .unwrap_or_else(|e| panic!("{pair}: {e}"));
            assert_eq!(rows, 3, "{pair}");
            let check =
                format!("SELECT COUNT(*), SUM(id), MAX(name), SUM(price), COUNT(day) FROM {dst}");
            let batches = collect(to, &*target, &check).await;
            let values: Vec<_> = (0..5).map(|c| cell(&batches[0], c, 0)).collect();
            assert_eq!(values[0].as_deref(), Some("3"), "{pair}");
            assert_eq!(values[1].as_deref(), Some("6"), "{pair}");
            assert_eq!(values[2].as_deref(), Some("it's"), "{pair}");
            let price: f64 = values[3].as_deref().unwrap().parse().unwrap();
            assert!((price - 3.75).abs() < 1e-9, "{pair}: {price}");
            assert_eq!(values[4].as_deref(), Some("2"), "{pair}");
            run(&*target, &format!("DROP TABLE {dst}")).await.unwrap();
        }
    }
    for b in &all {
        let conn = b.connect().await;
        let src = Dialect::from(b.config.driver).qualified(Some(&b.schema), "conf_xfer_src");
        run(&*conn, &format!("DROP TABLE {src}")).await.unwrap();
    }
}

/// An import that replaces the rows, one that upserts them on a unique key, and one cancelled.
async fn check_import_modes(
    b: &Backend,
    source: &dyn Connection,
    conn: &dyn Connection,
    other: &dyn Connection,
    plan: &platypus_transfer::import::ImportPlan,
    table: &str,
) {
    use platypus_sql::Dialect;
    use platypus_transfer::import::{ImportError, ImportMode, ImportPlan, import};

    let d = Dialect::from(b.config.driver);
    // Replace: the old rows go, in the same transaction.
    let count = format!("SELECT COUNT(*) FROM {table}");
    let mut again = plan.clone();
    again.create = false;
    again.mode = ImportMode::Replace;
    import((source, Dialect::DuckDb), (conn, d), &again, |_| true)
        .await
        .unwrap_or_else(|e| panic!("{}: replace: {e}", b.name));
    let batches = collect(b, other, &count).await;
    assert_eq!(
        cell(&batches[0], 0, 0).as_deref(),
        Some("3"),
        "{}: replaced",
        b.name
    );

    // Upsert on a unique key: a changed row is updated, a deleted one inserted again.
    // SQLite qualifies the index, not its table.
    let on = if b.speaks_sqlite() {
        "conf_import"
    } else {
        table
    };
    let index = format!("CREATE UNIQUE INDEX conf_import_id ON {on} (id)");
    run(conn, &index).await.unwrap();
    run(
        conn,
        &format!("UPDATE {table} SET name = 'old' WHERE id = 3"),
    )
    .await
    .unwrap();
    run(conn, &format!("DELETE FROM {table} WHERE id = 1"))
        .await
        .unwrap();
    conn.commit().await.unwrap();
    again.mode = ImportMode::Upsert;
    again.keys = vec!["id".to_owned()];
    import((source, Dialect::DuckDb), (conn, d), &again, |_| true)
        .await
        .unwrap_or_else(|e| panic!("{}: upsert: {e}", b.name));
    let check = format!(
        "SELECT COUNT(*), MAX(CASE WHEN id = 3 THEN name END) FROM {table} WHERE id IN (1, 3)"
    );
    let batches = collect(b, other, &check).await;
    assert_eq!(
        cell(&batches[0], 0, 0).as_deref(),
        Some("2"),
        "{}: upserted",
        b.name
    );
    assert_eq!(
        cell(&batches[0], 1, 0).as_deref(),
        Some("ink"),
        "{}: updated",
        b.name
    );
    let batches = collect(b, other, &count).await;
    assert_eq!(cell(&batches[0], 0, 0).as_deref(), Some("3"), "{}", b.name);

    // A new key whose name another row has fails, as an insert would: it updates nothing
    // (MySQL's `ON DUPLICATE KEY` alone would update the other row).
    // MySQL indexes a prefix of TEXT, SQL Server no NVARCHAR(MAX).
    let name = if d == Dialect::MySql {
        "name(20)"
    } else {
        "name"
    };
    if d == Dialect::SqlServer {
        let sql = format!("ALTER TABLE {table} ALTER COLUMN name NVARCHAR(100)");
        run(conn, &sql).await.unwrap();
    }
    let index = format!("CREATE UNIQUE INDEX conf_import_name ON {on} ({name})");
    run(conn, &index).await.unwrap();
    run(conn, &format!("UPDATE {table} SET id = 30 WHERE id = 3"))
        .await
        .unwrap();
    conn.commit().await.unwrap();
    let clash = import((source, Dialect::DuckDb), (conn, d), &again, |_| true).await;
    assert!(clash.is_err(), "{}: upsert on another unique key", b.name);
    let check = format!("SELECT COUNT(*), MAX(id) FROM {table} WHERE name = 'ink'");
    let batches = collect(b, other, &check).await;
    let values = (cell(&batches[0], 0, 0), cell(&batches[0], 1, 0));
    let expected = (Some("1".to_owned()), Some("30".to_owned()));
    assert_eq!(values, expected, "{}: untouched", b.name);

    // A cancelled import into a new table leaves no table, MySQL's committed DDL included.
    let cancelled = ImportPlan {
        table: "conf_import_cancel".to_owned(),
        ..plan.clone()
    };
    let result = import((source, Dialect::DuckDb), (conn, d), &cancelled, |_| false).await;
    assert!(
        matches!(result, Err(ImportError::Cancelled)),
        "{}: {result:?}",
        b.name
    );
    let gone = d.qualified(Some(&b.schema), "conf_import_cancel");
    assert!(
        run(conn, &format!("SELECT * FROM {gone}")).await.is_err(),
        "{}",
        b.name
    );
    conn.rollback().await.unwrap();
}

/// Two tables with a foreign key between them and a view, migrated from every database to
/// every other: columns, NOT NULL, primary key, identity, rows, defaults, a unique index, a
/// CHECK, the foreign key, and the view between databases of one kind.
#[tokio::test]
async fn migrates_tables_between_databases() {
    use platypus_sql::Dialect;
    use platypus_transfer::migrate::migrate;

    let _serial = SERIAL.lock().await;

    // ClickHouse enforces neither primary nor foreign keys.
    let all: Vec<_> = backends()
        .await
        .into_iter()
        .filter(|b| b.config.driver != DriverKind::ClickHouse)
        .collect();
    let tables = ["conf_mig_parent".to_owned(), "conf_mig_child".to_owned()];
    for from in &all {
        let source = from.connect().await;
        let sd = Dialect::from(from.config.driver);
        let parent = sd.qualified(Some(&from.schema), "conf_mig_parent");
        let child = sd.qualified(Some(&from.schema), "conf_mig_child");
        let view = sd.qualified(Some(&from.schema), "conf_mig_view");
        // SQLite names the referenced table without its schema, and qualifies the index.
        let (referenced, index) = if from.speaks_sqlite() {
            ("conf_mig_parent", format!("{}.conf_mig_code", from.schema))
        } else {
            (parent.as_str(), "conf_mig_code".to_owned())
        };
        let sequence = format!("{}.conf_mig_seq", from.schema);
        let id = match from.config.driver {
            DriverKind::Postgres => {
                "id INT GENERATED BY DEFAULT AS IDENTITY PRIMARY KEY".to_owned()
            }
            DriverKind::MySql => "id INT NOT NULL AUTO_INCREMENT PRIMARY KEY".to_owned(),
            DriverKind::SqlServer => "id INT IDENTITY(1, 1) PRIMARY KEY".to_owned(),
            DriverKind::Sqlite | DriverKind::Libsql => "id INTEGER PRIMARY KEY".to_owned(),
            DriverKind::DuckDb => format!("id INTEGER DEFAULT nextval('{sequence}') PRIMARY KEY"),
            // ClickHouse is filtered out above; the others have their own tests.
            _ => unreachable!("{}", from.name),
        };
        for to in all.iter().filter(|to| to.name != from.name) {
            let pair = format!("{} -> {}", from.name, to.name);
            let target = to.connect().await;
            let td = Dialect::from(to.config.driver);
            let tview = td.qualified(Some(&to.schema), "conf_mig_view");
            for (conn, d, schema, view) in [
                (&*source, sd, &from.schema, &view),
                (&*target, td, &to.schema, &tview),
            ] {
                let _ = run(conn, &format!("DROP VIEW {view}")).await;
                drop_tables(conn, d, schema, &tables).await;
            }
            let _ = run(&*source, &format!("DROP SEQUENCE {sequence}")).await;
            let mut setup = Vec::new();
            if from.config.driver == DriverKind::DuckDb {
                setup.push(format!("CREATE SEQUENCE {sequence}"));
            }
            setup.extend([
                format!(
                    "CREATE TABLE {parent} ({id}, name VARCHAR(20) NOT NULL, \
                     code VARCHAR(20) DEFAULT 'new', qty INT DEFAULT 0, \
                     CONSTRAINT conf_mig_qty CHECK (qty >= 0))"
                ),
                format!(
                    "CREATE UNIQUE INDEX {index} ON {} (code)",
                    if from.speaks_sqlite() {
                        referenced
                    } else {
                        &parent
                    }
                ),
                format!(
                    "CREATE TABLE {child} (id INT NOT NULL PRIMARY KEY, parent_id INT, \
                     CONSTRAINT conf_mig_fk FOREIGN KEY (parent_id) REFERENCES {referenced} (id))"
                ),
                format!("INSERT INTO {parent} (name, code) VALUES ('a', 'x'), ('b', 'y')"),
                format!("INSERT INTO {child} VALUES (10, 1), (11, NULL)"),
            ]);
            // The view travels only between databases of one kind: MySQL and MariaDB. Elsewhere
            // it would race the metadata search of the other tests, which reads every view.
            if sd == Dialect::MySql {
                setup.push(format!(
                    "CREATE VIEW {view} AS SELECT id, name FROM {referenced}"
                ));
            }
            for sql in setup {
                run(&*source, &sql)
                    .await
                    .unwrap_or_else(|e| panic!("{}: {sql}: {e}", from.name));
            }
            let done = migrate(
                (&*source, sd, &from.schema),
                (&*target, td, &to.schema),
                &tables,
                true,
                |_| true,
            )
            .await
            .unwrap_or_else(|e| panic!("{pair}: {e}"));
            assert_eq!(done.rows, 4, "{pair}");
            // Only MySQL sources have the view.
            let same = sd == td && sd == Dialect::MySql;
            check_migrated(to, &*source, &*target, &pair, &done.skipped, same).await;
            let _ = run(&*target, &format!("DROP VIEW {tview}")).await;
            drop_tables(&*target, td, &to.schema, &tables).await;
            let copied = td.qualified(Some(&to.schema), "conf_mig_parent_id_seq");
            let _ = run(&*target, &format!("DROP SEQUENCE {copied}")).await;
        }
        let _ = run(&*source, &format!("DROP VIEW {view}")).await;
        drop_tables(&*source, sd, &from.schema, &tables).await;
        let _ = run(&*source, &format!("DROP SEQUENCE {sequence}")).await;
    }
}

/// What [`migrates_tables_between_databases`] expects of the copy in `to`; `same` says whether
/// both databases are of one kind, which copies the view.
async fn check_migrated(
    to: &Backend,
    source: &dyn Connection,
    target: &dyn Connection,
    pair: &str,
    skipped: &[String],
    same: bool,
) {
    use platypus_sql::Dialect;

    let td = Dialect::from(to.config.driver);
    let tparent = td.qualified(Some(&to.schema), "conf_mig_parent");
    let tchild = td.qualified(Some(&to.schema), "conf_mig_child");
    let files = to.speaks_sqlite() || to.config.driver == DriverKind::DuckDb;
    // Only a CHECK written for another database may stay behind, in SQLite and DuckDB, the
    // view between databases of different kinds (the schema's other objects are not ours),
    // and on MySQL before 8.0.13 the default of a TEXT column, which it cannot have.
    let old_mysql = mysql_before(target, ServerVersion::new(8, 0, 13));
    let unexpected: Vec<_> = skipped
        .iter()
        .filter(|w| w.contains("conf_mig"))
        .filter(|w| (!files || !w.contains("CHECK")) && (same || !w.contains("are not copied to")))
        .filter(|w| !(old_mysql && w.contains(": default:")))
        .collect();
    assert!(unexpected.is_empty(), "{pair}: {unexpected:?}");
    // Identity and defaults: a new row is numbered after the copied ones.
    run(
        target,
        &format!("INSERT INTO {tparent} (name) VALUES ('c')"),
    )
    .await
    .unwrap_or_else(|e| panic!("{pair}: identity: {e}"));
    let check = format!(
        "SELECT MAX(id), MAX(CASE WHEN name = 'c' THEN code END), MAX(CASE WHEN name = 'c' THEN qty END) FROM {tparent}"
    );
    let batches = collect(to, target, &check).await;
    let values: Vec<_> = (0..3).map(|c| cell(&batches[0], c, 0)).collect();
    let code_default = skipped
        .iter()
        .any(|w| w.contains("conf_mig_parent.code: default:"));
    assert_eq!(
        values,
        [
            Some("3".to_owned()),
            (!code_default).then(|| "new".to_owned()),
            Some("0".to_owned())
        ],
        "{pair}: identity and defaults"
    );
    for (what, sql) in [
        (
            "not null",
            format!("INSERT INTO {tparent} (name) VALUES (NULL)"),
        ),
        (
            "unique index",
            format!("INSERT INTO {tparent} (name, code) VALUES ('d', 'x')"),
        ),
    ] {
        assert!(run(target, &sql).await.is_err(), "{pair}: {what}");
    }
    // MySQL before 8.0.16 parses a CHECK and ignores it: on either side there is none.
    let checked = !mysql_before(source, MYSQL_CHECKS) && !mysql_before(target, MYSQL_CHECKS);
    if checked && !files {
        let sql = format!("INSERT INTO {tparent} (name, code, qty) VALUES ('e', 'z', -1)");
        assert!(run(target, &sql).await.is_err(), "{pair}: check");
    }
    // The foreign key is declared; SQLite checks it only with `PRAGMA foreign_keys`.
    let details = target
        .table_details(&to.schema, "conf_mig_child")
        .await
        .unwrap();
    assert_eq!(details.foreign_keys.len(), 1, "{pair}: foreign key");
    if !to.speaks_sqlite() {
        let orphan = run(target, &format!("INSERT INTO {tchild} VALUES (12, 99)")).await;
        assert!(orphan.is_err(), "{pair}: foreign key");
    }
    if same {
        let tview = td.qualified(Some(&to.schema), "conf_mig_view");
        let batches = collect(to, target, &format!("SELECT COUNT(*) FROM {tview}")).await;
        assert_eq!(
            cell(&batches[0], 0, 0).as_deref(),
            Some("3"),
            "{pair}: view"
        );
    }
}

/// Drops `tables` of `schema`, last first; missing ones are fine.
async fn drop_tables(
    conn: &dyn Connection,
    d: platypus_sql::Dialect,
    schema: &str,
    tables: &[String],
) {
    for t in tables.iter().rev() {
        let _ = run(
            conn,
            &format!("DROP TABLE {}", d.qualified(Some(schema), t)),
        )
        .await;
    }
}

/// Fake rows for a parent and a child table: keys after the largest, foreign keys that point
/// at existing rows, values every column type takes.
#[tokio::test]
async fn fills_tables_with_fake_rows() {
    use platypus_sql::Dialect;
    use platypus_transfer::fake::{FakeColumn, Generator, fill};

    let _serial = SERIAL.lock().await;

    let tables = ["conf_fake_parent".to_owned(), "conf_fake_child".to_owned()];
    // ClickHouse has neither unique nor foreign keys.
    let all = backends().await;
    for b in all
        .iter()
        .filter(|b| b.config.driver != DriverKind::ClickHouse)
    {
        let conn = b.connect().await;
        let d = Dialect::from(b.config.driver);
        drop_tables(&*conn, d, &b.schema, &tables).await;
        let parent = d.qualified(Some(&b.schema), "conf_fake_parent");
        let child = d.qualified(Some(&b.schema), "conf_fake_child");
        let referenced = if b.speaks_sqlite() {
            "conf_fake_parent"
        } else {
            parent.as_str()
        };
        for sql in [
            format!(
                "CREATE TABLE {parent} (id INT NOT NULL PRIMARY KEY, name VARCHAR(40) NOT NULL, \
                 city VARCHAR(80) NOT NULL UNIQUE, CONSTRAINT conf_fake_pair UNIQUE (id, city))"
            ),
            format!(
                "CREATE TABLE {child} (id INT NOT NULL PRIMARY KEY, parent_id INT NOT NULL, \
                 parent_city VARCHAR(80) NOT NULL, email VARCHAR(80), day DATE, price DECIMAL(10, 2), \
                 FOREIGN KEY (parent_id, parent_city) REFERENCES {referenced} (id, city))"
            ),
            format!("INSERT INTO {parent} VALUES (7, 'seed', 'Milano')"),
        ] {
            run(&*conn, &sql)
                .await
                .unwrap_or_else(|e| panic!("{}: {sql}: {e}", b.name));
        }
        let column = |name: &str, generator| FakeColumn {
            name: name.to_owned(),
            generator,
            unique: false,
            data_type: None,
        };
        // The two columns of the composite foreign key, from one parent row.
        let reference = |column: &str| Generator::Reference {
            schema: Some(b.schema.clone()),
            table: "conf_fake_parent".to_owned(),
            column: column.to_owned(),
            key: Some("pair".to_owned()),
        };
        let at = (Some(b.schema.as_str()), "conf_fake_parent");
        // 20 rows with a unique city: there are fewer cities.
        let parents = vec![
            column("id", Generator::NextKey),
            column("name", Generator::FullName),
            FakeColumn {
                unique: true,
                ..column("city", Generator::City)
            },
        ];
        let rows = fill((&*conn, d), at, parents, 20, |_| true)
            .await
            .unwrap_or_else(|e| panic!("{}: {e}", b.name));
        assert_eq!(rows, 20, "{}", b.name);
        let children = vec![
            column("id", Generator::NextKey),
            column("parent_id", reference("id")),
            column("parent_city", reference("city")),
            column("email", Generator::Email),
            column("day", Generator::Date),
            column(
                "price",
                Generator::Decimal {
                    min: 0.0,
                    max: 100.0,
                    scale: 2,
                },
            ),
        ];
        fill(
            (&*conn, d),
            (Some(b.schema.as_str()), "conf_fake_child"),
            children,
            1200,
            |_| true,
        )
        .await
        .unwrap_or_else(|e| panic!("{}: {e}", b.name));
        let check = format!(
            "SELECT COUNT(*), MIN(p.id), MAX(p.id), COUNT(DISTINCT c.email) FROM {child} c JOIN {parent} p ON p.id = c.parent_id AND p.city = c.parent_city"
        );
        let batches = collect(b, &*conn, &check).await;
        let values: Vec<_> = (0..4).map(|c| cell(&batches[0], c, 0)).collect();
        // Every child has its parent, the keys follow the seed row, the emails differ.
        let expected = ["1200", "7", "27", "1200"].map(|v| Some(v.to_owned()));
        assert_eq!(values, expected, "{}", b.name);
        drop_tables(&*conn, d, &b.schema, &tables).await;
    }
}

/// Runs the table editor's script from `old` to `new`, then reads the table back.
async fn redesign(
    b: &Backend,
    conn: &dyn Connection,
    old: Option<&platypus_schema::TableDesign>,
    new: &platypus_schema::TableDesign,
) -> platypus_schema::TableDesign {
    let d = platypus_sql::Dialect::from(b.config.driver);
    let script = platypus_schema::script(d, &b.schema, old, new)
        .unwrap_or_else(|e| panic!("{}: {e}", b.name));
    platypus_schema::apply(conn, &script)
        .await
        .unwrap_or_else(|e| {
            panic!(
                "{}: {}: {}\n{}",
                b.name,
                e.statement,
                e.error,
                script.text(d)
            )
        });
    platypus_schema::load(conn, d, &b.schema, &new.name)
        .await
        .unwrap_or_else(|e| panic!("{}: {e}", b.name))
}

fn design_column(name: &str, data_type: &str, nullable: bool) -> platypus_schema::ColumnDesign {
    platypus_schema::ColumnDesign {
        name: name.to_owned(),
        data_type: data_type.to_owned(),
        nullable,
        ..platypus_schema::ColumnDesign::default()
    }
}

fn column_names(t: &platypus_schema::TableDesign) -> Vec<String> {
    t.columns.iter().map(|c| c.name.clone()).collect()
}

/// `conf_design`: a key, a UNIQUE and a CHECK constraint, an index, and a comment where the
/// database keeps one.
fn design_parent(comments: bool) -> platypus_schema::TableDesign {
    use platypus_schema::{
        ColumnDesign, ConstraintDesign, ConstraintType, IndexDesign, TableDesign,
    };

    TableDesign {
        name: "conf_design".to_owned(),
        columns: vec![
            ColumnDesign {
                primary_key: true,
                ..design_column("id", "INT", false)
            },
            design_column("code", "VARCHAR(20)", false),
            ColumnDesign {
                default: Some("0".to_owned()),
                ..design_column("total", "DECIMAL(10,2)", true)
            },
            ColumnDesign {
                comment: comments.then(|| "free text".to_owned()),
                ..design_column("note", "VARCHAR(100)", true)
            },
        ],
        indexes: vec![IndexDesign {
            name: "conf_design_total".to_owned(),
            columns: vec!["total".to_owned()],
            ..IndexDesign::default()
        }],
        constraints: vec![
            ConstraintDesign {
                name: "conf_design_code".to_owned(),
                columns: vec!["code".to_owned()],
                ..ConstraintDesign::default()
            },
            ConstraintDesign {
                name: "conf_design_positive".to_owned(),
                kind: ConstraintType::Check,
                check: "total >= 0".to_owned(),
                ..ConstraintDesign::default()
            },
        ],
        ..TableDesign::default()
    }
}

/// A table created, altered in place and rebuilt by the table editor, with its rows kept.
#[tokio::test]
async fn edits_table_designs() {
    use platypus_schema::{ColumnDesign, ConstraintType, IndexDesign};
    use platypus_sql::Dialect;

    let _serial = SERIAL.lock().await;
    let tables = ["conf_design".to_owned(), "conf_design_child".to_owned()];
    // ClickHouse has no primary, unique or foreign keys to design.
    let all = backends().await;
    for b in all
        .iter()
        .filter(|b| b.config.driver != DriverKind::ClickHouse)
    {
        let conn = b.connect().await;
        let d = Dialect::from(b.config.driver);
        drop_tables(&*conn, d, &b.schema, &tables).await;
        let comments = matches!(d, Dialect::Postgres | Dialect::MySql | Dialect::DuckDb);
        let created = redesign(b, &*conn, None, &design_parent(comments)).await;
        assert_eq!(
            column_names(&created),
            ["id", "code", "total", "note"],
            "{}",
            b.name
        );
        assert!(created.columns[0].primary_key, "{}", b.name);
        assert_eq!(created.indexes.len(), 1, "{}: {created:?}", b.name);
        // The primary key and the CHECK, which MySQL before 8.0.16 ignores.
        let constraints = if mysql_before(&*conn, MYSQL_CHECKS) {
            1
        } else {
            2
        };
        assert_eq!(
            created.constraints.len(),
            constraints,
            "{}: {created:?}",
            b.name
        );
        if comments {
            assert_eq!(
                created.columns[3].comment.as_deref(),
                Some("free text"),
                "{}",
                b.name
            );
        }
        let table = d.qualified(Some(&b.schema), "conf_design");
        run(
            &*conn,
            &format!("INSERT INTO {table} (id, code, total, note) VALUES (1, 'a', 5, 'x')"),
        )
        .await
        .unwrap_or_else(|e| panic!("{}: {e}", b.name));

        // In place where the database can: a rename, a new type, NOT NULL, a new column
        // and another index.
        let mut altered = created.clone();
        altered.columns[3].name = "remark".to_owned();
        // CockroachDB rewrites no column with a CHECK to a new type.
        let retype = b.name != "cockroach";
        if retype {
            altered.columns[2].data_type = "DECIMAL(12,2)".to_owned();
        }
        altered.columns[2].nullable = false;
        altered.columns.push(ColumnDesign {
            default: Some("'new'".to_owned()),
            ..design_column("status", "VARCHAR(10)", true)
        });
        altered.indexes = vec![IndexDesign {
            name: "conf_design_status".to_owned(),
            columns: vec!["status".to_owned()],
            ..IndexDesign::default()
        }];
        let read = redesign(b, &*conn, Some(&created), &altered).await;
        assert_eq!(
            column_names(&read),
            ["id", "code", "total", "remark", "status"],
            "{}",
            b.name
        );
        assert!(!read.columns[2].nullable, "{}", b.name);
        assert!(
            !retype || read.columns[2].data_type.contains("12"),
            "{}: {read:?}",
            b.name
        );
        assert_eq!(read.indexes.len(), 1, "{}: {read:?}", b.name);
        assert_eq!(read.indexes[0].name, "conf_design_status", "{}", b.name);
        let rows = collect(b, &*conn, &format!("SELECT remark FROM {table}")).await;
        assert_eq!(cell(&rows[0], 0, 0).as_deref(), Some("x"), "{}", b.name);

        edit_child_design(b, &*conn).await;

        // What SQLite and DuckDB rebuild: a dropped column and a dropped CHECK.
        let mut reduced = read.clone();
        reduced.columns.retain(|c| c.name != "remark");
        reduced
            .constraints
            .retain(|c| c.kind != ConstraintType::Check);
        let read = redesign(b, &*conn, Some(&read), &reduced).await;
        assert_eq!(
            column_names(&read),
            ["id", "code", "total", "status"],
            "{}",
            b.name
        );
        assert_eq!(read.constraints.len(), 1, "{}: {read:?}", b.name);
        assert!(read.columns[0].primary_key, "{}", b.name);
        let rows = collect(b, &*conn, &format!("SELECT code FROM {table}")).await;
        assert_eq!(cell(&rows[0], 0, 0).as_deref(), Some("a"), "{}", b.name);
        drop_tables(&*conn, d, &b.schema, &tables).await;
    }
}

/// A child of `conf_design` with a cascading foreign key, then the key changed, its column
/// renamed and a CHECK added; dropped at the end.
async fn edit_child_design(b: &Backend, conn: &dyn Connection) {
    use platypus_schema::{
        ColumnDesign, ConstraintDesign, ConstraintType, ForeignKeyDesign, TableDesign,
    };
    use platypus_sql::Dialect;

    let d = Dialect::from(b.config.driver);
    let actions = d != Dialect::DuckDb;
    let child = TableDesign {
        name: "conf_design_child".to_owned(),
        columns: vec![
            ColumnDesign {
                primary_key: true,
                ..design_column("id", "INT", false)
            },
            design_column("parent_id", "INT", true),
        ],
        foreign_keys: vec![ForeignKeyDesign {
            name: "conf_design_parent".to_owned(),
            columns: vec!["parent_id".to_owned()],
            ref_schema: b.schema.clone(),
            ref_table: "conf_design".to_owned(),
            ref_columns: vec!["id".to_owned()],
            on_delete: actions.then(|| "CASCADE".to_owned()),
            ..ForeignKeyDesign::default()
        }],
        ..TableDesign::default()
    };
    let created = redesign(b, conn, None, &child).await;
    assert_eq!(created.foreign_keys.len(), 1, "{}", b.name);
    if actions {
        assert_eq!(
            created.foreign_keys[0].on_delete.as_deref(),
            Some("CASCADE"),
            "{}",
            b.name
        );
    }
    let table = d.qualified(Some(&b.schema), "conf_design_child");
    run(
        conn,
        &format!("INSERT INTO {table} (id, parent_id) VALUES (10, 1)"),
    )
    .await
    .unwrap_or_else(|e| panic!("{}: {e}", b.name));
    let mut changed = created.clone();
    changed.foreign_keys[0].on_delete = None;
    "owner_id".clone_into(&mut changed.columns[1].name);
    changed.foreign_keys[0].columns = vec!["owner_id".to_owned()];
    changed.constraints.push(ConstraintDesign {
        name: "conf_design_child_id".to_owned(),
        kind: ConstraintType::Check,
        check: "id > 0".to_owned(),
        ..ConstraintDesign::default()
    });
    let read = redesign(b, conn, Some(&created), &changed).await;
    assert_eq!(column_names(&read), ["id", "owner_id"], "{}", b.name);
    assert_eq!(read.foreign_keys[0].on_delete, None, "{}", b.name);
    assert_eq!(read.foreign_keys[0].columns, ["owner_id"], "{}", b.name);
    let checks = usize::from(!mysql_before(conn, MYSQL_CHECKS));
    assert_eq!(read.constraints.len(), checks, "{}: {read:?}", b.name);
    let rows = collect(b, conn, &format!("SELECT owner_id FROM {table}")).await;
    assert_eq!(cell(&rows[0], 0, 0).as_deref(), Some("1"), "{}", b.name);
    run(conn, &format!("DROP TABLE {table}")).await.unwrap();
}

/// A comment written by the table editor's script reads back, on the table and on a column.
/// SQLite, libSQL and MongoDB have none: the editor keeps the field read-only there.
#[tokio::test]
async fn writes_comments() {
    use platypus_sql::Dialect;

    let _serial = SERIAL.lock().await;

    let tables = ["conf_comments".to_owned()];
    let all = backends().await;
    for b in all
        .iter()
        .filter(|b| !b.speaks_sqlite() && b.config.driver != DriverKind::Mongo)
    {
        let conn = b.connect().await;
        let d = Dialect::from(b.config.driver);
        drop_tables(&*conn, d, &b.schema, &tables).await;
        let table = d.qualified(Some(&b.schema), "conf_comments");
        let create = format!(
            "CREATE TABLE {table} (id INTEGER, note VARCHAR(20)){}",
            b.table_options("id")
        );
        run(&*conn, &create)
            .await
            .unwrap_or_else(|e| panic!("{}: {e}", b.name));
        let old = platypus_schema::load(&*conn, d, &b.schema, "conf_comments")
            .await
            .unwrap_or_else(|e| panic!("{}: {e}", b.name));
        let mut new = old.clone();
        new.comment = Some("All orders".to_owned());
        new.columns[1].comment = Some("free text".to_owned());
        let read = redesign(b, &*conn, Some(&old), &new).await;
        assert_eq!(read.comment.as_deref(), Some("All orders"), "{}", b.name);
        assert_eq!(
            read.columns[1].comment.as_deref(),
            Some("free text"),
            "{}",
            b.name
        );
        // Removing them clears them.
        let mut cleared = read.clone();
        cleared.comment = None;
        cleared.columns[1].comment = None;
        let read = redesign(b, &*conn, Some(&read), &cleared).await;
        assert_eq!(read.comment, None, "{}", b.name);
        assert_eq!(read.columns[1].comment, None, "{}", b.name);
        drop_tables(&*conn, d, &b.schema, &tables).await;
    }
}

/// A PostgreSQL table never analyzed has no statistics: its rows are counted instead of
/// "unknown" (the confirmation before changes to every row shows them).
#[tokio::test]
async fn qa_fixsafe_estimates_rows_of_tables_never_analyzed() {
    let _serial = SERIAL.lock().await;

    for b in backends()
        .await
        .into_iter()
        .filter(|b| b.name == "postgres")
    {
        let conn = b.connect().await;
        let table = format!("{}.qa_fixsafe_est", b.schema);
        run(&*conn, &format!("DROP TABLE IF EXISTS {table}"))
            .await
            .unwrap();
        run(&*conn, &format!("CREATE TABLE {table} (id int)"))
            .await
            .unwrap();
        run(&*conn, &format!("INSERT INTO {table} VALUES (1), (2), (3)"))
            .await
            .unwrap();
        let estimate = platypus_sql::Dialect::Postgres
            .estimate_rows(&*conn, Some(&b.schema), "qa_fixsafe_est")
            .await;
        run(&*conn, &format!("DROP TABLE {table}")).await.unwrap();
        assert_eq!(estimate.unwrap(), Some(3), "{}", b.name);
    }
}

/// Every SQLite keyword: SQLite has no catalog of them.
const SQLITE_KEYWORDS: &str = "ABORT ACTION ADD AFTER ALL ALTER ALWAYS ANALYZE AND AS ASC ATTACH \
    AUTOINCREMENT BEFORE BEGIN BETWEEN BY CASCADE CASE CAST CHECK COLLATE COLUMN COMMIT CONFLICT \
    CONSTRAINT CREATE CROSS CURRENT CURRENT_DATE CURRENT_TIME CURRENT_TIMESTAMP DATABASE DEFAULT \
    DEFERRABLE DEFERRED DELETE DESC DETACH DISTINCT DO DROP EACH ELSE END ESCAPE EXCEPT EXCLUDE \
    EXCLUSIVE EXISTS EXPLAIN FAIL FILTER FIRST FOLLOWING FOR FOREIGN FROM FULL GENERATED GLOB GROUP \
    GROUPS HAVING IF IGNORE IMMEDIATE IN INDEX INDEXED INITIALLY INNER INSERT INSTEAD INTERSECT INTO \
    IS ISNULL JOIN KEY LAST LEFT LIKE LIMIT MATCH MATERIALIZED NATURAL NO NOT NOTHING NOTNULL NULL \
    NULLS OF OFFSET ON OR ORDER OTHERS OUTER OVER PARTITION PLAN PRAGMA PRECEDING PRIMARY QUERY \
    RAISE RANGE RECURSIVE REFERENCES REGEXP REINDEX RELEASE RENAME REPLACE RESTRICT RETURNING \
    RIGHT ROLLBACK ROW ROWS SAVEPOINT SELECT SET TABLE TEMP TEMPORARY THEN TIES TO TRANSACTION \
    TRIGGER UNBOUNDED UNION UNIQUE UPDATE USING VACUUM VALUES VIEW VIRTUAL WHEN WHERE WINDOW WITH \
    WITHOUT";

/// The keywords of the backend's database, from its own catalog where it has one.
async fn keywords(b: &Backend, conn: &dyn Connection) -> Vec<String> {
    let sql = match b.name {
        "postgres" => "SELECT word FROM pg_get_keywords()",
        // MySQL lists its keywords from 8.0.
        "mysql" if mysql_before(conn, ServerVersion::new(8, 0, 0)) => return Vec::new(),
        "mysql" | "mariadb" => "SELECT WORD FROM information_schema.KEYWORDS",
        "duckdb" => "SELECT keyword_name FROM duckdb_keywords()",
        "sqlite" | "libsql" => {
            return SQLITE_KEYWORDS
                .split_whitespace()
                .map(str::to_owned)
                .collect();
        }
        _ => return Vec::new(),
    };
    let mut words = Vec::new();
    for batch in collect(b, conn, sql).await {
        for row in 0..batch.num_rows() {
            words.extend(cell(&batch, 0, row));
        }
    }
    words
}

/// A table named like each keyword of the database, written by `Dialect::quote_ident`, is
/// read back from its own rows: a name left bare is either a syntax error or, on
/// PostgreSQL, a value function such as `current_role` read in its place.
#[tokio::test]
async fn qa_fixnav_keyword_table_names_are_quoted_where_needed() {
    let _serial = SERIAL.lock().await;
    for b in backends().await {
        let conn = b.connect().await;
        let d = platypus_sql::Dialect::from(b.config.driver);
        let words = keywords(&b, &*conn).await;
        if words.is_empty() {
            continue;
        }
        let mut wrong = Vec::new();
        for word in &words {
            // PostgreSQL folds bare names to lower case; the others keep the case given.
            let name = word.to_lowercase();
            let table = d.quote_ident(&name);
            let created = run(&*conn, &b.create_temp(&table, "x INT")).await;
            if created.is_err() {
                wrong.push(format!("{table}: {}", created.unwrap_err()));
                continue;
            }
            let _ = run(&*conn, &format!("INSERT INTO {table} VALUES (42)")).await;
            let select = d.select(&platypus_sql::ObjectName::new(None, &name), &["x"], None);
            match conn.execute(&select, &ExecOptions::default()).await {
                Ok(ExecResult::Rows(stream)) => {
                    let batches: Vec<RecordBatch> = stream.try_collect().await.unwrap_or_default();
                    if batches.first().and_then(|b| cell(b, 0, 0)).as_deref() != Some("42") {
                        wrong.push(format!("{select}: other rows"));
                    }
                }
                other => wrong.push(format!("{select}: {other:?}")),
            }
            let _ = run(&*conn, &format!("DROP TABLE {table}")).await;
        }
        assert!(wrong.is_empty(), "{}: {wrong:#?}", b.name);
    }
}

/// PostgreSQL lists routines with their named arguments (`f(a integer)`); their details and
/// DDL are read by that name. A table dropped since it was listed has no details.
#[tokio::test]
async fn qa_fixnav_pg_routines_by_their_listed_name() {
    let _serial = SERIAL.lock().await;
    for b in backends().await {
        if b.name != "postgres" {
            continue;
        }
        let conn = b.connect().await;
        let s = "qa_fixnav_routines";
        run(&*conn, &format!("DROP SCHEMA IF EXISTS {s} CASCADE"))
            .await
            .unwrap();
        run(&*conn, &format!("CREATE SCHEMA {s}")).await.unwrap();
        run(
            &*conn,
            &format!(
                "CREATE FUNCTION {s}.add(a integer, b integer) RETURNS integer \
                 LANGUAGE sql AS 'SELECT a + b'"
            ),
        )
        .await
        .unwrap();
        run(
            &*conn,
            &format!("CREATE PROCEDURE {s}.log(msg text) LANGUAGE sql AS 'SELECT 1'"),
        )
        .await
        .unwrap();
        run(&*conn, &format!("CREATE TABLE {s}.\"t(1)\" (id int)"))
            .await
            .unwrap();
        let objects = conn.objects(s).await.unwrap();
        for o in objects.iter().filter(|o| o.kind != ObjectKind::Table) {
            let object = ObjectRef {
                schema: s,
                name: &o.name,
                kind: o.kind,
                parent: None,
            };
            let ddl = conn
                .object_ddl(&object)
                .await
                .unwrap_or_else(|e| panic!("{}: {e}", o.name));
            assert!(ddl.starts_with("CREATE OR REPLACE"), "{}: {ddl}", o.name);
            let props = conn.object_properties(&object).await.unwrap();
            assert!(!props.is_empty(), "{}", o.name);
        }
        let names: Vec<&str> = objects.iter().map(|o| o.name.as_str()).collect();
        assert!(names.contains(&"add(a integer, b integer)"), "{names:?}");

        // Drop and select a table whose name has a parenthesis.
        let d = platypus_sql::Dialect::Postgres;
        let t = platypus_sql::ObjectName::new(Some(s), "t(1)");
        run(&*conn, &d.select(&t, &[], None)).await.unwrap();
        run(&*conn, &d.drop(ObjectKind::Table, &t, false).unwrap())
            .await
            .unwrap();
        let missing = conn.table_details(s, "t(1)").await;
        assert!(
            matches!(&missing, Err(e) if e.to_string().contains("qa_fixnav_routines.\"t(1)\" not found")),
            "{missing:?}"
        );
        run(&*conn, &format!("DROP SCHEMA {s} CASCADE"))
            .await
            .unwrap();
    }
}

/// A SQLite or DuckDB table created again by the table editor keeps its generated columns,
/// collations, table options, partial indexes and triggers.
#[tokio::test]
async fn qa_fixnav_rebuild_keeps_what_the_design_does_not_show() {
    let _serial = SERIAL.lock().await;
    for b in backends().await {
        let sqlite = b.config.driver == DriverKind::Sqlite;
        if !sqlite && b.config.driver != DriverKind::DuckDb {
            continue;
        }
        let conn = b.connect().await;
        let d = platypus_sql::Dialect::from(b.config.driver);
        let _ = run(&*conn, "DROP TABLE qa_fixnav_g").await;
        let options = if sqlite { " WITHOUT ROWID" } else { "" };
        run(
            &*conn,
            &format!(
                "CREATE TABLE qa_fixnav_g (id INTEGER PRIMARY KEY, a INTEGER, \
                 b INTEGER GENERATED ALWAYS AS (a * 2) VIRTUAL, c VARCHAR COLLATE NOCASE){options}"
            ),
        )
        .await
        .unwrap();
        run(
            &*conn,
            "INSERT INTO qa_fixnav_g (id, a, c) VALUES (1, 5, 'x')",
        )
        .await
        .unwrap();
        if sqlite {
            run(
                &*conn,
                "CREATE INDEX qa_fixnav_gi ON qa_fixnav_g (a) WHERE a > 0",
            )
            .await
            .unwrap();
            run(
                &*conn,
                "CREATE TRIGGER qa_fixnav_gt AFTER UPDATE ON qa_fixnav_g \
                 BEGIN UPDATE qa_fixnav_g SET c = 'y' WHERE id = new.id AND c = 'x'; END",
            )
            .await
            .unwrap();
        }
        let old = platypus_schema::load(&*conn, d, &b.schema, "qa_fixnav_g")
            .await
            .unwrap();
        assert!(old.columns[2].generated.is_some(), "{}: {old:?}", b.name);
        assert_eq!(old.columns[2].default, None, "{}", b.name);
        let mut new = old.clone();
        new.columns[1].data_type = "BIGINT".to_owned();
        let read = redesign(&b, &*conn, Some(&old), &new).await;
        assert!(read.columns[2].generated.is_some(), "{}: {read:?}", b.name);
        assert_eq!(
            read.columns[3].collation.as_deref(),
            Some("NOCASE"),
            "{}",
            b.name
        );
        run(&*conn, "UPDATE qa_fixnav_g SET a = 7 WHERE id = 1")
            .await
            .unwrap();
        let batches = collect(&b, &*conn, "SELECT b, c FROM qa_fixnav_g").await;
        assert_eq!(cell(&batches[0], 0, 0).as_deref(), Some("14"), "{}", b.name);
        if sqlite {
            assert_eq!(read.options.as_deref(), Some("WITHOUT ROWID"));
            assert_eq!(read.triggers.len(), 1, "{read:?}");
            assert!(
                read.indexes[0]
                    .sql
                    .as_deref()
                    .is_some_and(|s| s.contains("WHERE a > 0"))
            );
            // The trigger ran again after the rebuild.
            assert_eq!(cell(&batches[0], 1, 0).as_deref(), Some("y"));
        }
        run(&*conn, "DROP TABLE qa_fixnav_g").await.unwrap();
    }
}

/// MySQL redefines a generated column with its clause, not with `EXTRA`'s words.
#[tokio::test]
async fn qa_fixnav_mysql_generated_column_is_kept() {
    let _serial = SERIAL.lock().await;
    for b in backends().await {
        if b.config.driver != DriverKind::MySql {
            continue;
        }
        let conn = b.connect().await;
        let d = platypus_sql::Dialect::MySql;
        let _ = run(&*conn, "DROP TABLE qa_fixnav_mg").await;
        run(
            &*conn,
            "CREATE TABLE qa_fixnav_mg (id int PRIMARY KEY, qty int, \
             dbl int GENERATED ALWAYS AS (qty * 2) VIRTUAL, \
             code varchar(10) COLLATE utf8mb4_bin)",
        )
        .await
        .unwrap();
        let old = platypus_schema::load(&*conn, d, &b.schema, "qa_fixnav_mg")
            .await
            .unwrap();
        assert_eq!(old.columns[2].extra, None, "{}: {old:?}", b.name);
        let mut new = old.clone();
        new.columns[2].comment = Some("doubled".to_owned());
        new.columns[3].comment = Some("code".to_owned());
        let read = redesign(&b, &*conn, Some(&old), &new).await;
        assert!(read.columns[2].generated.is_some(), "{}: {read:?}", b.name);
        assert_eq!(
            read.columns[3].collation.as_deref(),
            Some("utf8mb4_bin"),
            "{}",
            b.name
        );
        run(&*conn, "INSERT INTO qa_fixnav_mg (id, qty) VALUES (1, 4)")
            .await
            .unwrap();
        let batches = collect(&b, &*conn, "SELECT dbl FROM qa_fixnav_mg").await;
        assert_eq!(cell(&batches[0], 0, 0).as_deref(), Some("8"), "{}", b.name);
        run(&*conn, "DROP TABLE qa_fixnav_mg").await.unwrap();
    }
}

/// ClickHouse changes a column's type with `MODIFY COLUMN`; a failed script says what
/// stays applied.
#[tokio::test]
async fn qa_fixnav_clickhouse_alters_columns() {
    let _serial = SERIAL.lock().await;
    for b in backends().await {
        if b.config.driver != DriverKind::ClickHouse {
            continue;
        }
        let conn = b.connect().await;
        let d = platypus_sql::Dialect::ClickHouse;
        let _ = run(&*conn, "DROP TABLE IF EXISTS qa_fixnav_ch").await;
        run(
            &*conn,
            "CREATE TABLE qa_fixnav_ch (id Int32, qty Nullable(Int32) DEFAULT 1 COMMENT 'q') \
             ENGINE = MergeTree ORDER BY id",
        )
        .await
        .unwrap();
        let old = platypus_schema::load(&*conn, d, &b.schema, "qa_fixnav_ch")
            .await
            .unwrap();
        let mut new = old.clone();
        new.columns[1].data_type = "Nullable(Int64)".to_owned();
        new.columns[1].name = "amount".to_owned();
        let read = redesign(&b, &*conn, Some(&old), &new).await;
        assert_eq!(read.columns[1].name, "amount");
        assert_eq!(read.columns[1].data_type, "Nullable(Int64)");
        assert_eq!(read.columns[1].default.as_deref(), Some("1"));
        assert_eq!(read.columns[1].comment.as_deref(), Some("q"));

        // A rename, then a type ClickHouse rejects: the rename stays and is reported.
        let mut bad = read.clone();
        bad.columns[1].name = "qty".to_owned();
        bad.columns[1].data_type = "NoSuchType".to_owned();
        let script = platypus_schema::script(d, &b.schema, Some(&read), &bad).unwrap();
        assert!(
            script
                .warnings
                .iter()
                .any(|w| w.starts_with("ClickHouse runs each statement"))
        );
        let err = platypus_schema::apply(&*conn, &script).await.unwrap_err();
        assert_eq!(err.applied.len(), 1, "{err:?}");
        assert!(err.applied[0].contains("RENAME COLUMN"), "{err:?}");
        run(&*conn, "DROP TABLE qa_fixnav_ch").await.unwrap();
    }
}

/// SQL Server synonyms are listed, with their DDL; DuckDB prints ENUM types.
#[tokio::test]
async fn qa_fixnav_synonyms_and_enum_types() {
    let _serial = SERIAL.lock().await;
    for b in backends().await {
        let conn = b.connect().await;
        let (create, drop, name, kind, expected) = match b.config.driver {
            DriverKind::SqlServer => (
                format!(
                    "CREATE SYNONYM {}.qa_fixnav_syn FOR {}.customers",
                    b.schema, b.schema
                ),
                format!("DROP SYNONYM {}.qa_fixnav_syn", b.schema),
                "qa_fixnav_syn",
                ObjectKind::Synonym,
                format!(
                    "CREATE SYNONYM {}.qa_fixnav_syn FOR [{}].[customers];",
                    b.schema, b.schema
                ),
            ),
            DriverKind::DuckDb => (
                "CREATE TYPE \"qa fixnav mood\" AS ENUM ('sad', 'it''s ok')".to_owned(),
                "DROP TYPE \"qa fixnav mood\"".to_owned(),
                "qa fixnav mood",
                ObjectKind::Type,
                "CREATE TYPE \"qa fixnav mood\" AS ENUM ('sad', 'it''s ok');".to_owned(),
            ),
            _ => continue,
        };
        let _ = run(&*conn, &drop).await;
        run(&*conn, &create).await.unwrap();
        let objects = conn.objects(&b.schema).await.unwrap();
        assert!(
            objects.iter().any(|o| o.name == name && o.kind == kind),
            "{}: {objects:?}",
            b.name
        );
        let object = ObjectRef {
            schema: &b.schema,
            name,
            kind,
            parent: None,
        };
        let ddl = conn.object_ddl(&object).await;
        run(&*conn, &drop).await.unwrap();
        assert_eq!(ddl.unwrap(), expected, "{}", b.name);
    }
}

/// Every result of a statement, in turn: the batches of each result set, until a count.
async fn qa_fixsql_results(b: &Backend, conn: &dyn Connection, sql: &str) -> Vec<Vec<RecordBatch>> {
    let mut sets = Vec::new();
    let mut next = Some(
        conn.execute(sql, &ExecOptions::default())
            .await
            .unwrap_or_else(|e| panic!("{}: {sql}: {e}", b.name)),
    );
    while let Some(ExecResult::Rows(mut stream)) = next {
        let mut batches = Vec::new();
        while let Some(batch) = stream.next().await {
            batches.push(batch.unwrap_or_else(|e| panic!("{}: {sql}: {e}", b.name)));
        }
        sets.push(batches);
        next = match stream.take_next() {
            Some(follows) => follows
                .await
                .unwrap_or_else(|e| panic!("{}: {sql}: {e}", b.name)),
            None => None,
        };
    }
    sets
}

/// A Transact-SQL batch and a MySQL `CALL` return every result set, one after the other,
/// and leave the session free for the next statement.
#[tokio::test]
async fn qa_fixsql_multiple_result_sets() {
    let _serial = SERIAL.lock().await;

    for b in backends().await {
        let conn = b.connect().await;
        let conn = &*conn;
        let statement = match b.config.driver {
            DriverKind::SqlServer => "SELECT 1 AS a SELECT 2 AS b",
            DriverKind::MySql => {
                run(conn, "DROP PROCEDURE IF EXISTS qa_fixsql_two")
                    .await
                    .unwrap();
                run(
                    conn,
                    "CREATE PROCEDURE qa_fixsql_two() BEGIN SELECT 1 AS a; SELECT 2 AS b; END",
                )
                .await
                .unwrap_or_else(|e| panic!("{}: {e}", b.name));
                "CALL qa_fixsql_two()"
            }
            _ => continue,
        };
        let sets = qa_fixsql_results(&b, conn, statement).await;
        let names: Vec<_> = sets
            .iter()
            .map(|s| s[0].schema().field(0).name().clone())
            .collect();
        assert_eq!(names, ["a", "b"], "{}", b.name);
        assert_eq!(cell(&sets[1][0], 0, 0).as_deref(), Some("2"), "{}", b.name);
        // The next statement runs on the same session.
        let rows = collect(&b, conn, "SELECT 3").await;
        assert_eq!(cell(&rows[0], 0, 0).as_deref(), Some("3"), "{}", b.name);

        if b.config.driver == DriverKind::SqlServer {
            // The CATCH block's result is not lost.
            let sets = qa_fixsql_results(
                &b,
                conn,
                "BEGIN TRY SELECT 1/0 AS x END TRY BEGIN CATCH SELECT ERROR_MESSAGE() AS m END CATCH",
            )
            .await;
            let last = sets.last().unwrap();
            assert_eq!(last[0].schema().field(0).name(), "m", "{}", b.name);
            assert!(cell(&last[0], 0, 0).unwrap().contains("zero"), "{}", b.name);
            // The statements of a batch share its variables.
            let sets =
                qa_fixsql_results(&b, conn, "DECLARE @x int = 5;\nSELECT @x * 2 AS dbl;").await;
            assert_eq!(cell(&sets[0][0], 0, 0).as_deref(), Some("10"), "{}", b.name);
        } else {
            run(conn, "DROP PROCEDURE qa_fixsql_two").await.unwrap();
        }
    }
}

/// What the server says beside the results: PostgreSQL notices, Transact-SQL `PRINT`, MySQL
/// warnings.
#[tokio::test]
async fn qa_fixsql_server_messages() {
    let _serial = SERIAL.lock().await;

    for b in backends().await {
        let conn = b.connect().await;
        let conn = &*conn;
        let (sql, expected) = match (b.config.driver, b.name) {
            (DriverKind::Postgres, "postgres") => (
                "DO $$ BEGIN RAISE NOTICE 'qa_fixsql notice'; END $$",
                "qa_fixsql notice",
            ),
            (DriverKind::SqlServer, _) => ("PRINT 'qa_fixsql print'", "qa_fixsql print"),
            (DriverKind::MySql, _) => ("SELECT CAST('1x' AS SIGNED) AS n", "1x"),
            _ => continue,
        };
        conn.take_messages();
        qa_fixsql_results(&b, conn, sql).await;
        let messages = conn.take_messages();
        assert!(
            messages.iter().any(|m| m.contains(expected)),
            "{}: {messages:?}",
            b.name
        );
        assert!(conn.take_messages().is_empty(), "{}: taken once", b.name);
    }
}

/// In manual-commit mode a failed statement is undone alone: the changes before it stay,
/// and the commit keeps them.
#[tokio::test]
async fn qa_fixsql_manual_commit_survives_a_failed_statement() {
    let _serial = SERIAL.lock().await;

    for b in backends().await {
        if b.config.driver != DriverKind::Postgres {
            continue;
        }
        let conn = b.connect().await;
        let conn = &*conn;
        run(conn, &b.create_temp("qa_fixsql_tx", "id INT PRIMARY KEY"))
            .await
            .unwrap_or_else(|e| panic!("{}: {e}", b.name));
        conn.set_autocommit(false).await.unwrap();
        affected(&b, conn, "INSERT INTO qa_fixsql_tx VALUES (1)").await;
        let duplicate = execute(conn, "INSERT INTO qa_fixsql_tx VALUES (1)").await;
        assert!(duplicate.is_err(), "{}", b.name);
        assert!(conn.transaction().open, "{}", b.name);
        // The transaction goes on: it is not aborted.
        affected(&b, conn, "INSERT INTO qa_fixsql_tx VALUES (2)").await;
        conn.commit()
            .await
            .unwrap_or_else(|e| panic!("{}: {e}", b.name));
        conn.set_autocommit(true).await.unwrap();
        let rows = collect(&b, conn, "SELECT count(*) FROM qa_fixsql_tx").await;
        assert_eq!(cell(&rows[0], 0, 0).as_deref(), Some("2"), "{}", b.name);
    }
}

/// The schema comparison creates two tables joined by a foreign key, the linter reads them
/// back (the key has no index but in MySQL, which indexes every key), and a second
/// comparison drops them again.
#[tokio::test]
async fn compares_and_lints_schemas() {
    use platypus_schema::{ColumnDesign, ForeignKeyDesign, LintKind, TableDesign};
    use platypus_sql::Dialect;

    let _serial = SERIAL.lock().await;
    let tables = ["conf_cmp_parent".to_owned(), "conf_cmp_child".to_owned()];
    for b in backends().await.iter().filter(|b| b.keys()) {
        let conn = b.connect().await;
        let d = Dialect::from(b.config.driver);
        drop_tables(&*conn, d, &b.schema, &tables).await;
        let current = schema_designs(b, &*conn).await;
        let mut wanted = current.clone();
        wanted.push(TableDesign {
            name: "conf_cmp_child".to_owned(),
            columns: vec![
                ColumnDesign {
                    primary_key: true,
                    ..design_column("id", "INT", false)
                },
                design_column("parent_id", "INT", true),
            ],
            foreign_keys: vec![ForeignKeyDesign {
                name: "conf_cmp_child_parent".to_owned(),
                columns: vec!["parent_id".to_owned()],
                ref_schema: b.schema.clone(),
                ref_table: "conf_cmp_parent".to_owned(),
                ref_columns: vec!["id".to_owned()],
                ..ForeignKeyDesign::default()
            }],
            ..TableDesign::default()
        });
        wanted.push(TableDesign {
            name: "conf_cmp_parent".to_owned(),
            columns: vec![ColumnDesign {
                primary_key: true,
                ..design_column("id", "INT", false)
            }],
            ..TableDesign::default()
        });
        let script = platypus_schema::compare(d, &b.schema, &b.schema, &wanted, &current)
            .unwrap_or_else(|e| panic!("{}: {e}", b.name));
        assert_eq!(script.statements.len(), 2, "{}: {}", b.name, script.text(d));
        apply_script(b, &*conn, &script).await;

        let read = platypus_schema::read_tables(&*conn, &b.schema, Some(&tables))
            .await
            .unwrap_or_else(|e| panic!("{}: {e}", b.name));
        assert_eq!(read.len(), 2, "{}", b.name);
        let findings = platypus_schema::lint(d, &b.schema, &read);
        let unindexed = findings
            .iter()
            .any(|f| f.kind == LintKind::UnindexedForeignKey && f.table == "conf_cmp_child");
        assert_eq!(unindexed, d != Dialect::MySql, "{}: {findings:?}", b.name);

        let now = schema_designs(b, &*conn).await;
        let same = platypus_schema::compare(d, &b.schema, &b.schema, &now, &now)
            .unwrap_or_else(|e| panic!("{}: {e}", b.name));
        assert_eq!(same.statements, Vec::<String>::new(), "{}", b.name);
        let script = platypus_schema::compare(d, &b.schema, &b.schema, &current, &now)
            .unwrap_or_else(|e| panic!("{}: {e}", b.name));
        assert_eq!(script.statements.len(), 2, "{}: {}", b.name, script.text(d));
        assert!(
            script.statements[0].contains("conf_cmp_child"),
            "{}",
            b.name
        );
        apply_script(b, &*conn, &script).await;
        drop_tables(&*conn, d, &b.schema, &tables).await;
    }
}

async fn schema_designs(b: &Backend, conn: &dyn Connection) -> Vec<platypus_schema::TableDesign> {
    let d = platypus_sql::Dialect::from(b.config.driver);
    platypus_schema::load_designs(conn, d, &b.schema)
        .await
        .unwrap_or_else(|e| panic!("{}: {e}", b.name))
}

async fn apply_script(b: &Backend, conn: &dyn Connection, script: &platypus_schema::Script) {
    platypus_schema::apply(conn, script)
        .await
        .unwrap_or_else(|e| {
            panic!(
                "{}: {}: {}\n{}",
                b.name,
                e.statement,
                e.error,
                script.text(platypus_sql::Dialect::from(b.config.driver))
            )
        });
}

/// A libSQL table of 200 000 rows opens: its result is larger than a Hrana pipeline's
/// response may be (`RESPONSE_TOO_LARGE`), and a query inside a transaction sees its rows.
#[tokio::test]
async fn qa_fixdbs_libsql_large_results() {
    let Some(b) = libsql_backend().await else {
        return;
    };
    let conn = LibsqlDriver.connect(&b.config).await.unwrap();
    let table = "qa_fixdbs_big";
    run(&*conn, &format!("DROP TABLE IF EXISTS {table}"))
        .await
        .unwrap();
    run(
        &*conn,
        &format!(
            "CREATE TABLE {table} AS WITH RECURSIVE c(x) AS (SELECT 1 UNION ALL \
             SELECT x + 1 FROM c WHERE x < 200000) \
             SELECT x AS id, 'row number ' || x || ' with some padding text' AS label FROM c"
        ),
    )
    .await
    .unwrap();
    let batches = collect(&b, &*conn, &format!("SELECT * FROM {table}")).await;
    let rows: usize = batches.iter().map(RecordBatch::num_rows).sum();
    assert_eq!(rows, 200_000);
    // Queries and statements share the stream and its transaction.
    run(&*conn, "BEGIN").await.unwrap();
    run(&*conn, &format!("DELETE FROM {table} WHERE id > 10"))
        .await
        .unwrap();
    assert!(conn.transaction().open);
    let batches = collect(&b, &*conn, &format!("SELECT COUNT(*) FROM {table}")).await;
    assert_eq!(cell(&batches[0], 0, 0).as_deref(), Some("10"));
    assert!(conn.transaction().open);
    conn.rollback().await.unwrap();
    let batches = collect(&b, &*conn, &format!("SELECT COUNT(*) FROM {table}")).await;
    assert_eq!(cell(&batches[0], 0, 0).as_deref(), Some("200000"));
    run(&*conn, &format!("DROP TABLE {table}")).await.unwrap();
}

/// A session on the PostgreSQL-protocol server at `url`, or `None` when it is unreachable.
async fn qa_fixdbs_pg(url: &str) -> Option<Box<dyn Connection>> {
    let config = ConnectionConfig::from_url(url).ok()?;
    match PgDriver.connect(&config).await {
        Ok(conn) => Some(conn),
        Err(e) => {
            println!("skipping {url}: {e}");
            None
        }
    }
}

/// A table of `schema`, for the DDL and properties calls.
fn qa_fixdbs_table<'a>(schema: &'a str, name: &'a str) -> platypus_core::ObjectRef<'a> {
    platypus_core::ObjectRef {
        schema,
        name,
        kind: ObjectKind::Table,
        parent: None,
    }
}

/// CockroachDB's STRING(n), and its arrays copied to PostgreSQL.
#[tokio::test]
async fn qa_fixdbs_cockroach_strings_and_arrays() {
    let _serial = SERIAL.lock().await;
    if let Some(crdb) =
        qa_fixdbs_pg("postgres://root@localhost:56257/platypus?sslmode=disable").await
    {
        for sql in [
            "DROP SCHEMA IF EXISTS qa_fixdbs CASCADE",
            "CREATE SCHEMA qa_fixdbs",
            "CREATE TABLE qa_fixdbs.s (id INT8 PRIMARY KEY, name STRING(40), tags STRING[])",
            "INSERT INTO qa_fixdbs.s VALUES (1, 'a', ARRAY['x', 'y'])",
        ] {
            run(&*crdb, sql)
                .await
                .unwrap_or_else(|e| panic!("{sql}: {e}"));
        }
        let details = crdb.table_details("qa_fixdbs", "s").await.unwrap();
        assert_eq!(details.columns[1].data_type, "character varying(40)");
        let ddl = crdb
            .object_ddl(&qa_fixdbs_table("qa_fixdbs", "s"))
            .await
            .unwrap();
        assert!(ddl.contains("character varying(40)"), "{ddl}");
        run(&*crdb, "DROP TABLE qa_fixdbs.s").await.unwrap();
        for statement in platypus_sql::split(&ddl, platypus_sql::Dialect::Postgres) {
            run(&*crdb, statement.text(&ddl))
                .await
                .unwrap_or_else(|e| panic!("{ddl}: {e}"));
        }
        run(
            &*crdb,
            "INSERT INTO qa_fixdbs.s VALUES (1, 'a', ARRAY['x', 'y'])",
        )
        .await
        .unwrap();
        // The array's type for a copy into PostgreSQL.
        let ExecResult::Rows(stream) = execute(&*crdb, "SELECT tags FROM qa_fixdbs.s")
            .await
            .unwrap()
        else {
            panic!("no rows");
        };
        let field = stream.schema().field(0).clone();
        assert_eq!(
            platypus_transfer::import::target_type(&field, platypus_sql::Dialect::Postgres),
            "text[]"
        );
        drop(stream);
        run(&*crdb, "DROP SCHEMA qa_fixdbs CASCADE").await.unwrap();
    }
}

/// TimescaleDB's schemas, routines and hypertables.
#[tokio::test]
async fn qa_fixdbs_timescale_catalogs() {
    let _serial = SERIAL.lock().await;
    let admin = "postgres://platypus:platypus@localhost:55433/platypus?sslmode=disable";
    if let Some(admin) = qa_fixdbs_pg(admin).await {
        for sql in [
            "DROP DATABASE IF EXISTS qa_fixdbs_ts WITH (FORCE)",
            "CREATE DATABASE qa_fixdbs_ts",
        ] {
            run(&*admin, sql)
                .await
                .unwrap_or_else(|e| panic!("{sql}: {e}"));
        }
        let ts = qa_fixdbs_pg(
            "postgres://platypus:platypus@localhost:55433/qa_fixdbs_ts?sslmode=disable",
        )
        .await
        .unwrap();
        for sql in [
            "CREATE EXTENSION timescaledb",
            "CREATE TABLE metrics (ts timestamptz NOT NULL, device text NOT NULL, \
             temp double precision, PRIMARY KEY (device, ts))",
            "SELECT create_hypertable('metrics', 'ts', chunk_time_interval => interval '1 day')",
            "INSERT INTO metrics SELECT t, 'dev' || (i % 3), i FROM generate_series(\
             timestamptz '2025-01-01', timestamptz '2025-01-05', interval '1 hour') \
             WITH ORDINALITY AS g(t, i)",
            "ALTER TABLE metrics SET (timescaledb.compress, timescaledb.compress_segmentby = 'device')",
            "ANALYZE metrics",
        ] {
            run(&*ts, sql)
                .await
                .unwrap_or_else(|e| panic!("{sql}: {e}"));
        }
        let schemas = ts.schemas().await.unwrap();
        let system = |name: &str| schemas.iter().find(|s| s.name == name).map(|s| s.system);
        assert_eq!(system("_timescaledb_catalog"), Some(true));
        assert_eq!(system("timescaledb_information"), Some(true));
        assert_eq!(system("public"), Some(false));
        let objects = ts.objects("public").await.unwrap();
        assert!(
            !objects.iter().any(|o| o.name.starts_with("time_bucket(")),
            "the extension's functions are its own"
        );
        let properties = ts
            .object_properties(&qa_fixdbs_table("public", "metrics"))
            .await
            .unwrap();
        let size = properties
            .iter()
            .find(|p| p.name == "Table size")
            .map(|p| p.value.clone());
        assert!(size.is_some_and(|s| s != "0 bytes"), "{properties:?}");
        let ddl = ts
            .object_ddl(&qa_fixdbs_table("public", "metrics"))
            .await
            .unwrap();
        assert!(ddl.contains("SELECT create_hypertable("), "{ddl}");
        assert!(
            ddl.contains("timescaledb.compress_segmentby = 'device'"),
            "{ddl}"
        );
        drop(ts);
        run(&*admin, "DROP DATABASE qa_fixdbs_ts WITH (FORCE)")
            .await
            .unwrap();
    }
}

/// YugabyteDB's sharding in the DDL.
#[tokio::test]
async fn qa_fixdbs_yugabyte_sharding() {
    let _serial = SERIAL.lock().await;
    if let Some(yb) =
        qa_fixdbs_pg("postgres://yugabyte:yugabyte@localhost:55434/yugabyte?sslmode=disable").await
    {
        for sql in [
            "DROP TABLE IF EXISTS qa_fixdbs_r",
            "DROP TABLE IF EXISTS qa_fixdbs_h",
            "CREATE TABLE qa_fixdbs_r (id bigint, v text, PRIMARY KEY (id ASC)) SPLIT AT VALUES ((100))",
            "CREATE TABLE qa_fixdbs_h (id bigint PRIMARY KEY, v text) SPLIT INTO 3 TABLETS",
        ] {
            run(&*yb, sql)
                .await
                .unwrap_or_else(|e| panic!("{sql}: {e}"));
        }
        let ddl = yb
            .object_ddl(&qa_fixdbs_table("public", "qa_fixdbs_r"))
            .await
            .unwrap();
        assert!(ddl.contains("PRIMARY KEY (id ASC)"), "{ddl}");
        assert!(ddl.contains("SPLIT AT VALUES ((100))"), "{ddl}");
        let ddl = yb
            .object_ddl(&qa_fixdbs_table("public", "qa_fixdbs_h"))
            .await
            .unwrap();
        assert!(ddl.contains("PRIMARY KEY (id HASH)"), "{ddl}");
        assert!(ddl.contains("SPLIT INTO 3 TABLETS"), "{ddl}");
        run(&*yb, "DROP TABLE qa_fixdbs_r").await.unwrap();
        run(&*yb, "DROP TABLE qa_fixdbs_h").await.unwrap();
    }
}

/// The schemas a comparison round trip copies, `S.` standing for the schema: defaults, NOT
/// NULL, a unique column, a check, a composite key and a foreign key on it, a self-reference,
/// two tables referencing each other, odd names and a view.
fn qa3_compare_base(driver: DriverKind) -> &'static str {
    match driver {
        DriverKind::Postgres => {
            "CREATE TABLE S.customers (id serial PRIMARY KEY, email varchar(100) NOT NULL UNIQUE, name text DEFAULT 'anon', created timestamp DEFAULT now());
CREATE TABLE S.orders (id bigint PRIMARY KEY, customer_id int NOT NULL REFERENCES S.customers(id) ON DELETE CASCADE, total numeric(10,2) DEFAULT 0, status varchar(20) CHECK (status IN ('new','paid')));
CREATE INDEX orders_cust_status ON S.orders(customer_id, status);
CREATE TABLE S.employees (id int PRIMARY KEY, manager_id int REFERENCES S.employees(id));
CREATE TABLE S.parts (a int, b int, PRIMARY KEY (a, b));
CREATE TABLE S.part_refs (id int PRIMARY KEY, a int, b int, FOREIGN KEY (a, b) REFERENCES S.parts(a, b));
CREATE TABLE S.ca (id int PRIMARY KEY, cb_id int);
CREATE TABLE S.cb (id int PRIMARY KEY, ca_id int REFERENCES S.ca(id));
ALTER TABLE S.ca ADD CONSTRAINT ca_cb FOREIGN KEY (cb_id) REFERENCES S.cb(id);
CREATE TABLE S.\"Odd Table\" (\"Weird Col\" int PRIMARY KEY, \"select\" text, \"Customer Id\" int REFERENCES S.customers(id));
CREATE INDEX odd_customer ON S.\"Odd Table\" (\"Customer Id\");
CREATE VIEW S.v_orders AS SELECT id, total FROM S.orders"
        }
        DriverKind::MySql => {
            "CREATE TABLE S.customers (id int AUTO_INCREMENT PRIMARY KEY, email varchar(100) NOT NULL UNIQUE, name varchar(50) DEFAULT 'anon', created datetime DEFAULT CURRENT_TIMESTAMP);
CREATE TABLE S.orders (id bigint PRIMARY KEY, customer_id int NOT NULL, total decimal(10,2) DEFAULT 0, status varchar(20), CONSTRAINT orders_status CHECK (status IN ('new','paid')), CONSTRAINT orders_customer FOREIGN KEY (customer_id) REFERENCES S.customers(id) ON DELETE CASCADE);
CREATE INDEX orders_cust_status ON S.orders(customer_id, status);
CREATE TABLE S.employees (id int PRIMARY KEY, manager_id int, CONSTRAINT employees_manager FOREIGN KEY (manager_id) REFERENCES S.employees(id));
CREATE TABLE S.parts (a int, b int, PRIMARY KEY (a, b));
CREATE TABLE S.part_refs (id int PRIMARY KEY, a int, b int, CONSTRAINT part_refs_ab FOREIGN KEY (a, b) REFERENCES S.parts(a, b));
CREATE TABLE S.ca (id int PRIMARY KEY, cb_id int);
CREATE TABLE S.cb (id int PRIMARY KEY, ca_id int, CONSTRAINT cb_ca FOREIGN KEY (ca_id) REFERENCES S.ca(id));
ALTER TABLE S.ca ADD CONSTRAINT ca_cb FOREIGN KEY (cb_id) REFERENCES S.cb(id);
CREATE TABLE S.`Odd Table` (`Weird Col` int PRIMARY KEY, `select` text, `è` int);
CREATE VIEW S.v_orders AS SELECT id, total FROM S.orders"
        }
        _ => {
            "CREATE TABLE S.customers (id int IDENTITY(1,1) PRIMARY KEY, email varchar(100) NOT NULL UNIQUE, name nvarchar(50) DEFAULT 'anon', created datetime2 DEFAULT sysdatetime());
CREATE TABLE S.orders (id bigint PRIMARY KEY, customer_id int NOT NULL, total decimal(10,2) DEFAULT 0, status varchar(20), CONSTRAINT orders_status CHECK (status IN ('new','paid')), CONSTRAINT orders_customer FOREIGN KEY (customer_id) REFERENCES S.customers(id) ON DELETE CASCADE);
CREATE INDEX orders_cust_status ON S.orders(customer_id, status);
CREATE TABLE S.employees (id int PRIMARY KEY, manager_id int, CONSTRAINT employees_manager FOREIGN KEY (manager_id) REFERENCES S.employees(id));
CREATE TABLE S.parts (a int, b int, PRIMARY KEY (a, b));
CREATE TABLE S.part_refs (id int PRIMARY KEY, a int, b int, CONSTRAINT part_refs_ab FOREIGN KEY (a, b) REFERENCES S.parts(a, b));
CREATE TABLE S.ca (id int PRIMARY KEY, cb_id int);
CREATE TABLE S.cb (id int PRIMARY KEY, ca_id int, CONSTRAINT cb_ca FOREIGN KEY (ca_id) REFERENCES S.ca(id));
ALTER TABLE S.ca ADD CONSTRAINT ca_cb FOREIGN KEY (cb_id) REFERENCES S.cb(id);
CREATE TABLE S.[Odd Table] ([Weird Col] int PRIMARY KEY, [select] nvarchar(max), [è] int);
CREATE VIEW S.v_orders AS SELECT id, total FROM S.orders"
        }
    }
}

/// The same tables changed: types, NULL, defaults, an index, a check gone, a column more, a
/// table missing and two tables more (one referencing the other).
fn qa3_compare_variant(driver: DriverKind) -> &'static str {
    match driver {
        DriverKind::Postgres => {
            "CREATE TABLE S.customers (id serial PRIMARY KEY, email varchar(50), name text, legacy int);
CREATE TABLE S.orders (id bigint PRIMARY KEY, customer_id int REFERENCES S.customers(id), total numeric(8,2), status varchar(20));
CREATE INDEX orders_cust ON S.orders(customer_id);
CREATE TABLE S.gone_parent (id int PRIMARY KEY);
CREATE TABLE S.gone_child (id int PRIMARY KEY, p int REFERENCES S.gone_parent(id));
CREATE TABLE S.parts (a int, b int, c int, PRIMARY KEY (a, b))"
        }
        DriverKind::MySql => {
            "CREATE TABLE S.customers (id int AUTO_INCREMENT PRIMARY KEY, email varchar(50), name varchar(50), legacy int);
CREATE TABLE S.orders (id bigint PRIMARY KEY, customer_id int, total decimal(8,2), status varchar(20), CONSTRAINT orders_customer FOREIGN KEY (customer_id) REFERENCES S.customers(id));
CREATE INDEX orders_cust ON S.orders(customer_id);
CREATE TABLE S.gone_parent (id int PRIMARY KEY);
CREATE TABLE S.gone_child (id int PRIMARY KEY, p int, CONSTRAINT gone_child_p FOREIGN KEY (p) REFERENCES S.gone_parent(id));
CREATE TABLE S.parts (a int, b int, c int, PRIMARY KEY (a, b))"
        }
        _ => {
            "CREATE TABLE S.customers (id int IDENTITY(1,1) PRIMARY KEY, email varchar(50), name nvarchar(50), legacy int);
CREATE TABLE S.orders (id bigint PRIMARY KEY, customer_id int, total decimal(8,2), status varchar(20), CONSTRAINT orders_customer FOREIGN KEY (customer_id) REFERENCES S.customers(id));
CREATE INDEX orders_cust ON S.orders(customer_id);
CREATE TABLE S.gone_parent (id int PRIMARY KEY);
CREATE TABLE S.gone_child (id int PRIMARY KEY, p int, CONSTRAINT gone_child_p FOREIGN KEY (p) REFERENCES S.gone_parent(id));
CREATE TABLE S.parts (a int, b int, c int, PRIMARY KEY (a, b))"
        }
    }
}

/// A new table referencing a column the comparison adds to a table that is there: the
/// source and the target.
fn qa3_compare_new_ref(driver: DriverKind) -> [&'static str; 2] {
    match driver {
        DriverKind::MySql => [
            "CREATE TABLE S.accounts (id int PRIMARY KEY, code varchar(10) NOT NULL UNIQUE);
CREATE TABLE S.vouchers (id int PRIMARY KEY, account_code varchar(10), CONSTRAINT vouchers_account FOREIGN KEY (account_code) REFERENCES S.accounts(code))",
            "CREATE TABLE S.accounts (id int PRIMARY KEY)",
        ],
        _ => [
            "CREATE TABLE S.accounts (id int PRIMARY KEY, code varchar(10) NOT NULL UNIQUE);
CREATE TABLE S.vouchers (id int PRIMARY KEY, account_code varchar(10) REFERENCES S.accounts(code))",
            "CREATE TABLE S.accounts (id int PRIMARY KEY)",
        ],
    }
}

/// Empties the schema `name` (a database on MySQL), creating it if needed, and runs `sql`
/// in it.
async fn qa3_compare_seed(conn: &dyn Connection, driver: DriverKind, name: &str, sql: &str) {
    let reset = match driver {
        DriverKind::Postgres => vec![
            format!("DROP SCHEMA IF EXISTS {name} CASCADE"),
            format!("CREATE SCHEMA {name}"),
        ],
        DriverKind::MySql => vec![
            format!("DROP DATABASE IF EXISTS {name}"),
            format!("CREATE DATABASE {name}"),
        ],
        _ => vec![
            format!(
                "IF SCHEMA_ID('{name}') IS NOT NULL BEGIN
                 DECLARE @f nvarchar(max) = N'';
                 SELECT @f += N'ALTER TABLE {name}.' + QUOTENAME(OBJECT_NAME(parent_object_id)) + N' DROP CONSTRAINT ' + QUOTENAME(name) + N';' FROM sys.foreign_keys WHERE schema_id = SCHEMA_ID('{name}');
                 EXEC (@f);
                 DECLARE @v nvarchar(max) = N'';
                 SELECT @v += N'DROP VIEW {name}.' + QUOTENAME(name) + N';' FROM sys.views WHERE schema_id = SCHEMA_ID('{name}');
                 EXEC (@v);
                 DECLARE @t nvarchar(max) = N'';
                 SELECT @t += N'DROP TABLE {name}.' + QUOTENAME(name) + N';' FROM sys.tables WHERE schema_id = SCHEMA_ID('{name}');
                 EXEC (@t);
                 DROP SCHEMA {name};
                 END"
            ),
            format!("CREATE SCHEMA {name}"),
        ],
    };
    for s in reset {
        run(conn, &s)
            .await
            .unwrap_or_else(|e| panic!("{name}: {s}: {e}"));
    }
    for s in sql.split(";\n").filter(|s| !s.trim().is_empty()) {
        let s = s.replace("S.", &format!("{name}."));
        run(conn, &s)
            .await
            .unwrap_or_else(|e| panic!("{name}: {s}: {e}"));
    }
}

/// The script that makes `dst` like `src` on `conn`.
async fn qa3_compare(conn: &dyn Connection, driver: DriverKind) -> platypus_schema::Script {
    let d = platypus_sql::Dialect::from(driver);
    let src = platypus_schema::load_designs(conn, d, QA3_SRC)
        .await
        .unwrap();
    let dst = platypus_schema::load_designs(conn, d, QA3_DST)
        .await
        .unwrap();
    platypus_schema::compare(d, QA3_DST, QA3_SRC, &src, &dst).unwrap()
}

/// Runs `script` as the SQL editor does: its text, one statement after the other.
async fn qa3_run_text(conn: &dyn Connection, driver: DriverKind, script: &platypus_schema::Script) {
    let text = script.text(platypus_sql::Dialect::from(driver));
    for s in text.split(";\n").filter(|s| !s.trim().is_empty()) {
        run(conn, s)
            .await
            .unwrap_or_else(|e| panic!("{driver:?}: {s}: {e}\n{text}"));
    }
}

const QA3_SRC: &str = "qa3c_src";
const QA3_DST: &str = "qa3c_dst";

/// Schema comparison round trips: identical schemas need nothing; the script that makes a
/// changed copy like its source runs as the editor runs it (no `BEGIN;` for SQL Server),
/// names nothing of the source, and a second comparison finds nothing (generated constraint
/// names, serial columns, CHECKs the database prints its own way, keys to added columns).
#[tokio::test]
async fn qa3_schema_compare_round_trips() {
    let _serial = SERIAL.lock().await;
    for b in backends()
        .await
        .into_iter()
        .filter(|b| matches!(b.name, "postgres" | "mysql" | "mariadb" | "sqlserver"))
    {
        let driver = b.config.driver;
        let mut config = b.config.clone();
        if driver == DriverKind::MySql {
            // Creating databases takes more than the test user may do.
            "root".clone_into(&mut config.user);
            config.password = Some(platypus_core::Secret::new("platypus"));
        }
        let conn = self::driver(driver)
            .connect(&config)
            .await
            .unwrap_or_else(|e| panic!("{}: {e}", b.name));
        let conn = &*conn;
        let base = qa3_compare_base(driver);
        qa3_compare_seed(conn, driver, QA3_SRC, base).await;
        qa3_compare_seed(conn, driver, QA3_DST, base).await;
        let same = qa3_compare(conn, driver).await;
        assert_eq!(
            same.statements,
            Vec::<String>::new(),
            "{}: identical",
            b.name
        );

        qa3_compare_seed(conn, driver, QA3_DST, qa3_compare_variant(driver)).await;
        let script = qa3_compare(conn, driver).await;
        let d = platypus_sql::Dialect::from(driver);
        let text = script.text(d);
        assert!(!text.contains(QA3_SRC), "{}: {text}", b.name);
        assert!(!text.lines().any(|l| l == "BEGIN;") || driver != DriverKind::SqlServer);
        qa3_run_text(conn, driver, &script).await;
        let again = qa3_compare(conn, driver).await;
        assert_eq!(
            again.statements,
            Vec::<String>::new(),
            "{}: after\n{text}",
            b.name
        );

        let [src, dst] = qa3_compare_new_ref(driver);
        qa3_compare_seed(conn, driver, QA3_SRC, src).await;
        qa3_compare_seed(conn, driver, QA3_DST, dst).await;
        let script = qa3_compare(conn, driver).await;
        qa3_run_text(conn, driver, &script).await;
        let again = qa3_compare(conn, driver).await;
        assert_eq!(
            again.statements,
            Vec::<String>::new(),
            "{}: new ref",
            b.name
        );

        for name in [QA3_SRC, QA3_DST] {
            qa3_compare_seed(conn, driver, name, "").await;
        }
    }
}

/// The schema linter on PostgreSQL finds the indexes of quoted columns, and on MySQL a CHECK
/// is read as MySQL takes it back.
#[tokio::test]
async fn qa3_schema_lint_and_checks_read_back() {
    let _serial = SERIAL.lock().await;
    for b in backends()
        .await
        .into_iter()
        .filter(|b| matches!(b.name, "postgres" | "mysql"))
    {
        let conn = b.connect().await;
        let d = platypus_sql::Dialect::from(b.config.driver);
        let table = d.qualified(Some(&b.schema), "qa3_quoted");
        let parent = d.qualified(Some(&b.schema), "qa3_quoted_parent");
        let col = d.quote_ident("Customer Id");
        let _ = run(&*conn, &format!("DROP TABLE IF EXISTS {table}")).await;
        let _ = run(&*conn, &format!("DROP TABLE IF EXISTS {parent}")).await;
        run(
            &*conn,
            &format!("CREATE TABLE {parent} (id int PRIMARY KEY)"),
        )
        .await
        .unwrap();
        run(
            &*conn,
            &format!(
                "CREATE TABLE {table} (id int PRIMARY KEY, {col} int, status varchar(10),
                 CONSTRAINT qa3_quoted_status CHECK (status IN ('new','it''s')),
                 CONSTRAINT qa3_quoted_fk FOREIGN KEY ({col}) REFERENCES {parent}(id))"
            ),
        )
        .await
        .unwrap();
        if d == platypus_sql::Dialect::Postgres {
            run(
                &*conn,
                &format!("CREATE INDEX qa3_quoted_idx ON {table} ({col})"),
            )
            .await
            .unwrap();
        }
        let only = ["qa3_quoted".to_owned(), "qa3_quoted_parent".to_owned()];
        let read = platypus_schema::read_tables(&*conn, &b.schema, Some(&only))
            .await
            .unwrap();
        let findings = platypus_schema::lint(d, &b.schema, &read);
        assert!(
            !findings
                .iter()
                .any(|f| f.kind == platypus_schema::LintKind::UnindexedForeignKey),
            "{}: {findings:?}",
            b.name
        );
        if mysql_before(&*conn, MYSQL_CHECKS) {
            run(&*conn, &format!("DROP TABLE {table}")).await.unwrap();
            run(&*conn, &format!("DROP TABLE {parent}")).await.unwrap();
            continue;
        }
        // The CHECK as read creates the table again.
        let design = platypus_schema::load(&*conn, d, &b.schema, "qa3_quoted")
            .await
            .unwrap();
        let check = &design
            .constraints
            .iter()
            .find(|c| c.name == "qa3_quoted_status")
            .unwrap()
            .check;
        // MySQL's CHECK_CLAUSE escapes every quote: `(_utf8mb4\'new\', ...)`.
        assert!(!check.contains(r"4\'new"), "{}: {check}", b.name);
        run(&*conn, &format!("DROP TABLE {table}")).await.unwrap();
        let script = platypus_schema::script(d, &b.schema, None, &design).unwrap();
        apply_script(&b, &*conn, &script).await;
        run(&*conn, &format!("DROP TABLE {table}")).await.unwrap();
        run(&*conn, &format!("DROP TABLE {parent}")).await.unwrap();
    }
}
