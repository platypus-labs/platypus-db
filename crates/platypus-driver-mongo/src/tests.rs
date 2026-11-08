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

/// What an `execute` that must fail says.
async fn refused(conn: &dyn Connection, src: &str) -> String {
    conn.execute(src, &ExecOptions::default())
        .await
        .unwrap_err()
        .to_string()
}

#[tokio::test]
async fn bulk_write_runs_in_order_and_says_how_far_it_got() {
    let Some(conn) = connect().await else { return };
    let conn = &*conn;
    scratch(conn, "t_bulk").await;
    // Every kind, in order; each counts what it changed.
    assert_eq!(
        affected(
            conn,
            "db.t_bulk.bulkWrite([
                { insertOne: { document: { _id: 1, n: 1 } } },
                { insertOne: { document: { _id: 2, n: 2 } } },
                { insertOne: { document: { _id: 3, n: 3 } } },
                { updateOne: { filter: { _id: 1 }, update: { $inc: { n: 10 } } } },
                { updateMany: { filter: { n: { $lte: 3 } }, update: { $set: { low: true } } } },
                { updateOne: { filter: { _id: 4 }, update: { $set: { n: 4 } }, upsert: true } },
                { replaceOne: { filter: { _id: 2 }, replacement: { n: 20 } } },
                { deleteOne: { filter: { _id: 3 } } },
                { deleteMany: { filter: { n: 4 } } },
            ])"
        )
        .await,
        3 + 1 + 2 + 1 + 1 + 1 + 1
    );
    let left = rows(conn, "db.t_bulk.find({}, { n: 1 }).sort({ _id: 1 })").await;
    let n = left[0]
        .column(1)
        .as_primitive::<arrow_array::types::Int32Type>();
    assert_eq!(n.values(), &[11, 20]);

    // Ordered, the first failure stops it: what ran stays, the rest does not run.
    let stopped = refused(
        conn,
        "db.t_bulk.bulkWrite([
            { insertOne: { document: { _id: 5 } } },
            { deleteOne: { filter: { _id: 1 } } },
            { insertOne: { document: { _id: 2 } } },
            { deleteOne: { filter: { _id: 5 } } },
        ])",
    )
    .await;
    assert!(stopped.contains("operation 3 of 4"), "{stopped}");
    assert!(stopped.contains("2 documents changed"), "{stopped}");
    assert_eq!(count(&rows(conn, "db.t_bulk.find({ _id: 5 })").await), 1);

    // Unordered, the rest goes on, and the error says how many failed.
    let unordered = refused(
        conn,
        "db.t_bulk.bulkWrite([
            { insertOne: { document: { _id: 2 } } },
            { insertOne: { document: { _id: 6 } } },
            { insertOne: { document: { _id: 5 } } },
            { insertOne: { document: { _id: 7 } } },
        ], { ordered: false })",
    )
    .await;
    assert!(unordered.contains("operation 1 of 4"), "{unordered}");
    assert!(
        unordered.contains("2 of 4 failed, 2 documents changed"),
        "{unordered}"
    );
    assert_eq!(
        count(&rows(conn, "db.t_bulk.find({ _id: { $in: [6, 7] } })").await),
        2
    );

    // Neighbouring inserts go together: the failure names the batch, those before it are in.
    let batch = refused(
        conn,
        "db.t_bulk.bulkWrite([
            { insertOne: { document: { _id: 8 } } },
            { insertOne: { document: { _id: 2 } } },
            { insertOne: { document: { _id: 9 } } },
        ])",
    )
    .await;
    assert!(batch.contains("operations 1-3 of 3"), "{batch}");
    assert_eq!(count(&rows(conn, "db.t_bulk.find({ _id: 8 })").await), 1);
    assert_eq!(count(&rows(conn, "db.t_bulk.find({ _id: 9 })").await), 0);

    // Its options reach the server, and common ones go with each write.
    affected(conn, "db.t_bulk.deleteMany({})").await;
    affected(conn, "db.t_bulk.insertOne({ _id: 1, x: [1, 2, 3] })").await;
    assert_eq!(
        affected(
            conn,
            "db.t_bulk.bulkWrite([
                { updateOne: { filter: { _id: 1 }, update: { $set: { 'x.$[e]': 0 } }, arrayFilters: [{ e: { $gte: 2 } }] } },
            ], { writeConcern: { w: 1 } })"
        )
        .await,
        1
    );
    assert_eq!(
        count(&rows(conn, "db.t_bulk.find({ x: [1, 0, 0] })").await),
        1
    );
    affected(conn, "db.t_bulk.drop()").await;
}

