//! Values of DuckDB's types as they reach the app, and the statement outcomes the worker
//! tells apart. Run on an in-memory database.

use arrow_array::cast::AsArray;
use arrow_schema::{DataType, IntervalUnit, TimeUnit};
use futures::TryStreamExt;
use platypus_core::{
    Connection, ConnectionConfig, DriverKind, Error, ExecOptions, ExecResult, META_DB_TYPE,
};

use super::*;

async fn memory() -> DuckDbConnection {
    let mut config = ConnectionConfig::new(DriverKind::DuckDb);
    config.database = ":memory:".to_owned();
    DuckDbConnection::connect(&config).await.unwrap()
}

async fn rows(conn: &dyn Connection, sql: &str) -> Vec<RecordBatch> {
    match conn.execute(sql, &ExecOptions::default()).await.unwrap() {
        ExecResult::Rows(stream) => stream.try_collect().await.unwrap(),
        other @ ExecResult::Affected { .. } => panic!("{sql}: {other:?}"),
    }
}

/// `(Arrow type, DuckDB type, value as text)` of each column of the one-row `sql`.
async fn decoded(conn: &DuckDbConnection, sql: &str) -> Vec<(DataType, String, String)> {
    let batches = rows(conn, sql).await;
    let batch = &batches[0];
    let options = arrow_cast::display::FormatOptions::default().with_null("NULL");
    batch
        .schema()
        .fields()
        .iter()
        .zip(batch.columns())
        .map(|(field, column)| {
            let text = arrow_cast::display::ArrayFormatter::try_new(column.as_ref(), &options)
                .unwrap()
                .value(0)
                .to_string();
            (
                field.data_type().clone(),
                field.metadata()[META_DB_TYPE].clone(),
                text,
            )
        })
        .collect()
}

#[tokio::test]
async fn decodes_scalar_types() {
    crate::testing::load();
    let conn = memory().await;
    let got = decoded(
        &conn,
        "SELECT 12.50::DECIMAL(12, 2), 170141183460469231731687303715884105727::HUGEINT,
                INTERVAL '1 month 2 days 3 seconds', '\\xDE\\xAD'::BLOB,
                'a1a2a3a4-b1b2-c1c2-d1d2-d3d4d5d6d7d8'::UUID,
                TIMESTAMPTZ '2024-03-01 10:00:00+02', TIMESTAMP '2024-03-01 10:00:00.123456',
                DATE '2024-02-29', NULL::INTEGER, 255::UTINYINT, '{\"a\": 1}'::JSON",
    )
    .await;
    let expect = [
        (DataType::Decimal128(12, 2), "DECIMAL(12,2)", "12.50"),
        (
            DataType::Decimal128(38, 0),
            "HUGEINT",
            "170141183460469231731687303715884105727",
        ),
        (
            DataType::Interval(IntervalUnit::MonthDayNano),
            "INTERVAL",
            "1 mons 2 days 3.000000000 secs",
        ),
        (DataType::Binary, "BLOB", "dead"),
        (
            DataType::Utf8,
            "UUID",
            "a1a2a3a4-b1b2-c1c2-d1d2-d3d4d5d6d7d8",
        ),
        (
            DataType::Timestamp(TimeUnit::Microsecond, Some("+00:00".into())),
            "TIMESTAMP WITH TIME ZONE",
            "2024-03-01T08:00:00Z",
        ),
        (
            DataType::Timestamp(TimeUnit::Microsecond, None),
            "TIMESTAMP",
            "2024-03-01T10:00:00.123456",
        ),
        (DataType::Date32, "DATE", "2024-02-29"),
        (DataType::Int32, "INTEGER", "NULL"),
        (DataType::UInt8, "UTINYINT", "255"),
        (DataType::Utf8, "JSON", "{\"a\": 1}"),
    ];
    assert_eq!(got.len(), expect.len());
    for (got, (data_type, db_type, text)) in got.iter().zip(expect) {
        assert_eq!(
            (&got.0, got.1.as_str(), got.2.as_str()),
            (&data_type, db_type, text)
        );
    }
}

