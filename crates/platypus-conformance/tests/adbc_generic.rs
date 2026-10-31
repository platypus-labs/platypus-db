//! The generic ADBC driver ("ADBC with your own library"), tried on the ASF SQLite ADBC driver,
//! which the driver registry serves: it is downloaded into `target/conformance-drivers` the
//! first time (the same folder as the Trino driver of `bridges.rs`) and loaded as the file the
//! user would choose. Skipped when the registry cannot be reached.

use std::path::PathBuf;

use arrow_array::cast::AsArray as _;
use arrow_array::{Array as _, RecordBatch};
use futures::TryStreamExt as _;
use platypus_core::{
    Connection, ConnectionConfig, Driver as _, DriverKind, Error, ExecOptions, ExecResult,
};
use platypus_driver_adbc::AdbcDriver;
use tokio_util::sync::CancellationToken;

fn drivers_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/conformance-drivers")
}

/// The library of the registry's ADBC driver `package`, downloaded the first time; `None` when
/// the registry is out of reach.
async fn library(package: &str) -> Option<PathBuf> {
    let dir = drivers_dir();
    if platypus_drivers::installed(&dir, package).is_none() {
        let registry = platypus_drivers::Registry::public(&dir);
        let installed = async {
            let p = registry.package(package).await?;
            registry
                .install(&p, |_, _| {}, &CancellationToken::new())
                .await
        }
        .await;
        if let Err(e) = installed {
            println!("skipping the generic ADBC driver: cannot download {package}: {e}");
            return None;
        }
    }
    platypus_drivers::installed(&dir, package).map(|i| i.library)
}

fn config(library: &std::path::Path, path: &std::path::Path) -> ConnectionConfig {
    let mut config = ConnectionConfig::new(DriverKind::Adbc);
    config
        .options
        .insert("driver_file".to_owned(), library.display().to_string());
    config
        .options
        .insert("uri".to_owned(), path.display().to_string());
    config
}

fn driver() -> AdbcDriver {
    AdbcDriver {
        data_dir: drivers_dir(),
    }
}

async fn rows(conn: &dyn Connection, sql: &str) -> Vec<RecordBatch> {
    match conn.execute(sql, &ExecOptions::default()).await {
        Ok(ExecResult::Rows(stream)) => stream.try_collect().await.unwrap(),
        Ok(ExecResult::Affected { .. }) => Vec::new(),
        Err(e) => panic!("{sql}: {e}"),
    }
}

#[tokio::test]
async fn generic_adbc_runs_queries_on_the_chosen_library() {
    let Some(library) = library("sqlite").await else {
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("generic.db");
    let conn = driver()
        .connect(&config(&library, &file))
        .await
        .unwrap_or_else(|e| panic!("cannot load {}: {e}", library.display()));

    rows(
        &*conn,
        "CREATE TABLE t (id INTEGER, name VARCHAR, price DECIMAL(6, 2))",
    )
    .await;
    rows(
        &*conn,
        "INSERT INTO t VALUES (1, 'a', 1.50), (2, NULL, 2.25), (3, 'c', NULL)",
    )
    .await;
    let batches = rows(&*conn, "SELECT id, name, price FROM t ORDER BY id").await;
    let total: usize = batches.iter().map(RecordBatch::num_rows).sum();
    assert_eq!(total, 3);
    let batch = &batches[0];
    let names: Vec<_> = batch
        .schema()
        .fields()
        .iter()
        .map(|f| f.name().clone())
        .collect();
    assert_eq!(names, ["id", "name", "price"]);
    let name = batch.column(1).as_string_opt::<i32>();
    let name = name.expect("a string column");
    assert_eq!(name.value(0), "a");
    assert!(name.is_null(1));

    // A SQL error comes back as an error, not as a crash, and the session goes on.
    let err = conn.execute("SELEC 1", &ExecOptions::default()).await.err();
    let err = match err {
        Some(e) => e.to_string(),
        None => panic!("a syntax error must fail"),
    };
    assert_ne!(err, "");
    assert_eq!(rows(&*conn, "SELECT count(*) FROM t").await.len(), 1);
    drop(conn);

    // The data is in the file the options named: a second connection sees it.
    let again = driver().connect(&config(&library, &file)).await.unwrap();
    let batches = rows(&*again, "SELECT count(*) AS n FROM t").await;
    assert_eq!(batches[0].num_rows(), 1);
    assert!(file.exists());
}

#[tokio::test]
async fn generic_adbc_explains_a_missing_or_wrong_library() {
    let dir = tempfile::tempdir().unwrap();
    let driver = driver();

    // No library chosen.
    let config = ConnectionConfig::new(DriverKind::Adbc);
    let err = driver.connect(&config).await.err().unwrap();
    assert!(matches!(err, Error::Config(_)), "{err}");

    // A file that is not there names itself.
    let mut config = ConnectionConfig::new(DriverKind::Adbc);
    let missing = dir.path().join("nothing.dll");
    config
        .options
        .insert("driver_file".to_owned(), missing.display().to_string());
    let err = driver.connect(&config).await.err().unwrap().to_string();
    assert!(err.contains("nothing.dll"), "{err}");

    // A file that is not a driver library is refused with an error, not a crash.
    let junk = dir.path().join("junk.dll");
    std::fs::write(&junk, b"not a library").unwrap();
    config
        .options
        .insert("driver_file".to_owned(), junk.display().to_string());
    assert!(driver.connect(&config).await.is_err());
}

/// DuckDB's ADBC driver exports `duckdb_adbc_init`, not the standard `AdbcDriverInit`, and the
/// generic driver has no field for the entrypoint: the library cannot be loaded.
#[tokio::test]
#[ignore = "bug: the generic ADBC driver cannot name a library's entrypoint (DuckDB's)"]
async fn generic_adbc_loads_duckdb() {
    let Some(library) = library("duckdb").await else {
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    let mut config = config(&library, &dir.path().join("generic.duckdb"));
    config.options.remove("uri");
    config.options.insert(
        "path".to_owned(),
        dir.path().join("generic.duckdb").display().to_string(),
    );
    let conn = driver().connect(&config).await.unwrap();
    assert_eq!(rows(&*conn, "SELECT 1 AS one").await.len(), 1);
}