/// `arrayFilters`, `collation`, `hint` and the like used to be read and dropped: a write
/// reached documents other than the ones asked for.
#[tokio::test]
async fn options_of_a_write_reach_the_server() {
    let Some(conn) = connect().await else { return };
    let conn = &*conn;
    scratch(conn, "t_options").await;
    affected(
        conn,
        "db.t_options.insertOne({ _id: 1, x: [1, 2, 3], s: 'Ann' })",
    )
    .await;
    affected(conn, "db.t_options.updateOne({ _id: 1 }, { $set: { 'x.$[e]': 0 } }, { arrayFilters: [{ e: { $gte: 2 } }] })").await;
    assert_eq!(
        count(&rows(conn, "db.t_options.find({ x: [1, 0, 0] })").await),
        1
    );
    // Without the collation this matches nothing: the case differs.
    assert_eq!(
        affected(
            conn,
            "db.t_options.updateOne({ s: 'ANN' }, { $set: { n: 1 } })"
        )
        .await,
        0
    );
    assert_eq!(
        affected(conn, "db.t_options.updateOne({ s: 'ANN' }, { $set: { n: 1 } }, { collation: { locale: 'en', strength: 2 } })").await,
        1
    );
    // A hint the collection has no index for: only the server can refuse it, if it gets it.
    let hint = refused(
        conn,
        "db.t_options.updateOne({ s: 'Ann' }, { $set: { n: 2 } }, { hint: 'no_such_index' })",
    )
    .await;
    assert!(hint.to_lowercase().contains("hint"), "{hint}");
    let hint = refused(
        conn,
        "db.t_options.deleteOne({ s: 'Ann' }, { hint: 'no_such_index' })",
    )
    .await;
    assert!(hint.to_lowercase().contains("hint"), "{hint}");
    // `let` variables, in a filter.
    assert_eq!(
        affected(conn, "db.t_options.updateOne({ $expr: { $eq: ['$s', '$$who'] } }, { $set: { n: 3 } }, { let: { who: 'Ann' } })").await,
        1
    );
    // Ordered or not decides whether a bad document stops the rest.
    let stopped = refused(conn, "db.t_options.insertMany([{ _id: 1 }, { _id: 2 }])").await;
    assert!(stopped.contains("document 0"), "{stopped}");
    assert_eq!(count(&rows(conn, "db.t_options.find({ _id: 2 })").await), 0);
    refused(
        conn,
        "db.t_options.insertMany([{ _id: 1 }, { _id: 3 }], { ordered: false })",
    )
    .await;
    assert_eq!(count(&rows(conn, "db.t_options.find({ _id: 3 })").await), 1);
    // Deleting with a collation.
    assert_eq!(
        affected(
            conn,
            "db.t_options.deleteOne({ s: 'ANN' }, { collation: { locale: 'en', strength: 2 } })"
        )
        .await,
        1
    );
    // Reads too.
    affected(conn, "db.t_options.insertOne({ _id: 9, s: 'Bob' })").await;
    assert_eq!(
        count(
            &rows(
                conn,
                "db.t_options.find({ s: 'BOB' }).collation({ locale: 'en', strength: 2 })"
            )
            .await
        ),
        1
    );
    assert_eq!(count(&rows(conn, "db.t_options.find({ s: 'BOB' }, null, { collation: { locale: 'en', strength: 2 } })").await), 1);
    assert_eq!(
        count(&rows(conn, "db.t_options.find({ s: 'BOB' })").await),
        0
    );
    assert_eq!(count(&rows(conn, "db.t_options.distinct('s', { s: 'BOB' }, { collation: { locale: 'en', strength: 2 } })").await), 1);
    let found = refused(conn, "db.t_options.find().hint('no_such_index')").await;
    assert!(found.to_lowercase().contains("hint"), "{found}");
    let counted = refused(
        conn,
        "db.t_options.countDocuments({}, { hint: 'no_such_index' })",
    )
    .await;
    assert!(
        counted.to_lowercase().contains("hint") || counted.contains("index"),
        "{counted}"
    );
    affected(conn, "db.t_options.drop()").await;
}