#[tokio::test]
async fn unions_are_the_value_of_their_member() {
    crate::testing::load();
    let conn = memory().await;
    let batches = rows(
        &conn,
        "SELECT un FROM (VALUES (1, 5::UNION(n INTEGER, s VARCHAR)), \
         (2, NULL), (3, 'x'::UNION(n INTEGER, s VARCHAR))) t(id, un) ORDER BY id",
    )
    .await;
    let batch = &batches[0];
    assert_eq!(batch.schema().field(0).data_type(), &DataType::Utf8);
    assert_eq!(
        batch.schema().field(0).metadata()[META_DB_TYPE],
        "UNION(n INTEGER, s VARCHAR)"
    );
    let values: Vec<Option<&str>> = batch.column(0).as_string::<i32>().iter().collect();
    assert_eq!(values, [Some("5"), None, Some("x")]);
}

#[tokio::test]
async fn decodes_nested_types_and_enums() {
    crate::testing::load();
    let conn = memory().await;
    conn.execute(
        "CREATE TYPE mood AS ENUM ('sad', 'happy')",
        &ExecOptions::default(),
    )
    .await
    .unwrap();
    let got = decoded(
        &conn,
        "SELECT [1, NULL, 3], {'name': 'x', 'n': 2}, MAP {'k': 1.5}, 'happy'::mood,
                [[1], []]::INTEGER[][], [1, 2]::INTEGER[2]",
    )
    .await;
    let types: Vec<&str> = got.iter().map(|g| g.1.as_str()).collect();
    assert_eq!(
        types,
        [
            "INTEGER[]",
            "STRUCT(name VARCHAR, n INTEGER)",
            "MAP(VARCHAR, DECIMAL(2,1))",
            "ENUM",
            "INTEGER[][]",
            "INTEGER[]"
        ]
    );
    assert!(matches!(got[0].0, DataType::List(_)));
    assert_eq!(got[0].2, "[1, NULL, 3]");
    assert!(matches!(got[1].0, DataType::Struct(_)));
    assert_eq!(got[1].2, "{name: x, n: 2}");
    assert!(matches!(got[2].0, DataType::Map(..)));
    assert!(matches!(got[3].0, DataType::Dictionary(..)));
    assert_eq!(got[3].2, "happy");
    assert!(matches!(got[5].0, DataType::FixedSizeList(..)));
}

#[tokio::test]
async fn tells_row_counts_from_results() {
    crate::testing::load();
    let conn = memory().await;
    let opts = ExecOptions::default();
    let affected = |r: ExecResult| match r {
        ExecResult::Affected { rows } => rows,
        other @ ExecResult::Rows(_) => panic!("{other:?}"),
    };
    let r = conn
        .execute("CREATE TABLE t (id INT)", &opts)
        .await
        .unwrap();
    assert_eq!(affected(r), 0);
    let r = conn
        .execute("INSERT INTO t VALUES (1), (2)", &opts)
        .await
        .unwrap();
    assert_eq!(affected(r), 2);
    let r = conn
        .execute("COMMENT ON TABLE t IS 'x'", &opts)
        .await
        .unwrap();
    assert_eq!(affected(r), 0);
    // Queries are rows even when they look like a row count.
    let batches = rows(&conn, "SELECT count(*) AS \"Count\" FROM t").await;
    assert_eq!(
        batches[0]
            .column(0)
            .as_primitive::<arrow_array::types::Int64Type>()
            .value(0),
        2
    );
    let batches = rows(&conn, "INSERT INTO t VALUES (3) RETURNING id AS \"Count\"").await;
    assert_eq!(batches[0].num_rows(), 1);
    let batches = rows(&conn, "PRAGMA table_info('t')").await;
    assert_eq!(batches[0].num_rows(), 1);
}

#[tokio::test]
async fn reports_errors_raised_while_fetching() {
    crate::testing::load();
    let conn = memory().await;
    let result = conn
        .execute("SELECT 'x'::INTEGER", &ExecOptions::default())
        .await;
    match result {
        Err(Error::Database(e)) => {
            assert_eq!(e.code.as_deref(), Some("Conversion"), "{e}");
            // DuckDB points at the cast.
            assert_eq!(e.position, Some(11), "{e}");
        }
        other => panic!("{other:?}"),
    }
}

