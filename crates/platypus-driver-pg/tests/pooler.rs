//! Through `PgBouncer` in transaction mode, as `docker/pgbouncer` runs it: each transaction may
//! get another server session, and the session is reset after it (`DISCARD ALL`), so a
//! statement prepared with a name in one round trip is gone at the next.
//!
//! Set `PLATYPUS_TEST_PGBOUNCER_URL` to require the pooler; without it the tests are skipped
//! when the default one is unreachable.

use futures::TryStreamExt;
use platypus_core::{Connection, ConnectionConfig, ExecOptions, ExecResult};
use platypus_driver_pg::PgConnection;

const DEFAULT_URL: &str = "postgres://platypus:platypus@localhost:56432/platypus?sslmode=disable";

async fn connect(pooler: Option<&str>) -> Option<PgConnection> {
    let (url, required) = match std::env::var("PLATYPUS_TEST_PGBOUNCER_URL") {
        Ok(url) => (url, true),
        Err(_) => (DEFAULT_URL.to_owned(), false),
    };
    let mut config = ConnectionConfig::from_url(&url).unwrap();
    if let Some(mode) = pooler {
        config
            .options
            .insert(platypus_core::POOLER.to_owned(), mode.to_owned());
    }
    match PgConnection::connect(&config).await {
        Ok(conn) => Some(conn),
        Err(e) if !required => {
            eprintln!("skipping: PgBouncer unreachable at {DEFAULT_URL}: {e}");
            None
        }
        Err(e) => panic!("cannot connect to PLATYPUS_TEST_PGBOUNCER_URL: {e}"),
    }
}

/// The rows of `sql` and its column names.
async fn rows(conn: &PgConnection, sql: &str, options: &ExecOptions) -> (usize, Vec<String>) {
    match conn.execute(sql, options).await {
        Ok(ExecResult::Rows(stream)) => {
            let columns = stream
                .schema()
                .fields()
                .iter()
                .map(|f| f.name().clone())
                .collect();
            let batches: Vec<_> = stream
                .try_collect()
                .await
                .unwrap_or_else(|e| panic!("{sql}: {e}"));
            (
                batches
                    .iter()
                    .map(platypus_core::arrow_array::RecordBatch::num_rows)
                    .sum(),
                columns,
            )
        }
        Ok(other) => panic!("{sql}: expected rows, got {other:?}"),
        Err(e) => panic!("{sql}: {e}"),
    }
}

#[tokio::test]
async fn named_statements_break_without_pooler_mode() {
    let Some(conn) = connect(None).await else {
        return;
    };
    let error = conn
        .execute("SELECT 1", &ExecOptions::default())
        .await
        .expect_err("PgBouncer resets the session between prepare and run");
    assert!(error.to_string().contains("prepared statement"), "{error}");
}

#[tokio::test]
async fn runs_statements_through_a_transaction_pooler() {
    let Some(conn) = connect(Some("transaction")).await else {
        return;
    };
    let options = ExecOptions::default();
    assert_eq!(
        rows(&conn, "SELECT 1 AS one", &options).await,
        (1, vec!["one".to_owned()])
    );
    let (count, columns) = rows(
        &conn,
        "SELECT g, g::text AS t FROM generate_series(1, 2500) g",
        &options,
    )
    .await;
    assert_eq!(
        (count, columns),
        (2500, vec!["g".to_owned(), "t".to_owned()])
    );
    // An empty result keeps its columns.
    let (count, columns) = rows(
        &conn,
        "SELECT id, country FROM demo.customers WHERE false",
        &options,
    )
    .await;
    assert_eq!(
        (count, columns.as_slice()),
        (0, ["id".to_owned(), "country".to_owned()].as_slice())
    );
    // A statement without rows reports its count.
    match conn
        .execute(
            "UPDATE demo.customers SET country = country WHERE id < 0",
            &options,
        )
        .await
        .unwrap()
    {
        ExecResult::Affected { rows } => assert_eq!(rows, 0),
        rows @ ExecResult::Rows(_) => panic!("{rows:?}"),
    }
    // Errors stay errors, and the session goes on.
    assert!(
        conn.execute("SELECT * FROM no_such_table", &options)
            .await
            .is_err()
    );
    assert_eq!(rows(&conn, "SELECT 2", &options).await.0, 1);

    // Paged results read through a cursor in a transaction of their own.
    let paged = ExecOptions {
        batch_size: 100,
        paged: true,
    };
    assert_eq!(
        rows(&conn, "SELECT g FROM generate_series(1, 1000) g", &paged)
            .await
            .0,
        1000
    );

    // In manual commit, the transaction holds one server session: prepared statements live.
    conn.set_autocommit(false).await.unwrap();
    assert_eq!(rows(&conn, "SELECT 3", &options).await.0, 1);
    assert_eq!(rows(&conn, "SELECT 4", &options).await.0, 1);
    conn.rollback().await.unwrap();
    conn.set_autocommit(true).await.unwrap();

    // The navigator's metadata.
    assert!(
        conn.schemas()
            .await
            .unwrap()
            .iter()
            .any(|s| s.name == "demo")
    );
    let details = conn.table_details("demo", "customers").await.unwrap();
    assert!(details.columns.iter().any(|c| c.name == "id"));
    assert_ne!(
        conn.catalog_columns().await.unwrap(),
        [] as [platypus_core::CatalogColumn; 0]
    );
}

#[tokio::test]
async fn reads_types_of_the_user_through_a_transaction_pooler() {
    let Some(conn) = connect(Some("transaction")).await else {
        return;
    };
    let options = ExecOptions::default();
    // A type of the user's: the client looks it up in the catalog, unnamed too.
    conn.execute("DROP TYPE IF EXISTS demo.pool_mood", &options)
        .await
        .unwrap();
    conn.execute("CREATE TYPE demo.pool_mood AS ENUM ('ok', 'sad')", &options)
        .await
        .unwrap();
    for _ in 0..3 {
        assert_eq!(
            rows(&conn, "SELECT 'sad'::demo.pool_mood AS mood", &options)
                .await
                .0,
            1
        );
    }
    // Another type of the user's, once the client's lookups were prepared on another
    // server session.
    conn.execute("DROP TYPE IF EXISTS demo.pool_size", &options)
        .await
        .unwrap();
    conn.execute("CREATE TYPE demo.pool_size AS ENUM ('s', 'm')", &options)
        .await
        .unwrap();
    assert_eq!(
        rows(&conn, "SELECT 'm'::demo.pool_size AS size", &options)
            .await
            .0,
        1
    );
    conn.execute("DROP TYPE demo.pool_size", &options)
        .await
        .unwrap();
    conn.execute("DROP TYPE demo.pool_mood", &options)
        .await
        .unwrap();
}
