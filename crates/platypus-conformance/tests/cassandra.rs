//! The scenarios of `conformance.rs` that apply to a CQL database, on Apache Cassandra and
//! `ScyllaDB`: server info, scripts, typed results and paging, the catalog (keyspaces, tables,
//! views, types, functions, keys and indexes) and its DDL, lightweight transactions, the
//! server's errors, cancel, and saving the grid's changes with their undo. Both come from
//! `docker/compose.yml` (`docker compose --profile cassandra --profile scylla up -d --wait
//! cassandra scylla`, ports 59042 and 59043, no login). `PLATYPUS_TEST_CASSANDRA` points at
//! another server (`cassandra://user:password@host:9042`) and makes it the only one, required.
//! A server that cannot be reached is skipped.
//!
//! Each test works in a keyspace of its own (`pdb_<test>`), which it drops first: they run in
//! parallel.

use std::time::Duration;

use arrow_cast::display::array_value_to_string;
use futures::TryStreamExt;
use platypus_core::arrow_array::{Array, RecordBatch};
use platypus_core::arrow_schema::DataType;
use platypus_core::{
    Connection, ConnectionConfig, Driver, Error, ExecOptions, ExecResult, META_DB_TYPE, ObjectKind,
    ObjectRef,
};
use platypus_driver_cassandra::CassandraDriver;
use platypus_driver_cassandra::cql;
use platypus_driver_cassandra::edit::{self, Edits, Udts, Update};

const DEFAULT_URLS: [&str; 2] = [
    "cassandra://cassandra:cassandra@localhost:59042",
    "cassandra://localhost:59043",
];

/// A server under test: its name and a connection.
struct Server {
    name: String,
    conn: Box<dyn Connection>,
}

async fn connect_to(url: &str) -> platypus_core::Result<Box<dyn Connection>> {
    let mut config = ConnectionConfig::from_url(url).unwrap();
    config.connect_timeout = Some(Duration::from_secs(5));
    CassandraDriver.connect(&config).await
}

/// The servers that answer; with `PLATYPUS_TEST_CASSANDRA`, that one, which must.
async fn servers() -> Vec<Server> {
    if std::env::var("PLATYPUS_TEST_ONLY").is_ok_and(|only| only != "cassandra") {
        return Vec::new();
    }
    let urls: Vec<(String, bool)> = match std::env::var("PLATYPUS_TEST_CASSANDRA") {
        Ok(url) => vec![(url, true)],
        Err(_) => DEFAULT_URLS
            .iter()
            .map(|u| ((*u).to_owned(), false))
            .collect(),
    };
    let mut out = Vec::new();
    for (url, required) in urls {
        match connect_to(&url).await {
            Ok(conn) => out.push(Server {
                name: conn.server_info().product.clone(),
                conn,
            }),
            Err(e) if !required => eprintln!("skipping cassandra: unreachable at {url}: {e}"),
            Err(e) => panic!("cannot connect to PLATYPUS_TEST_CASSANDRA: {e}"),
        }
    }
    out
}

async fn run(conn: &dyn Connection, src: &str) -> Vec<RecordBatch> {
    run_paged(conn, src, 1024).await
}

async fn run_paged(conn: &dyn Connection, src: &str, batch_size: usize) -> Vec<RecordBatch> {
    let options = ExecOptions {
        batch_size,
        paged: true,
    };
    match conn.execute(src, &options).await {
        Ok(ExecResult::Rows(stream)) => stream.try_collect().await.unwrap(),
        Ok(ExecResult::Affected { .. }) => panic!("{src}: rows expected"),
        Err(e) => panic!("{src}: {e}"),
    }
}

/// Runs a statement that returns no rows.
async fn exec(conn: &dyn Connection, src: &str) {
    match conn.execute(src, &ExecOptions::default()).await {
        Ok(ExecResult::Affected { rows }) => assert_eq!(rows, 0, "{src}"),
        Ok(ExecResult::Rows(_)) => panic!("{src}: no rows expected"),
        Err(e) => panic!("{src}: {e}"),
    }
}

async fn fails(conn: &dyn Connection, src: &str) -> Error {
    match conn.execute(src, &ExecOptions::default()).await {
        Ok(_) => panic!("{src}: expected an error"),
        Err(e) => e,
    }
}

/// The text of every cell of `batches`, by row (NULL as `None`).
fn cells(batches: &[RecordBatch]) -> Vec<Vec<Option<String>>> {
    let mut out = Vec::new();
    for batch in batches {
        for row in 0..batch.num_rows() {
            out.push(
                (0..batch.num_columns())
                    .map(|c| {
                        let column = batch.column(c);
                        (!column.is_null(row)).then(|| array_value_to_string(column, row).unwrap())
                    })
                    .collect(),
            );
        }
    }
    out
}

/// The cells of the rows a query returns.
async fn text_rows(conn: &dyn Connection, sql: &str) -> Vec<Vec<Option<String>>> {
    cells(&run(conn, sql).await)
}

fn rows(batches: &[RecordBatch]) -> usize {
    batches.iter().map(RecordBatch::num_rows).sum()
}

#[allow(clippy::unnecessary_wraps)]
fn some(text: &str) -> Option<String> {
    Some(text.to_owned())
}

/// An empty keyspace of that name, and the statements run in it.
async fn keyspace(conn: &dyn Connection, name: &str, statements: &[&str]) {
    exec(conn, &format!("DROP KEYSPACE IF EXISTS {name}")).await;
    exec(
        conn,
        &format!(
            "CREATE KEYSPACE {name} WITH replication = {{'class': 'SimpleStrategy', 'replication_factor': 1}}"
        ),
    )
    .await;
    for statement in statements {
        exec(conn, statement).await;
    }
}

