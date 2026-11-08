//! The scenarios of `conformance.rs` that apply to a database without SQL, on MongoDB:
//! server info, metadata, capabilities, cancel, abandoned results, errors and transactions.
//! MongoDB comes from the `mongo` profile of `docker/compose.yml`, or `PLATYPUS_TEST_MONGO`;
//! the transactions need its replica set `mongo-rs`, or `PLATYPUS_TEST_MONGO_RS`. Without
//! them the tests are skipped.
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

/// A single-node replica set: a standalone server runs no transactions.
const REPLICA_SET_URL: &str = "mongodb://localhost:57018/?replicaSet=rs0";

async fn connect() -> Option<Box<dyn Connection>> {
    connect_to("PLATYPUS_TEST_MONGO", DEFAULT_URL).await
}

async fn connect_to(variable: &str, default: &str) -> Option<Box<dyn Connection>> {
    // Another database alone (`PLATYPUS_TEST_ONLY`): not this one.
    if std::env::var("PLATYPUS_TEST_ONLY").is_ok_and(|only| only != "mongo") {
        return None;
    }
    let (url, required) = match std::env::var(variable) {
        Ok(url) => (url, true),
        Err(_) => (default.to_owned(), false),
    };
    let mut config = ConnectionConfig::from_url(&url).unwrap();
    config.connect_timeout = Some(Duration::from_secs(3));
    match MongoDriver.connect(&config).await {
        Ok(conn) => Some(conn),
        Err(e) if !required => {
            eprintln!("skipping mongo: unreachable at {url}: {e}");
            None
        }
        Err(e) => panic!("cannot connect to {variable}: {e}"),
    }
}

async fn rows(conn: &dyn Connection, src: &str) -> usize {
    let ExecResult::Rows(stream) = conn.execute(src, &ExecOptions::default()).await.unwrap() else {
        panic!("{src}: expected rows");
    };
    let batches: Vec<_> = stream.try_collect().await.unwrap();
    batches
        .iter()
        .map(platypus_core::arrow_array::RecordBatch::num_rows)
        .sum()
}

#[tokio::test]
async fn connects_and_reports_the_server() {
    let Some(conn) = connect().await else { return };
    let info = conn.server_info();
    assert_eq!(info.product, "MongoDB");
    assert_ne!(info.version, "");
    assert!(!info.encrypted);
    assert!(!conn.is_closed());
    assert!(!conn.transaction().open && conn.transaction().autocommit);
}

