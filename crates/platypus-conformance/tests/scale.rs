//! Scale: a schema of 10,000 tables (navigator, completion, metadata search), a result of a
//! million rows streamed to files, and cells of tens of megabytes, on PostgreSQL, MySQL,
//! MariaDB, SQL Server, SQLite and DuckDB. The servers come from `docker/compose.yml`, as in
//! `conformance.rs`; an unreachable one is skipped unless its `PLATYPUS_TEST_*_URL` is set.
//!
//! The time limits are those a user notices, a few times what the dev machine measures:
//! they catch a query that grows quadratically, not a slow laptop. Run with
//! `-- --nocapture` to see the timings.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use arrow_array::cast::AsArray;
use arrow_array::{Array, RecordBatch};
use arrow_schema::DataType;
use futures::{StreamExt, TryStreamExt};
use platypus_core::{
    Connection, ConnectionConfig, Driver, DriverKind, ExecOptions, ExecResult, ObjectKind,
};
use platypus_driver_duckdb::DuckDbDriver;
use platypus_driver_mssql::MssqlDriver;
use platypus_driver_mysql::MySqlDriver;
use platypus_driver_pg::PgDriver;
use platypus_driver_sqlite::SqliteDriver;
use platypus_transfer::{ExportOptions, Exporter, Format};
use tempfile::TempDir;

/// Tables of the large schema. MySQL and MariaDB take ~5 ms per `CREATE TABLE`, even
/// for in-memory tables, and DDL commits on its own there: they get fewer, unless
/// `PLATYPUS_SCALE_TABLES` sets the count for every database.
const TABLES: usize = 10_000;
const MYSQL_TABLES: usize = 2_000;
/// Tables created or dropped per transaction: PostgreSQL holds a lock per table until
/// the commit, and its lock table holds a few thousand.
const DDL_CHUNK: usize = 500;

const SERIES_ROWS: usize = 1_000_000;
/// Rows per batch, as the export of the app reads them.
const EXPORT_BATCH: usize = 8192;

/// The large text cell: 32 MiB of `0123456789abcdef`.
const TEXT_BYTES: usize = 32 << 20;
/// The large binary cell: 16 MiB of `00 ff`.
const BINARY_BYTES: usize = 16 << 20;

/// Listing the objects of the schema, as the navigator does when it is expanded.
const LIST_LIMIT: Duration = Duration::from_secs(3);
/// The columns of every table, which the editor loads for completion.
const CATALOG_LIMIT: Duration = Duration::from_secs(5);
/// One table among many: its columns, keys and indexes, when its row is expanded.
const DETAILS_LIMIT: Duration = Duration::from_secs(1);
const SEARCH_LIMIT: Duration = Duration::from_secs(3);
/// A million rows read and written to CSV, NDJSON and Parquet at once.
const SERIES_LIMIT: Duration = Duration::from_secs(60);
/// The large cells read and written to every format.
const CELLS_LIMIT: Duration = Duration::from_secs(30);

/// Held by every test: each one loads the servers on its own.
static SERIAL: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

fn driver(kind: DriverKind) -> &'static dyn Driver {
    match kind {
        DriverKind::Postgres => &PgDriver,
        DriverKind::MySql => &MySqlDriver,
        DriverKind::SqlServer => &MssqlDriver,
        DriverKind::Sqlite => &SqliteDriver,
        DriverKind::DuckDb => &DuckDbDriver,
        other => unreachable!("{other:?}"),
    }
}

struct Target {
    name: &'static str,
    config: ConnectionConfig,
    /// Where the large schema's tables go: a schema of their own, except in MySQL and
    /// MariaDB, where a schema is a database and the user has only `platypus`.
    schema: &'static str,
    tables: usize,
    /// Keeps the SQLite or DuckDB file alive.
    _dir: Option<TempDir>,
}

impl Target {
    fn kind(&self) -> DriverKind {
        self.config.driver
    }

    async fn connect(&self) -> Box<dyn Connection> {
        driver(self.kind())
            .connect(&self.config)
            .await
            .unwrap_or_else(|e| panic!("{}: cannot connect: {e}", self.name))
    }

    fn table(&self, i: usize) -> String {
        format!("{}.scale_t_{i:05}", self.schema)
    }

    fn create_table(&self, i: usize) -> String {
        let engine = if self.kind() == DriverKind::MySql {
            " ENGINE=MEMORY"
        } else {
            ""
        };
        format!(
            "CREATE TABLE {} (id INT PRIMARY KEY, name VARCHAR(100), amount DECIMAL(10, 2)){engine}",
            self.table(i)
        )
    }