#[tokio::test]
async fn find_one_and_answers_with_the_document() {
    let Some(conn) = connect().await else { return };
    let conn = &*conn;
    scratch(conn, "t_foam").await;
    affected(
        conn,
        "db.t_foam.insertMany([{ _id: 1, n: 1 }, { _id: 2, n: 2 }])",
    )
    .await;
    let value = |batches: &[RecordBatch], column: &str| -> i32 {
        let schema = batches[0].schema();
        batches[0]
            .column(schema.index_of(column).unwrap())
            .as_primitive::<arrow_array::types::Int32Type>()
            .value(0)
    };
    // The document as it was, by default; `sort` picks which.
    let before = rows(
        conn,
        "db.t_foam.findOneAndUpdate({}, { $inc: { n: 10 } }, { sort: { _id: -1 } })",
    )
    .await;
    assert_eq!(value(&before, "n"), 2);
    let after = rows(
        conn,
        "db.t_foam.findOneAndUpdate({ _id: 2 }, { $inc: { n: 10 } }, { returnDocument: 'after' })",
    )
    .await;
    assert_eq!(value(&after, "n"), 22);
    let legacy = rows(
        conn,
        "db.t_foam.findOneAndUpdate({ _id: 1 }, { $set: { n: 5 } }, { returnNewDocument: true })",
    )
    .await;
    assert_eq!(value(&legacy, "n"), 5);
    let projected = rows(conn, "db.t_foam.findOneAndReplace({ _id: 1 }, { n: 6 }, { returnDocument: 'after', projection: { n: 1, _id: 0 } })").await;
    assert!(projected[0].schema().index_of("_id").is_err());
    assert_eq!(value(&projected, "n"), 6);
    let modified = rows(
        conn,
        "db.t_foam.findAndModify({ query: { _id: 1 }, update: { $set: { n: 7 } }, new: true })",
    )
    .await;
    assert_eq!(value(&modified, "n"), 7);
    // Nothing matched: no document, no error.
    assert_eq!(
        count(&rows(conn, "db.t_foam.findOneAndDelete({ _id: 99 })").await),
        0
    );
    let deleted = rows(conn, "db.t_foam.findOneAndDelete({ _id: 1 })").await;
    assert_eq!(value(&deleted, "n"), 7);
    assert_eq!(count(&rows(conn, "db.t_foam.find({ _id: 1 })").await), 0);
    let removed = rows(
        conn,
        "db.t_foam.findAndModify({ query: { _id: 2 }, remove: true })",
    )
    .await;
    assert_eq!(value(&removed, "n"), 22);
    assert_eq!(count(&rows(conn, "db.t_foam.find()").await), 0);
    affected(conn, "db.t_foam.drop()").await;
}

