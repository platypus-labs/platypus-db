//! The scenarios of `conformance.rs` that apply to a database without SQL, on MongoDB:
//! server info, metadata, capabilities, cancel, abandoned results and errors. MongoDB
//! comes from the `mongo` profile of `docker/compose.yml`, or `PLATYPUS_TEST_MONGO`;
//! without it the tests are skipped.
//!
//! Lost connections are left out: the client is a pool that reconnects by itself, so no
//! server-side kill closes the session.

use std::time::Duration;

use futures::{StreamExt, TryStreamExt};
use platypus_core::{
    Connection, ConnectionConfig, Driver, Error, ExecOptions, ExecResult, ObjectKind, ObjectRef,
};
use platypus_driver_mongo::MongoDriver;

const DEFAULT_URL: &str = "mongodb://platypus:platypus@localhost:57017/platypus?authSource=admin";

async fn connect() -> Option<Box<dyn Connection>> {
    let (url, required) = match std::env::var("PLATYPUS_TEST_MONGO") {
        Ok(url) => (url, true),
        Err(_) => (DEFAULT_URL.to_owned(), false),
    };
    let mut config = ConnectionConfig::from_url(&url).unwrap();
    config.connect_timeout = Some(Duration::from_secs(3));
    match MongoDriver.connect(&config).await {
        Ok(conn) => Some(conn),
        Err(e) if !required => {
            eprintln!("skipping mongo: unreachable at {url}: {e}");
            None
        }
        Err(e) => panic!("cannot connect to PLATYPUS_TEST_MONGO: {e}"),
    }
}

async fn rows(conn: &dyn Connection, src: &str) -> usize {
    let ExecResult::Rows(stream) = conn.execute(src, &ExecOptions::default()).await.unwrap() else {
        panic!("{src}: expected rows");
    };
    let batches: Vec<_> = stream.try_collect().await.unwrap();
    batches.iter().map(|b| b.num_rows()).sum()
}

#[tokio::test]
async fn connects_and_reports_the_server() {
    let Some(conn) = connect().await else { return };
    let info = conn.server_info();
    assert_eq!(info.product, "MongoDB");
    assert!(!info.version.is_empty());
    assert!(!info.encrypted);
    assert!(!conn.is_closed());
    assert!(!conn.transaction().open && conn.transaction().autocommit);
}

#[tokio::test]
async fn capabilities_match_optional_methods() {
    let Some(conn) = connect().await else { return };
    let caps = conn.capabilities();
    assert!(caps.cancel && !caps.transactions);
    assert!(matches!(
        conn.set_autocommit(false).await,
        Err(Error::Unsupported(_))
    ));
    let explain = conn.explain("db.customers.find()", false).await;
    assert_eq!(matches!(explain, Err(Error::Unsupported(_))), !caps.explain);
    let object = ObjectRef {
        schema: "platypus",
        name: "customers",
        kind: ObjectKind::Table,
        parent: None,
    };
    let ddl = conn.object_ddl(&object).await;
    assert_eq!(
        matches!(ddl, Err(Error::Unsupported(_))),
        !caps.object_ddl,
        "{ddl:?}"
    );
    let search = conn.search_metadata("customers").await;
    assert_eq!(
        matches!(search, Err(Error::Unsupported(_))),
        !caps.metadata_search
    );
    let databases = conn.databases().await;
    assert_eq!(
        matches!(databases, Err(Error::Unsupported(_))),
        !caps.multiple_databases
    );
}

#[tokio::test]
async fn navigates_metadata() {
    let Some(conn) = connect().await else { return };
    let schemas = conn.schemas().await.unwrap();
    assert!(schemas.iter().any(|s| s.name == "platypus" && s.current));
    let objects = conn.objects("platypus").await.unwrap();
    let names: Vec<_> = objects.iter().map(|o| o.name.as_str()).collect();
    for name in ["customer_totals", "customers", "orders"] {
        assert!(names.contains(&name), "{names:?}");
    }
    assert!(names.windows(2).all(|w| w[0] <= w[1]), "sorted: {names:?}");
    let details = conn.table_details("platypus", "customers").await.unwrap();
    assert!(details.columns.iter().any(|c| c.name == "email"));
    assert!(details.indexes.iter().any(|i| i.primary));
    assert!(
        conn.referencing_keys("platypus", "customers")
            .await
            .unwrap()
            .is_empty()
    );
    assert!(!conn.catalog_columns().await.unwrap().is_empty());
}

#[tokio::test]
async fn cancels_running_queries() {
    let Some(conn) = connect().await else { return };
    let handle = conn.cancel_handle();
    let cancel = tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(500)).await;
        handle.cancel().await
    });
    let endless = "db.customers.find({ $where: 'sleep(60000) || true' })";
    let result = tokio::time::timeout(
        Duration::from_secs(30),
        conn.execute(endless, &ExecOptions::default()),
    )
    .await
    .expect("cancel did not stop the query");
    assert!(matches!(result, Err(Error::Cancelled)), "{result:?}");
    cancel.await.unwrap().unwrap();
    assert_eq!(rows(&*conn, "db.customers.find().limit(1)").await, 1);
}

#[tokio::test]
async fn abandoned_results_free_the_connection() {
    let Some(conn) = connect().await else { return };
    for paged in [false, true] {
        let options = ExecOptions {
            batch_size: 100,
            paged,
        };
        let ExecResult::Rows(mut stream) =
            conn.execute("db.orders.find()", &options).await.unwrap()
        else {
            panic!("expected rows");
        };
        assert_eq!(stream.next().await.unwrap().unwrap().num_rows(), 100);
        drop(stream);
        let n = tokio::time::timeout(
            Duration::from_secs(10),
            rows(&*conn, "db.orders.find().limit(7)"),
        )
        .await
        .expect("connection stuck after drop");
        assert_eq!(n, 7);
    }
}

#[tokio::test]
async fn maps_errors_with_their_position() {
    let Some(conn) = connect().await else { return };
    let Err(Error::Database(e)) = conn
        .execute("db.orders.find({ status: 'paid' ", &ExecOptions::default())
        .await
    else {
        panic!("expected a syntax error");
    };
    assert_eq!(e.position, Some(33));
    let Err(Error::Database(e)) = conn
        .execute(
            "db.orders.aggregate([{ $nope: 1 }])",
            &ExecOptions::default(),
        )
        .await
    else {
        panic!("expected a server error");
    };
    assert!(e.code.is_some() && !e.message.is_empty(), "{e:?}");
}