#[tokio::test]
async fn opens_files_and_refuses_missing_folders() {
    crate::testing::load();
    let dir = tempfile::tempdir().unwrap();
    let mut config = ConnectionConfig::new(DriverKind::DuckDb);
    config.database = dir.path().join("new.duckdb").to_string_lossy().into_owned();
    let conn = DuckDbDriver.connect(&config).await.unwrap();
    assert_eq!(conn.server_info().product, "DuckDB");
    let version = &conn.server_info().version;
    assert!(
        version.starts_with(|c: char| c.is_ascii_digit()),
        "{version}"
    );
    // A second session on the same file shares the database instead of failing on its lock.
    let other = DuckDbDriver.connect(&config).await.unwrap();
    other
        .execute("CREATE TABLE shared (id INT)", &ExecOptions::default())
        .await
        .unwrap();
    drop(other);
    let batches = rows(&*conn, "SELECT count(*) FROM shared").await;
    assert_eq!(batches[0].num_rows(), 1);
    drop(conn);
    config.database = dir
        .path()
        .join("missing")
        .join("x.duckdb")
        .to_string_lossy()
        .into_owned();
    assert!(matches!(
        DuckDbDriver.connect(&config).await,
        Err(Error::Connect(_))
    ));
    config.database.clear();
    assert!(matches!(
        DuckDbDriver.connect(&config).await,
        Err(Error::Config(_))
    ));
}

async fn open_file(path: &std::path::Path) -> Box<dyn Connection> {
    let mut config = ConnectionConfig::new(DriverKind::DuckDb);
    config.database = path.to_string_lossy().into_owned();
    DuckDbDriver.connect(&config).await.unwrap()
}

/// The single value of `sql` as text.
async fn value(conn: &dyn Connection, sql: &str) -> String {
    let batches = rows(conn, sql).await;
    arrow_cast::display::array_value_to_string(batches[0].column(0), 0).unwrap()
}

#[tokio::test]
async fn opens_data_files_as_views() {
    crate::testing::load();
    let dir = tempfile::tempdir().unwrap();
    let csv = dir.path().join("sales.csv");
    std::fs::write(&csv, "id,item\n1,pen\n2,ink\n").unwrap();
    let json = dir.path().join("events.ndjson");
    std::fs::write(
        &json,
        "{\"id\": 1, \"ok\": true}\n{\"id\": 2, \"ok\": false}\n",
    )
    .unwrap();
    let parquet = dir.path().join("p.parquet");
    let make = memory().await;
    let copy = format!(
        "COPY (SELECT 42 AS answer) TO '{}' (FORMAT parquet)",
        parquet.display()
    );
    make.execute(&copy, &ExecOptions::default()).await.unwrap();

    let conn = open_file(&csv).await;
    assert_eq!(value(&*conn, "SELECT sum(id) FROM sales").await, "3");
    let objects = conn.objects("main").await.unwrap();
    assert_eq!(objects[0].name, "sales");
    let conn = open_file(&json).await;
    assert_eq!(
        value(&*conn, "SELECT count(*) FROM events WHERE ok").await,
        "1"
    );
    let conn = open_file(&parquet).await;
    assert_eq!(value(&*conn, "SELECT answer FROM p").await, "42");

    let mut config = ConnectionConfig::new(DriverKind::DuckDb);
    config.database = dir
        .path()
        .join("missing.csv")
        .to_string_lossy()
        .into_owned();
    assert!(matches!(
        DuckDbDriver.connect(&config).await,
        Err(Error::Connect(_))
    ));
}

