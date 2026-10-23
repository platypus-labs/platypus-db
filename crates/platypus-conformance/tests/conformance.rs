//! The same scenarios against every driver: PostgreSQL, MySQL, MariaDB and SQLite.
//!
//! Server databases come from `docker/compose.yml`. Setting `PLATYPUS_TEST_PG_URL`,
//! `PLATYPUS_TEST_MYSQL_URL` or `PLATYPUS_TEST_MARIADB_URL` makes that database required
//! (CI does); otherwise an unreachable default database is skipped. SQLite always runs, on
//! a temporary file.

use std::time::Duration;

use arrow_array::RecordBatch;
use arrow_cast::display::array_value_to_string;
use futures::{StreamExt, TryStreamExt};
use platypus_core::{
    Connection, ConnectionConfig, Driver, DriverKind, Error, ExecOptions, ExecResult, ObjectKind,
};
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
    /// Keeps the SQLite file alive.
    _dir: Option<TempDir>,
}

impl Backend {
    async fn connect(&self) -> Box<dyn Connection> {
        driver(self.config.driver)
            .connect(&self.config)
            .await
            .unwrap_or_else(|e| panic!("{}: cannot connect: {e}", self.name))
    }
}

fn driver(kind: DriverKind) -> &'static dyn Driver {
    match kind {
        DriverKind::Postgres => &PgDriver,
        DriverKind::MySql => &MySqlDriver,
        DriverKind::Sqlite => &SqliteDriver,
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
        series_sql: format!(
            "WITH RECURSIVE s(id) AS (SELECT 1 UNION ALL SELECT id + 1 FROM s WHERE id < {SERIES_ROWS})
             SELECT id, 'row ' || id AS label FROM s"
        ),
        endless_sql: "WITH RECURSIVE s(n) AS (SELECT 1 UNION ALL SELECT n + 1 FROM s) \
                      SELECT count(*) FROM s",
        _dir: Some(dir),
    }
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
                _dir: None,
            });
        }
    }
    all.push(sqlite_backend().await);
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

async fn affected(b: &Backend, conn: &dyn Connection, sql: &str) -> u64 {
    match conn.execute(sql, &ExecOptions::default()).await {
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
    match conn.execute(sql, &ExecOptions::default()).await? {
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
        let options = ExecOptions { batch_size: 1000 };
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
        let ddl = affected(
            &b,
            conn,
            "CREATE TEMPORARY TABLE conf_t (id INT, v VARCHAR(10))",
        )
        .await;
        assert_eq!(ddl, 0, "{}: DDL", b.name);
        let sql = "INSERT INTO conf_t VALUES (1, 'a'), (2, 'b'), (3, 'c')";
        assert_eq!(affected(&b, conn, sql).await, 3, "{}: insert", b.name);
        let sql = "UPDATE conf_t SET v = 'z' WHERE id >= 2";
        assert_eq!(affected(&b, conn, sql).await, 2, "{}: update", b.name);
        let rows = collect(&b, conn, "SELECT count(*) FROM conf_t WHERE v = 'z'").await;
        assert_eq!(cell(&rows[0], 0, 0).as_deref(), Some("2"), "{}", b.name);
        assert_eq!(
            affected(&b, conn, "DELETE FROM conf_t").await,
            3,
            "{}: delete",
            b.name
        );
        assert_eq!(
            affected(&b, conn, "DROP TABLE conf_t").await,
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
        match run(&*conn, "SELECT * FROMM t").await {
            Err(Error::Database(e)) => assert_eq!(e.position, Some(10), "{}: {e}", b.name),
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
        let cancel = tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(500)).await;
            handle.cancel().await
        });
        let result = tokio::time::timeout(Duration::from_secs(30), run(&*conn, b.endless_sql))
            .await
            .unwrap_or_else(|_| panic!("{}: cancel did not stop the query", b.name));
        cancel.await.unwrap().unwrap();
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
        let options = ExecOptions { batch_size: 100 };
        let ExecResult::Rows(mut stream) = conn.execute(&b.series_sql, &options).await.unwrap()
        else {
            panic!("{}: expected rows", b.name);
        };
        let first = stream.next().await.unwrap().unwrap();
        assert_eq!(first.num_rows(), 100, "{}", b.name);
        drop(stream);
        let rows = tokio::time::timeout(Duration::from_secs(10), collect(&b, &*conn, "SELECT 7"))
            .await
            .unwrap_or_else(|_| panic!("{}: connection stuck after drop", b.name));
        assert_eq!(cell(&rows[0], 0, 0).as_deref(), Some("7"), "{}", b.name);
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

        assert_eq!(orders.foreign_keys.len(), 1, "{}", b.name);
        let fk = &orders.foreign_keys[0];
        assert_eq!(fk.columns, ["customer_id"], "{}", b.name);
        assert_eq!(fk.ref_table, "customers", "{}", b.name);
        assert_eq!(fk.ref_columns, ["id"], "{}", b.name);

        let view = conn
            .table_details(&b.schema, "customer_totals")
            .await
            .unwrap();
        assert!(view.columns.len() >= 2, "{}: view columns", b.name);

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
    }
}
