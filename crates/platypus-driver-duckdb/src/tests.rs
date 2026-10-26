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
