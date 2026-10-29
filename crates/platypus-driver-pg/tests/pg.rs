//! Integration tests against the dev PostgreSQL from `docker/compose.yml`.
//!
//! Set `PLATYPUS_TEST_PG_URL` to require a database (CI does); without it the tests are
//! skipped when the default local database is unreachable.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use arrow_array::RecordBatch;
use arrow_cast::display::array_value_to_string;
use arrow_schema::{DataType, IntervalUnit, TimeUnit};
use futures::{StreamExt, TryStreamExt};
use platypus_core::{
    Connection, ConnectionConfig, Error, ExecOptions, ExecResult, META_DB_TYPE, RecordBatchStream,
};
use platypus_driver_pg::PgConnection;

const DEFAULT_URL: &str = "postgres://platypus:platypus@localhost:55432/platypus?sslmode=disable";

async fn connect() -> Option<PgConnection> {
    let (url, required) = match std::env::var("PLATYPUS_TEST_PG_URL") {
        Ok(url) => (url, true),
        Err(_) => (DEFAULT_URL.to_owned(), false),
    };
    let config = ConnectionConfig::from_url(&url).unwrap();
    match PgConnection::connect(&config).await {
        Ok(conn) => Some(conn),
        Err(e) if !required => {
            eprintln!("skipping: PostgreSQL unreachable at {DEFAULT_URL}: {e}");
            None
        }
        Err(e) => panic!("cannot connect to PLATYPUS_TEST_PG_URL: {e}"),
    }
}

async fn rows(conn: &PgConnection, sql: &str) -> RecordBatchStream {
    match conn.execute(sql, &ExecOptions::default()).await.unwrap() {
        ExecResult::Rows(stream) => stream,
        other @ ExecResult::Affected { .. } => panic!("expected rows, got {other:?}"),
    }
}

async fn collect(conn: &PgConnection, sql: &str) -> Vec<RecordBatch> {
    rows(conn, sql).await.try_collect().await.unwrap()
}

/// Column name -> (Arrow type, db type label, rendered value of `row`).
fn row_values(batch: &RecordBatch, row: usize) -> HashMap<String, (DataType, String, String)> {
    batch
        .schema()
        .fields()
        .iter()
        .zip(batch.columns())
        .map(|(field, column)| {
            let value = if column.is_null(row) {
                "NULL".to_owned()
            } else {
                array_value_to_string(column, row).unwrap()
            };
            let label = field
                .metadata()
                .get(META_DB_TYPE)
                .cloned()
                .unwrap_or_default();
            (
                field.name().clone(),
                (field.data_type().clone(), label, value),
            )
        })
        .collect()
}

#[tokio::test]
async fn reports_server_version() {
    let Some(conn) = connect().await else { return };
    let info = conn.server_info();
    assert_eq!(info.product, "PostgreSQL");
    assert!(
        info.version.starts_with("17"),
        "unexpected version {}",
        info.version
    );
}

#[tokio::test]
async fn honours_ssl_modes() {
    let Some(plain) = connect().await else { return };
    assert!(!plain.server_info().encrypted);

    let url = std::env::var("PLATYPUS_TEST_PG_URL").unwrap_or_else(|_| DEFAULT_URL.to_owned());
    let with_mode = |mode: &str| {
        let mut config = ConnectionConfig::from_url(&url).unwrap();
        config.ssl_mode = mode.parse().unwrap();
        config
    };

    for mode in ["prefer", "require"] {
        let conn = PgConnection::connect(&with_mode(mode)).await.unwrap();
        assert!(conn.server_info().encrypted, "{mode} should negotiate TLS");
        // Cancel requests go through TLS too.
        conn.cancel_handle().cancel().await.unwrap();
    }

    // The dev server uses a self-signed certificate the OS does not trust.
    for mode in ["verify-ca", "verify-full"] {
        let err = PgConnection::connect(&with_mode(mode)).await.unwrap_err();
        assert!(
            matches!(&err, Error::Connect(msg) if msg.contains("certificate")),
            "{mode}: {err:?}"
        );
    }
}

