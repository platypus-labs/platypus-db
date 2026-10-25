//! The same scenarios against every driver: PostgreSQL, MySQL, MariaDB, SQL Server,
//! ClickHouse, SQLite, DuckDB and libSQL.
//!
//! Server databases come from `docker/compose.yml` (SQL Server and libSQL from their
//! `sqlserver` and `libsql` profiles). Setting `PLATYPUS_TEST_PG_URL`,
//! `PLATYPUS_TEST_MYSQL_URL`, `PLATYPUS_TEST_MARIADB_URL`, `PLATYPUS_TEST_MSSQL_URL`,
//! `PLATYPUS_TEST_CLICKHOUSE_URL` or `PLATYPUS_TEST_LIBSQL_URL` makes that database required
//! (CI does); otherwise an unreachable default database is skipped. SQLite and DuckDB always
//! run, on temporary files.

use std::time::Duration;

use arrow_array::RecordBatch;
use arrow_array::cast::AsArray;
use arrow_cast::display::array_value_to_string;
use futures::{StreamExt, TryStreamExt};
use platypus_core::geometry;
use platypus_core::{
    Connection, ConnectionConfig, ConstraintKind, Driver, DriverKind, Error, ExecOptions,
    ExecResult, MatchKind, ObjectKind, ObjectRef, PlanNode, SessionInit,
};
use platypus_driver_clickhouse::ClickHouseDriver;
use platypus_driver_duckdb::DuckDbDriver;
use platypus_driver_libsql::LibsqlDriver;
use platypus_driver_mongo::MongoDriver;
use platypus_driver_mssql::MssqlDriver;
use platypus_driver_mysql::MySqlDriver;
use platypus_driver_pg::PgDriver;
use platypus_driver_sqlite::SqliteDriver;
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
    let mut all = Vec::new();
    if let Some(config) = server_config(
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
    all.extend(pg_wire_backends().await);
    all.extend(mssql_backend().await);
    all.extend(clickhouse_backend().await);
    all.push(sqlite_backend().await);
    all.push(duckdb_backend().await);
    all.extend(libsql_backend().await);
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
    for b in backends().await {
        let conn = b.connect().await;
        let server = conn.server_info();
        assert_eq!(server.product, b.product, "{}", b.name);
        assert!(!server.version.is_empty(), "{}: empty version", b.name);
    }
}

#[tokio::test]
async fn selects_scalars() {
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
    }
}