#[tokio::test]
async fn indexes_statistics_and_names() {
    let Some(conn) = connect().await else { return };
    let conn = &*conn;
    scratch(conn, "t_admin").await;
    affected(conn, "db.t_admin.insertOne({ _id: 1, a: 1, b: 2 })").await;
    let names = |batches: Vec<RecordBatch>| -> Vec<String> {
        let schema = batches[0].schema();
        let column = batches[0]
            .column(schema.index_of("name").unwrap())
            .as_string::<i32>();
        column.iter().flatten().map(str::to_owned).collect()
    };
    rows(conn, "db.t_admin.createIndexes([{ a: 1 }, { b: 1 }])").await;
    assert_eq!(count(&rows(conn, "db.t_admin.getIndexes()").await), 3);
    assert_eq!(count(&rows(conn, "db.t_admin.getIndexSpecs()").await), 3);
    rows(conn, "db.t_admin.hideIndex('a_1')").await;
    rows(conn, "db.t_admin.unhideIndex('a_1')").await;
    rows(conn, "db.t_admin.dropIndexes('b_1')").await;
    assert_eq!(count(&rows(conn, "db.t_admin.getIndexes()").await), 2);
    rows(conn, "db.t_admin.dropIndexes()").await;
    assert_eq!(count(&rows(conn, "db.t_admin.getIndexes()").await), 1);
    affected(conn, "db.t_admin.ensureIndex({ a: 1 })").await;
    assert_eq!(count(&rows(conn, "db.t_admin.getIndexes()").await), 2);

    assert_eq!(count(&rows(conn, "db.t_admin.stats()").await), 1);
    assert_eq!(count(&rows(conn, "db.t_admin.validate()").await), 1);
    assert_eq!(count(&rows(conn, "db.t_admin.latencyStats()").await), 1);
    for stat in [
        "dataSize",
        "storageSize",
        "totalIndexSize",
        "totalSize",
        "isCapped",
    ] {
        let answer = rows(conn, &format!("db.t_admin.{stat}()")).await;
        assert!(answer[0].schema().index_of(stat).is_ok(), "{stat}");
    }
    assert_eq!(names(rows(conn, "db.t_admin.getName()").await), ["t_admin"]);
    assert_eq!(
        names(rows(conn, "db.t_admin.getFullName()").await),
        ["platypus.t_admin"]
    );
    assert_eq!(names(rows(conn, "db.getName()").await), ["platypus"]);
    assert_eq!(count(&rows(conn, "db.t_admin.exists()").await), 1);
    assert_eq!(count(&rows(conn, "db.t_none.exists()").await), 0);
    assert_eq!(count(&rows(conn, "db.serverStatus()").await), 1);
    assert_eq!(count(&rows(conn, "db.hello()").await), 1);
    assert_eq!(count(&rows(conn, "show users").await), 1);
    assert_eq!(count(&rows(conn, "show logs").await), 1);
    assert_eq!(
        count(&rows(conn, "db.aggregate([{ $documents: [{ a: 1 }, { a: 2 }] }])").await),
        2
    );
    affected(conn, "db.t_admin.drop()").await;
}

fn holds(node: &platypus_core::PlanNode, operation: &str) -> bool {
    node.operation == operation || node.children.iter().any(|c| holds(c, operation))
}

