//! Tests against the MongoDB of `docker/compose.yml` (profile `mongo`), or the server at
//! `PLATYPUS_TEST_MONGO`; skipped when the default server is unreachable.

use arrow_array::RecordBatch;
use arrow_array::cast::AsArray;
use arrow_schema::DataType;
use futures::TryStreamExt;
use platypus_core::{
    Connection, ConnectionConfig, Driver, Error, ExecOptions, ExecResult, META_DB_TYPE,
};

use crate::MongoDriver;

const DEFAULT_URL: &str = "mongodb://platypus:platypus@localhost:57017/?authSource=admin";

async fn connect() -> Option<Box<dyn Connection>> {
    let (url, required) = match std::env::var("PLATYPUS_TEST_MONGO") {
        Ok(url) => (url, true),
        Err(_) => (DEFAULT_URL.to_owned(), false),
    };
    let mut config = ConnectionConfig::from_url(&url).unwrap();
    config.connect_timeout = Some(std::time::Duration::from_secs(3));
    match MongoDriver.connect(&config).await {
        Ok(conn) => Some(conn),
        Err(e) if !required => {
            eprintln!("skipping mongo: unreachable at {url}: {e}");
            None
        }
        Err(e) => panic!("cannot connect to PLATYPUS_TEST_MONGO: {e}"),
    }
}

async fn rows(conn: &dyn Connection, src: &str) -> Vec<RecordBatch> {
    match conn.execute(src, &ExecOptions::default()).await {
        Ok(ExecResult::Rows(stream)) => stream.try_collect().await.unwrap(),
        other => panic!("{src}: {other:?}"),
    }
}

async fn affected(conn: &dyn Connection, src: &str) -> u64 {
    match conn.execute(src, &ExecOptions::default()).await {
        Ok(ExecResult::Affected { rows }) => rows,
        other => panic!("{src}: {other:?}"),
    }
}

fn count(batches: &[RecordBatch]) -> usize {
    batches.iter().map(RecordBatch::num_rows).sum()
}

/// A collection only this test uses, in the `platypus` database.
async fn scratch(conn: &dyn Connection, name: &str) {
    affected(conn, "use platypus").await;
    let _ = conn
        .execute(&format!("db.{name}.drop()"), &ExecOptions::default())
        .await;
}

#[tokio::test]
async fn reads_the_seeded_database() {
    let Some(conn) = connect().await else { return };
    assert_eq!(conn.server_info().product, "MongoDB");
    assert!(conn.server_info().version.starts_with('8'));
    affected(&*conn, "use platypus").await;

    let batches = rows(
        &*conn,
        "db.customers.find({ country: 'IT' }).sort({ _id: 1 }).limit(3)",
    )
    .await;
    assert_eq!(count(&batches), 3);
    let schema = batches[0].schema();
    assert_eq!(schema.field(0).name(), "_id");
    let tags = schema.field_with_name("tags").unwrap();
    assert_eq!(tags.metadata()[META_DB_TYPE], "array");
    let address = schema.field_with_name("address").unwrap();
    assert_eq!(address.metadata()[META_DB_TYPE], "document");
    assert!(matches!(
        schema.field_with_name("created_at").unwrap().data_type(),
        DataType::Timestamp(..)
    ));
    let ids = batches[0]
        .column(0)
        .as_primitive::<arrow_array::types::Int32Type>();
    assert_eq!(ids.values(), &[5, 10, 15]);

    let n = rows(&*conn, "db.orders.countDocuments({ status: 'paid' })").await;
    assert_eq!(
        n[0].column(0)
            .as_primitive::<arrow_array::types::Int64Type>()
            .value(0),
        5000
    );
    let statuses = rows(&*conn, "db.orders.distinct('status')").await;
    assert_eq!(count(&statuses), 4);
    let grouped = rows(
        &*conn,
        "db.orders.aggregate([{ $group: { _id: '$status', n: { $sum: 1 } } }, { $sort: { _id: 1 } }])",
    )
    .await;
    assert_eq!(count(&grouped), 4);
    assert_eq!(
        grouped[0].column(0).as_string::<i32>().value(0),
        "cancelled"
    );
    assert_eq!(
        count(&rows(&*conn, "db.customers.find({ country: 'XX' })").await),
        0
    );
    let view = rows(&*conn, "db.customer_totals.find({ _id: 1 })").await;
    assert_eq!(count(&view), 1);
    assert!(count(&rows(&*conn, "show collections").await) >= 3);
    assert!(count(&rows(&*conn, "show dbs").await) >= 2);
    assert_eq!(count(&rows(&*conn, "db.runCommand({ ping: 1 })").await), 1);
}

