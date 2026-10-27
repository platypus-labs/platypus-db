//! Values every driver must return exactly as the database holds them, and write back: the
//! ones that do not fit a plain Arrow type (infinities, zero dates, `NaN`, 100 ns times,
//! wide money), types decoded from their wire format, and results whose later rows have
//! other types than the first ones. Each database runs when its container of
//! `docker/compose.yml` is reachable; tables are named `qa_fixdrv_*` and dropped again.

use std::time::Duration;

use arrow_schema::DataType;
use futures::TryStreamExt;
use platypus_core::{Connection, ConnectionConfig, Driver, DriverKind, ExecOptions, ExecResult};
use platypus_driver_duckdb::DuckDbDriver;
use platypus_driver_mongo::MongoDriver;
use platypus_driver_mssql::MssqlDriver;
use platypus_driver_mysql::MySqlDriver;
use platypus_driver_pg::PgDriver;
use platypus_driver_sqlite::SqliteDriver;
use platypus_sql::Dialect;
use platypus_transfer::import::{ImportColumn, ImportMode, ImportPlan, import, rows_text};

/// The connection to `url`, or `None` (with a note) when its server is down.
async fn connect(driver: &dyn Driver, url: &str) -> Option<Box<dyn Connection>> {
    let mut config = ConnectionConfig::from_url(url).unwrap();
    config.connect_timeout = Some(Duration::from_secs(3));
    match driver.connect(&config).await {
        Ok(conn) => Some(conn),
        Err(e) => {
            eprintln!("skipping {url}: {e}");
            None
        }
    }
}

const PG: &str = "postgres://platypus:platypus@localhost:55432/platypus?sslmode=disable";
const MYSQL: &str = "mysql://platypus:platypus@localhost:53306/platypus";
const MARIADB: &str = "mysql://platypus:platypus@localhost:53307/platypus";
const MSSQL: &str = "sqlserver://sa:Platypus-2026@localhost:51433/platypus?sslmode=require";
const MONGO: &str = "mongodb://platypus:platypus@localhost:57017/platypus?authSource=admin";

/// A connection to a database file of SQLite or DuckDB.
async fn file_connection(
    driver: &dyn Driver,
    kind: DriverKind,
    path: &std::path::Path,
) -> Box<dyn Connection> {
    let mut config = ConnectionConfig::new(kind);
    config.database = path.to_string_lossy().into_owned();
    driver.connect(&config).await.unwrap()
}

async fn run(conn: &dyn Connection, sql: &str) -> u64 {
    match conn.execute(sql, &ExecOptions::default()).await {
        Ok(ExecResult::Affected { rows }) => rows,
        Ok(ExecResult::Rows(stream)) => {
            let _: Vec<_> = stream.try_collect().await.unwrap();
            0
        }
        Err(e) => panic!("{sql}: {e}"),
    }
}

/// The rows of `sql` as every text output shows them, with the column types.
async fn texts_with(
    conn: &dyn Connection,
    sql: &str,
    batch_size: usize,
) -> (Vec<DataType>, Vec<Vec<Option<String>>>) {
    let options = ExecOptions {
        batch_size,
        ..ExecOptions::default()
    };
    let ExecResult::Rows(stream) = conn
        .execute(sql, &options)
        .await
        .unwrap_or_else(|e| panic!("{sql}: {e}"))
    else {
        panic!("{sql}: no rows");
    };
    let types = stream
        .schema()
        .fields()
        .iter()
        .map(|f| f.data_type().clone())
        .collect();
    let batches: Vec<_> = stream
        .try_collect()
        .await
        .unwrap_or_else(|e| panic!("{sql}: {e}"));
    let mut rows = Vec::new();
    for batch in &batches {
        rows.extend(rows_text(batch).unwrap());
    }
    (types, rows)
}

async fn texts(conn: &dyn Connection, sql: &str) -> Vec<Vec<Option<String>>> {
    texts_with(conn, sql, 1000).await.1
}