#[tokio::test]
async fn reports_the_server_and_answers() {
    for Server { name, conn } in servers().await {
        let info = conn.server_info();
        assert!(matches!(name.as_str(), "Cassandra" | "ScyllaDB"), "{name}");
        assert!(info.version_number().is_some(), "{name}: {}", info.version);
        assert!(!info.encrypted);
        assert!(conn.check_alive().await);
        assert!(!conn.is_closed());
        let caps = conn.capabilities();
        assert!(caps.cancel && caps.object_ddl && !caps.transactions);
        assert!(conn.transaction().autocommit);
        assert!(conn.set_autocommit(false).await.is_err());
        let rows = run(
            &*conn,
            "SELECT release_version, cluster_name FROM system.local",
        )
        .await;
        assert_eq!(rows[0].num_rows(), 1, "{name}");
        // Nothing runs, so cancelling is not an error.
        conn.cancel_handle().cancel().await.unwrap();
        assert!(conn.check_alive().await);
    }
}

#[tokio::test]
async fn refuses_a_wrong_address_and_a_wrong_driver() {
    let mut config = ConnectionConfig::from_url("cassandra://127.0.0.1:1").unwrap();
    config.connect_timeout = Some(Duration::from_secs(2));
    let err = CassandraDriver.connect(&config).await.unwrap_err();
    assert!(matches!(err, Error::Connect(_)), "{err}");
    let other = ConnectionConfig::from_url("redis://localhost").unwrap();
    assert!(matches!(
        CassandraDriver.connect(&other).await,
        Err(Error::Config(_))
    ));
    let mut empty = ConnectionConfig::new(platypus_core::DriverKind::Cassandra);
    empty.host = " ".to_owned();
    assert!(matches!(
        CassandraDriver.connect(&empty).await,
        Err(Error::Config(_))
    ));
}

#[tokio::test]
async fn runs_scripts_and_keyspace_changes() {
    for Server { name, conn } in servers().await {
        let ks = "pdb_script";
        keyspace(&*conn, ks, &[]).await;
        // Several statements: the last one answers; strings keep their `;`.
        let result = run(
            &*conn,
            &format!(
                "CREATE TABLE {ks}.t (id int PRIMARY KEY, s text); -- a comment; here\n\
                 INSERT INTO {ks}.t (id, s) VALUES (1, 'a;b'); /* ; */\n\
                 INSERT INTO {ks}.t (id, s) VALUES (2, $$it's$$);\n\
                 SELECT id, s FROM {ks}.t;"
            ),
        )
        .await;
        let mut got = cells(&result);
        got.sort();
        assert_eq!(
            got,
            [[some("1"), some("a;b")], [some("2"), some("it's")]],
            "{name}"
        );
        // A batch holds the `;` of its statements.
        exec(
            &*conn,
            &format!(
                "BEGIN BATCH INSERT INTO {ks}.t (id, s) VALUES (3, 'c'); INSERT INTO {ks}.t (id, s) VALUES (4, 'd'); APPLY BATCH"
            ),
        )
        .await;
        assert_eq!(
            rows(&run(&*conn, &format!("SELECT * FROM {ks}.t")).await),
            4
        );
        // USE changes what bare names mean, and the schema marked current.
        exec(&*conn, &format!("USE {ks}")).await;
        assert_eq!(rows(&run(&*conn, "SELECT * FROM t WHERE id = 3").await), 1);
        let schemas = conn.schemas().await.unwrap();
        assert!(schemas.iter().any(|s| s.name == ks && s.current), "{name}");
        assert_eq!(schemas.iter().filter(|s| s.current).count(), 1);
        let err = fails(&*conn, "USE pdb_nonexistent").await;
        assert!(matches!(err, Error::Database(_)), "{err}");
        // Nothing but a comment is no statement.
        let err = fails(&*conn, " -- nothing\n ; ").await;
        assert!(err.to_string().contains("no statement"), "{err}");
        assert_eq!(cql::split("SELECT 1; ;").len(), 1);
    }
}

