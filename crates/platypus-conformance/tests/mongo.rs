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
    assert_eq!(state(conn), (false, false), "a read opens none");

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
async fn reads_open_no_transaction_and_outlive_its_lifetime() {
    let _one = REPLICA_SET.lock().await;
    let Some((conn, other)) = manual_pair("tx_reads").await else {
        return;
    };
    let (conn, other) = (&*conn, &*other);
    let limit = |n: u32| {
        format!("db.adminCommand({{ setParameter: 1, transactionLifetimeLimitSeconds: {n} }})")
    };
    run(other, "db.tx_reads.insertMany([{ _id: 1 }, { _id: 2 }])").await;
    run(other, &limit(1)).await;
    let options = ExecOptions {
        batch_size: 1,
        paged: true,
    };
    let reads = async {
        assert_eq!(
            rows(conn, "db.tx_reads.find()").await,
            2,
            "what is committed"
        );
        let ExecResult::Rows(mut stream) = conn
            .execute("db.tx_reads.find().sort({ _id: 1 })", &options)
            .await
            .unwrap()
        else {
            panic!("expected rows");
        };
        assert_eq!(stream.next().await.unwrap().unwrap().num_rows(), 1);
        let opened = state(conn);
        // Past the lifetime of a transaction: browsing still works.
        tokio::time::sleep(Duration::from_secs(3)).await;
        let later = conn
            .execute("db.tx_reads.find()", &ExecOptions::default())
            .await
            .map(|_| ());
        let after = state(conn);
        // A write opens one; the result read before it goes on outside it.
        run(conn, "db.tx_reads.insertOne({ _id: 3 })").await;
        let written = state(conn);
        let rest = stream.next().await.map(|b| b.map(|b| b.num_rows()));
        let own = rows(conn, "db.tx_reads.find()").await;
        let others = rows(other, "db.tx_reads.find()").await;
        conn.rollback().await.unwrap();
        (opened, later, after, written, rest, own, others)
    }
    .await;
    run(other, &limit(60)).await;
    let (opened, later, after, written, rest, own, others) = reads;
    assert_eq!(opened, (false, false), "a find opens none");
    assert!(later.is_ok(), "{later:?}");
    assert_eq!(after, (false, false));
    assert_eq!(written, (false, true), "the first write opens it");
    assert!(matches!(rest, Some(Ok(1))), "{rest:?}");
    assert_eq!(own, 3, "its own write, once open");
    assert_eq!(others, 2, "not another's");
    assert_eq!(rows(other, "db.tx_reads.find()").await, 2, "rolled back");
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
async fn server_commands_that_write_run_in_the_transaction_or_are_refused() {
    let _one = REPLICA_SET.lock().await;
    let Some((conn, other)) = manual_pair("tx_cmd").await else {
        return;
    };
    let (conn, other) = (&*conn, &*other);
    for src in [
        "db.runCommand({ insert: 'tx_cmd', documents: [{ _id: 1 }, { _id: 2 }] })",
        "db.runCommand({ update: 'tx_cmd', updates: [{ q: { _id: 1 }, u: { $set: { n: 1 } } }] })",
        "db.runCommand({ findAndModify: 'tx_cmd', query: { _id: 2 }, remove: true })",
        "db.runCommand({ find: 'tx_cmd', filter: {} })",
    ] {
        run(conn, src).await;
        assert_eq!(state(conn), (false, true), "{src}");
    }
    assert_eq!(
        rows(conn, "db.tx_cmd.find({ n: 1 })").await,
        1,
        "its own writes"
    );
    assert_eq!(
        rows(other, "db.tx_cmd.find()").await,
        0,
        "nothing applied at once"
    );
    for src in [
        "db.runCommand({ drop: 'tx_cmd' })",
        "db.dropDatabase()",
        "db.aggregate([{ $documents: [{ a: 1 }] }, { $out: 'tx_cmd_out' }])",
    ] {
        let refused = conn.execute(src, &ExecOptions::default()).await;
        assert!(
            matches!(refused, Err(Error::Unsupported(_))),
            "{src}: {refused:?}"
        );
        assert_eq!(state(conn), (false, true), "{src}");
    }
    conn.rollback().await.unwrap();
    assert_eq!(rows(other, "db.tx_cmd.find()").await, 0, "rolled back");
    assert_eq!(rows(other, "db.tx_cmd.exists()").await, 1, "not dropped");
}

#[tokio::test]
async fn a_secondary_read_preference_reads_the_transaction_from_the_primary() {
    let _one = REPLICA_SET.lock().await;
    // A variable no one sets: the replica set of `REPLICA_SET_URL`, skipped when unreachable.
    let url = format!("{REPLICA_SET_URL}&readPreference=secondaryPreferred");
    let Some(conn) = connect_to("PLATYPUS_TEST_MONGO_RS_READ_PREFERENCE", &url).await else {
        return;
    };
    let conn = &*conn;
    run(conn, "use platypus").await;
    run(conn, "db.tx_readpref.drop()").await;
    run(conn, "db.createCollection('tx_readpref')").await;
    conn.set_autocommit(false).await.unwrap();
    run(conn, "db.tx_readpref.insertOne({ _id: 1 })").await;
    assert_eq!(
        rows(conn, "db.tx_readpref.find()").await,
        1,
        "its own write"
    );
    assert_eq!(rows(conn, "db.tx_readpref.countDocuments({})").await, 1);
    assert_eq!(state(conn), (false, true), "the transaction stays");
    conn.rollback().await.unwrap();
}

#[tokio::test]
async fn a_cancel_before_the_next_batch_leaves_the_transaction() {
    let _one = REPLICA_SET.lock().await;
    let Some((conn, other)) = manual_pair("tx_stale").await else {
        return;
    };
    let (conn, other) = (&*conn, &*other);
    let docs: Vec<String> = (0..1500).map(|i| format!("{{ _id: {i} }}")).collect();
    run(
        conn,
        &format!("db.tx_stale.insertMany([{}])", docs.join(", ")),
    )
    .await;
    let options = ExecOptions {
        batch_size: 1000,
        paged: true,
    };
    let ExecResult::Rows(mut stream) = conn.execute("db.tx_stale.find()", &options).await.unwrap()
    else {
        panic!("expected rows");
    };
    stream.next().await.unwrap().unwrap();
    // The cancel of a statement that ran meanwhile, while the result sat unread.
    conn.cancel_handle().cancel().await.unwrap();
    let rest = stream.next().await;
    assert!(matches!(rest, Some(Err(Error::Cancelled))), "{rest:?}");
    assert_eq!(state(conn), (false, true), "the transaction stays");
    assert_eq!(rows(conn, "db.tx_stale.find().limit(5)").await, 5);
    conn.commit().await.unwrap();
    assert_eq!(rows(other, "db.tx_stale.find()").await, 1500, "committed");
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

/// What the grid's save does on a connection in autocommit: the commands in a transaction of
/// their own, committed only when every one changed the one document it names.
async fn save_in_own_transaction(conn: &dyn Connection, commands: &[String]) -> Result<(), String> {
    let statements: Vec<&str> = commands.iter().map(String::as_str).collect();
    conn.set_autocommit(false)
        .await
        .map_err(|e| e.to_string())?;
    let ran = conn
        .execute_all(&statements, &vec![1; statements.len()])
        .await;
    let ok = ran.error.is_none()
        && ran.rows.len() == statements.len()
        && ran.rows.iter().all(|&n| n == 1);
    if ok {
        conn.commit().await.map_err(|e| e.to_string())?;
    } else {
        conn.rollback().await.map_err(|e| e.to_string())?;
    }
    conn.set_autocommit(true).await.map_err(|e| e.to_string())?;
    if ok {
        Ok(())
    } else {
        Err(ran
            .error
            .map_or_else(|| "short count".to_owned(), |e| e.to_string()))
    }
}

#[tokio::test]
async fn the_grids_save_is_all_or_nothing_on_a_replica_set() {
    use platypus_driver_mongo::edit::{Edits, Update, script};
    let _one = REPLICA_SET.lock().await;
    let Some((conn, other)) = manual_pair("grid_atomic").await else {
        return;
    };
    let (conn, other) = (&*conn, &*other);
    conn.set_autocommit(true).await.unwrap();
    run(
        conn,
        "db.grid_atomic.insertMany([{ _id: 1, v: 'a' }, { _id: 2, v: 'b' }])",
    )
    .await;
    let columns = vec!["_id".to_owned(), "v".to_owned()];
    let types = vec!["int".to_owned(), "string".to_owned()];
    let some = |t: &str| Some(t.to_owned());
    let update = |id: &str, v: &str| Update {
        loaded: vec![some(id), None],
        set: vec![(1, some(v))],
    };
    let edits = |updates| Edits {
        db: None,
        collection: "grid_atomic",
        columns: &columns,
        types: &types,
        updates,
        inserts: Vec::new(),
        deletes: Vec::new(),
    };
    let changed = |filter: &'static str| async move {
        rows(other, &format!("db.grid_atomic.find({filter})")).await
    };

    // The second document is gone: the short count undoes the first update.
    let gone = script(&edits(vec![update("1", "x"), update("9", "y")])).unwrap();
    let error = save_in_own_transaction(conn, &gone.commands)
        .await
        .unwrap_err();
    assert_eq!(error, "short count");
    assert_eq!(
        changed("{ v: 'x' }").await,
        0,
        "the first update was undone"
    );
    assert!(conn.transaction().autocommit && !conn.transaction().open);

    // Every command works: all are saved, and visible to another session.
    let good = script(&edits(vec![update("1", "x"), update("2", "y")])).unwrap();
    save_in_own_transaction(conn, &good.commands).await.unwrap();
    assert_eq!(changed("{ v: { $in: ['x', 'y'] } }").await, 2);
}

/// The grid's edits of `grid_types` on `conn`: numbers keep the type the document stores when
/// it holds them, other values are written as they are, and the undo puts the numbers back.
async fn edits_keep_the_stored_types(conn: &dyn Connection) {
    use platypus_driver_mongo::edit::{Edits, Update, script};
    run(conn, "use platypus").await;
    run(conn, "db.grid_types.drop()").await;
    run(
        conn,
        "db.grid_types.insertOne({ _id: 1, i: NumberInt(1), l: NumberLong(1), d: 1.5, \
         m: NumberInt(2), s: 'x', o: { a: 1 } })",
    )
    .await;
    let columns: Vec<String> = ["_id", "i", "l", "d", "m", "s", "o"]
        .map(str::to_owned)
        .to_vec();
    // A long column holds ints too, a double column any number.
    let types: Vec<String> = [
        "int", "long", "long", "double", "long", "string", "document",
    ]
    .map(str::to_owned)
    .to_vec();
    let some = |t: &str| Some(t.to_owned());
    let edits = Edits {
        db: None,
        collection: "grid_types",
        columns: &columns,
        types: &types,
        updates: vec![Update {
            loaded: vec![
                some("1"),
                some("1"),
                some("1"),
                some("1.5"),
                some("2"),
                some("x"),
                some("{\"a\":1}"),
            ],
            set: vec![
                (1, some("5")),
                (2, some("6")),
                (3, some("7")),
                (4, some("3.5")),
                (5, some("$abc")),
                (6, some("{\"$k\": 1}")),
            ],
        }],
        inserts: Vec::new(),
        deletes: Vec::new(),
    };
    let s = script(&edits).unwrap();
    let commands: Vec<&str> = s.commands.iter().map(String::as_str).collect();
    let ran = conn.execute_all(&commands, &[1]).await;
    assert!(ran.error.is_none() && ran.rows == [1], "{ran:?}");
    let saved = "db.grid_types.find({ _id: 1, i: { $eq: 5, $type: 'int' }, \
                 l: { $eq: 6, $type: 'long' }, d: { $eq: 7, $type: 'double' }, \
                 m: { $eq: 3.5, $type: 'double' }, s: { $eq: '$abc', $type: 'string' }, \
                 $expr: { $eq: ['$o', { $literal: { $k: 1 } }] } })";
    assert_eq!(rows(conn, saved).await, 1, "{}", s.commands[0]);

    let undo: Vec<&str> = s
        .undo
        .iter()
        .map(String::as_str)
        .filter(|l| !l.starts_with("//"))
        .collect();
    let ran = conn.execute_all(&undo, &[1]).await;
    assert!(ran.error.is_none() && ran.rows == [1], "{ran:?}");
    // The int that became a double comes back as one: the undo says so.
    let restored = "db.grid_types.find({ _id: 1, i: { $eq: 1, $type: 'int' }, \
                    l: { $eq: 1, $type: 'long' }, d: { $eq: 1.5, $type: 'double' }, \
                    m: { $eq: 2, $type: 'double' }, s: 'x', o: { a: 1 } })";
    assert_eq!(rows(conn, restored).await, 1, "{:?}", s.undo);
    assert!(
        s.undo
            .iter()
            .any(|l| l.starts_with("//") && l.contains('m'))
    );
    run(conn, "db.grid_types.drop()").await;
}

#[tokio::test]
async fn the_grids_edits_keep_the_stored_types() {
    if let Some(conn) = connect().await {
        edits_keep_the_stored_types(&*conn).await;
    }
    let _one = REPLICA_SET.lock().await;
    if let Some(conn) = connect_to("PLATYPUS_TEST_MONGO_RS", REPLICA_SET_URL).await {
        edits_keep_the_stored_types(&*conn).await;
    }
}

/// The data viewer's statement for documents an agent proposes to change (`propose_edits`)
/// reads exactly the documents the proposal names, by an `ObjectId` or a number `_id` as the
/// collection stores it, and gives each `_id` back as the grid shows it.
#[tokio::test]
async fn proposed_edits_open_the_documents_they_name() {
    use platypus_sql::proposed::{Edits, Row, Update};
    let Some(conn) = connect().await else { return };
    let conn = &*conn;
    let id = |n: u8| format!("65a1b2c3d4e5f60718293a{n:02x}");
    run(conn, "db.proposed_oids.drop()").await;
    run(conn, "db.proposed_ints.drop()").await;
    run(
        conn,
        &format!(
            "db.proposed_oids.insertMany([{{ _id: ObjectId('{}'), n: 1 }}, \
             {{ _id: ObjectId('{}'), n: 2 }}, {{ _id: ObjectId('{}'), n: 3 }}])",
            id(1),
            id(2),
            id(3)
        ),
    )
    .await;
    run(
        conn,
        "db.proposed_ints.insertMany([{ _id: 1, n: 1 }, { _id: 2, n: 2 }, { _id: 3, n: 3 }])",
    )
    .await;
    let key = |v: &str| -> Row { vec![("_id".to_owned(), Some(v.to_owned()))] };
    let set: Row = vec![("n".to_owned(), Some("9".to_owned()))];
    let edits = |table: &str, updated: &str, deleted: &str| Edits {
        schema: "platypus".to_owned(),
        table: table.to_owned(),
        updates: vec![Update {
            key: key(updated),
            set: set.clone(),
        }],
        deletes: vec![key(deleted)],
        ..Edits::default()
    };

    // ObjectIds in upper case: read as ObjectIds, shown in lower case.
    let details = conn
        .table_details("platypus", "proposed_oids")
        .await
        .unwrap();
    let proposal = edits(
        "proposed_oids",
        &id(1).to_ascii_uppercase(),
        &id(3).to_ascii_uppercase(),
    );
    let (viewer, shown) = platypus_mcp::mongo_viewer(&proposal, &details).unwrap();
    assert_eq!(rows(conn, &viewer.sql).await, 2, "{}", viewer.sql);
    assert_eq!(shown.updates[0].key, key(&id(1)));
    assert_eq!(shown.deletes[0], key(&id(3)));

    // Numbers as the collection stores them: `02` is the int 2.
    let details = conn
        .table_details("platypus", "proposed_ints")
        .await
        .unwrap();
    let (viewer, shown) =
        platypus_mcp::mongo_viewer(&edits("proposed_ints", "02", "3"), &details).unwrap();
    assert_eq!(rows(conn, &viewer.sql).await, 2, "{}", viewer.sql);
    assert_eq!(shown.updates[0].key, key("2"));
    // Only inserts: the whole collection.
    let inserts = Edits {
        schema: "platypus".to_owned(),
        table: "proposed_ints".to_owned(),
        inserts: vec![vec![("n".to_owned(), Some("4".to_owned()))]],
        ..Edits::default()
    };
    let (viewer, _) = platypus_mcp::mongo_viewer(&inserts, &details).unwrap();
    assert_eq!(rows(conn, &viewer.sql).await, 3, "{}", viewer.sql);
    // An ObjectId's text is not an int `_id`: refused, never read as another document.
    assert!(platypus_mcp::mongo_viewer(&edits("proposed_ints", &id(1), "3"), &details).is_err());
    run(conn, "db.proposed_oids.drop()").await;
    run(conn, "db.proposed_ints.drop()").await;
}