#[tokio::test]
async fn maps_every_type_in_type_zoo() {
    let Some(conn) = connect().await else { return };
    let batches = collect(&conn, "SELECT * FROM demo.type_zoo ORDER BY id").await;
    assert_eq!(batches.len(), 1);
    let batch = &batches[0];
    assert_eq!(batch.num_rows(), 2);

    let v = row_values(batch, 0);
    let check = |name: &str, data_type: DataType, label: &str, value: &str| {
        let (dt, lbl, val) = &v[name];
        assert_eq!(dt, &data_type, "{name}: arrow type");
        assert_eq!(lbl, label, "{name}: db type label");
        assert_eq!(val, value, "{name}: value");
    };
    check("c_bool", DataType::Boolean, "bool", "true");
    check("c_int2", DataType::Int16, "int2", "32767");
    check("c_int4", DataType::Int32, "int4", "2147483647");
    check("c_int8", DataType::Int64, "int8", "9223372036854775807");
    check("c_float4", DataType::Float32, "float4", "3.14");
    check("c_float8", DataType::Float64, "float8", "2.718281828459045");
    check(
        "c_numeric",
        DataType::Decimal128(20, 6),
        "numeric(20,6)",
        "12345678901234.123456",
    );
    check(
        "c_numeric_big",
        DataType::Utf8,
        "numeric",
        "123456789012345678901234567890123456789012.5",
    );
    check("c_text", DataType::Utf8, "text", "hello 🦆");
    check("c_varchar", DataType::Utf8, "varchar(50)", "varchar");
    check("c_char", DataType::Utf8, "char(5)", "abc  ");
    check("c_bytea", DataType::Binary, "bytea", "deadbeef");
    check("c_date", DataType::Date32, "date", "2026-01-31");
    check(
        "c_time",
        DataType::Time64(TimeUnit::Microsecond),
        "time",
        "13:37:00",
    );
    check("c_timetz", DataType::Utf8, "timetz", "13:37:00+02");
    check(
        "c_timestamp",
        DataType::Timestamp(TimeUnit::Microsecond, None),
        "timestamp",
        "2026-01-31T13:37:00",
    );
    // Stored as 13:37+02, i.e. 11:37 UTC.
    check(
        "c_timestamptz",
        DataType::Timestamp(TimeUnit::Microsecond, Some("+00:00".into())),
        "timestamptz",
        "2026-01-31T11:37:00Z",
    );
    check(
        "c_interval",
        DataType::Interval(IntervalUnit::MonthDayNano),
        "interval",
        "1 days 2 hours",
    );
    check(
        "c_uuid",
        DataType::Utf8,
        "uuid",
        "a0eebc99-9c0b-4ef8-bb6d-6bb9bd380a11",
    );
    check("c_json", DataType::Utf8, "json", r#"{"a": 1}"#);
    check("c_jsonb", DataType::Utf8, "jsonb", r#"{"b": [1, 2]}"#);
    check("c_int_array", DataType::Utf8, "int4[]", "{1,2,3}");
    check("c_text_array", DataType::Utf8, "text[]", "{x,y}");
    check("c_inet", DataType::Utf8, "inet", "192.168.1.1");
    check("c_mood", DataType::Utf8, "mood", "happy");
    check("c_point", DataType::Utf8, "point", "(1,2)");
    check("c_int4range", DataType::Utf8, "int4range", "[1,10)");

    // Second row: everything but the id is NULL.
    let nulls = row_values(batch, 1);
    for (name, (_, _, value)) in nulls {
        if name != "id" {
            assert_eq!(value, "NULL", "{name} should be NULL");
        }
    }
}

#[tokio::test]
async fn streams_big_table_in_batches() {
    let Some(conn) = connect().await else { return };
    let options = ExecOptions {
        batch_size: 4096,
        ..ExecOptions::default()
    };
    let ExecResult::Rows(mut stream) = conn
        .execute("SELECT * FROM demo.big_table", &options)
        .await
        .unwrap()
    else {
        panic!("expected rows");
    };
    let mut total = 0;
    let mut batches = 0;
    while let Some(batch) = stream.next().await {
        let batch = batch.unwrap();
        assert!(batch.num_rows() <= 4096);
        total += batch.num_rows();
        batches += 1;
    }
    assert_eq!(total, 1_000_000);
    assert_eq!(batches, 1_000_000_usize.div_ceil(4096));
}

#[tokio::test]
async fn reports_affected_rows() {
    let Some(conn) = connect().await else { return };
    let opts = ExecOptions::default();
    let r = conn
        .execute("CREATE TEMP TABLE t (x int)", &opts)
        .await
        .unwrap();
    assert!(matches!(r, ExecResult::Affected { rows: 0 }), "{r:?}");
    let r = conn
        .execute("INSERT INTO t VALUES (1), (2), (3)", &opts)
        .await
        .unwrap();
    assert!(matches!(r, ExecResult::Affected { rows: 3 }), "{r:?}");
    let r = conn
        .execute("UPDATE t SET x = x + 1 WHERE x > 1", &opts)
        .await
        .unwrap();
    assert!(matches!(r, ExecResult::Affected { rows: 2 }), "{r:?}");

    // RETURNING turns DML into a row-returning statement.
    let batches = collect(&conn, "DELETE FROM t WHERE x = 1 RETURNING x").await;
    assert_eq!(batches.iter().map(RecordBatch::num_rows).sum::<usize>(), 1);
}

#[tokio::test]
async fn reports_syntax_error_position() {
    let Some(conn) = connect().await else { return };
    let err = conn
        .execute("SELECT 1 FROM WHERE", &ExecOptions::default())
        .await
        .unwrap_err();
    let Error::Database(db) = err else {
        panic!("expected database error, got {err:?}")
    };
    assert_eq!(db.code.as_deref(), Some("42601"));
    assert_eq!(db.position, Some(15));
}

#[tokio::test]
async fn reports_error_positions_of_paged_reads_in_the_statement() {
    let Some(conn) = connect().await else { return };
    let paged = ExecOptions {
        paged: true,
        ..ExecOptions::default()
    };
    // Outside and inside a transaction: the cursor prefix differs.
    for begin in [false, true] {
        if begin {
            conn.execute("BEGIN", &ExecOptions::default())
                .await
                .unwrap();
        }
        let Err(Error::Database(db)) = conn
            .execute("\n  SELECT nope FROM demo.big_table", &paged)
            .await
        else {
            panic!("expected a database error");
        };
        assert_eq!(db.position, Some(11), "in transaction: {begin}");
        if begin {
            conn.execute("ROLLBACK", &ExecOptions::default())
                .await
                .unwrap();
        }
    }
}

#[tokio::test]
async fn cancels_running_query() {
    let Some(conn) = connect().await else { return };
    // The server only flushes once the statement produces output, so `execute` itself can
    // block on a slow query: the handle has to be armed before calling it.
    let handle = conn.cancel_handle();
    let canceller = tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(300)).await;
        handle.cancel().await
    });
    let started = Instant::now();
    let err = match conn
        .execute("SELECT pg_sleep(30)", &ExecOptions::default())
        .await
    {
        Err(e) => e,
        Ok(ExecResult::Rows(mut stream)) => stream.next().await.unwrap().unwrap_err(),
        Ok(other) => panic!("unexpected {other:?}"),
    };
    canceller.await.unwrap().unwrap();
    assert!(matches!(err, Error::Cancelled), "{err:?}");
    assert!(started.elapsed() < Duration::from_secs(5));

    // The connection is still usable.
    let batches = collect(&conn, "SELECT 1").await;
    assert_eq!(batches[0].num_rows(), 1);
}