#[tokio::test]
async fn reads_every_type_as_a_typed_column() {
    for Server { name, conn } in servers().await {
        let ks = "pdb_types";
        keyspace(
            &*conn,
            ks,
            &[
                "CREATE TYPE pdb_types.address (street text, zip int, tags list<text>)",
                "CREATE TABLE pdb_types.t (\
                    id int PRIMARY KEY, a ascii, t text, ti tinyint, si smallint, bi bigint, vi varint, \
                    d decimal, f float, db double, b boolean, u uuid, tu timeuuid, ts timestamp, \
                    dt date, tm time, bl blob, ip inet, du duration, \
                    l list<int>, s set<text>, m map<text, int>, tup tuple<int, text>, \
                    addr frozen<address>, nested map<text, frozen<list<int>>>)",
                "INSERT INTO pdb_types.t (id, a, t, ti, si, bi, vi, d, f, db, b, u, tu, ts, dt, tm, bl, ip, du, l, s, m, tup, addr, nested) VALUES (\
                    1, 'ascii', 'Zażółć', -5, 300, 9223372036854775807, 123456789012345678901234567890, \
                    12345.6700, 1.5, 2.25, true, 123e4567-e89b-12d3-a456-426614174000, \
                    50554d6e-29bb-11e5-b345-feff819cdc9f, '2024-03-09T13:05:09.250Z', '2024-03-09', \
                    '13:05:09.123456789', 0xdeadbeef, '10.0.0.1', 1y2mo3d4h5m6s, \
                    [1, 2], {'x', 'y'}, {'k': 1}, (1, 'one'), {street: 'Main', zip: 12345, tags: ['a']}, {'q': [1, 2]})",
                "INSERT INTO pdb_types.t (id) VALUES (2)",
            ],
        )
        .await;
        let result = run(&*conn, &format!("SELECT * FROM {ks}.t WHERE id = 1")).await;
        let schema = result[0].schema();
        let field = |n: &str| schema.field_with_name(n).unwrap().clone();
        for (column, ty, db_type) in [
            ("id", DataType::Int32, "int"),
            ("a", DataType::Utf8, "ascii"),
            ("ti", DataType::Int8, "tinyint"),
            ("si", DataType::Int16, "smallint"),
            ("bi", DataType::Int64, "bigint"),
            ("vi", DataType::Utf8, "varint"),
            ("d", DataType::Utf8, "decimal"),
            ("f", DataType::Float32, "float"),
            ("db", DataType::Float64, "double"),
            ("b", DataType::Boolean, "boolean"),
            ("u", DataType::Utf8, "uuid"),
            ("tu", DataType::Utf8, "timeuuid"),
            ("dt", DataType::Date32, "date"),
            ("bl", DataType::Binary, "blob"),
            ("ip", DataType::Utf8, "inet"),
            ("du", DataType::Utf8, "duration"),
            ("l", DataType::Utf8, "list<int>"),
            ("m", DataType::Utf8, "map<text, int>"),
            ("tup", DataType::Utf8, "tuple<int, text>"),
            ("addr", DataType::Utf8, "address"),
            ("nested", DataType::Utf8, "map<text, list<int>>"),
        ] {
            let f = field(column);
            assert_eq!(f.data_type(), &ty, "{name}: {column}");
            assert_eq!(f.metadata()[META_DB_TYPE], db_type, "{name}: {column}");
        }
        assert!(matches!(field("ts").data_type(), DataType::Timestamp(..)));
        assert!(matches!(field("tm").data_type(), DataType::Time64(_)));
        let row = &cells(&result)[0];
        let at = |n: &str| row[schema.index_of(n).unwrap()].clone().unwrap();
        assert_eq!(at("t"), "Zażółć");
        assert_eq!(at("bi"), "9223372036854775807");
        assert_eq!(at("vi"), "123456789012345678901234567890");
        assert_eq!(at("d"), "12345.6700");
        assert_eq!(at("f"), "1.5");
        assert_eq!(at("u"), "123e4567-e89b-12d3-a456-426614174000");
        assert_eq!(at("tu"), "50554d6e-29bb-11e5-b345-feff819cdc9f");
        assert_eq!(at("dt"), "2024-03-09");
        assert!(
            at("ts").starts_with("2024-03-09T13:05:09.25"),
            "{}",
            at("ts")
        );
        assert!(at("tm").starts_with("13:05:09.123456789"), "{}", at("tm"));
        assert_eq!(at("ip"), "10.0.0.1");
        assert_eq!(at("du"), "1y2mo3d4h5m6s");
        assert_eq!(at("l"), "[1,2]");
        assert_eq!(at("m"), r#"{"k":1}"#);
        assert_eq!(at("tup"), r#"[1,"one"]"#);
        let addr: serde_json::Value = serde_json::from_str(&at("addr")).unwrap();
        assert_eq!(
            addr,
            serde_json::json!({"street": "Main", "zip": 12345, "tags": ["a"]})
        );
        let set: serde_json::Value = serde_json::from_str(&at("s")).unwrap();
        assert_eq!(set, serde_json::json!(["x", "y"]));
        assert_eq!(at("nested"), r#"{"q":[1,2]}"#);
        // The blob is bytes.
        let blobs = result[0]
            .column(schema.index_of("bl").unwrap())
            .as_any()
            .downcast_ref::<platypus_core::arrow_array::BinaryArray>()
            .unwrap();
        assert_eq!(blobs.value(0), [0xde, 0xad, 0xbe, 0xef]);
        // Unset columns are NULL, not empty.
        let empty = run(&*conn, &format!("SELECT * FROM {ks}.t WHERE id = 2")).await;
        let nulls = cells(&empty);
        assert_eq!(nulls[0][0], some("2"));
        assert!(
            nulls[0][1..].iter().all(Option::is_none),
            "{name}: {:?}",
            nulls[0]
        );
    }
}

#[tokio::test]
async fn pages_a_large_result_as_the_reader_asks() {
    for Server { name, conn } in servers().await {
        let ks = "pdb_paging";
        keyspace(
            &*conn,
            ks,
            &["CREATE TABLE pdb_paging.t (p int, c int, v text, PRIMARY KEY (p, c))"],
        )
        .await;
        for c in 0..250 {
            exec(
                &*conn,
                &format!("INSERT INTO {ks}.t (p, c, v) VALUES (1, {c}, 'v{c}')"),
            )
            .await;
        }
        let batches = run_paged(&*conn, &format!("SELECT c FROM {ks}.t WHERE p = 1"), 100).await;
        assert_eq!(
            batches
                .iter()
                .map(RecordBatch::num_rows)
                .collect::<Vec<_>>(),
            [100, 100, 50],
            "{name}"
        );
        let all = cells(&batches);
        assert_eq!(all[0][0], some("0"));
        assert_eq!(all[249][0], some("249"));
        // A reader that stops early leaves the rest unread.
        let ExecResult::Rows(mut stream) = conn
            .execute(
                &format!("SELECT c FROM {ks}.t WHERE p = 1"),
                &ExecOptions {
                    batch_size: 10,
                    paged: true,
                },
            )
            .await
            .unwrap()
        else {
            panic!("rows expected");
        };
        assert_eq!(stream.try_next().await.unwrap().unwrap().num_rows(), 10);
        drop(stream);
        assert!(conn.check_alive().await);
        // No rows: a stream of no batches, with its columns known.
        let ExecResult::Rows(stream) = conn
            .execute(
                &format!("SELECT c, v FROM {ks}.t WHERE p = 99"),
                &ExecOptions::default(),
            )
            .await
            .unwrap()
        else {
            panic!("rows expected");
        };
        assert_eq!(stream.schema().fields().len(), 2);
        assert_eq!(stream.try_collect::<Vec<_>>().await.unwrap().len(), 0);
        // A limit is the server's.
        let limited = run_paged(
            &*conn,
            &format!("SELECT c FROM {ks}.t WHERE p = 1 LIMIT 7"),
            5,
        )
        .await;
        assert_eq!(rows(&limited), 7);
    }
}

#[tokio::test]
async fn reports_lightweight_transactions() {
    for Server { name, conn } in servers().await {
        let ks = "pdb_lwt";
        keyspace(
            &*conn,
            ks,
            &["CREATE TABLE pdb_lwt.t (id int PRIMARY KEY, v text)"],
        )
        .await;
        let first = run(
            &*conn,
            &format!("INSERT INTO {ks}.t (id, v) VALUES (1, 'a') IF NOT EXISTS"),
        )
        .await;
        let applied = first[0].schema().field(0).name().clone();
        assert_eq!(applied, "[applied]", "{name}");
        assert_eq!(cells(&first)[0][0], some("true"));
        let second = run(
            &*conn,
            &format!("INSERT INTO {ks}.t (id, v) VALUES (1, 'b') IF NOT EXISTS"),
        )
        .await;
        let row = &cells(&second)[0];
        assert_eq!(row[0], some("false"), "{name}");
        // The row that is there comes back with it.
        assert!(row.contains(&some("a")), "{row:?}");
        let update = run(
            &*conn,
            &format!("UPDATE {ks}.t SET v = 'c' WHERE id = 1 IF v = 'zzz'"),
        )
        .await;
        assert_eq!(cells(&update)[0][0], some("false"));
        let update = run(
            &*conn,
            &format!("UPDATE {ks}.t SET v = 'c' WHERE id = 1 IF v = 'a'"),
        )
        .await;
        assert_eq!(cells(&update)[0][0], some("true"));
        assert_eq!(
            cells(&run(&*conn, &format!("SELECT v FROM {ks}.t")).await),
            [[some("c")]]
        );
    }
}

#[tokio::test]
async fn reports_the_servers_errors() {
    for Server { name, conn } in servers().await {
        let ks = "pdb_errors";
        keyspace(
            &*conn,
            ks,
            &["CREATE TABLE pdb_errors.t (id int PRIMARY KEY, v text)"],
        )
        .await;
        let syntax = fails(&*conn, "SELEC 1").await;
        let Error::Database(db) = &syntax else {
            panic!("{name}: {syntax}");
        };
        assert_eq!(db.code.as_deref(), Some("SyntaxError"), "{name}: {db}");
        let missing = fails(&*conn, &format!("SELECT * FROM {ks}.nope")).await;
        let Error::Database(db) = &missing else {
            panic!("{name}: {missing}");
        };
        assert_eq!(db.code.as_deref(), Some("Invalid"), "{name}: {db}");
        assert!(db.message.to_lowercase().contains("nope"), "{db}");
        let exists = fails(&*conn, &format!("CREATE TABLE {ks}.t (id int PRIMARY KEY)")).await;
        let Error::Database(db) = &exists else {
            panic!("{name}: {exists}");
        };
        assert_eq!(db.code.as_deref(), Some("AlreadyExists"), "{name}: {db}");
        // An error in a later page surfaces from the stream; a wrong type from the statement.
        let invalid = fails(
            &*conn,
            &format!("INSERT INTO {ks}.t (id, v) VALUES ('x', 1)"),
        )
        .await;
        assert!(matches!(invalid, Error::Database(_)), "{invalid}");
        // The connection works after errors.
        assert!(conn.check_alive().await);
        assert!(!conn.is_closed());
    }
}

#[tokio::test]
async fn cancels_a_result_being_read() {
    for Server { name, conn } in servers().await {
        let ks = "pdb_cancel";
        keyspace(
            &*conn,
            ks,
            &["CREATE TABLE pdb_cancel.t (p int, c int, PRIMARY KEY (p, c))"],
        )
        .await;
        for c in 0..60 {
            exec(
                &*conn,
                &format!("INSERT INTO {ks}.t (p, c) VALUES (1, {c})"),
            )
            .await;
        }
        let ExecResult::Rows(mut stream) = conn
            .execute(
                &format!("SELECT c FROM {ks}.t WHERE p = 1"),
                &ExecOptions {
                    batch_size: 10,
                    paged: true,
                },
            )
            .await
            .unwrap()
        else {
            panic!("rows expected");
        };
        assert!(stream.try_next().await.unwrap().is_some());
        conn.cancel_handle().cancel().await.unwrap();
        let err = stream.try_next().await.unwrap_err();
        assert!(matches!(err, Error::Cancelled), "{name}: {err}");
        // The next statement is not affected.
        assert_eq!(
            rows(&run(&*conn, &format!("SELECT c FROM {ks}.t WHERE p = 1")).await),
            60
        );
    }
}

fn object<'a>(schema: &'a str, name: &'a str, kind: ObjectKind) -> ObjectRef<'a> {
    ObjectRef {
        schema,
        name,
        kind,
        parent: None,
    }
}

/// A keyspace with a table of every kind of key, a type, a view and a secondary index.
async fn catalog_keyspace(conn: &dyn Connection, ks: &str) {
    let statements = [
        format!("CREATE TYPE {ks}.address (street text, zip int)"),
        format!(
            "CREATE TABLE {ks}.orders (\
                region text, customer int, placed timestamp, note text, tags set<text>, total int, owner text STATIC, \
                PRIMARY KEY ((region, customer), placed)) WITH CLUSTERING ORDER BY (placed DESC) \
                AND comment = 'the orders' AND default_time_to_live = 3600"
        ),
        format!("CREATE TABLE {ks}.\"MixedCase\" (\"Id\" int PRIMARY KEY, v int)"),
        format!("CREATE INDEX orders_note ON {ks}.orders (note)"),
        format!(
            "CREATE MATERIALIZED VIEW {ks}.orders_by_total AS SELECT region, customer, placed, note, total FROM {ks}.orders \
                WHERE total IS NOT NULL AND region IS NOT NULL AND customer IS NOT NULL AND placed IS NOT NULL \
                PRIMARY KEY (total, region, customer, placed)"
        ),
    ];
    let statements: Vec<&str> = statements.iter().map(String::as_str).collect();
    keyspace(conn, ks, &statements).await;
}

#[tokio::test]
async fn lists_the_catalog() {
    for Server { name, conn } in servers().await {
        let ks = "pdb_catalog";
        catalog_keyspace(&*conn, ks).await;
        let schemas = conn.schemas().await.unwrap();
        let find = |n: &str| schemas.iter().find(|s| s.name == n);
        assert!(find("system").unwrap().system, "{name}");
        assert!(find("system_schema").unwrap().system);
        assert!(!find(ks).unwrap().system);
        let objects = conn.objects(ks).await.unwrap();
        let kinds: Vec<(String, ObjectKind)> =
            objects.iter().map(|o| (o.name.clone(), o.kind)).collect();
        for expected in [
            ("MixedCase", ObjectKind::Table),
            ("orders", ObjectKind::Table),
            ("orders_by_total", ObjectKind::MaterializedView),
            ("address", ObjectKind::Type),
        ] {
            assert!(
                kinds.contains(&(expected.0.to_owned(), expected.1)),
                "{name}: {kinds:?}"
            );
        }
        let orders = objects.iter().find(|o| o.name == "orders").unwrap();
        assert_eq!(orders.comment.as_deref(), Some("the orders"));
        let view = objects
            .iter()
            .find(|o| o.name == "orders_by_total")
            .unwrap();
        assert_eq!(view.parent.as_deref(), Some("orders"));
        // The system keyspace has its tables too.
        assert!(
            conn.objects("system_schema")
                .await
                .unwrap()
                .iter()
                .any(|o| o.name == "tables"),
            "{name}"
        );
    }
}

#[tokio::test]
async fn describes_a_table() {
    for Server { name, conn } in servers().await {
        let ks = "pdb_details";
        catalog_keyspace(&*conn, ks).await;
        // Keys and columns, in CQL's order.
        let details = conn.table_details(ks, "orders").await.unwrap();
        let names: Vec<&str> = details.columns.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(
            &names[..3],
            ["region", "customer", "placed"],
            "{name}: {names:?}"
        );
        let key: Vec<&str> = details
            .columns
            .iter()
            .filter(|c| c.primary_key)
            .map(|c| c.name.as_str())
            .collect();
        assert_eq!(key, ["region", "customer", "placed"]);
        let by = |n: &str| details.columns.iter().find(|c| c.name == n).unwrap();
        assert_eq!(by("region").comment.as_deref(), Some("partition key"));
        assert_eq!(
            by("placed").comment.as_deref(),
            Some("clustering key, descending")
        );
        assert_eq!(by("owner").comment.as_deref(), Some("static"));
        assert_eq!(by("tags").data_type, "set<text>");
        assert!(by("note").nullable && !by("region").nullable);
        let primary = details.indexes.iter().find(|i| i.primary).unwrap();
        assert_eq!(primary.columns, ["region", "customer", "placed"]);
        let secondary = details
            .indexes
            .iter()
            .find(|i| i.name == "orders_note")
            .unwrap();
        assert_eq!(secondary.columns, ["note"]);
        assert!(!secondary.primary && !secondary.unique);
        assert_eq!(
            details.constraints[0].definition,
            "PRIMARY KEY ((region, customer), placed)"
        );
        let mixed = conn.table_details(ks, "MixedCase").await.unwrap();
        assert_eq!(mixed.columns[0].name, "Id");
        let view_details = conn.table_details(ks, "orders_by_total").await.unwrap();
        assert_eq!(view_details.columns[0].name, "total");
        let err = conn.table_details(ks, "nope").await.unwrap_err();
        assert!(err.to_string().contains("nope"), "{err}");
        // Completion sees the columns.
        let catalog = conn.catalog_columns().await.unwrap();
        assert!(
            catalog
                .iter()
                .any(|c| c.schema == ks && c.table == "orders" && c.column == "note"),
            "{name}"
        );
        assert_eq!(conn.referencing_keys(ks, "orders").await.unwrap().len(), 0);
    }
}

#[tokio::test]
async fn writes_the_ddl_of_objects() {
    for Server { name, conn } in servers().await {
        let ks = "pdb_ddl";
        keyspace(
            &*conn,
            ks,
            &[
                "CREATE TYPE pdb_ddl.address (street text, zip int)",
                "CREATE TABLE pdb_ddl.orders (\
                    region text, customer int, placed timestamp, note text, owner text STATIC, addr frozen<address>, \
                    PRIMARY KEY ((region, customer), placed)) WITH CLUSTERING ORDER BY (placed DESC) \
                    AND comment = 'it''s orders' AND default_time_to_live = 3600 AND gc_grace_seconds = 100",
                "CREATE INDEX orders_note ON pdb_ddl.orders (note)",
                "CREATE MATERIALIZED VIEW pdb_ddl.by_note AS SELECT region, customer, placed, note FROM pdb_ddl.orders \
                    WHERE note IS NOT NULL AND region IS NOT NULL AND customer IS NOT NULL AND placed IS NOT NULL \
                    PRIMARY KEY (note, region, customer, placed)",
            ],
        )
        .await;
        let ddl = conn
            .object_ddl(&object(ks, "orders", ObjectKind::Table))
            .await
            .unwrap();
        assert!(
            ddl.starts_with("CREATE TABLE pdb_ddl.orders ("),
            "{name}: {ddl}"
        );
        assert!(ddl.contains("owner text STATIC"), "{ddl}");
        assert!(
            ddl.contains("PRIMARY KEY ((region, customer), placed)"),
            "{ddl}"
        );
        assert!(ddl.contains("CLUSTERING ORDER BY (placed DESC)"), "{ddl}");
        assert!(ddl.contains("comment = 'it''s orders'"), "{ddl}");
        assert!(ddl.contains("default_time_to_live = 3600"), "{ddl}");
        assert!(
            ddl.contains("CREATE INDEX orders_note ON pdb_ddl.orders (note);"),
            "{ddl}"
        );
        // The DDL runs: drop the table and create it again from it.
        let create = ddl.split("\nCREATE INDEX").next().unwrap().to_owned();
        exec(&*conn, "DROP MATERIALIZED VIEW pdb_ddl.by_note").await;
        exec(&*conn, "DROP TABLE pdb_ddl.orders").await;
        exec(&*conn, &create).await;
        let again = conn
            .object_ddl(&object(ks, "orders", ObjectKind::Table))
            .await
            .unwrap();
        assert!(
            again.contains("PRIMARY KEY ((region, customer), placed)"),
            "{name}: {again}"
        );
        let ty = conn
            .object_ddl(&object(ks, "address", ObjectKind::Type))
            .await
            .unwrap();
        assert_eq!(
            ty, "CREATE TYPE pdb_ddl.address (\n    street text,\n    zip int\n);\n",
            "{name}"
        );
        let missing = conn
            .object_ddl(&object(ks, "nope", ObjectKind::Table))
            .await;
        assert!(missing.is_err());
        let sequence = conn
            .object_ddl(&object(ks, "x", ObjectKind::Sequence))
            .await;
        assert!(matches!(sequence, Err(Error::Unsupported(_))));
        let props = conn
            .object_properties(&object(ks, "orders", ObjectKind::Table))
            .await
            .unwrap();
        let prop = |n: &str| props.iter().find(|p| p.name == n).map(|p| p.value.clone());
        assert_eq!(
            prop("Partition key").as_deref(),
            Some("region, customer"),
            "{name}"
        );
        assert_eq!(prop("Clustering key").as_deref(), Some("placed"));
        assert_eq!(prop("default_time_to_live").as_deref(), Some("3600"));
    }
}

#[tokio::test]
async fn lists_functions_and_aggregates_with_their_ddl() {
    for Server { name, conn } in servers().await {
        let ks = "pdb_funcs";
        keyspace(
            &*conn,
            ks,
            &["CREATE TABLE pdb_funcs.t (id int PRIMARY KEY, v int, n text)"],
        )
        .await;
        // Cassandra runs Java; ScyllaDB runs Lua and Wasm.
        let (language, body, state) = if name == "Cassandra" {
            (
                "java",
                "return x + 1;",
                "return (state == null ? 0 : state) + v;",
            )
        } else {
            ("lua", "return x + 1", "return (state or 0) + v")
        };
        exec(
            &*conn,
            &format!("CREATE FUNCTION pdb_funcs.plus_one (x int) CALLED ON NULL INPUT RETURNS int LANGUAGE {language} AS '{body}'"),
        )
        .await;
        exec(
            &*conn,
            &format!("CREATE FUNCTION pdb_funcs.add_state (state int, v int) CALLED ON NULL INPUT RETURNS int LANGUAGE {language} AS '{state}'"),
        )
        .await;
        exec(
            &*conn,
            "CREATE AGGREGATE pdb_funcs.total (int) SFUNC add_state STYPE int INITCOND 0",
        )
        .await;
        let objects = conn.objects(ks).await.unwrap();
        let find = |n: &str| objects.iter().find(|o| o.name == n);
        assert_eq!(
            find("plus_one(int)").unwrap().kind,
            ObjectKind::Function,
            "{name}"
        );
        assert_eq!(
            find("add_state(int, int)").unwrap().kind,
            ObjectKind::Function
        );
        assert_eq!(
            find("total(int)").unwrap().comment.as_deref(),
            Some("aggregate")
        );
        let ddl = conn
            .object_ddl(&object(ks, "plus_one(int)", ObjectKind::Function))
            .await
            .unwrap();
        assert!(
            ddl.starts_with("CREATE OR REPLACE FUNCTION pdb_funcs.plus_one (x int)"),
            "{name}: {ddl}"
        );
        assert!(ddl.contains("CALLED ON NULL INPUT"), "{ddl}");
        assert!(ddl.contains(&format!("LANGUAGE {language}")), "{ddl}");
        assert!(ddl.contains("RETURNS int"), "{ddl}");
        let aggregate = conn
            .object_ddl(&object(ks, "total(int)", ObjectKind::Function))
            .await
            .unwrap();
        assert!(
            aggregate.contains("CREATE OR REPLACE AGGREGATE pdb_funcs.total (int)"),
            "{aggregate}"
        );
        assert!(aggregate.contains("SFUNC add_state"), "{aggregate}");
        assert!(aggregate.contains("STYPE int"), "{aggregate}");
        assert!(aggregate.contains("INITCOND 0"), "{aggregate}");
        // The generated statements run.
        exec(&*conn, "DROP AGGREGATE pdb_funcs.total").await;
        exec(&*conn, &aggregate).await;
        exec(&*conn, "INSERT INTO pdb_funcs.t (id, v) VALUES (1, 5)").await;
        exec(&*conn, "INSERT INTO pdb_funcs.t (id, v) VALUES (2, 7)").await;
        assert_eq!(
            cells(&run(&*conn, "SELECT pdb_funcs.total(v) FROM pdb_funcs.t").await),
            [[some("12")]],
            "{name}"
        );
        assert!(
            conn.object_ddl(&object(ks, "nope(int)", ObjectKind::Function))
                .await
                .is_err()
        );
    }
}

/// The columns, CQL types and user-defined types of the table `ks.items`, which holds two
/// rows.
struct Items {
    columns: Vec<String>,
    types: Vec<String>,
    udts: Udts,
}

async fn items(conn: &dyn Connection, ks: &str) -> Items {
    keyspace(
        conn,
        ks,
        &[
            &format!("CREATE TYPE {ks}.address (street text, zip int)"),
            &format!(
                "CREATE TABLE {ks}.items (\
                    id int, day date, name text, qty int, price decimal, tags set<text>, props map<text, int>, \
                    addr frozen<address>, data blob, ref uuid, at timestamp, \
                    PRIMARY KEY (id, day))"
            ),
            &format!("INSERT INTO {ks}.items (id, day, name, qty, tags) VALUES (1, '2024-01-01', 'old', 5, {{'a'}})"),
            &format!("INSERT INTO {ks}.items (id, day, name) VALUES (2, '2024-01-02', 'gone')"),
        ],
    )
    .await;
    let owned = |names: &[&str]| names.iter().map(|s| (*s).to_owned()).collect();
    Items {
        columns: owned(&[
            "id", "day", "name", "qty", "price", "tags", "props", "addr", "data", "ref", "at",
        ]),
        types: owned(&[
            "int",
            "date",
            "text",
            "int",
            "decimal",
            "set<text>",
            "map<text, int>",
            "address",
            "blob",
            "uuid",
            "timestamp",
        ]),
        udts: platypus_driver_cassandra::meta::udts(conn, ks)
            .await
            .unwrap(),
    }
}

/// The rows of `ks.items` as the grid loads them.
async fn load_items(
    conn: &dyn Connection,
    ks: &str,
    columns: &[String],
) -> Vec<Vec<Option<String>>> {
    text_rows(
        conn,
        &format!("SELECT {} FROM {ks}.items", columns.join(", ")),
    )
    .await
}

/// The cell of a column holding JSON, parsed.
fn json_cell(cell: Option<&String>) -> serde_json::Value {
    serde_json::from_str(cell.unwrap()).unwrap()
}

#[tokio::test]
async fn saves_the_grids_changes() {
    for Server { name, conn } in servers().await {
        let ks = "pdb_edit";
        let Items {
            columns,
            types,
            udts,
        } = items(&*conn, ks).await;
        assert_eq!(
            udts["address"][1],
            ("zip".to_owned(), "int".to_owned()),
            "{name}"
        );
        let before = load_items(&*conn, ks, &columns).await;
        let row1 = before.iter().find(|r| r[0] == some("1")).unwrap().clone();
        let row2 = before.iter().find(|r| r[0] == some("2")).unwrap().clone();
        let edits = Edits {
            keyspace: ks,
            table: "items",
            columns: &columns,
            types: &types,
            key: &[0, 1],
            udts: &udts,
            updates: vec![Update {
                loaded: row1,
                set: vec![
                    (2, some("it's new")),
                    (3, None),
                    (4, some("12.50")),
                    (5, some(r#"["b","c"]"#)),
                    (6, some(r#"{"x":1}"#)),
                    (7, some(r#"{"street":"Main","zip":7}"#)),
                    (8, some("0xCAFE")),
                    (9, some("123e4567-e89b-12d3-a456-426614174000")),
                    (10, some("2024-03-09 13:05:09.250+00:00")),
                ],
            }],
            inserts: vec![vec![
                (0, some("3")),
                (1, some("2024-02-02")),
                (2, some("fresh")),
            ]],
            deletes: vec![row2],
        };
        let script = edit::script(&edits).unwrap();
        let refs: Vec<&str> = script.commands.iter().map(String::as_str).collect();
        let ran = conn.execute_all(&refs, &[]).await;
        assert!(ran.error.is_none(), "{name}: {:?}\n{refs:#?}", ran.error);
        assert_eq!(ran.rows.len(), 3);
        let after = load_items(&*conn, ks, &columns).await;
        assert_eq!(after.len(), 2, "{name}: {after:?}");
        let new1 = after.iter().find(|r| r[0] == some("1")).unwrap();
        assert_eq!(new1[2], some("it's new"));
        assert_eq!(new1[3], None);
        assert_eq!(new1[4], some("12.50"));
        assert_eq!(
            new1[8].as_deref().map(str::to_lowercase).as_deref(),
            Some("cafe")
        );
        assert_eq!(new1[9], some("123e4567-e89b-12d3-a456-426614174000"));
        assert!(
            new1[10]
                .as_deref()
                .unwrap()
                .starts_with("2024-03-09T13:05:09.25"),
            "{:?}",
            new1[10]
        );
        assert_eq!(json_cell(new1[5].as_ref()), serde_json::json!(["b", "c"]));
        assert_eq!(json_cell(new1[6].as_ref()), serde_json::json!({"x": 1}));
        assert_eq!(
            json_cell(new1[7].as_ref()),
            serde_json::json!({"street": "Main", "zip": 7})
        );
        let new3 = after.iter().find(|r| r[0] == some("3")).unwrap();
        assert_eq!(
            (new3[1].clone(), new3[2].clone()),
            (some("2024-02-02"), some("fresh"))
        );
    }
}

#[tokio::test]
async fn undoes_the_grids_changes() {
    for Server { name, conn } in servers().await {
        let ks = "pdb_undo";
        let Items {
            columns,
            types,
            udts,
        } = items(&*conn, ks).await;
        let before = load_items(&*conn, ks, &columns).await;
        let row1 = before.iter().find(|r| r[0] == some("1")).unwrap().clone();
        let row2 = before.iter().find(|r| r[0] == some("2")).unwrap().clone();
        let edits = Edits {
            keyspace: ks,
            table: "items",
            columns: &columns,
            types: &types,
            key: &[0, 1],
            udts: &udts,
            updates: vec![Update {
                loaded: row1,
                set: vec![
                    (2, some("changed")),
                    (3, some("9")),
                    (5, None),
                    (6, some(r#"{"x":1}"#)),
                ],
            }],
            inserts: vec![vec![
                (0, some("3")),
                (1, some("2024-02-02")),
                (2, some("fresh")),
            ]],
            deletes: vec![row2],
        };
        let script = edit::script(&edits).unwrap();
        let refs: Vec<&str> = script.commands.iter().map(String::as_str).collect();
        let ran = conn.execute_all(&refs, &[]).await;
        assert!(ran.error.is_none(), "{name}: {:?}", ran.error);
        assert_eq!(load_items(&*conn, ks, &columns).await.len(), 2);
        // The undo script puts back the updated and deleted rows and removes the new one.
        assert!(script.undo.iter().any(|s| s.starts_with("--")));
        let undo: Vec<&str> = script
            .undo
            .iter()
            .filter(|s| !s.starts_with("--"))
            .map(String::as_str)
            .collect();
        let ran = conn.execute_all(&undo, &[]).await;
        assert!(ran.error.is_none(), "{name}: {:?}\n{undo:#?}", ran.error);
        let mut restored = load_items(&*conn, ks, &columns).await;
        let mut expected = before;
        restored.sort();
        expected.sort();
        assert_eq!(restored.len(), expected.len(), "{name}: {restored:?}");
        for (r, e) in restored.iter().zip(&expected) {
            for i in [0, 1, 2, 3, 4, 8, 9, 10] {
                assert_eq!(r[i], e[i], "{name}: column {}", columns[i]);
            }
            // The set and map come back as the same JSON.
            for i in [5, 6, 7] {
                assert_eq!(
                    r[i].is_some(),
                    e[i].is_some(),
                    "{name}: column {}",
                    columns[i]
                );
                if r[i].is_some() {
                    assert_eq!(json_cell(r[i].as_ref()), json_cell(e[i].as_ref()));
                }
            }
        }
    }
}

#[tokio::test]
async fn stops_at_the_first_statement_that_fails() {
    for Server { name, conn } in servers().await {
        let ks = "pdb_stop";
        keyspace(
            &*conn,
            ks,
            &["CREATE TABLE pdb_stop.items (id int PRIMARY KEY)"],
        )
        .await;
        let bad = [
            "INSERT INTO pdb_stop.items (id) VALUES (9)",
            "INSERT INTO pdb_stop.nope (id) VALUES (1)",
            "INSERT INTO pdb_stop.items (id) VALUES (10)",
        ];
        let ran = conn.execute_all(&bad, &[]).await;
        assert_eq!(ran.rows.len(), 1, "{name}");
        assert!(ran.error.is_some());
        assert_eq!(
            rows(&run(&*conn, &format!("SELECT id FROM {ks}.items WHERE id = 10")).await),
            0
        );
        assert_eq!(
            rows(&run(&*conn, &format!("SELECT id FROM {ks}.items WHERE id = 9")).await),
            1
        );
    }
}

#[tokio::test]
async fn connects_through_a_tunnel_address_and_refuses_tls_on_a_plain_port() {
    for url in DEFAULT_URLS {
        let Ok(plain) = connect_to(url).await else {
            continue;
        };
        drop(plain);
        // `host` names the server; `connect_addr` is where the tunnel's local end listens.
        let mut config = ConnectionConfig::from_url(url).unwrap();
        let port = config.port.unwrap();
        config.host = "cassandra.invalid".to_owned();
        config.port = None;
        config.connect_addr = Some(std::net::SocketAddr::from(([127, 0, 0, 1], port)));
        config.connect_timeout = Some(Duration::from_secs(5));
        let conn = CassandraDriver.connect(&config).await.unwrap();
        assert!(conn.check_alive().await, "{url}");
        assert_eq!(rows(&run(&*conn, "SELECT key FROM system.local").await), 1);
        // TLS asked of a port that speaks plain CQL: a connection error, not a hang.
        let mut tls = ConnectionConfig::from_url(url).unwrap();
        tls.ssl_mode = platypus_core::SslMode::Require;
        tls.connect_timeout = Some(Duration::from_secs(5));
        let err = CassandraDriver.connect(&tls).await.unwrap_err();
        assert!(matches!(err, Error::Connect(_)), "{url}: {err}");
    }
}

#[tokio::test]
async fn refuses_a_wrong_or_missing_login() {
    // Only the compose Cassandra has a login (the user `cassandra`).
    if std::env::var("PLATYPUS_TEST_CASSANDRA").is_ok()
        || connect_to(DEFAULT_URLS[0]).await.is_err()
    {
        return;
    }
    for url in [
        "cassandra://cassandra:wrong@localhost:59042",
        "cassandra://nobody:cassandra@localhost:59042",
        "cassandra://localhost:59042",
    ] {
        let Err(err) = connect_to(url).await else {
            panic!("{url}: connected without a valid login");
        };
        let Error::Connect(message) = &err else {
            panic!("{url}: {err}");
        };
        let lower = message.to_lowercase();
        assert!(
            lower.contains("authenticat")
                || lower.contains("credential")
                || lower.contains("password"),
            "{url}: {message}"
        );
    }
}