fn row(values: &[&str]) -> Vec<Option<String>> {
    values.iter().map(|v| Some((*v).to_owned())).collect()
}

#[tokio::test]
async fn pg_special_values_show_and_copy() {
    let Some(conn) = connect(&PgDriver, PG).await else {
        return;
    };
    let conn = &*conn;
    assert_eq!(
        texts(
            conn,
            "SELECT 'infinity'::date, '-infinity'::timestamp, 'infinity'::timestamptz, \
             'NaN'::numeric(10,2), 0.00123::numeric(3,5), '0044-03-15 BC'::date"
        )
        .await,
        [row(&[
            "infinity",
            "-infinity",
            "infinity",
            "NaN",
            "0.00123",
            "0044-03-15 BC"
        ])]
    );

    // A copy into a table of PostgreSQL keeps them.
    run(
        conn,
        "DROP TABLE IF EXISTS qa_fixdrv_inf, qa_fixdrv_inf_copy",
    )
    .await;
    run(
        conn,
        "CREATE TABLE qa_fixdrv_inf (id int PRIMARY KEY, d date, t timestamptz, n numeric(10,2))",
    )
    .await;
    run(
        conn,
        "INSERT INTO qa_fixdrv_inf VALUES (1, 'infinity', '-infinity', 'NaN'), \
         (2, '2024-01-02', '2024-01-02 03:04:05+00', 1.5), (3, '0044-03-15 BC', 'infinity', NULL)",
    )
    .await;
    let target = connect(&PgDriver, PG).await.unwrap();
    let column = |name: &str, data_type: &str| ImportColumn {
        source: name.to_owned(),
        target: name.to_owned(),
        data_type: Some(data_type.to_owned()),
        not_null: false,
        default: None,
    };
    let plan = ImportPlan {
        source_table: "qa_fixdrv_inf".to_owned(),
        source_schema: None,
        schema: None,
        table: "qa_fixdrv_inf_copy".to_owned(),
        create: true,
        mode: ImportMode::Append,
        keys: Vec::new(),
        primary_key: Vec::new(),
        identity: None,
        constraints: Vec::new(),
        columns: vec![
            column("id", "int"),
            column("d", "date"),
            column("t", "timestamptz"),
            column("n", "numeric(10,2)"),
        ],
    };
    let copied = import(
        (conn, Dialect::Postgres),
        (&*target, Dialect::Postgres),
        &plan,
        |_| true,
    )
    .await;
    let same = "SELECT (SELECT count(*) FROM qa_fixdrv_inf a JOIN qa_fixdrv_inf_copy b \
                ON a.id = b.id AND a.d = b.d AND a.t = b.t AND a.n IS NOT DISTINCT FROM b.n)";
    let matched = texts(conn, same).await;
    run(
        conn,
        "DROP TABLE IF EXISTS qa_fixdrv_inf, qa_fixdrv_inf_copy",
    )
    .await;
    assert_eq!(copied.unwrap(), 3);
    assert_eq!(matched, [row(&["3"])]);
}

#[tokio::test]
async fn pg_types_decode_as_postgresql_prints_them() {
    let Some(conn) = connect(&PgDriver, PG).await else {
        return;
    };
    let conn = &*conn;
    assert_eq!(
        texts(
            conn,
            "SELECT B'1010', '101'::varbit, 'a & (b | !c)'::tsquery, 'the quick fox'::tsvector, \
             '16/B374D848'::pg_lsn, '08:00:2b:01:02:03:04:05'::macaddr8, pg_typeof(1), \
             'pg_class'::regclass, 'now'::regproc, ROW(1, 'a b')"
        )
        .await,
        [row(&[
            "1010",
            "101",
            "'a' & ( 'b' | !'c' )",
            "'fox' 'quick' 'the'",
            "16/B374D848",
            "08:00:2b:01:02:03:04:05",
            "integer",
            "pg_class",
            "now",
            "(1,\"a b\")"
        ])]
    );
    let (types, rows) = texts_with(conn, "SELECT FROM generate_series(1, 3)", 1000).await;
    assert_eq!(types.len(), 0, "no columns");
    assert_eq!(rows.len(), 3, "rows without columns");
}