/// A plan, of a read and of writes that stay unapplied, as the app's tab asks for it.
#[tokio::test]
async fn explains_without_changing_anything() {
    let Some(conn) = connect().await else { return };
    let conn = &*conn;
    assert!(conn.capabilities().explain);
    scratch(conn, "t_explain").await;
    affected(
        conn,
        "db.t_explain.insertMany([{ _id: 1, a: 1 }, { _id: 2, a: 2 }])",
    )
    .await;
    affected(conn, "db.t_explain.createIndex({ a: 1 })").await;

    let planned = conn
        .explain("db.t_explain.find({ a: 1 })", false)
        .await
        .unwrap();
    assert!(!planned.analyzed);
    assert!(planned.plan.contains("queryPlanner"), "{}", planned.plan);
    assert!(planned.root.actual_rows.is_none());
    let ran = conn
        .explain("db.t_explain.find({ a: 1 })", true)
        .await
        .unwrap();
    assert!(ran.analyzed);
    assert_eq!(ran.root.actual_rows, Some(1.0));
    assert!(holds(&ran.root, "IXSCAN"), "{:?}", ran.root);

    // The statement's own explain, by either spelling, and its verbosity wins.
    let own = conn
        .explain(
            "db.t_explain.find({ a: 1 }).explain('executionStats')",
            false,
        )
        .await
        .unwrap();
    assert!(own.analyzed);
    let chained = conn
        .explain("db.t_explain.explain().find({ a: 1 })", true)
        .await
        .unwrap();
    assert!(!chained.analyzed);
    assert_eq!(
        count(&rows(conn, "db.t_explain.find({ a: 1 }).explain()").await),
        1
    );

    let grouped = conn
        .explain(
            "db.t_explain.aggregate([{ $group: { _id: '$a' } }, { $sort: { _id: 1 } }])",
            true,
        )
        .await
        .unwrap();
    assert_ne!(grouped.root.operation, "");
    conn.explain("db.t_explain.countDocuments({ a: 1 })", false)
        .await
        .unwrap();
    conn.explain("db.t_explain.distinct('a', { a: 1 })", false)
        .await
        .unwrap();

    // Even with what would run it, a write stays unapplied.
    for write in [
        "db.t_explain.deleteMany({})",
        "db.t_explain.updateMany({}, { $set: { a: 9 } })",
        "db.t_explain.replaceOne({ _id: 1 }, { a: 9 })",
        "db.t_explain.findOneAndDelete({ _id: 1 })",
        "db.t_explain.findOneAndUpdate({ _id: 1 }, { $set: { a: 9 } })",
    ] {
        conn.explain(write, true)
            .await
            .unwrap_or_else(|e| panic!("{write}: {e}"));
    }
    assert_eq!(
        count(&rows(conn, "db.t_explain.find({ a: { $in: [1, 2] } })").await),
        2
    );

    // What has no plan says so, and is not run.
    for none in [
        "show dbs",
        "db.t_explain.drop()",
        "db.t_explain.insertOne({ a: 3 })",
        "db.runCommand({ ping: 1 })",
    ] {
        let refused = conn.explain(none, false).await.unwrap_err().to_string();
        assert!(refused.contains("plan"), "{none}: {refused}");
    }
    assert_eq!(count(&rows(conn, "db.t_explain.find()").await), 2);
    affected(conn, "db.t_explain.drop()").await;
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
/// The single-node replica set of `docker/compose.yml` (service `mongo-rs`), or the one at
/// `PLATYPUS_TEST_MONGO_RS`: transactions need one.
const REPLICA_SET_URL: &str = "mongodb://localhost:57018/?replicaSet=rs0";

async fn connect_replica_set() -> Option<Box<dyn Connection>> {
    let (url, required) = match std::env::var("PLATYPUS_TEST_MONGO_RS") {
        Ok(url) => (url, true),
        Err(_) => (REPLICA_SET_URL.to_owned(), false),
    };
    let mut config = ConnectionConfig::from_url(&url).unwrap();
    config.connect_timeout = Some(std::time::Duration::from_secs(3));
    match MongoDriver.connect(&config).await {
        Ok(conn) => Some(conn),
        Err(e) if !required => {
            eprintln!("skipping mongo replica set: unreachable at {url}: {e}");
            None
        }
        Err(e) => panic!("cannot connect to PLATYPUS_TEST_MONGO_RS: {e}"),
    }
}

/// Two sessions of the replica set, the first in manual mode, with an empty collection `name`
/// in database `platypus`.
async fn manual_pair(name: &str) -> Option<(Box<dyn Connection>, Box<dyn Connection>)> {
    let conn = connect_replica_set().await?;
    let other = connect_replica_set().await?;
    scratch(&*conn, name).await;
    affected(&*conn, &format!("db.createCollection('{name}')")).await;
    affected(&*other, "use platypus").await;
    conn.set_autocommit(false).await.unwrap();
    Some((conn, other))
}

fn state(conn: &dyn Connection) -> (bool, bool) {
    let tx = conn.transaction();
    (tx.autocommit, tx.open)
}

fn names(batches: &[RecordBatch]) -> Vec<String> {
    batches
        .iter()
        .flat_map(|b| {
            b.column(0)
                .as_string::<i32>()
                .iter()
                .flatten()
                .map(str::to_owned)
                .collect::<Vec<_>>()
        })
        .collect()
}

#[tokio::test]
async fn a_standalone_server_has_no_transactions() {
    let Some(conn) = connect().await else { return };
    assert!(!conn.capabilities().transactions);
    let Err(Error::Unsupported(why)) = conn.set_autocommit(false).await else {
        panic!("manual commit on a standalone server");
    };
    assert!(
        why.contains("replica set") && why.contains("standalone"),
        "{why}"
    );
    conn.set_autocommit(true).await.unwrap();
    assert_eq!(state(&*conn), (true, false));
    conn.commit().await.unwrap();
    conn.rollback().await.unwrap();
}

#[tokio::test]
async fn errors_end_the_transaction_only_when_the_server_does() {
    let Some((conn, other)) = manual_pair("t_tx_errors").await else {
        return;
    };
    let (conn, other) = (&*conn, &*other);
    affected(other, "db.t_tx_errors.insertOne({ _id: 1 })").await;

    // A pipeline the server refuses leaves the transaction as it was.
    affected(conn, "db.t_tx_errors.insertOne({ _id: 2 })").await;
    let bad = refused(conn, "db.t_tx_errors.aggregate([{ $nope: 1 }])").await;
    assert!(!bad.contains("ended the transaction"), "{bad}");
    assert_eq!(state(conn), (false, true));
    // A duplicate key ends it: the insert before it is gone.
    let duplicate = refused(conn, "db.t_tx_errors.insertOne({ _id: 1 })").await;
    assert!(
        duplicate.contains("E11000")
            && duplicate.contains("ended the transaction")
            && duplicate.contains("discarded"),
        "{duplicate}"
    );
    assert_eq!(state(conn), (false, false));
    assert_eq!(count(&rows(other, "db.t_tx_errors.find()").await), 1);
    // The next statement opens another.
    affected(conn, "db.t_tx_errors.insertOne({ _id: 3 })").await;
    assert_eq!(state(conn), (false, true));
    // An index on a collection that exists, which a transaction refuses, ends it too.
    let index = refused(conn, "db.t_tx_errors.createIndex({ a: 1 })").await;
    assert!(index.contains("ended the transaction"), "{index}");
    assert_eq!(state(conn), (false, false));
    conn.commit().await.unwrap();
    assert_eq!(count(&rows(other, "db.t_tx_errors.find()").await), 1);
}

#[tokio::test]
async fn writes_and_creations_run_in_the_transaction() {
    let Some((conn, other)) = manual_pair("t_tx_writes").await else {
        return;
    };
    let (conn, other) = (&*conn, &*other);
    let _ = other
        .execute("db.t_tx_fresh.drop()", &ExecOptions::default())
        .await;
    affected(
        conn,
        "db.t_tx_writes.insertMany([{ _id: 1, n: 1 }, { _id: 2, n: 2 }])",
    )
    .await;
    affected(
        conn,
        "db.t_tx_writes.updateOne({ _id: 1 }, { $inc: { n: 10 } })",
    )
    .await;
    affected(
        conn,
        "db.t_tx_writes.bulkWrite([{ deleteOne: { filter: { _id: 2 } } }, \
         { insertOne: { document: { _id: 3, n: 3 } } }])",
    )
    .await;
    let found = rows(
        conn,
        "db.t_tx_writes.findOneAndUpdate({ _id: 3 }, { $set: { n: 30 } }, \
         { returnDocument: 'after' })",
    )
    .await;
    assert_eq!(count(&found), 1);
    let n = rows(conn, "db.t_tx_writes.countDocuments({ n: { $gt: 10 } })").await;
    assert_eq!(
        n[0].column(0)
            .as_primitive::<arrow_array::types::Int64Type>()
            .value(0),
        2
    );
    assert_eq!(count(&rows(conn, "db.t_tx_writes.distinct('n')").await), 2);
    // A collection and its index made in the transaction exist only once it commits.
    affected(conn, "db.createCollection('t_tx_fresh')").await;
    affected(conn, "db.t_tx_fresh.createIndex({ a: 1 })").await;
    assert_eq!(count(&rows(other, "db.t_tx_writes.find()").await), 0);
    let listed = names(&rows(other, "db.getCollectionNames()").await);
    assert!(!listed.contains(&"t_tx_fresh".to_owned()), "{listed:?}");
    conn.commit().await.unwrap();
    assert_eq!(count(&rows(other, "db.t_tx_writes.find()").await), 2);
    assert_eq!(count(&rows(other, "db.t_tx_fresh.getIndexes()").await), 2);
    conn.set_autocommit(true).await.unwrap();
    affected(conn, "db.t_tx_fresh.drop()").await;
}

#[tokio::test]
async fn what_a_transaction_refuses_is_refused_in_manual_mode() {
    let Some((conn, _other)) = manual_pair("t_tx_refused").await else {
        return;
    };
    let conn = &*conn;
    affected(conn, "db.t_tx_refused.insertOne({ _id: 1 })").await;
    for src in [
        "db.t_tx_refused.drop()",
        "db.t_tx_refused.renameCollection('t_tx_elsewhere')",
        "db.t_tx_refused.dropIndex('a_1')",
        "db.t_tx_refused.aggregate([{ $out: 't_tx_out' }])",
        "db.createView('t_tx_view', 't_tx_refused', [])",
    ] {
        let Err(Error::Unsupported(why)) = conn.execute(src, &ExecOptions::default()).await else {
            panic!("{src}: not refused");
        };
        assert!(why.contains("manual commit"), "{src}: {why}");
        assert_eq!(state(conn), (false, true), "{src}");
    }
    assert_eq!(count(&rows(conn, "db.t_tx_refused.find()").await), 1);
    conn.rollback().await.unwrap();
    // With no transaction open too: a rollback could not undo it.
    assert!(matches!(
        conn.execute("db.t_tx_refused.drop()", &ExecOptions::default())
            .await,
        Err(Error::Unsupported(_))
    ));
    assert_eq!(state(conn), (false, false));
}

#[tokio::test]
async fn results_of_a_transaction_end_with_it() {
    let Some((conn, _other)) = manual_pair("t_tx_stale").await else {
        return;
    };
    let conn = &*conn;
    let docs: Vec<String> = (0..30).map(|i| format!("{{ _id: {i} }}")).collect();
    affected(
        conn,
        &format!("db.t_tx_stale.insertMany([{}])", docs.join(", ")),
    )
    .await;
    let options = ExecOptions {
        batch_size: 10,
        paged: true,
    };
    let ExecResult::Rows(mut stream) = conn
        .execute("db.t_tx_stale.find()", &options)
        .await
        .unwrap()
    else {
        panic!("expected rows");
    };
    let first = futures::StreamExt::next(&mut stream)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(first.num_rows(), 10);
    conn.rollback().await.unwrap();
    let rest = futures::StreamExt::next(&mut stream).await.unwrap();
    let Err(Error::Other(why)) = rest else {
        panic!("a result outlived its transaction: {rest:?}");
    };
    assert!(why.contains("has ended"), "{why}");
    assert_eq!(count(&rows(conn, "db.t_tx_stale.find()").await), 0);
}

#[tokio::test]
async fn a_cancel_rolls_the_transaction_back() {
    let Some((conn, other)) = manual_pair("t_tx_cancel").await else {
        return;
    };
    let (conn, other) = (&*conn, &*other);
    affected(conn, "db.t_tx_cancel.insertMany([{ _id: 1 }, { _id: 2 }])").await;
    let handle = conn.cancel_handle();
    let cancel = tokio::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        handle.cancel().await
    });
    let endless = "db.t_tx_cancel.find({ $where: 'sleep(60000) || true' })";
    let result = tokio::time::timeout(
        std::time::Duration::from_secs(30),
        conn.execute(endless, &ExecOptions::default()),
    )
    .await
    .expect("cancel did not stop the query");
    assert!(matches!(result, Err(Error::Cancelled)), "{result:?}");
    cancel.await.unwrap().unwrap();
    assert_eq!(state(conn), (false, false));
    assert_eq!(count(&rows(other, "db.t_tx_cancel.find()").await), 0);
    // The session goes on with another transaction.
    affected(conn, "db.t_tx_cancel.insertOne({ _id: 3 })").await;
    conn.commit().await.unwrap();
    assert_eq!(count(&rows(other, "db.t_tx_cancel.find()").await), 1);
}