#[tokio::test]
async fn capabilities_match_optional_methods() {
    let Some(conn) = connect().await else { return };
    let caps = conn.capabilities();
    assert!(caps.cancel);
    // The default server is standalone: no transactions.
    let manual = conn.set_autocommit(false).await;
    assert_eq!(
        matches!(manual, Err(Error::Unsupported(_))),
        !caps.transactions,
        "{manual:?}"
    );
    conn.set_autocommit(true).await.unwrap();
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
    assert_eq!(
        conn.referencing_keys("platypus", "customers")
            .await
            .unwrap(),
        [] as [platypus_core::ReferencingKey; 0]
    );
    assert_ne!(
        conn.catalog_columns().await.unwrap(),
        [] as [platypus_core::CatalogColumn; 0]
    );
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

#[tokio::test]
async fn fills_a_collection_with_fake_documents() {
    use platypus_transfer::fake::{FakeColumn, Generator, fill_collection};
    use platypus_transfer::import::ImportError;

    let Some(conn) = connect().await else { return };
    let conn = &*conn;
    let run = |src: &'static str| async move {
        conn.execute(src, &ExecOptions::default()).await.unwrap();
    };
    run("db.fake_rows.drop()").await;
    run(r#"db.fake_rows.insertOne({ _id: 7, email: "seed@example.com", tags: ["seed"] })"#).await;
    run("db.fake_rows.createIndex({ email: 1 }, { unique: true })").await;
    let column = |name: &str, generator, unique| FakeColumn {
        name: name.to_owned(),
        generator,
        unique,
        data_type: None,
    };
    let columns = vec![
        // A long field takes longs, even the small numbers.
        FakeColumn {
            data_type: Some("long".to_owned()),
            ..column("visits", Generator::Integer { min: 1, max: 9 }, false)
        },
        column("_id", Generator::NextKey, false),
        column("email", Generator::Email, true),
        column("tags", Generator::WordList, false),
        column("address", Generator::Document, false),
        column("created_at", Generator::Date, false),
        column("owner", Generator::ObjectId, false),
    ];
    let mut seen = Vec::new();
    let at = (Some("platypus"), "fake_rows");
    let written = fill_collection(conn, at, columns.clone(), 600, |n| {
        seen.push(n);
        true
    })
    .await
    .unwrap();
    assert_eq!(
        (written, seen),
        (600, vec![500, 600]),
        "a statement per batch"
    );
    assert_eq!(rows(conn, "db.fake_rows.find()").await, 601);
    let typed = "db.fake_rows.find({ _id: { $gt: 7, $lte: 607 }, tags: { $type: 'array' }, \
                 address: { $type: 'object' }, created_at: { $type: 'date' }, \
                 owner: { $type: 'objectId' }, visits: { $type: 'long' } })";
    assert_eq!(rows(conn, typed).await, 600);

    // Another run goes on after the largest key, with unique values of its own.
    fill_collection(conn, (None, "fake_rows"), columns, 10, |_| true)
        .await
        .unwrap();
    assert_eq!(
        rows(conn, "db.fake_rows.find({ _id: { $gt: 607 } })").await,
        10
    );

    // A unique index takes one null; the server's errors arrive as they are.
    let null = vec![column("email", Generator::Null, true)];
    let refused = fill_collection(conn, at, null, 2, |_| true).await;
    assert!(matches!(refused, Err(ImportError::Plan(_))), "{refused:?}");
    let taken = vec![column("_id", Generator::Sequence { start: 7 }, false)];
    let failed = fill_collection(conn, at, taken, 1, |_| true).await;
    assert!(
        matches!(failed, Err(ImportError::Database(_))),
        "{failed:?}"
    );
    assert_eq!(rows(conn, "db.fake_rows.find()").await, 611);

    // No next key for a field whose name a query reads as a path.
    let dotted = vec![column("a.b", Generator::NextKey, false)];
    let refused = fill_collection(conn, at, dotted, 1, |_| true).await;
    assert!(matches!(refused, Err(ImportError::Plan(_))), "{refused:?}");

    // Stop ends before the next statement; asked during the last one, the run is complete.
    let name = || vec![column("email", Generator::Email, true)];
    let stopped = fill_collection(conn, at, name(), 600, |_| false).await;
    assert!(
        matches!(stopped, Err(ImportError::Cancelled)),
        "{stopped:?}"
    );
    assert_eq!(rows(conn, "db.fake_rows.find()").await, 1111);
    let late = fill_collection(conn, at, name(), 10, |_| false).await;
    assert_eq!(late.unwrap(), 10);
    assert_eq!(rows(conn, "db.fake_rows.find()").await, 1121);
    run("db.fake_rows.drop()").await;
}
/// The transaction tests one at a time: one lowers the server's transaction lifetime, which
/// would end the transactions of the others.
static REPLICA_SET: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

/// Two sessions of the replica set in database `platypus`, the first in manual mode, with an
/// empty collection `name`.
async fn manual_pair(name: &str) -> Option<(Box<dyn Connection>, Box<dyn Connection>)> {
    let conn = connect_to("PLATYPUS_TEST_MONGO_RS", REPLICA_SET_URL).await?;
    let other = connect_to("PLATYPUS_TEST_MONGO_RS", REPLICA_SET_URL).await?;
    assert!(conn.capabilities().transactions, "a replica set runs them");
    for (session, statement) in [
        (&conn, "use platypus".to_owned()),
        (&other, "use platypus".to_owned()),
        (&conn, format!("db.{name}.drop()")),
        (&conn, format!("db.createCollection('{name}')")),
    ] {
        session
            .execute(&statement, &ExecOptions::default())
            .await
            .unwrap();
    }
    conn.set_autocommit(false).await.unwrap();
    Some((conn, other))
}

fn state(conn: &dyn Connection) -> (bool, bool) {
    let tx = conn.transaction();
    (tx.autocommit, tx.open)
}

async fn run(conn: &dyn Connection, src: &str) {
    conn.execute(src, &ExecOptions::default()).await.unwrap();
}

#[tokio::test]
async fn commits_and_rolls_back_on_a_replica_set() {
    let _one = REPLICA_SET.lock().await;
    let Some((conn, other)) = manual_pair("tx_commit").await else {
        return;
    };
    let (conn, other) = (&*conn, &*other);
    assert_eq!(state(conn), (false, false), "nothing open yet");
    // Nothing open: both succeed and do nothing.
    conn.commit().await.unwrap();
    conn.rollback().await.unwrap();

    run(conn, "db.tx_commit.insertOne({ _id: 1 })").await;
    assert_eq!(state(conn), (false, true), "the first statement opens it");
    assert_eq!(rows(conn, "db.tx_commit.find()").await, 1, "its own write");
    assert_eq!(rows(other, "db.tx_commit.find()").await, 0, "not another's");
    assert!(
        conn.set_autocommit(true).await.is_err(),
        "no switch while a transaction is open"
    );
    conn.rollback().await.unwrap();
    assert_eq!(state(conn), (false, false));
    assert_eq!(rows(conn, "db.tx_commit.find()").await, 0, "rolled back");
    assert_eq!(state(conn), (false, true), "a read opens one too");
    conn.rollback().await.unwrap();

    run(conn, "db.tx_commit.insertOne({ _id: 2 })").await;
    run(
        conn,
        "db.tx_commit.updateOne({ _id: 2 }, { $set: { n: 1 } })",
    )
    .await;
    assert_eq!(rows(other, "db.tx_commit.find()").await, 0);
    conn.commit().await.unwrap();
    assert_eq!(state(conn), (false, false));
    assert_eq!(
        rows(other, "db.tx_commit.find({ n: 1 })").await,
        1,
        "committed"
    );

    conn.set_autocommit(true).await.unwrap();
    assert_eq!(state(conn), (true, false));
    run(conn, "db.tx_commit.insertOne({ _id: 3 })").await;
    assert_eq!(state(conn), (true, false), "autocommit opens none");
    assert_eq!(rows(other, "db.tx_commit.find()").await, 2);
}

#[tokio::test]
async fn streams_more_than_a_batch_in_a_transaction() {
    let _one = REPLICA_SET.lock().await;
    let Some((conn, other)) = manual_pair("tx_stream").await else {
        return;
    };
    let (conn, other) = (&*conn, &*other);
    let docs: Vec<String> = (0..2500).map(|i| format!("{{ _id: {i} }}")).collect();
    run(
        conn,
        &format!("db.tx_stream.insertMany([{}])", docs.join(", ")),
    )
    .await;
    let options = ExecOptions {
        batch_size: 1000,
        paged: true,
    };
    let ExecResult::Rows(mut stream) = conn
        .execute("db.tx_stream.find().sort({ _id: 1 })", &options)
        .await
        .unwrap()
    else {
        panic!("expected rows");
    };
    let mut sizes = vec![stream.next().await.unwrap().unwrap().num_rows()];
    // Another statement runs between two batches, in the same transaction.
    assert_eq!(
        rows(conn, "db.tx_stream.find({ _id: { $gte: 2400 } })").await,
        100
    );
    while let Some(batch) = stream.next().await {
        sizes.push(batch.unwrap().num_rows());
    }
    assert_eq!(sizes, [1000, 1000, 500]);
    assert_eq!(state(conn), (false, true));
    assert_eq!(rows(other, "db.tx_stream.find()").await, 0);

    // An abandoned result leaves the transaction usable.
    let ExecResult::Rows(mut stream) = conn.execute("db.tx_stream.find()", &options).await.unwrap()
    else {
        panic!("expected rows");
    };
    stream.next().await.unwrap().unwrap();
    drop(stream);
    assert_eq!(rows(conn, "db.tx_stream.find().limit(7)").await, 7);
    conn.rollback().await.unwrap();
    assert_eq!(rows(other, "db.tx_stream.find()").await, 0);
}

#[tokio::test]
async fn metadata_in_manual_mode_leaves_the_transaction_open() {
    let _one = REPLICA_SET.lock().await;
    let Some((conn, other)) = manual_pair("tx_meta").await else {
        return;
    };
    let (conn, other) = (&*conn, &*other);
    run(
        conn,
        "db.tx_meta.insertOne({ _id: 1, email: 'a@example.com' })",
    )
    .await;
    for src in [
        "show dbs",
        "show collections",
        "show users",
        "db.getCollectionNames()",
        "db.getCollectionInfos()",
        "db.stats()",
        "db.serverStatus()",
        "db.runCommand({ ping: 1 })",
        "db.adminCommand({ listDatabases: 1 })",
        "rs.status()",
        "db.tx_meta.getIndexes()",
        "db.tx_meta.stats()",
        "db.tx_meta.estimatedDocumentCount()",
        "db.tx_meta.find().explain()",
        "db.tx_meta.aggregate([{ $indexStats: {} }])",
    ] {
        let result = conn.execute(src, &ExecOptions::default()).await;
        assert!(result.is_ok(), "{src}: {result:?}");
        assert_eq!(state(conn), (false, true), "{src}");
    }
    conn.schemas().await.unwrap();
    conn.objects("platypus").await.unwrap();
    conn.table_details("platypus", "tx_meta").await.unwrap();
    conn.catalog_columns().await.unwrap();
    conn.explain("db.tx_meta.find()", true).await.unwrap();
    // What MongoDB does not run in a transaction is refused, and the transaction stays.
    let drop = conn
        .execute("db.tx_meta.drop()", &ExecOptions::default())
        .await;
    assert!(matches!(drop, Err(Error::Unsupported(_))), "{drop:?}");
    assert_eq!(state(conn), (false, true));
    assert_eq!(rows(conn, "db.tx_meta.find()").await, 1);
    conn.commit().await.unwrap();
    assert_eq!(rows(other, "db.tx_meta.find()").await, 1);
}

#[tokio::test]
async fn a_transaction_the_server_ended_is_closed() {
    let _one = REPLICA_SET.lock().await;
    let Some((conn, other)) = manual_pair("tx_expired").await else {
        return;
    };
    let (conn, other) = (&*conn, &*other);
    let limit = |n: u32| {
        format!("db.adminCommand({{ setParameter: 1, transactionLifetimeLimitSeconds: {n} }})")
    };
    run(other, &limit(1)).await;
    let wait = || tokio::time::sleep(Duration::from_secs(3));

    run(conn, "db.tx_expired.insertOne({ _id: 1 })").await;
    wait().await;
    let statement = conn
        .execute(
            "db.tx_expired.insertOne({ _id: 2 })",
            &ExecOptions::default(),
        )
        .await
        .map(|_| ());
    let after_statement = state(conn);
    run(conn, "db.tx_expired.insertOne({ _id: 3 })").await;
    let reopened = state(conn);
    wait().await;
    let commit = conn.commit().await;
    let after_commit = state(conn);
    run(other, &limit(60)).await;

    let Err(e) = statement else {
        panic!("a statement ran in an expired transaction");
    };
    let message = e.to_string();
    assert!(
        message.contains("1 seconds") && message.contains("discarded"),
        "{message}"
    );
    assert_eq!(after_statement, (false, false), "closed once known");
    assert_eq!(reopened, (false, true), "the next statement opens another");
    let Err(e) = commit else {
        panic!("an expired transaction committed");
    };
    let message = e.to_string();
    assert!(
        message.contains("1 seconds") && message.contains("discarded"),
        "{message}"
    );
    assert_eq!(after_commit, (false, false));
    assert_eq!(
        rows(other, "db.tx_expired.find()").await,
        0,
        "nothing stays"
    );
}