#[tokio::test]
async fn pg_failed_import_drops_nothing_it_did_not_create() {
    let Some(conn) = connect(&PgDriver, PG).await else {
        return;
    };
    let conn = &*conn;
    run(conn, "DROP SCHEMA IF EXISTS qa_fixdrv_s CASCADE").await;
    run(conn, "DROP TABLE IF EXISTS qa_fixdrv_src").await;
    run(conn, "CREATE SCHEMA qa_fixdrv_s").await;
    run(conn, "CREATE TABLE qa_fixdrv_s.qa_fixdrv_t (x int)").await;
    run(conn, "CREATE TABLE qa_fixdrv_src (v text)").await;
    run(conn, "INSERT INTO qa_fixdrv_src VALUES ('far too long')").await;
    // The target's search path finds the other schema's table after `public`.
    let target = connect(&PgDriver, PG).await.unwrap();
    run(&*target, "SET search_path TO public, qa_fixdrv_s").await;
    let plan = ImportPlan {
        source_table: "qa_fixdrv_src".to_owned(),
        source_schema: None,
        schema: None,
        table: "qa_fixdrv_t".to_owned(),
        create: true,
        mode: ImportMode::Append,
        keys: Vec::new(),
        primary_key: Vec::new(),
        identity: None,
        constraints: Vec::new(),
        columns: vec![ImportColumn {
            source: "v".to_owned(),
            target: "v".to_owned(),
            data_type: Some("varchar(2)".to_owned()),
            not_null: false,
            default: None,
        }],
    };
    let result = import(
        (conn, Dialect::Postgres),
        (&*target, Dialect::Postgres),
        &plan,
        |_| true,
    )
    .await;
    let left = texts(conn, "SELECT to_regclass('qa_fixdrv_s.qa_fixdrv_t')::text").await;
    run(conn, "DROP SCHEMA IF EXISTS qa_fixdrv_s CASCADE").await;
    run(
        conn,
        "DROP TABLE IF EXISTS qa_fixdrv_src, public.qa_fixdrv_t",
    )
    .await;
    assert!(result.is_err(), "the value is too long");
    assert_eq!(left, [row(&["qa_fixdrv_s.qa_fixdrv_t"])]);
}

async fn mysql_dates_and_zones(url: &str) {
    let Some(conn) = connect(&MySqlDriver, url).await else {
        return;
    };
    let conn = &*conn;
    run(conn, "SET SESSION sql_mode = 'ALLOW_INVALID_DATES'").await;
    run(conn, "DROP TEMPORARY TABLE IF EXISTS qa_fixdrv_dates").await;
    run(
        conn,
        "CREATE TEMPORARY TABLE qa_fixdrv_dates (id int, d date, dt datetime, ts timestamp NULL)",
    )
    .await;
    run(
        conn,
        "INSERT INTO qa_fixdrv_dates VALUES (1, '0000-00-00', '0000-00-00 00:00:00', \
         '2024-01-02 03:04:05'), (2, '2024-02-31', '2024-02-30 10:00:00', NULL)",
    )
    .await;
    assert_eq!(
        texts(conn, "SELECT d, dt, ts FROM qa_fixdrv_dates ORDER BY id").await,
        [
            row(&["0000-00-00", "0000-00-00 00:00:00", "2024-01-02T03:04:05Z"]),
            vec![
                Some("2024-02-31".to_owned()),
                Some("2024-02-30 10:00:00".to_owned()),
                None
            ],
        ]
    );
    // A zero date as a key finds its row.
    assert_eq!(
        run(
            conn,
            "UPDATE qa_fixdrv_dates SET id = 3 WHERE d = '0000-00-00' AND dt = '0000-00-00 00:00:00'"
        )
        .await,
        1
    );

    // In another zone, TIMESTAMP values are that zone's wall times, without a label.
    run(conn, "SET time_zone = '+02:00'").await;
    let (types, rows) = texts_with(conn, "SELECT ts FROM qa_fixdrv_dates WHERE id = 3", 1000).await;
    assert!(
        matches!(types[0], DataType::Timestamp(_, None)),
        "{types:?}"
    );
    assert_eq!(rows, [row(&["2024-01-02T05:04:05"])]);
    // Written back as shown, the value is the same instant.
    assert_eq!(
        run(
            conn,
            "UPDATE qa_fixdrv_dates SET id = 4 WHERE ts = '2024-01-02T05:04:05'"
        )
        .await,
        1
    );
    run(conn, "SET time_zone = 'UTC'").await;
    let (types, _) = texts_with(conn, "SELECT ts FROM qa_fixdrv_dates", 1000).await;
    assert!(
        matches!(types[0], DataType::Timestamp(_, Some(_))),
        "{types:?}"
    );
    run(conn, "DROP TEMPORARY TABLE qa_fixdrv_dates").await;
}