/// A workbook with one sheet, `People`, written by hand: the parts calamine reads.
fn write_xlsx(path: &std::path::Path, alan_age: u32) {
    use std::io::Write as _;
    let sheet = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?><worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><sheetData><row r="1"><c r="A1" t="inlineStr"><is><t>name</t></is></c><c r="B1" t="inlineStr"><is><t>age</t></is></c><c r="C1" t="inlineStr"><is><t>score</t></is></c></row><row r="2"><c r="A2" t="inlineStr"><is><t>Ada</t></is></c><c r="B2"><v>36</v></c><c r="C2"><v>9.5</v></c></row><row r="3"><c r="A3" t="inlineStr"><is><t>Alan</t></is></c><c r="B3"><v>{alan_age}</v></c></row></sheetData></worksheet>"#
    );
    let parts = [
        (
            "[Content_Types].xml",
            r#"<?xml version="1.0" encoding="UTF-8"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/xl/workbook.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml"/><Override PartName="/xl/worksheets/sheet1.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml"/></Types>"#,
        ),
        (
            "_rels/.rels",
            r#"<?xml version="1.0" encoding="UTF-8"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="xl/workbook.xml"/></Relationships>"#,
        ),
        (
            "xl/workbook.xml",
            r#"<?xml version="1.0" encoding="UTF-8"?><workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><sheets><sheet name="People" sheetId="1" r:id="rId1"/></sheets></workbook>"#,
        ),
        (
            "xl/_rels/workbook.xml.rels",
            r#"<?xml version="1.0" encoding="UTF-8"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/sheet1.xml"/></Relationships>"#,
        ),
        ("xl/worksheets/sheet1.xml", sheet.as_str()),
    ];
    let mut zip = zip::ZipWriter::new(std::fs::File::create(path).unwrap());
    let options =
        zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
    for (name, text) in parts {
        zip.start_file(name, options).unwrap();
        zip.write_all(text.as_bytes()).unwrap();
    }
    zip.finish().unwrap();
}

#[tokio::test]
async fn opens_workbook_sheets_as_tables() {
    crate::testing::load();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("book.xlsx");
    write_xlsx(&path, 41);
    let conn = open_file(&path).await;
    assert_eq!(value(&*conn, "SELECT sum(age) FROM People").await, "77");
    // A new connection reads the workbook again once it changed.
    write_xlsx(&path, 42);
    let other = open_file(&path).await;
    assert_eq!(value(&*other, "SELECT sum(age) FROM People").await, "78");
    assert_eq!(
        value(
            &*conn,
            "SELECT typeof(age) || ' ' || typeof(score) FROM People LIMIT 1"
        )
        .await,
        "BIGINT DOUBLE"
    );
    assert_eq!(
        value(&*conn, "SELECT count(*) FROM People WHERE score IS NULL").await,
        "1"
    );
}

#[tokio::test]
async fn named_memory_databases_are_their_own() {
    crate::testing::load();
    let (_database, conn) = crate::memory_database("tests-named").unwrap();
    conn.execute_batch("CREATE TABLE kept AS SELECT 42 AS n")
        .unwrap();
    let mut config = ConnectionConfig::new(DriverKind::DuckDb);
    config.database = crate::memory_path("tests-named");
    let session = DuckDbConnection::connect(&config).await.unwrap();
    let batches = rows(&session, "SELECT n FROM kept").await;
    assert_eq!(
        batches[0]
            .column(0)
            .as_primitive::<arrow_array::types::Int32Type>()
            .value(0),
        42
    );
    // The in-memory database the app's sessions share does not have it.
    let shared = memory().await;
    assert!(
        shared
            .execute("SELECT n FROM kept", &ExecOptions::default())
            .await
            .is_err()
    );
}

#[tokio::test]
async fn a_memory_database_named_like_a_file_is_not_read() {
    crate::testing::load();
    let (_database, conn) = crate::memory_database("tests-named.csv").unwrap();
    conn.execute_batch("CREATE TABLE kept AS SELECT 7 AS n")
        .unwrap();
    let mut config = ConnectionConfig::new(DriverKind::DuckDb);
    config.database = crate::memory_path("tests-named.csv");
    let session = DuckDbConnection::connect(&config).await.unwrap();
    assert_eq!(value(&session, "SELECT n FROM kept").await, "7");
}