    /// `SERIES_ROWS` rows: `id` (1..), `label` and `amount`.
    fn series_sql(&self) -> String {
        match self.kind() {
            DriverKind::Postgres => format!(
                "SELECT g AS id, 'row ' || g AS label, g * 0.25 AS amount \
                 FROM generate_series(1, {SERIES_ROWS}) g"
            ),
            DriverKind::MySql => {
                "SELECT s.id, CONCAT('row ', s.id) AS label, s.id * 0.25 AS amount \
                 FROM (SELECT 1 + a.d + 10 * b.d + 100 * c.d + 1000 * e.d + 10000 * f.d \
                       + 100000 * g.d AS id \
                       FROM digits a, digits b, digits c, digits e, digits f, digits g) s \
                 ORDER BY s.id"
                    .to_owned()
            }
            DriverKind::SqlServer => "WITH digits (d) AS (\
                   SELECT d FROM (VALUES (0), (1), (2), (3), (4), (5), (6), (7), (8), (9)) v (d)), \
                 s (id) AS (\
                   SELECT 1 + a.d + 10 * b.d + 100 * c.d + 1000 * e.d + 10000 * f.d + 100000 * g.d \
                   FROM digits a, digits b, digits c, digits e, digits f, digits g) \
                 SELECT id, CONCAT('row ', id) AS label, id * 0.25 AS amount FROM s ORDER BY id"
                .to_owned(),
            DriverKind::Sqlite => format!(
                "WITH RECURSIVE s(id) AS (SELECT 1 UNION ALL SELECT id + 1 FROM s WHERE id < {SERIES_ROWS}) \
                 SELECT id, 'row ' || id AS label, id * 0.25 AS amount FROM s"
            ),
            DriverKind::DuckDb => format!(
                "SELECT range + 1 AS id, 'row ' || (range + 1) AS label, (range + 1) * 0.25 AS amount \
                 FROM range({SERIES_ROWS})"
            ),
            other => unreachable!("{other:?}"),
        }
    }