#[tokio::test]
async fn streams_results_in_batches() {
    let Some(conn) = connect().await else { return };
    affected(&*conn, "use platypus").await;
    let options = ExecOptions {
        batch_size: 1000,
        paged: true,
    };
    let ExecResult::Rows(stream) = conn.execute("db.orders.find()", &options).await.unwrap() else {
        panic!("expected rows");
    };
    let batches: Vec<RecordBatch> = stream.try_collect().await.unwrap();
    assert_eq!(batches.len(), 20);
    assert_eq!(count(&batches), 20_000);
    // An abandoned stream leaves the connection usable.
    let ExecResult::Rows(mut stream) = conn.execute("db.orders.find()", &options).await.unwrap()
    else {
        panic!("expected rows");
    };
    futures::StreamExt::next(&mut stream)
        .await
        .unwrap()
        .unwrap();
    drop(stream);
    assert_eq!(
        count(&rows(&*conn, "db.customers.find().limit(2)").await),
        2
    );
}

#[tokio::test]
async fn writes_documents() {
    let Some(conn) = connect().await else { return };
    scratch(&*conn, "t_writes").await;
    assert_eq!(
        affected(&*conn, "db.t_writes.insertOne({ _id: 1, n: 1 })").await,
        1
    );
    assert_eq!(
        affected(&*conn, "db.t_writes.insertMany([{ _id: 2, n: 2 }, { _id: 3, n: 3, d: NumberDecimal('1.50'), at: ISODate('2024-01-02') }])").await,
        2
    );
    assert_eq!(
        affected(
            &*conn,
            "db.t_writes.updateMany({ n: { $gte: 2 } }, { $inc: { n: 10 } })"
        )
        .await,
        2
    );
    assert_eq!(
        affected(
            &*conn,
            "db.t_writes.updateOne({ _id: 9 }, { $set: { n: 9 } }, { upsert: true })"
        )
        .await,
        1
    );
    assert_eq!(
        affected(&*conn, "db.t_writes.replaceOne({ _id: 1 }, { n: 100 })").await,
        1
    );
    assert_eq!(
        affected(&*conn, "db.t_writes.deleteOne({ _id: 9 })").await,
        1
    );
    let batches = rows(
        &*conn,
        "db.t_writes.find({}, { n: 1, d: 1, at: 1 }).sort({ _id: 1 })",
    )
    .await;
    let schema = batches[0].schema();
    assert_eq!(
        schema.field_with_name("d").unwrap().metadata()[META_DB_TYPE],
        "decimal"
    );
    let n = batches[0]
        .column(1)
        .as_primitive::<arrow_array::types::Int32Type>();
    assert_eq!(n.values(), &[100, 12, 13]);
    let d = batches[0]
        .column(schema.index_of("d").unwrap())
        .as_string::<i32>();
    assert_eq!(d.value(2), "1.50");
    assert_eq!(
        affected(&*conn, "db.t_writes.createIndex({ n: -1 })").await,
        0
    );
    assert_eq!(count(&rows(&*conn, "db.t_writes.getIndexes()").await), 2);
    assert_eq!(affected(&*conn, "db.t_writes.deleteMany({})").await, 3);
    scratch(&*conn, "t_renamed").await;
    affected(&*conn, "db.t_writes.renameCollection('t_renamed')").await;
    assert_eq!(count(&rows(&*conn, "db.t_renamed.getIndexes()").await), 2);
    affected(&*conn, "db.t_renamed.drop()").await;
}

