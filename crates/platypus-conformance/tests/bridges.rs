//! Databases reached through a bridge: Trino through its ADBC driver, downloaded from the
//! driver registry into `target/conformance-drivers` the first time. Skipped when the
//! container (`docker compose --profile trino up -d --wait`) or the registry is unreachable.

use std::path::PathBuf;
use std::time::Duration;

use arrow_array::RecordBatch;
use arrow_array::cast::AsArray as _;
use futures::{StreamExt as _, TryStreamExt as _};
use platypus_core::{
    Connection, ConnectionConfig, Driver as _, DriverKind, Error, ExecOptions, ExecResult,
    ObjectKind, SslMode,
};
use platypus_driver_adbc::AdbcDriver;
use tokio::sync::OnceCell;
use tokio_util::sync::CancellationToken;

const TRINO: (&str, u16) = ("127.0.0.1", 58085);

/// Where the tests keep the downloaded driver, across runs.
fn drivers_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/conformance-drivers")
}

/// The Trino driver, downloaded once; `None` when the registry cannot be reached.
async fn trino_driver() -> Option<&'static PathBuf> {
    static DIR: OnceCell<Option<PathBuf>> = OnceCell::const_new();
    DIR.get_or_init(|| async {
        let dir = drivers_dir();
        if platypus_drivers::installed(&dir, "trino").is_none() {
            let registry = platypus_drivers::Registry::public(&dir);
            let installed = async {
                let p = registry.package("trino").await?;
                registry
                    .install(&p, |_, _| {}, &CancellationToken::new())
                    .await
            }
            .await;
            if let Err(e) = installed {
                println!("skipping Trino: cannot download its ADBC driver: {e}");
                return None;
            }
        }
        Some(dir)
    })
    .await
    .as_ref()
}

/// A session on the Trino container, in `memory.default`.
async fn trino() -> Option<Box<dyn Connection>> {
    trino_in("default").await
}

/// A session on the Trino container, in `memory.<schema>`.
async fn trino_in(schema: &str) -> Option<Box<dyn Connection>> {
    let reachable = tokio::time::timeout(
        Duration::from_secs(2),
        tokio::net::TcpStream::connect(TRINO),
    )
    .await
    .is_ok_and(|c| c.is_ok());
    if !reachable {
        println!("skipping Trino: unreachable at {}:{}", TRINO.0, TRINO.1);
        return None;
    }
    let dir = trino_driver().await?;
    let mut config = ConnectionConfig::new(DriverKind::Trino);
    TRINO.0.clone_into(&mut config.host);
    config.port = Some(TRINO.1);
    "platypus".clone_into(&mut config.user);
    "memory".clone_into(&mut config.database);
    config.ssl_mode = SslMode::Disable;
    config
        .options
        .insert("schema".to_owned(), schema.to_owned());
    let driver = AdbcDriver {
        data_dir: dir.clone(),
    };
    Some(
        driver
            .connect(&config)
            .await
            .unwrap_or_else(|e| panic!("Trino: cannot connect: {e}")),
    )
}

async fn rows(conn: &dyn Connection, sql: &str) -> Vec<RecordBatch> {
    match conn.execute(sql, &ExecOptions::default()).await.unwrap() {
        ExecResult::Rows(stream) => stream.try_collect().await.unwrap(),
        ExecResult::Affected { .. } => panic!("{sql}: no rows"),
    }
}

#[tokio::test]
async fn trino_select_types() {
    let Some(conn) = trino().await else { return };
    assert_eq!(conn.server_info().product, "Trino");
    let batches = rows(
        &*conn,
        "SELECT 1 AS i, CAST(2.5 AS DECIMAL(5, 2)) AS d, 'x' AS t, true AS b, \
         DATE '2024-01-02' AS dt, TIMESTAMP '2024-01-02 03:04:05' AS ts, \
         CAST(NULL AS VARCHAR) AS n",
    )
    .await;
    let batch = &batches[0];
    assert_eq!((batch.num_rows(), batch.num_columns()), (1, 7));
    let names: Vec<_> = batch
        .schema()
        .fields()
        .iter()
        .map(|f| f.name().clone())
        .collect();
    assert_eq!(names, ["i", "d", "t", "b", "dt", "ts", "n"]);
    let text = arrow_cast::cast(batch.column(2), &arrow_schema::DataType::Utf8).unwrap();
    assert_eq!(text.as_string::<i32>().value(0), "x");
    assert!(batch.column(6).is_null(0));
}

#[tokio::test]
async fn trino_long_result_streams() {
    let Some(conn) = trino().await else { return };
    let options = ExecOptions {
        batch_size: 1000,
        paged: true,
    };
    let ExecResult::Rows(stream) = conn
        .execute(
            "SELECT a * 1000 + b AS x FROM UNNEST(sequence(0, 99)) AS s(a) \
             CROSS JOIN UNNEST(sequence(1, 1000)) AS t(b)",
            &options,
        )
        .await
        .unwrap()
    else {
        panic!("no rows");
    };
    let batches: Vec<RecordBatch> = stream.try_collect().await.unwrap();
    assert_eq!(
        batches.iter().map(RecordBatch::num_rows).sum::<usize>(),
        100_000
    );
    assert!(batches.iter().all(|b| b.num_rows() <= 1000));
}