#[tokio::test]
async fn mysql_zero_dates_and_session_zones() {
    mysql_dates_and_zones(MYSQL).await;
}

#[tokio::test]
async fn mariadb_zero_dates_and_session_zones() {
    mysql_dates_and_zones(MARIADB).await;
}

#[tokio::test]
async fn mssql_money_and_times_are_exact_and_keys_save() {
    let Some(conn) = connect(&MssqlDriver, MSSQL).await else {
        return;
    };
    let conn = &*conn;
    assert_eq!(
        texts(
            conn,
            "SELECT CAST(922337203685477.5807 AS money), CAST(98765432109876.5432 AS money), \
             CAST('2024-01-02T03:04:05.997' AS datetime), \
             CAST('2024-01-02T03:04:05.1234567' AS datetime2(7)), \
             CAST('2024-01-02T03:04:05.1234567+02:00' AS datetimeoffset(7))"
        )
        .await,
        [row(&[
            "922337203685477.5807",
            "98765432109876.5432",
            "2024-01-02T03:04:05.997",
            "2024-01-02 03:04:05.1234567",
            "2024-01-02 03:04:05.1234567 +02:00"
        ])]
    );
    run(
        conn,
        "CREATE TABLE #qa_fixdrv_keys (k datetime, k2 datetime2(7), k3 datetimeoffset, v int)",
    )
    .await;
    run(
        conn,
        "INSERT INTO #qa_fixdrv_keys VALUES ('2024-01-02T03:04:05.003', \
         '2024-01-02T03:04:05.1234567', '2024-01-02T03:04:05.1234567+02:00', 1)",
    )
    .await;
    let shown = texts(conn, "SELECT k, k2, k3 FROM #qa_fixdrv_keys").await;
    let [k, k2, k3] = [0, 1, 2].map(|i| shown[0][i].clone().unwrap());
    // The values as shown, written back as the keys of an edit, find the row.
    let update =
        format!("UPDATE #qa_fixdrv_keys SET v = 2 WHERE k = '{k}' AND k2 = '{k2}' AND k3 = '{k3}'");
    assert_eq!(run(conn, &update).await, 1, "{update}");
    run(conn, "DROP TABLE #qa_fixdrv_keys").await;
}