#[tokio::test]
async fn dropping_a_stream_cancels_it_without_hitting_the_next_query() {
    let Some(conn) = connect().await else { return };
    let mut stream = rows(&conn, "SELECT g FROM generate_series(1, 200000000) g").await;
    stream.next().await.unwrap().unwrap();
    drop(stream);

    let started = Instant::now();
    let batches = tokio::time::timeout(Duration::from_secs(10), collect(&conn, "SELECT 42"))
        .await
        .expect("next query blocked by the abandoned one");
    assert_eq!(
        array_value_to_string(batches[0].column(0), 0).unwrap(),
        "42"
    );
    assert!(started.elapsed() < Duration::from_secs(5));
}

/// Whether a transaction is open on the server: two statements then share a transaction id.
async fn in_transaction(conn: &PgConnection) -> bool {
    let xid = async || {
        let batches = collect(conn, "SELECT txid_current()").await;
        array_value_to_string(batches[0].column(0), 0).unwrap()
    };
    xid().await == xid().await
}

#[tokio::test]
async fn paged_reads_end_their_own_transaction() {
    let Some(conn) = connect().await else { return };
    let paged = ExecOptions {
        batch_size: 500,
        paged: true,
    };
    let read = |sql| conn.execute(sql, &paged);

    // Read in full, a page at a time.
    let ExecResult::Rows(stream) = read("SELECT * FROM demo.big_table WHERE id <= 1234")
        .await
        .unwrap()
    else {
        panic!("expected rows");
    };
    let batches: Vec<RecordBatch> = stream.try_collect().await.unwrap();
    assert_eq!(
        batches.iter().map(RecordBatch::num_rows).sum::<usize>(),
        1234
    );
    assert!(batches.iter().all(|b| b.num_rows() <= 500));
    assert!(!in_transaction(&conn).await, "committed at the end");

    // Abandoned after a page: the next statement runs at once, outside any transaction.
    let ExecResult::Rows(mut stream) = read("SELECT * FROM demo.big_table").await.unwrap() else {
        panic!("expected rows");
    };
    assert_eq!(stream.next().await.unwrap().unwrap().num_rows(), 500);
    drop(stream);
    let started = Instant::now();
    assert!(!in_transaction(&conn).await, "closed when dropped");
    assert!(started.elapsed() < Duration::from_secs(2));
    assert!(!conn.transaction().open);

    // A failing read leaves the session usable.
    let ExecResult::Rows(stream) = read("SELECT 1 / (g - 600) FROM generate_series(1, 1000) g")
        .await
        .unwrap()
    else {
        panic!("expected rows");
    };
    let failed: Result<Vec<RecordBatch>, _> = stream.try_collect().await;
    assert!(failed.is_err(), "division by zero on the second page");
    assert!(!in_transaction(&conn).await, "rolled back after the error");
}