#[tokio::test]
async fn attached_databases_are_listed_as_schemas() {
    crate::testing::load();
    let dir = tempfile::tempdir().unwrap();
    let mut config = ConnectionConfig::new(DriverKind::DuckDb);
    config.database = dir
        .path()
        .join("main.duckdb")
        .to_string_lossy()
        .into_owned();
    let conn = DuckDbConnection::connect(&config).await.unwrap();
    let other = dir.path().join("other.duckdb");
    let other = other.to_string_lossy().replace('\\', "/");
    for sql in [
        format!("ATTACH '{other}' AS other"),
        "CREATE TABLE other.o (id INTEGER PRIMARY KEY, v VARCHAR)".to_owned(),
        "CREATE TABLE t (n INTEGER)".to_owned(),
    ] {
        conn.execute(&sql, &ExecOptions::default()).await.unwrap();
    }
    let schemas = conn.schemas().await.unwrap();
    let names: Vec<_> = schemas.iter().map(|s| s.name.as_str()).collect();
    assert!(
        names.contains(&"main") && names.contains(&"other"),
        "{names:?}"
    );
    let objects = conn.objects("other").await.unwrap();
    assert_eq!(
        objects.iter().map(|o| o.name.as_str()).collect::<Vec<_>>(),
        ["o"]
    );
    let details = conn.table_details("other", "o").await.unwrap();
    let columns: Vec<_> = details.columns.iter().map(|c| c.name.as_str()).collect();
    assert_eq!(columns, ["id", "v"]);
    assert!(details.columns[0].primary_key);
    // The current database's own schema is not mixed with it.
    let main = conn.objects("main").await.unwrap();
    assert_eq!(
        main.iter().map(|o| o.name.as_str()).collect::<Vec<_>>(),
        ["t"]
    );
}

/// A connection to an in-memory database of its own, `name`, not the shared `:memory:`.
async fn own_memory(name: &str) -> DuckDbConnection {
    let mut config = ConnectionConfig::new(DriverKind::DuckDb);
    config.database = memory_path(name);
    DuckDbConnection::connect(&config).await.unwrap()
}

/// Runs a statement that returns no rows.
async fn run(conn: &dyn Connection, sql: &str) {
    conn.execute(sql, &ExecOptions::default())
        .await
        .unwrap_or_else(|e| panic!("{sql}: {e}"));
}

/// The appender casts text to each column's type, leaves the columns not named to their
/// default, and finds a table by its quoted, qualified name.
#[tokio::test]
async fn bulk_load_casts_text_and_names_columns() {
    crate::testing::load();
    let conn = own_memory("bulk_casts").await;
    run(&conn, "CREATE SCHEMA \"my s\"").await;
    run(
        &conn,
        "CREATE TABLE \"my s\".\"t\"\"x\" (id INTEGER PRIMARY KEY, amount DECIMAL(10, 2), \
         day DATE, stamp TIMESTAMPTZ, ok BOOLEAN, note VARCHAR DEFAULT 'none', raw BLOB)",
    )
    .await;
    let columns = ["id", "amount", "day", "stamp", "ok", "raw"].map(|c| format!("\"{c}\""));
    let row = |values: [Option<&str>; 6]| values.map(|v| v.map(str::to_owned)).to_vec();
    let loaded = conn
        .bulk_load(&platypus_core::BulkRows {
            table: "\"my s\".\"t\"\"x\"",
            columns: &columns,
            hex: &[false, false, false, false, false, true],
            rows: &[
                row([
                    Some("1"),
                    Some("12.345"),
                    Some("2024-02-29"),
                    Some("2024-03-01 10:00:00+02:00"),
                    Some("true"),
                    Some("00ff"),
                ]),
                row([Some("2"), None, None, None, Some("false"), Some("")]),
            ],
        })
        .await
        .unwrap();
    assert_eq!(loaded, 2);
    let got = decoded(
        &conn,
        "SELECT amount, day, stamp = '2024-03-01 10:00:00+02:00'::TIMESTAMPTZ, ok, note, hex(raw) FROM \"my s\".\"t\"\"x\" \
         WHERE id = 1",
    )
    .await;
    let texts: Vec<_> = got.into_iter().map(|(_, _, text)| text).collect();
    assert_eq!(
        texts,
        ["12.35", "2024-02-29", "true", "true", "none", "00FF"]
    );
}