#[tokio::test]
async fn trino_cancel_long_query() {
    let Some(conn) = trino().await else { return };
    let conn: std::sync::Arc<dyn Connection> = conn.into();
    let cancel = conn.cancel_handle();
    let running = {
        let conn = conn.clone();
        tokio::spawn(async move {
            // Generated on the fly: far longer than the test waits.
            conn.execute(
                "SELECT count(*) FROM tpch.sf1000.lineitem",
                &ExecOptions::default(),
            )
            .await
            .map(|_| ())
        })
    };
    tokio::time::sleep(Duration::from_secs(2)).await;
    cancel.cancel().await.unwrap();
    let outcome = tokio::time::timeout(Duration::from_secs(15), running)
        .await
        .expect("the cancelled query should end")
        .unwrap();
    assert!(matches!(outcome, Err(Error::Cancelled)), "{outcome:?}");
    // The session waits on the given-up statement: the app opens another one.
    assert!(conn.is_closed());
    let fresh = trino().await.unwrap();
    assert_eq!(rows(&*fresh, "SELECT 1").await[0].num_rows(), 1);
}

#[tokio::test]
async fn trino_metadata() {
    let Some(conn) = trino().await else { return };
    let table = "bridge_meta";
    let _ = conn
        .execute(
            &format!("DROP TABLE IF EXISTS {table}"),
            &ExecOptions::default(),
        )
        .await;
    conn.execute(
        &format!("CREATE TABLE {table} (id BIGINT, name VARCHAR)"),
        &ExecOptions::default(),
    )
    .await
    .unwrap();
    let schemas = conn.schemas().await.unwrap();
    let default = schemas.iter().find(|s| s.name == "default").unwrap();
    assert!(default.current);
    let objects = conn.objects("default").await.unwrap();
    let found = objects.iter().find(|o| o.name == table).unwrap();
    assert_eq!(found.kind, ObjectKind::Table);
    let details = conn.table_details("default", table).await.unwrap();
    let columns: Vec<_> = details.columns.iter().map(|c| c.name.as_str()).collect();
    assert_eq!(columns, ["id", "name"]);
    let catalog = conn.catalog_columns().await.unwrap();
    assert!(
        catalog
            .iter()
            .any(|c| c.schema == "default" && c.table == table && c.column == "id")
    );
    conn.execute(&format!("DROP TABLE {table}"), &ExecOptions::default())
        .await
        .unwrap();
}

#[tokio::test]
async fn trino_syntax_error_is_database_error() {
    let Some(conn) = trino().await else { return };
    let err = conn
        .execute("SELEC 1", &ExecOptions::default())
        .await
        .unwrap_err();
    assert!(matches!(err, Error::Database(_)), "{err}");
}

async fn run(conn: &dyn Connection, sql: &str) {
    if let Err(e) = conn.execute(sql, &ExecOptions::default()).await {
        panic!("{sql}: {e}");
    }
}

#[tokio::test]
async fn qa_fixdbs_trino_current_schema() {
    let Some(conn) = trino().await else { return };
    run(&*conn, "CREATE SCHEMA IF NOT EXISTS memory.qa_fixdbs_s").await;
    let other = trino_in("qa_fixdbs_s").await.unwrap();
    let schemas = other.schemas().await.unwrap();
    let current: Vec<_> = schemas
        .iter()
        .filter(|s| s.current)
        .map(|s| s.name.as_str())
        .collect();
    assert_eq!(current, ["qa_fixdbs_s"]);
    run(&*conn, "DROP SCHEMA memory.qa_fixdbs_s").await;
}

/// Values of the one-column result of `sql`, as text.
async fn texts(conn: &dyn Connection, sql: &str) -> Vec<Option<String>> {
    let mut out = Vec::new();
    for batch in rows(conn, sql).await {
        let text = arrow_cast::cast(batch.column(0), &arrow_schema::DataType::Utf8).unwrap();
        out.extend(text.as_string::<i32>().iter().map(|v| v.map(str::to_owned)));
    }
    out
}