#[tokio::test]
async fn paged_reads_return_once_the_query_has_run() {
    let Some(conn) = connect().await else { return };
    let paged = ExecOptions {
        paged: true,
        ..ExecOptions::default()
    };
    let started = Instant::now();
    let result = conn.execute("SELECT pg_sleep(0.3), 1", &paged).await;
    assert!(matches!(result, Ok(ExecResult::Rows(_))));
    assert!(
        started.elapsed() >= Duration::from_millis(300),
        "returned before running"
    );
    // An error on the first rows comes from the statement itself.
    assert!(conn.execute("SELECT 1 / 0", &paged).await.is_err());
    assert!(!in_transaction(&conn).await);
}

#[tokio::test]
async fn bulk_loads_rows_with_copy() {
    let Some(conn) = connect().await else { return };
    let options = ExecOptions::default();
    conn.execute("DROP TABLE IF EXISTS pg_bulk", &options)
        .await
        .unwrap();
    conn.execute(
        "CREATE TABLE pg_bulk (id int, note text, raw bytea, ok boolean)",
        &options,
    )
    .await
    .unwrap();
    let columns = ["id", "note", "raw", "ok"].map(str::to_owned);
    let rows = vec![
        vec![
            Some("1".to_owned()),
            Some("a \"quoted\", line\nbreak \\ tab\t".to_owned()),
            Some("dead00".to_owned()),
            Some("true".to_owned()),
        ],
        vec![Some("2".to_owned()), Some(String::new()), None, None],
    ];
    let bulk = platypus_core::BulkRows {
        table: "pg_bulk",
        columns: &columns,
        hex: &[false, false, true, false],
        rows: &rows,
    };
    // In manual commit, as a production connection: the load joins its transaction.
    conn.set_autocommit(false).await.unwrap();
    conn.execute("BEGIN", &options).await.unwrap();
    conn.execute("SELECT 1", &options).await.unwrap();
    assert_eq!(conn.bulk_load(&bulk).await.unwrap(), 2);
    conn.commit().await.unwrap();
    let empty = platypus_core::BulkRows { rows: &[], ..bulk };
    assert_eq!(conn.bulk_load(&empty).await.unwrap(), 0);
    let batches = collect(
        &conn,
        "SELECT id, note, encode(raw, 'hex'), ok, note IS NULL FROM pg_bulk ORDER BY id",
    )
    .await;
    let cell = |c: usize, r: usize| array_value_to_string(batches[0].column(c), r).unwrap();
    assert_eq!(cell(1, 0), "a \"quoted\", line\nbreak \\ tab\t");
    assert_eq!(
        (cell(2, 0), cell(3, 0)),
        ("dead00".to_owned(), "true".to_owned())
    );
    assert_eq!(
        (cell(1, 1), cell(4, 1)),
        (String::new(), "false".to_owned()),
        "empty, not NULL"
    );
    // A load that fails leaves the session usable.
    let wrong = ["id", "missing"].map(str::to_owned);
    let failing = platypus_core::BulkRows {
        columns: &wrong,
        hex: &[false, false],
        ..empty
    };
    assert!(conn.bulk_load(&failing).await.is_err());
    conn.rollback().await.unwrap();
    let bad = vec![vec![Some("x".to_owned()), None, None, None]];
    assert!(
        conn.bulk_load(&platypus_core::BulkRows { rows: &bad, ..bulk })
            .await
            .is_err()
    );
    conn.rollback().await.unwrap();
    assert_eq!(
        collect(&conn, "SELECT COUNT(*) FROM pg_bulk").await[0].num_rows(),
        1
    );
    conn.execute("DROP TABLE pg_bulk", &options).await.unwrap();
}
