//! Databases reached through a bridge: Trino through its ADBC driver, downloaded from the
//! driver registry into `target/conformance-drivers` the first time. Skipped when the
//! container (`docker compose --profile trino up -d --wait`) or the registry is unreachable.

use std::path::PathBuf;
use std::time::Duration;

use arrow_array::RecordBatch;
use arrow_array::cast::AsArray as _;
use futures::TryStreamExt as _;
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
            let installed = match registry.package("trino").await {
                Ok(p) => {
                    registry
                        .install(&p, |_, _| {}, &CancellationToken::new())
                        .await
                }
                Err(e) => Err(e),
            };
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
        .insert("schema".to_owned(), "default".to_owned());
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