#[tokio::test]
async fn lists_catalog_columns_for_completion() {
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

/// Some step has actual rows and its own share of the time.
fn plan_has_actuals(node: &PlanNode) -> bool {
    (node.actual_rows.is_some() && node.self_ms.is_some())
        || node.children.iter().any(plan_has_actuals)
}

/// Plans come back as a tree. With analyze the statement runs, and what it changes is
/// rolled back, in autocommit and inside an open transaction alike.
#[tokio::test]
async fn explains_queries() {
    for b in backends().await {
        let conn = b.connect().await;
        let conn = &*conn;
        // Plans are optional: `capabilities_match_optional_methods` checks the others.
        if !conn.capabilities().explain {
            continue;
        }
        let create = format!("CREATE {}TABLE conf_plan (id INT)", b.temporary());
        affected(&b, conn, &create).await;
        affected(&b, conn, "INSERT INTO conf_plan VALUES (1), (2), (3)").await;
        let count = || async {
            let rows = collect(&b, conn, "SELECT count(*) FROM conf_plan").await;
            cell(&rows[0], 0, 0).unwrap()
        };
        let select = "SELECT * FROM conf_plan WHERE id > 1";
        let plan = conn.explain(select, false).await.unwrap();
        assert!(!plan.analyzed && !plan.plan.is_empty(), "{}", b.name);
        assert!(
            plan_mentions(&plan.root, "conf_plan"),
            "{}: {:#?}",
            b.name,
            plan.root
        );

        let plan = conn.explain(select, true).await.unwrap();
        // SQLite (and libSQL) has no plan that runs the statement.
        let analyzes = !b.speaks_sqlite();
        assert_eq!(plan.analyzed, analyzes, "{}", b.name);
        assert_eq!(
            plan_has_actuals(&plan.root),
            analyzes,
            "{}: {:#?}",
            b.name,
            plan.root
        );

        // MySQL analyzes only the multi-table form of DELETE.
        let delete = if b.product == "MySQL" {
            "DELETE p FROM conf_plan p WHERE p.id > 1"
        } else {
            "DELETE FROM conf_plan WHERE id > 1"
        };
        conn.explain(delete, true)
            .await
            .unwrap_or_else(|e| panic!("{}: {e}", b.name));
        assert_eq!(count().await, "3", "{}: analyze rolled back", b.name);
        assert!(!conn.transaction().open, "{}", b.name);

        conn.set_autocommit(false).await.unwrap();
        affected(&b, conn, "INSERT INTO conf_plan VALUES (4)").await;
        conn.explain(delete, true).await.unwrap();
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
    for b in backends().await {
        let conn = b.connect().await;
        let conn = &*conn;
        // Plans are optional: `capabilities_match_optional_methods` checks the others.
        if !conn.capabilities().explain {
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
        // SQLite plans have no row counts to judge a table by.
        let expected: &[&str] = if b.speaks_sqlite() {
            &[]
        } else {
            &["conf_big"]
        };
        assert_eq!(tables, expected, "{}: {:#?}", b.name, plan.root);
    }
}

/// Every optional method agrees with its capability flag: unsupported ones fail with
/// `Error::Unsupported`, supported ones never do.
#[tokio::test]
async fn capabilities_match_optional_methods() {
    for b in backends().await {
        let conn = b.connect().await;
        let caps = conn.capabilities();
        // libSQL's HTTP protocol has no way to interrupt a statement.
        assert_eq!(caps.cancel, b.config.driver != DriverKind::Libsql, "{}", b.name);
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
    }
}

/// Constraints, incoming references, triggers and DDL, on a probe table created for the
/// test (with a primary key, a unique column, a check and a trigger) and on the fixtures.
#[tokio::test]
async fn describes_objects_and_their_ddl() {
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
        DriverKind::Mongo => unreachable!("MongoDB has its own tests in mongo.rs"),
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
        DriverKind::Mongo => unreachable!("MongoDB has its own tests in mongo.rs"),
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
    // SQLite keeps checks only in the table's SQL.
    if !b.speaks_sqlite() {
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
        DriverKind::Mongo => unreachable!("MongoDB has its own tests in mongo.rs"),
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
    use platypus_sql::dml::{self, Column, Value, ValueKind};

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

        let inserts = [
            dml::insert(
                d,
                t,
                &columns,
                &[
                    (0, Value::Text("1")),
                    (1, Value::Text(name)),
                    (2, Value::Text("12.50")),
                    (3, Value::Text("00ff10")),
                    (4, Value::Text(at)),
                    (5, Value::Default),
                ],
            )
            .unwrap(),
            dml::insert(d, t, &columns, &[(0, Value::Text("2"))]).unwrap(),
        ];
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
        dml::apply(conn, d, &[update])
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
    use platypus_sql::dml::{self, ApplyError, Value};

    let (d, t) = (Dialect::from(b.config.driver), &b.temp("conf_edit"));

    // A statement that misses its row rolls back the ones before it.
    let ok = dml::update(d, t, columns, &[(1, Value::Text("x"))], &[(0, Some("2"))]).unwrap();
    let missing = dml::update(d, t, columns, &[(1, Value::Text("y"))], &[(0, Some("99"))]).unwrap();
    let err = dml::apply(conn, d, &[ok, missing.clone()])
        .await
        .unwrap_err();
    assert!(
        matches!(err, ApplyError::RowCount { index: 1, rows: 0 }),
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
    dml::apply(conn, d, &[delete])
        .await
        .unwrap_or_else(|e| panic!("{}: delete: {e}", b.name));
    assert!(conn.transaction().open, "{}: left uncommitted", b.name);
    // A missing row that changed nothing keeps the session's work, savepoints or not.
    let err = dml::apply(conn, d, &[missing]).await.unwrap_err();
    assert!(
        matches!(err, ApplyError::RowCount { index: 0, rows: 0 }),
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

#[tokio::test]
async fn estimates_table_rows() {
    use platypus_sql::Dialect;
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

/// PostgreSQL arrays and composite values: the fields of composite columns, and literals the
/// data editor joins that come back as the same elements.
#[tokio::test]
async fn edits_arrays_and_composites() {
    use platypus_sql::Dialect;
    use platypus_sql::dml::{self, Column, Value, ValueKind};
    use platypus_sql::pg_literal::{join_array, join_composite, split_array, split_composite};

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
            join_array(&tags),
            join_array(&nums),
            join_composite(&pair),
            join_array(&pairs),
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
        dml::apply(conn, Dialect::from(b.config.driver), &[update])
            .await
            .unwrap_or_else(|e| panic!("{}: update: {e}", b.name));

        let rows = collect(
            &b,
            conn,
            "SELECT tags, nums, pair, pairs FROM demo.conf_arrays",
        )
        .await;
        let shown = |column| cell(&rows[0], column, 0).unwrap();
        assert_eq!(split_array(&shown(0)), Some(tags));
        assert_eq!(split_array(&shown(1)), Some(nums));
        assert_eq!(split_composite(&shown(2)), Some(pair.clone()));
        let read = split_array(&shown(3)).unwrap();
        assert_eq!(read[1], None);
        assert_eq!(split_composite(read[0].as_deref().unwrap()), Some(pair));

        run(conn, "DROP TABLE demo.conf_arrays").await.unwrap();
        run(conn, "DROP TYPE demo.conf_pair").await.unwrap();
    }
}

#[tokio::test]
async fn searches_metadata() {
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
            DriverKind::Mongo => unreachable!("MongoDB has its own tests in mongo.rs"),
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
            DriverKind::Sqlite | DriverKind::Libsql | DriverKind::DuckDb | DriverKind::ClickHouse => {
                ("SELECT 'POINT (1 2)'", vec![point.clone()])
            }
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
    for b in backends().await {
        let mut config = b.config.clone();
        let (check, expected) = match config.driver {
            DriverKind::Mongo => unreachable!("MongoDB has its own tests in mongo.rs"),
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
                DriverKind::Mongo => unreachable!("MongoDB has its own tests in mongo.rs"),
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
