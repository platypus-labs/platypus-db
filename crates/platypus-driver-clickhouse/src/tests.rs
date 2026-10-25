//! Against the `clickhouse` service of `docker/compose.yml`; skipped when it is not running.
//! `PLATYPUS_TEST_CLICKHOUSE_URL` points elsewhere and makes the server required.

use arrow_array::RecordBatch;
use arrow_cast::display::array_value_to_string;
use futures::TryStreamExt;
use platypus_core::{Connection, ConnectionConfig, Driver, Error, ExecOptions, ExecResult};

use super::*;

const DEFAULT_URL: &str = "clickhouse://platypus:platypus@localhost:58123/platypus";

async fn connect() -> Option<Box<dyn Connection>> {
    let (url, required) = match std::env::var("PLATYPUS_TEST_CLICKHOUSE_URL") {
        Ok(url) => (url, true),
        Err(_) => (DEFAULT_URL.to_owned(), false),
    };
    let config = ConnectionConfig::from_url(&url).unwrap();
    match ClickHouseDriver.connect(&config).await {
        Ok(conn) => Some(conn),
        Err(e) if !required => {
            eprintln!("skipping clickhouse: {e}");
            None
        }
        Err(e) => panic!("cannot connect: {e}"),
    }
}

async fn rows(conn: &dyn Connection, sql: &str, batch_size: usize) -> Vec<RecordBatch> {
    let options = ExecOptions {
        batch_size,
        ..ExecOptions::default()
    };
    match conn.execute(sql, &options).await.unwrap() {
        ExecResult::Rows(stream) => stream.try_collect().await.unwrap(),
        ExecResult::Affected { .. } => panic!("{sql}: no rows"),
    }
}

fn text(batch: &RecordBatch, column: usize, row: usize) -> String {
    array_value_to_string(batch.column(column), row).unwrap()
}

#[tokio::test]
async fn the_session_keeps_settings_database_and_temporary_tables() {
    let Some(conn) = connect().await else { return };
    let conn = &*conn;
    let current = rows(conn, "SELECT currentDatabase()", 10).await;
    assert_eq!(text(&current[0], 0, 0), "platypus");
    conn.execute("SET max_threads = 3", &ExecOptions::default())
        .await
        .unwrap();
    let setting = rows(conn, "SELECT getSetting('max_threads')", 10).await;
    assert_eq!(text(&setting[0], 0, 0), "3");
    let create = conn
        .execute(
            "CREATE TEMPORARY TABLE kept (id UInt8);",
            &ExecOptions::default(),
        )
        .await
        .unwrap();
    assert!(matches!(create, ExecResult::Affected { rows: 0 }));
    let insert = conn
        .execute("INSERT INTO kept VALUES (1), (2)", &ExecOptions::default())
        .await
        .unwrap();
    assert!(
        matches!(insert, ExecResult::Affected { rows: 2 }),
        "{insert:?}"
    );
    let count = rows(conn, "SELECT count() FROM kept", 10).await;
    assert_eq!(text(&count[0], 0, 0), "2");
}

#[tokio::test]
async fn decodes_types_and_slices_blocks() {
    let Some(conn) = connect().await else { return };
    let batches = rows(
        &*conn,
        "SELECT toInt64(number) AS n, toString(number) AS s, toNullable(toDate('2024-03-01')) AS d,
                toDecimal64(1.5, 2) AS dec, [1, 2] AS arr, toBool(number % 2 = 0) AS even
         FROM numbers(2500)",
        1000,
    )
    .await;
    let sizes: Vec<usize> = batches.iter().map(RecordBatch::num_rows).collect();
    assert_eq!(sizes.iter().sum::<usize>(), 2500);
    assert!(sizes.iter().all(|&n| n <= 1000), "{sizes:?}");
    let first = &batches[0];
    assert_eq!(text(first, 0, 1), "1");
    assert_eq!(text(first, 1, 1), "1");
    assert_eq!(text(first, 2, 0), "2024-03-01");
    assert_eq!(text(first, 3, 0), "1.50");
    assert_eq!(text(first, 4, 0), "[1, 2]");
    assert_eq!(text(first, 5, 0), "true");
    let empty = rows(&*conn, "SELECT 1 AS x WHERE 0", 10).await;
    assert!(empty.iter().all(|b| b.num_rows() == 0));
}

#[tokio::test]
async fn text_formats_come_back_as_lines() {
    let Some(conn) = connect().await else { return };
    let batches = rows(&*conn, "SELECT number FROM numbers(3) FORMAT CSV", 10).await;
    assert_eq!(batches[0].schema().field(0).name(), "CSV");
    assert_eq!(batches[0].num_rows(), 3);
    assert_eq!(text(&batches[0], 0, 2), "2");
}

#[tokio::test]
async fn errors_carry_code_and_position_and_manual_commit_is_refused() {
    let Some(conn) = connect().await else { return };
    match conn
        .execute("SELECT * FROMM t", &ExecOptions::default())
        .await
    {
        Err(Error::Database(e)) => {
            assert_eq!(e.code.as_deref(), Some("62"));
            // `FROMM` reads as an alias of `*`; the error is at `t`.
            assert_eq!(e.position, Some(16));
        }
        other => panic!("{other:?}"),
    }
    // An error in the middle of the stream, after rows were sent.
    let options = ExecOptions {
        batch_size: 10,
        ..ExecOptions::default()
    };
    let ExecResult::Rows(stream) = conn
        .execute(
            "SELECT throwIf(number = 300000, 'boom') FROM numbers(1000000) \
             SETTINGS max_block_size = 1000",
            &options,
        )
        .await
        .unwrap()
    else {
        panic!("expected rows");
    };
    match stream.try_collect::<Vec<_>>().await {
        Err(Error::Database(e)) => assert!(e.message.contains("boom"), "{e}"),
        other => panic!("{:?}", other.map(|b| b.len())),
    }
    assert!(conn.set_autocommit(false).await.is_err());
    assert!(!conn.capabilities().transactions);
    let one = rows(&*conn, "SELECT 1", 10).await;
    assert_eq!(text(&one[0], 0, 0), "1");
}

#[tokio::test]
async fn types_arrow_lacks_come_back_as_text_and_datetimes_as_timestamps() {
    let Some(conn) = connect().await else { return };
    let b = rows(
        &*conn,
        "SELECT toDateTime('2024-03-01 10:00:00', 'UTC') AS dt,
                toDateTime64('2024-03-01 10:00:00.123', 3, 'Europe/Rome') AS dt64,
                toUUID('ad3b8a2b-49e3-4aa1-8a88-49803355b74a') AS u,
                CAST('b' AS Enum8('a' = 1, 'b' = 2)) AS e, toIPv4('1.2.3.4') AS ip,
                toInt128(-5) AS big, toFixedString('ab', 2) AS fs, [1, 2] AS arr",
        10,
    )
    .await;
    let batch = &b[0];
    let expected = [
        "2024-03-01T10:00:00Z",
        "2024-03-01T09:00:00.123Z",
        "ad3b8a2b-49e3-4aa1-8a88-49803355b74a",
        "b",
        "1.2.3.4",
        "-5",
        "ab",
        "[1, 2]",
    ];
    for (i, want) in expected.iter().enumerate() {
        assert_eq!(
            &text(batch, i, 0),
            want,
            "{}",
            batch.schema().field(i).name()
        );
    }
    let schema = batch.schema();
    let db_type = schema.field(2).metadata().get(platypus_core::META_DB_TYPE);
    assert_eq!(db_type.map(String::as_str), Some("UUID"));
}