    /// One row: `t`, `TEXT_BYTES` of text, and `b`, `BINARY_BYTES` of binary, built by
    /// the database.
    fn cells_sql(&self) -> String {
        let pattern = TEXT_BYTES / 16;
        let pairs = BINARY_BYTES / 2;
        match self.kind() {
            DriverKind::Postgres => format!(
                "SELECT repeat('0123456789abcdef', {pattern}) AS t, \
                 decode(repeat('00ff', {pairs}), 'hex') AS b"
            ),
            DriverKind::MySql => format!(
                "SELECT REPEAT('0123456789abcdef', {pattern}) AS t, \
                 UNHEX(REPEAT('00ff', {pairs})) AS b"
            ),
            DriverKind::SqlServer => format!(
                "SELECT REPLICATE(CAST('0123456789abcdef' AS varchar(max)), {pattern}) AS t, \
                 CONVERT(varbinary(max), REPLICATE(CAST('00ff' AS varchar(max)), {pairs}), 2) AS b"
            ),
            DriverKind::Sqlite => format!(
                "SELECT replace(hex(zeroblob({pattern})), '00', '0123456789abcdef') AS t, \
                 unhex(replace(hex(zeroblob({pairs})), '00', '00ff')) AS b"
            ),
            DriverKind::DuckDb => format!(
                "SELECT repeat('0123456789abcdef', {pattern}) AS t, \
                 unhex(repeat('00ff', {pairs})) AS b"
            ),
            other => unreachable!("{other:?}"),
        }
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

/// Tables of the large schema: `PLATYPUS_SCALE_TABLES`, else `default`.
fn table_count(default: usize) -> usize {
    std::env::var("PLATYPUS_SCALE_TABLES").map_or(default, |n| n.parse().unwrap())
}

/// A temporary SQLite or DuckDB file.
fn local(kind: DriverKind, name: &'static str, file: &str) -> Target {
    let dir = tempfile::tempdir().unwrap();
    let mut config = ConnectionConfig::new(kind);
    config.database = dir.path().join(file).to_string_lossy().into_owned();
    Target {
        name,
        config,
        schema: "main",
        tables: table_count(TABLES),
        _dir: Some(dir),
    }
}

async fn targets() -> Vec<Target> {
    let servers = [
        (
            "postgres",
            "PLATYPUS_TEST_PG_URL",
            "postgres://platypus:platypus@localhost:55432/platypus?sslmode=disable",
            "scale_10k",
            TABLES,
        ),
        (
            "mysql",
            "PLATYPUS_TEST_MYSQL_URL",
            "mysql://platypus:platypus@localhost:53306/platypus",
            "platypus",
            MYSQL_TABLES,
        ),
        (
            "mariadb",
            "PLATYPUS_TEST_MARIADB_URL",
            "mysql://platypus:platypus@localhost:53307/platypus",
            "platypus",
            MYSQL_TABLES,
        ),
        (
            "sqlserver",
            "PLATYPUS_TEST_MSSQL_URL",
            "sqlserver://sa:Platypus-2026@localhost:51433/platypus?sslmode=require",
            "scale_10k",
            TABLES,
        ),
    ];
    let mut all = Vec::new();
    for (name, env, default, schema, count) in servers {
        if let Some(config) = server_config(env, default, name).await {
            all.push(Target {
                name,
                config,
                schema,
                tables: table_count(count),
                _dir: None,
            });
        }
    }
    let sqlite = local(DriverKind::Sqlite, "sqlite", "scale.db");
    std::fs::File::create(&sqlite.config.database).unwrap();
    all.push(sqlite);
    platypus_driver_duckdb::testing::load();
    // DuckDB creates the file; an empty one is not a database.
    all.push(local(DriverKind::DuckDb, "duckdb", "scale.duckdb"));
    all
}

async fn run(t: &Target, conn: &dyn Connection, sql: &str) {
    conn.execute(sql, &ExecOptions::default())
        .await
        .unwrap_or_else(|e| panic!("{}: {sql}: {e}", t.name));
}

/// Runs `statements` in transactions of `DDL_CHUNK`; MySQL commits each DDL statement on
/// its own.
async fn run_chunked(t: &Target, conn: &dyn Connection, statements: &[String]) {
    let transactions = t.kind() != DriverKind::MySql;
    for chunk in statements.chunks(DDL_CHUNK) {
        if transactions {
            conn.set_autocommit(false).await.unwrap();
        }
        for sql in chunk {
            run(t, conn, sql).await;
        }
        if transactions {
            conn.commit().await.unwrap();
            conn.set_autocommit(true).await.unwrap();
        }
    }
}

/// Drops the scale tables a failed run left behind, and the schema with them.
async fn drop_scale_tables(t: &Target, conn: &dyn Connection) {
    let exists = conn
        .schemas()
        .await
        .unwrap()
        .iter()
        .any(|s| s.name == t.schema);
    if !exists {
        return;
    }
    let drops: Vec<String> = conn
        .objects(t.schema)
        .await
        .unwrap()
        .iter()
        .filter(|o| o.kind == ObjectKind::Table && o.name.starts_with("scale_t_"))
        .map(|o| format!("DROP TABLE {}.{}", t.schema, o.name))
        .collect();
    run_chunked(t, conn, &drops).await;
    if t.schema == "scale_10k" {
        run(t, conn, "DROP SCHEMA scale_10k").await;
    }
}

/// How long `f` took, printed.
async fn timed<T>(t: &Target, what: &str, f: impl Future<Output = T>) -> (T, Duration) {
    let started = Instant::now();
    let out = f.await;
    let took = started.elapsed();
    eprintln!("{}: {what}: {took:.2?}", t.name);
    (out, took)
}

#[tokio::test]
async fn lists_ten_thousand_tables() {
    let _serial = SERIAL.lock().await;

    for t in targets().await {
        let conn = t.connect().await;
        drop_scale_tables(&t, conn.as_ref()).await;
        if t.schema == "scale_10k" {
            run(&t, conn.as_ref(), "CREATE SCHEMA scale_10k").await;
        }
        let create: Vec<String> = (1..=t.tables).map(|i| t.create_table(i)).collect();
        timed(
            &t,
            &format!("create {} tables", t.tables),
            run_chunked(&t, conn.as_ref(), &create),
        )
        .await;

        // Measured before the tables are dropped, asserted after: a failed limit leaves
        // no tables behind.
        let checks = check_large_schema(&t, conn.as_ref()).await;
        drop_scale_tables(&t, conn.as_ref()).await;
        for (what, took, limit) in checks {
            assert!(took < limit, "{}: {what} took {took:.2?}", t.name);
        }
    }
}

/// What the navigator, the editor and the metadata search load from the large schema,
/// with how long it took and its limit.
async fn check_large_schema(
    t: &Target,
    conn: &dyn Connection,
) -> Vec<(&'static str, Duration, Duration)> {
    if t.kind() == DriverKind::MySql {
        // MySQL computes the statistics (rows, size) of new tables at their first listing,
        // ~0.5 ms a table, and keeps them a day: what is timed is any later listing.
        conn.objects(t.schema).await.unwrap();
    }
    let (objects, listing) = timed(t, "objects", conn.objects(t.schema)).await;
    let objects = objects.unwrap();
    let tables = objects
        .iter()
        .filter(|o| o.kind == ObjectKind::Table && o.name.starts_with("scale_t_"))
        .count();
    assert_eq!(tables, t.tables, "{}", t.name);

    let (catalog, columns) = timed(t, "catalog columns", conn.catalog_columns()).await;
    let catalog = catalog.unwrap();
    let scale_columns = catalog
        .iter()
        .filter(|c| c.schema == t.schema && c.table.starts_with("scale_t_"))
        .count();
    assert_eq!(scale_columns, 3 * t.tables, "{}", t.name);

    let last = format!("scale_t_{:05}", t.tables);
    let (details, one) = timed(t, "table details", conn.table_details(t.schema, &last)).await;
    let details = details.unwrap();
    let names: Vec<_> = details.columns.iter().map(|c| c.name.as_str()).collect();
    assert_eq!(names, ["id", "name", "amount"], "{}", t.name);
    assert!(details.columns[0].primary_key, "{}", t.name);

    let mut checks = vec![
        ("listing the objects", listing, LIST_LIMIT),
        ("listing the catalog columns", columns, CATALOG_LIMIT),
        ("reading one table's details", one, DETAILS_LIMIT),
    ];
    if conn.capabilities().metadata_search {
        let (found, search) = timed(t, "metadata search", conn.search_metadata(&last)).await;
        let found = found.unwrap();
        assert!(
            found.iter().any(|m| m.schema == t.schema && m.name == last),
            "{}: {last} not in {found:?}",
            t.name
        );
        checks.push(("searching the metadata", search, SEARCH_LIMIT));
    }
    checks
}

/// The file `Files` writes in `dir` for `format`.
fn export_path(dir: &Path, format: Format) -> PathBuf {
    dir.join(format!("export.{}", format.extension()))
}

/// The CSV, NDJSON and Parquet files of a result, written as the batches arrive.
struct Files {
    exporters: Vec<(Format, Exporter)>,
}

impl Files {
    fn create(dir: &Path, schema: &arrow_schema::Schema, formats: &[Format]) -> Self {
        let exporters = formats
            .iter()
            .map(|&format| {
                let path = export_path(dir, format);
                let options = ExportOptions {
                    format,
                    ..ExportOptions::default()
                };
                (format, Exporter::create(&path, schema, &options).unwrap())
            })
            .collect();
        Self { exporters }
    }

    fn write(&mut self, batch: &RecordBatch) {
        for (format, exporter) in &mut self.exporters {
            exporter
                .write(batch)
                .unwrap_or_else(|e| panic!("{format:?}: {e}"));
        }
    }

    /// The rows each file holds.
    fn finish(self) -> Vec<(Format, u64)> {
        self.exporters
            .into_iter()
            .map(|(format, exporter)| (format, exporter.finish().unwrap()))
            .collect()
    }
}

#[tokio::test]
async fn streams_a_million_rows_to_files() {
    let _serial = SERIAL.lock().await;

    for t in targets().await {
        let conn = t.connect().await;
        let dir = tempfile::tempdir().unwrap();
        let options = ExecOptions {
            batch_size: EXPORT_BATCH,
            ..ExecOptions::default()
        };
        let started = Instant::now();
        let sql = t.series_sql();
        let Ok(ExecResult::Rows(mut stream)) = conn.execute(&sql, &options).await else {
            panic!("{}: no rows from {sql}", t.name);
        };
        let mut files = Files::create(
            dir.path(),
            &stream.schema(),
            &[Format::Csv, Format::Ndjson, Format::Parquet],
        );
        let (mut rows, mut batches, mut largest, mut last_id) = (0, 0, 0, 0);
        while let Some(batch) = stream.next().await {
            let batch = batch.unwrap_or_else(|e| panic!("{}: {e}", t.name));
            rows += batch.num_rows();
            batches += 1;
            largest = largest.max(batch.num_rows());
            if batch.num_rows() > 0 {
                last_id = int_at(&batch, 0, batch.num_rows() - 1);
            }
            files.write(&batch);
        }
        let written = files.finish();
        let took = started.elapsed();
        eprintln!(
            "{}: {SERIES_ROWS} rows to CSV, NDJSON and Parquet: {took:.2?}",
            t.name
        );

        assert_eq!(rows, SERIES_ROWS, "{}", t.name);
        // Streamed: never more than a batch in memory.
        assert!(
            largest <= EXPORT_BATCH,
            "{}: a batch of {largest} rows",
            t.name
        );
        assert!(
            batches >= SERIES_ROWS / EXPORT_BATCH,
            "{}: {batches} batches",
            t.name
        );
        assert_eq!(last_id, i64::try_from(SERIES_ROWS).unwrap(), "{}", t.name);
        for (format, rows) in written {
            assert_eq!(rows, SERIES_ROWS as u64, "{}: {format:?}", t.name);
        }
        let csv = std::fs::read_to_string(export_path(dir.path(), Format::Csv)).unwrap();
        assert_eq!(
            csv.lines().count(),
            SERIES_ROWS + 1,
            "{}: CSV lines",
            t.name
        );
        assert!(took < SERIES_LIMIT, "{}: {took:.2?}", t.name);
    }
}

/// The integer at `row` of `column`, whatever its width.
fn int_at(batch: &RecordBatch, column: usize, row: usize) -> i64 {
    let array = arrow_cast::cast(batch.column(column), &DataType::Int64).unwrap();
    array
        .as_primitive::<arrow_array::types::Int64Type>()
        .value(row)
}

/// The bytes of a text or binary value, whatever its Arrow type.
fn bytes_at(array: &dyn Array, row: usize) -> &[u8] {
    match array.data_type() {
        DataType::Utf8 => array.as_string::<i32>().value(row).as_bytes(),
        DataType::LargeUtf8 => array.as_string::<i64>().value(row).as_bytes(),
        DataType::Utf8View => array.as_string_view().value(row).as_bytes(),
        DataType::Binary => array.as_binary::<i32>().value(row),
        DataType::LargeBinary => array.as_binary::<i64>().value(row),
        DataType::BinaryView => array.as_binary_view().value(row),
        other => panic!("not text or binary: {other}"),
    }
}

#[tokio::test]
async fn reads_and_exports_cells_of_tens_of_megabytes() {
    let _serial = SERIAL.lock().await;

    for t in targets().await {
        let conn = t.connect().await;
        let started = Instant::now();
        let sql = t.cells_sql();
        let batches: Vec<RecordBatch> = match conn.execute(&sql, &ExecOptions::default()).await {
            Ok(ExecResult::Rows(stream)) => stream
                .try_collect()
                .await
                .unwrap_or_else(|e| panic!("{}: {e}", t.name)),
            other => panic!("{}: expected rows, got {other:?}", t.name),
        };
        let read = started.elapsed();
        let batch = batches.iter().find(|b| b.num_rows() > 0).unwrap();

        let text = bytes_at(batch.column(0).as_ref(), 0);
        assert_eq!(text.len(), TEXT_BYTES, "{}: text length", t.name);
        assert!(text.starts_with(b"0123456789abcdef"), "{}", t.name);
        assert!(text.ends_with(b"0123456789abcdef"), "{}", t.name);
        let binary = bytes_at(batch.column(1).as_ref(), 0);
        assert_eq!(binary.len(), BINARY_BYTES, "{}: binary length", t.name);
        assert_eq!(&binary[..4], [0x00, 0xff, 0x00, 0xff], "{}", t.name);

        let dir = tempfile::tempdir().unwrap();
        let mut files = Files::create(
            dir.path(),
            &batch.schema(),
            &[
                Format::Csv,
                Format::Json,
                Format::Sql,
                Format::Xlsx,
                Format::Parquet,
            ],
        );
        files.write(batch);
        files.finish();
        let took = started.elapsed();
        eprintln!(
            "{}: {} MiB of cells read in {read:.2?}, exported to five formats in {took:.2?}",
            t.name,
            (TEXT_BYTES + BINARY_BYTES) >> 20
        );

        let size = |format: Format| {
            std::fs::metadata(export_path(dir.path(), format))
                .unwrap()
                .len()
        };
        // The text formats hold the whole text, and the binary as hex or base64.
        for format in [Format::Csv, Format::Json, Format::Sql] {
            assert!(size(format) > TEXT_BYTES as u64, "{}: {format:?}", t.name);
        }
        // Excel cuts a cell at 32,767 characters: the file stays small.
        assert!(size(Format::Xlsx) < 1 << 20, "{}: XLSX", t.name);
        assert!(size(Format::Parquet) > 0, "{}: Parquet", t.name);
        let csv = std::fs::read_to_string(export_path(dir.path(), Format::Csv)).unwrap();
        assert!(
            csv.contains(std::str::from_utf8(text).unwrap()),
            "{}: the CSV holds the text whole",
            t.name
        );
        assert!(took < CELLS_LIMIT, "{}: {took:.2?}", t.name);
    }
}