#[tokio::test]
async fn sqlite_later_rows_of_other_types_keep_their_values() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("qa.db");
    // SQLite opens existing files only.
    std::fs::File::create(&path).unwrap();
    let conn = file_connection(&SqliteDriver, DriverKind::Sqlite, &path).await;
    let conn = &*conn;
    run(conn, "CREATE TABLE qa_fixdrv_mixed (id INTEGER, v)").await;
    run(
        conn,
        "WITH RECURSIVE n(i) AS (SELECT 1 UNION ALL SELECT i + 1 FROM n WHERE i < 300) \
         INSERT INTO qa_fixdrv_mixed SELECT i, i FROM n",
    )
    .await;
    run(
        conn,
        "INSERT INTO qa_fixdrv_mixed VALUES (301, 'text'), (302, 9007199254740993), (303, 0.5)",
    )
    .await;
    let (types, rows) = texts_with(conn, "SELECT v FROM qa_fixdrv_mixed ORDER BY id", 100).await;
    assert_eq!(types, [DataType::Utf8]);
    assert_eq!(rows.len(), 303);
    assert_eq!(rows[300], row(&["text"]));
    assert_eq!(rows[301], row(&["9007199254740993"]));
    assert_eq!(rows[302], row(&["0.5"]));
    // Reals and integers beyond 2^53: text, never rounded.
    let (types, rows) = texts_with(
        conn,
        "SELECT v FROM qa_fixdrv_mixed WHERE id > 301 ORDER BY id",
        1,
    )
    .await;
    assert_eq!(types, [DataType::Utf8]);
    assert_eq!(rows[0], row(&["9007199254740993"]));
}

#[tokio::test]
async fn mongo_later_documents_keep_their_values_and_fields() {
    let Some(conn) = connect(&MongoDriver, MONGO).await else {
        return;
    };
    let conn = &*conn;
    run(conn, "db.qa_fixdrv_mixed.drop()").await;
    let mut docs: Vec<String> = (1..=20)
        .map(|i| format!("{{ _id: {i}, qty: {i} }}"))
        .collect();
    docs.push("{ _id: 21, qty: NumberLong(\"5000000000\") }".to_owned());
    docs.push("{ _id: 22, qty: 2.5, late: \"x\" }".to_owned());
    run(
        conn,
        &format!("db.qa_fixdrv_mixed.insertMany([{}])", docs.join(", ")),
    )
    .await;
    let (types, rows) = texts_with(conn, "db.qa_fixdrv_mixed.find().sort({ _id: 1 })", 5).await;
    let aggregated = texts_with(
        conn,
        "db.qa_fixdrv_mixed.aggregate([{ $sort: { _id: 1 } }])",
        5,
    )
    .await;
    run(conn, "db.qa_fixdrv_mixed.drop()").await;
    assert_eq!(types.len(), 3, "{types:?}: a column for the late field");
    assert_eq!(types[1], DataType::Float64);
    assert_eq!(rows.len(), 22);
    assert_eq!(rows[0][1].as_deref(), Some("1.0"));
    assert_eq!(rows[20][1].as_deref(), Some("5000000000.0"));
    assert_eq!(rows[21], row(&["22", "2.5", "x"]));
    assert_eq!(aggregated.1, rows);
}

#[tokio::test]
async fn duckdb_special_values_and_bits() {
    let dir = tempfile::tempdir().unwrap();
    let conn = file_connection(
        &DuckDbDriver,
        DriverKind::DuckDb,
        &dir.path().join("qa.duckdb"),
    )
    .await;
    assert_eq!(
        texts(
            &*conn,
            "SELECT 'infinity'::DATE, '-infinity'::TIMESTAMP, 'infinity'::TIMESTAMPTZ, \
             '1010'::BIT, [1, NULL, 3]"
        )
        .await,
        [row(&[
            "infinity",
            "-infinity",
            "infinity",
            "1010",
            "[1, NULL, 3]"
        ])]
    );
    // The largest HUGEINT is i128::MAX, the reserved decimal NaN of PostgreSQL's numeric:
    // here it is the number it is.
    assert_eq!(
        texts(
            &*conn,
            "SELECT 170141183460469231731687303715884105727::HUGEINT, \
             -170141183460469231731687303715884105727::HUGEINT"
        )
        .await,
        [row(&[
            "170141183460469231731687303715884105727",
            "-170141183460469231731687303715884105727"
        ])]
    );
}
