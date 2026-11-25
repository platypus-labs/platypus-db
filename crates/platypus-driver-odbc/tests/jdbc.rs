//! The JDBC bridge end to end, on an H2 in-memory database. Skipped unless `PLATYPUS_TEST_JAVA`
//! (a `java` program or a Java home) and `PLATYPUS_TEST_JARS` (the path of H2's jar) are set.

use arrow_array::{Array as _, Int64Array, RecordBatch, StringArray};
use futures::{StreamExt as _, TryStreamExt as _};
use platypus_core::{
    Connection, ConnectionConfig, Driver as _, DriverKind, Error, ExecOptions, ExecResult,
};
use platypus_driver_odbc::OdbcDriver;

async fn connect() -> Option<Box<dyn Connection>> {
    let java = std::env::var("PLATYPUS_TEST_JAVA").ok()?;
    let jars = std::env::var("PLATYPUS_TEST_JARS").ok()?;
    let mut config = ConnectionConfig::new(DriverKind::Odbc);
    "sa".clone_into(&mut config.user);
    for (key, value) in [
        ("jdbc.java", java.as_str()),
        ("jdbc.jars", jars.as_str()),
        ("jdbc.url", "jdbc:h2:mem:bridge;DB_CLOSE_DELAY=-1"),
    ] {
        config.options.insert(key.to_owned(), value.to_owned());
    }
    Some(
        OdbcDriver
            .connect(&config)
            .await
            .expect("connects through the bridge"),
    )
}

async fn rows(conn: &dyn Connection, sql: &str) -> Vec<RecordBatch> {
    match conn.execute(sql, &ExecOptions::default()).await {
        Ok(ExecResult::Rows(stream)) => stream.try_collect().await.unwrap(),
        Ok(ExecResult::Affected { .. }) => panic!("{sql}: no rows"),
        Err(e) => panic!("{sql}: {e}"),
    }
}

async fn run(conn: &dyn Connection, sql: &str) -> u64 {
    match conn.execute(sql, &ExecOptions::default()).await {
        Ok(ExecResult::Affected { rows }) => rows,
        Ok(ExecResult::Rows(_)) => panic!("{sql}: rows"),
        Err(e) => panic!("{sql}: {e}"),
    }
}

#[tokio::test]
async fn runs_statements_and_reads_the_catalog() {
    let Some(conn) = connect().await else {
        eprintln!("skipping: PLATYPUS_TEST_JAVA and PLATYPUS_TEST_JARS are not set");
        return;
    };
    assert_eq!(conn.server_info().product, "H2");
    run(
        &*conn,
        "create table t(a int primary key, b varchar(20), c double)",
    )
    .await;
    assert_eq!(
        run(
            &*conn,
            "insert into t values (1, 'one \"q\" \\ x', 1.5), (2, null, null)"
        )
        .await,
        2
    );

    let batches = rows(&*conn, "select a, b from t order by a").await;
    let batch = &batches[0];
    let a = batch
        .column(0)
        .as_any()
        .downcast_ref::<Int64Array>()
        .unwrap();
    let b = batch
        .column(1)
        .as_any()
        .downcast_ref::<StringArray>()
        .unwrap();
    assert_eq!((a.value(0), a.value(1)), (1, 2));
    assert_eq!(b.value(0), "one \"q\" \\ x");
    assert!(b.is_null(1));

    // A database error keeps its SQLSTATE.
    match conn
        .execute("select * from nonexistent", &ExecOptions::default())
        .await
    {
        Err(Error::Database(e)) => assert_eq!(e.code.as_deref(), Some("42S02")),
        other => panic!("expected a database error, got {:?}", other.map(|_| ())),
    }

    let schemas = conn.schemas().await.unwrap();
    assert!(schemas.iter().any(|s| s.name == "PUBLIC"));
    let objects = conn.objects("PUBLIC").await.unwrap();
    assert!(objects.iter().any(|o| o.name == "T"));
    let details = conn.table_details("PUBLIC", "T").await.unwrap();
    assert_eq!(details.columns.len(), 3);
}

#[tokio::test]
async fn a_dropped_result_leaves_the_session_usable() {
    let Some(conn) = connect().await else {
        return;
    };
    // 10 000 rows in batches of 10: the result is dropped after the first batch.
    let options = ExecOptions {
        batch_size: 10,
        ..ExecOptions::default()
    };
    let ExecResult::Rows(mut stream) = conn
        .execute("select x from system_range(1, 10000)", &options)
        .await
        .unwrap()
    else {
        panic!("rows expected");
    };
    assert_eq!(stream.try_next().await.unwrap().unwrap().num_rows(), 10);
    drop(stream);
    let batches = rows(&*conn, "select 42").await;
    let n = batches[0]
        .column(0)
        .as_any()
        .downcast_ref::<Int64Array>()
        .unwrap();
    assert_eq!(n.value(0), 42);
}

#[tokio::test]
async fn commit_and_rollback_end_a_manual_transaction() {
    let Some(conn) = connect().await else {
        return;
    };
    run(&*conn, "create table tx(a int)").await;
    conn.set_autocommit(false).await.unwrap();
    run(&*conn, "insert into tx values (1)").await;
    conn.rollback().await.unwrap();
    run(&*conn, "insert into tx values (2)").await;
    conn.commit().await.unwrap();
    let batches = rows(&*conn, "select count(*) from tx").await;
    let n = batches[0]
        .column(0)
        .as_any()
        .downcast_ref::<Int64Array>()
        .unwrap();
    assert_eq!(n.value(0), 1);
}

#[tokio::test]
async fn cancel_stops_a_statement_that_has_not_returned_yet() {
    let Some(conn) = connect().await else {
        eprintln!("skipping: PLATYPUS_TEST_JAVA and PLATYPUS_TEST_JARS are not set");
        return;
    };
    let cancel = conn.cancel_handle();
    let canceller = tokio::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        cancel.cancel().await
    });
    let started = std::time::Instant::now();
    let outcome = conn
        .execute(
            "select sum(a.x * b.x) from system_range(1, 200000) a, system_range(1, 200000) b",
            &ExecOptions::default(),
        )
        .await;
    canceller.await.unwrap().unwrap();
    assert!(outcome.is_err(), "the statement was not cancelled");
    assert!(started.elapsed() < std::time::Duration::from_secs(20));
    // The session is still there: the bridge cancelled the statement, not the connection.
    assert!(!conn.is_closed());
    let batches = rows(&*conn, "select 1").await;
    assert_eq!(batches[0].num_rows(), 1);
}

#[tokio::test]
async fn cancel_while_rows_arrive_ends_the_result_with_an_error() {
    let Some(conn) = connect().await else {
        eprintln!("skipping: PLATYPUS_TEST_JAVA and PLATYPUS_TEST_JARS are not set");
        return;
    };
    // Many batches: the cancel comes while most of them are still to be sent.
    let outcome = conn
        .execute(
            "select x from system_range(1, 2000000)",
            &ExecOptions::default(),
        )
        .await;
    let Ok(ExecResult::Rows(mut stream)) = outcome else {
        panic!("no rows: {:?}", outcome.err());
    };
    assert!(stream.try_next().await.unwrap().is_some());
    conn.cancel_handle().cancel().await.unwrap();
    // A truncated result must not look whole.
    let mut ended = None;
    while let Some(next) = stream.next().await {
        if let Err(e) = next {
            ended = Some(e);
            break;
        }
    }
    assert!(matches!(ended, Some(Error::Cancelled)), "{ended:?}");
    drop(stream);
    let batches = rows(&*conn, "select 1").await;
    assert_eq!(batches[0].num_rows(), 1);
}