#[tokio::test]
async fn qa_fixdbs_trino_statements_end_with_a_terminator() {
    let Some(conn) = trino().await else { return };
    run(&*conn, "DROP SCHEMA IF EXISTS memory.qa_fixdbs_t CASCADE").await;
    run(&*conn, "CREATE SCHEMA memory.qa_fixdbs_t;").await;
    run(&*conn, "CREATE TABLE memory.qa_fixdbs_t.g (id bigint);").await;
    run(&*conn, "INSERT INTO memory.qa_fixdbs_t.g VALUES (1);").await;
    run(
        &*conn,
        "ALTER TABLE \"qa_fixdbs_t\".\"g\" RENAME TO \"g2\";
-- renamed",
    )
    .await;
    let ddl = conn
        .object_ddl(&platypus_core::ObjectRef {
            schema: "qa_fixdbs_t",
            name: "g2",
            kind: ObjectKind::Table,
            parent: None,
        })
        .await
        .unwrap();
    assert!(ddl.contains("CREATE TABLE"), "{ddl}");
    assert!(conn.capabilities().object_ddl);
    assert_eq!(
        texts(&*conn, "SELECT count(*) FROM memory.qa_fixdbs_t.g2;").await,
        [Some("1".to_owned())]
    );
    run(&*conn, "DROP TABLE \"qa_fixdbs_t\".\"g2\";").await;
    run(&*conn, "DROP SCHEMA memory.qa_fixdbs_t").await;
}

#[tokio::test]
async fn qa_fixdbs_trino_generates_and_imports_typed_values() {
    use platypus_sql::Dialect;
    use platypus_transfer::fake::{FakeColumn, Generator, fill};
    use platypus_transfer::import::{
        ImportColumn, ImportMode, ImportPlan, import_rows, target_type,
    };

    let Some(conn) = trino().await else { return };
    run(&*conn, "DROP SCHEMA IF EXISTS memory.qa_fixdbs_g CASCADE").await;
    run(&*conn, "CREATE SCHEMA memory.qa_fixdbs_g").await;
    run(
        &*conn,
        "CREATE TABLE memory.qa_fixdbs_g.g (id bigint, ts timestamp(3), d date,          u uuid, name varchar, t time(3), j json)",
    )
    .await;
    let column = |name: &str, generator, data_type: &str| FakeColumn {
        name: name.to_owned(),
        generator,
        unique: false,
        data_type: Some(data_type.to_owned()),
    };
    let columns = vec![
        column("id", Generator::Integer { min: 1, max: 1000 }, "bigint"),
        column("ts", Generator::Timestamp, "timestamp(3)"),
        column("d", Generator::Date, "date"),
        column("u", Generator::Uuid, "uuid"),
        column("name", Generator::FullName, "varchar"),
        column("t", Generator::Time, "time(3)"),
        column("j", Generator::Document, "json"),
    ];
    let at = (Some("qa_fixdbs_g"), "g");
    let added = fill((&*conn, Dialect::Ansi), at, columns, 10, |_| true)
        .await
        .unwrap();
    assert_eq!(added, 10);
    assert_eq!(
        texts(&*conn, "SELECT count(*) FROM memory.qa_fixdbs_g.g").await,
        [Some("10".to_owned())]
    );
    // Import into a new table, its types as the dialog suggests them.
    let batch = rows(
        &*conn,
        "SELECT 1 AS id, 'x' AS name, 1.25e0 AS amount, DATE '2025-01-02' AS day,          TIMESTAMP '2025-01-02 10:00:00' AS ts",
    )
    .await
    .remove(0);
    let schema = batch.schema();
    let plan = ImportPlan {
        source_table: String::new(),
        source_schema: None,
        schema: Some("qa_fixdbs_g".to_owned()),
        table: "imp".to_owned(),
        create: true,
        mode: ImportMode::Append,
        keys: Vec::new(),
        primary_key: Vec::new(),
        identity: None,
        constraints: Vec::new(),
        columns: schema
            .fields()
            .iter()
            .map(|f| ImportColumn {
                source: f.name().clone(),
                target: f.name().clone(),
                data_type: Some(target_type(f, Dialect::Ansi)),
                not_null: false,
                default: None,
            })
            .collect(),
    };
    let stream =
        platypus_core::RecordBatchStream::new(schema, futures::stream::iter([Ok(batch)]).boxed());
    let imported = import_rows(stream, (&*conn, Dialect::Ansi), &plan, |_| true)
        .await
        .unwrap();
    assert_eq!(imported, 1);
    assert_eq!(
        texts(&*conn, "SELECT day FROM memory.qa_fixdbs_g.imp").await,
        [Some("2025-01-02".to_owned())]
    );
    run(&*conn, "DROP SCHEMA memory.qa_fixdbs_g CASCADE").await;
}

#[tokio::test]
async fn qa_fixdbs_trino_maps_rows_and_uuids() {
    let Some(conn) = trino().await else { return };
    let batches = rows(
        &*conn,
        "SELECT MAP(ARRAY['k'], ARRAY[1]) AS m, CAST(ROW(1, 'x') AS row(a integer, b varchar)) AS r,          UUID 'b0eebc99-9c0b-4ef8-bb6d-6bb9bd380a12' AS u",
    )
    .await;
    let batch = &batches[0];
    let text = |c: usize| batch.column(c).as_string::<i32>().value(0).to_owned();
    assert_eq!(text(0), "{k: 1}");
    assert_eq!(text(1), "{a: 1, b: x}");
    assert_eq!(text(2), "b0eebc99-9c0b-4ef8-bb6d-6bb9bd380a12");
}