#[tokio::test]
async fn reports_errors() {
    let Some(conn) = connect().await else { return };
    let Err(Error::Database(e)) = conn
        .execute("db.c.find({ a: })", &ExecOptions::default())
        .await
    else {
        panic!("expected a syntax error");
    };
    assert_eq!(
        (e.code.as_deref(), e.position),
        (Some("SyntaxError"), Some(16))
    );
    let Err(Error::Database(e)) = conn
        .execute("db.c.find({ $bogus: 1 })", &ExecOptions::default())
        .await
    else {
        panic!("expected a server error");
    };
    assert_eq!(e.code.as_deref(), Some("BadValue"));
    scratch(&*conn, "t_dup").await;
    affected(&*conn, "db.t_dup.insertOne({ _id: 1 })").await;
    let Err(Error::Database(e)) = conn
        .execute("db.t_dup.insertOne({ _id: 1 })", &ExecOptions::default())
        .await
    else {
        panic!("expected a duplicate key error");
    };
    assert!(e.message.contains("duplicate key"), "{e}");
    affected(&*conn, "db.t_dup.drop()").await;
}

#[tokio::test]
async fn cancels_running_queries() {
    let Some(conn) = connect().await else { return };
    scratch(&*conn, "t_sleep").await;
    affected(&*conn, "db.t_sleep.insertOne({ _id: 1 })").await;
    let handle = conn.cancel_handle();
    let cancel = tokio::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        handle.cancel().await
    });
    let started = std::time::Instant::now();
    let result = conn
        .execute(
            "db.t_sleep.find({ $where: 'sleep(30000) || true' })",
            &ExecOptions::default(),
        )
        .await;
    assert!(matches!(result, Err(Error::Cancelled)), "{result:?}");
    assert!(started.elapsed() < std::time::Duration::from_secs(10));
    cancel.await.unwrap().unwrap();
    assert_eq!(count(&rows(&*conn, "db.t_sleep.find()").await), 1);
    affected(&*conn, "db.t_sleep.drop()").await;
}

#[tokio::test]
async fn navigates_metadata() {
    let Some(conn) = connect().await else { return };
    affected(&*conn, "use platypus").await;
    let schemas = conn.schemas().await.unwrap();
    assert!(
        schemas
            .iter()
            .any(|s| s.name == "platypus" && s.current && !s.system)
    );
    assert!(schemas.iter().any(|s| s.name == "admin" && s.system));
    let objects = conn.objects("platypus").await.unwrap();
    let kinds: Vec<_> = objects.iter().map(|o| (o.name.as_str(), o.kind)).collect();
    assert!(kinds.contains(&("customers", platypus_core::ObjectKind::Table)));
    assert!(kinds.contains(&("customer_totals", platypus_core::ObjectKind::View)));
    let details = conn.table_details("platypus", "orders").await.unwrap();
    let columns: Vec<_> = details.columns.iter().map(|c| c.name.as_str()).collect();
    assert_eq!(
        columns,
        ["_id", "customer_id", "total", "status", "placed_at"]
    );
    assert!(details.columns[0].primary_key);
    assert!(
        details
            .indexes
            .iter()
            .any(|i| i.columns == ["customer_id"] && !i.unique)
    );
    let catalog = conn.catalog_columns().await.unwrap();
    assert!(
        catalog
            .iter()
            .any(|c| c.table == "customers" && c.column == "email")
    );

    let object = platypus_core::ObjectRef {
        schema: "platypus",
        name: "customers",
        kind: platypus_core::ObjectKind::Table,
        parent: None,
    };
    let ddl = conn.object_ddl(&object).await.unwrap();
    assert!(
        ddl.starts_with("db.createCollection(\"customers\")"),
        "{ddl}"
    );
    assert!(
        ddl.contains("db.customers.createIndex({ email: 1 }, { unique: true });"),
        "{ddl}"
    );
    let view = platypus_core::ObjectRef {
        name: "customer_totals",
        kind: platypus_core::ObjectKind::View,
        ..object
    };
    assert!(
        conn.object_ddl(&view)
            .await
            .unwrap()
            .starts_with("db.createView(\"customer_totals\", \"customers\", [{ $lookup")
    );
    let props = conn.object_properties(&object).await.unwrap();
    assert!(
        props
            .iter()
            .any(|p| p.name == "Documents" && p.value == "1000"),
        "{props:?}"
    );
}