/// A failed row leaves no rows, also past the 2048 rows the appender writes at a time, in
/// autocommit mode; in a transaction the rollback undoes the load.
#[tokio::test]
async fn bulk_load_is_all_or_nothing() {
    crate::testing::load();
    let conn = own_memory("bulk_atomic").await;
    run(&conn, "CREATE TABLE t (id INTEGER PRIMARY KEY)").await;
    let columns = ["id".to_owned()];
    let ids: Vec<Vec<Option<String>>> = (0..5000).map(|i| vec![Some(i.to_string())]).collect();
    let mut failing = ids.clone();
    failing.push(vec![Some("not a number".to_owned())]);
    let bulk = |rows| platypus_core::BulkRows {
        table: "t",
        columns: &columns,
        hex: &[false],
        rows,
    };
    let count =
        async |conn: &DuckDbConnection| decoded(conn, "SELECT COUNT(*) FROM t").await[0].2.clone();
    assert!(conn.bulk_load(&bulk(&failing)).await.is_err());
    assert_eq!(count(&conn).await, "0");

    conn.set_autocommit(false).await.unwrap();
    assert_eq!(conn.bulk_load(&bulk(&ids)).await.unwrap(), 5000);
    conn.rollback().await.unwrap();
    assert_eq!(count(&conn).await, "0");
    assert_eq!(conn.bulk_load(&bulk(&ids)).await.unwrap(), 5000);
    conn.commit().await.unwrap();
    assert_eq!(count(&conn).await, "5000");
}

/// A bare table name is the one the session's search path finds, as for its statements.
#[tokio::test]
async fn bulk_load_follows_the_search_path() {
    crate::testing::load();
    let conn = own_memory("bulk_search_path").await;
    run(&conn, "CREATE SCHEMA sales").await;
    run(&conn, "SET search_path = 'sales'").await;
    run(&conn, "CREATE TABLE t (id INTEGER)").await;
    let columns = ["id".to_owned()];
    let loaded = conn
        .bulk_load(&platypus_core::BulkRows {
            table: "t",
            columns: &columns,
            hex: &[false],
            rows: &[vec![Some("7".to_owned())]],
        })
        .await
        .unwrap();
    assert_eq!(loaded, 1);
    assert_eq!(decoded(&conn, "SELECT id FROM sales.t").await[0].2, "7");
}

#[tokio::test]
async fn unsigned_128_bit_numbers_keep_their_top_half() {
    crate::testing::load();
    let conn = memory().await;
    let got = decoded(
        &conn,
        "SELECT 340282366920938463463374607431768211455::UHUGEINT,
                170141183460469231731687303715884105728::UHUGEINT, 5::UHUGEINT, NULL::UHUGEINT",
    )
    .await;
    let texts: Vec<_> = got
        .iter()
        .map(|(t, db, text)| (t.clone(), db.as_str(), text.as_str()))
        .collect();
    let wide = DataType::Decimal256(39, 0);
    assert_eq!(
        texts,
        [
            (
                wide.clone(),
                "UHUGEINT",
                "340282366920938463463374607431768211455"
            ),
            (
                wide.clone(),
                "UHUGEINT",
                "170141183460469231731687303715884105728"
            ),
            (wide.clone(), "UHUGEINT", "5"),
            (wide, "UHUGEINT", "NULL"),
        ]
    );
}

#[tokio::test]
async fn times_with_a_zone_keep_their_offset() {
    crate::testing::load();
    let conn = memory().await;
    let got = decoded(
        &conn,
        "SELECT TIMETZ '03:04:05+02' AS t, TIMETZ '03:04:05+02'::VARCHAR AS s",
    )
    .await;
    assert_eq!(got[0].1, "TIME WITH TIME ZONE");
    assert_eq!(got[0].2, "03:04:05+02");
    assert_eq!(got[1].2, "03:04:05+02");
    // Through a CTE with a trailing comment and semicolon, and with other columns around it.
    let got = decoded(
        &conn,
        "WITH x AS (SELECT 1 AS a, TIMETZ '10:00:00-05:30' AS t, 2 AS b) SELECT * FROM x; -- done",
    )
    .await;
    assert_eq!(got[0].2, "1");
    assert_eq!(
        (got[1].1.as_str(), got[1].2.as_str()),
        ("TIME WITH TIME ZONE", "10:00:00-05:30")
    );
    assert_eq!(got[2].2, "2");
    // The query runs once: its sequence moves by one.
    run(&conn, "CREATE SEQUENCE once_seq").await;
    let got = decoded(
        &conn,
        "SELECT nextval('once_seq') AS n, TIMETZ '01:02:03+01' AS t",
    )
    .await;
    assert_eq!((got[0].2.as_str(), got[1].2.as_str()), ("1", "01:02:03+01"));
    let got = decoded(&conn, "SELECT currval('once_seq') AS c").await;
    assert_eq!(got[0].2, "1");
    // A statement that is no plain query keeps its TIMETZ column as it is.
    let got = decoded(&conn, "EXPLAIN SELECT TIMETZ '01:02:03+01'").await;
    assert_eq!(got.len(), 2, "{got:?}");
}

/// The text the grid shows for a value, written back with the data editor's literals, is the
/// value again: the deleted row of an undo script, an edit of a shown value.
#[tokio::test]
async fn shown_values_write_back_as_themselves() {
    use platypus_sql::dml::{Column, ValueKind};
    crate::testing::load();
    let conn = memory().await;
    run(
        &conn,
        "CREATE TABLE src (mp MAP(VARCHAR, INTEGER), mt MAP(VARCHAR, VARCHAR), d DATE,
                           ts TIMESTAMP, u UHUGEINT, z TIMETZ);
         INSERT INTO src VALUES (MAP {'k': 1, 'a b': NULL}, MAP {'x y': 'it''s'},
           DATE '0044-03-15 (BC)', TIMESTAMP '0044-03-15 (BC) 12:00:00.5',
           340282366920938463463374607431768211455::UHUGEINT, TIMETZ '03:04:05+02');
         CREATE TABLE dst AS SELECT * FROM src WHERE false;",
    )
    .await;
    let batches = rows(&conn, "SELECT * FROM src").await;
    let batch = &batches[0];
    let options = arrow_cast::display::FormatOptions::default();
    let literals: Vec<String> = batch
        .schema()
        .fields()
        .iter()
        .zip(batch.columns())
        .map(|(field, column)| {
            let text = platypus_core::special::formatter(column.as_ref(), &options)
                .unwrap()
                .value(0)
                .to_string();
            let column = Column {
                name: field.name(),
                kind: ValueKind::of(field.data_type()),
                default: None,
            };
            column
                .literal(platypus_sql::Dialect::DuckDb, Some(&text))
                .unwrap()
        })
        .collect();
    run(
        &conn,
        &format!("INSERT INTO dst VALUES ({})", literals.join(", ")),
    )
    .await;
    assert_eq!(value(&conn, "SELECT count(*) FROM dst").await, "1");
    assert_eq!(
        value(
            &conn,
            "SELECT count(*) FROM (SELECT * FROM src EXCEPT SELECT * FROM dst)"
        )
        .await,
        "0"
    );
}

/// A column the table fills from an expression is left out of the inserts of a copied row.
#[tokio::test]
async fn generated_columns_are_read_from_the_table_definition() {
    use platypus_sql::generated::{Generated, generated_columns};
    crate::testing::load();
    let conn = memory().await;
    run(
        &conn,
        "CREATE SEQUENCE q4s; CREATE TABLE q4g (id INTEGER DEFAULT nextval('q4s') PRIMARY KEY,
           n INTEGER, g INTEGER GENERATED ALWAYS AS (n * 2), d INTEGER DEFAULT 5)",
    )
    .await;
    let found = generated_columns(&conn, platypus_sql::Dialect::DuckDb, "main", "q4g")
        .await
        .unwrap();
    let found: Vec<_> = found.into_iter().map(|c| (c.name, c.generated)).collect();
    assert_eq!(
        found,
        [
            ("id".to_owned(), Generated::Numbered),
            ("g".to_owned(), Generated::Computed)
        ]
    );
}
