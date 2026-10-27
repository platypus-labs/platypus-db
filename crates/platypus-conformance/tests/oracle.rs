//! Oracle through its native driver, on the container of the `oracle` profile
//! (`docker compose --profile oracle up -d --wait`). The driver loads Oracle Instant Client
//! from `target/conformance-drivers`, else the system's; without one the tests are skipped.
//! `PLATYPUS_TEST_ORACLE_CLIENT=download` downloads Instant Client there first.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use arrow_array::RecordBatch;
use arrow_cast::display::array_value_to_string;
use futures::{StreamExt as _, TryStreamExt as _};
use platypus_core::{
    Connection, ConnectionConfig, ConstraintKind, Driver as _, DriverKind, Error, ExecOptions,
    ExecResult, ObjectKind, ObjectRef, Secret, SessionInit, SslMode,
};
use platypus_driver_oracle::OracleDriver;
use tokio::sync::OnceCell;
use tokio_util::sync::CancellationToken;

const PORT: u16 = 51521;

/// Where the tests keep Instant Client, across runs.
fn drivers_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/conformance-drivers")
}

/// Instant Client in the tests' folder, downloaded when asked to.
async fn client_dir() -> &'static PathBuf {
    static DIR: OnceCell<PathBuf> = OnceCell::const_new();
    DIR.get_or_init(|| async {
        let dir = drivers_dir();
        let download = std::env::var("PLATYPUS_TEST_ORACLE_CLIENT").is_ok_and(|v| v == "download");
        if download && platypus_drivers::installed_vendor(&dir, "oracle").is_none() {
            let package = platypus_drivers::vendor_package("oracle")
                .expect("no Instant Client for this platform");
            platypus_drivers::install_vendor(&dir, &package, |_, _| {}, &CancellationToken::new())
                .await
                .unwrap_or_else(|e| panic!("cannot download Instant Client: {e}"));
        }
        dir
    })
    .await
}

fn config() -> ConnectionConfig {
    let mut config = ConnectionConfig::new(DriverKind::Oracle);
    "127.0.0.1".clone_into(&mut config.host);
    config.port = Some(PORT);
    "FREEPDB1".clone_into(&mut config.database);
    "platypus".clone_into(&mut config.user);
    config.password = Some(Secret::new("platypus"));
    config.ssl_mode = SslMode::Disable;
    config
}

async fn connect_with(config: &ConnectionConfig) -> Option<Box<dyn Connection>> {
    let reachable = tokio::time::timeout(
        Duration::from_secs(2),
        tokio::net::TcpStream::connect(("127.0.0.1", PORT)),
    )
    .await
    .is_ok_and(|c| c.is_ok());
    if !reachable {
        println!("skipping Oracle: unreachable at 127.0.0.1:{PORT}");
        return None;
    }
    let driver = OracleDriver {
        data_dir: client_dir().await.clone(),
    };
    match driver.connect(config).await {
        Ok(conn) => Some(conn),
        Err(Error::DriverMissing { .. }) => {
            println!("skipping Oracle: no Instant Client (PLATYPUS_TEST_ORACLE_CLIENT=download)");
            None
        }
        Err(e) => panic!("Oracle: cannot connect: {e}"),
    }
}

async fn oracle() -> Option<Box<dyn Connection>> {
    connect_with(&config()).await
}

async fn rows(conn: &dyn Connection, sql: &str) -> Vec<RecordBatch> {
    match conn.execute(sql, &ExecOptions::default()).await {
        Ok(ExecResult::Rows(stream)) => stream.try_collect().await.unwrap(),
        Ok(ExecResult::Affected { .. }) => panic!("{sql}: no rows"),
        Err(e) => panic!("{sql}: {e}"),
    }
}

async fn run(conn: &dyn Connection, sql: &str) -> u64 {
    match conn.execute(sql, &ExecOptions::default()).await {
        Ok(ExecResult::Affected { rows }) => rows,
        Ok(ExecResult::Rows(_)) => 0,
        Err(e) => panic!("{sql}: {e}"),
    }
}

fn cell(batch: &RecordBatch, column: usize, row: usize) -> String {
    array_value_to_string(batch.column(column), row).unwrap()
}

async fn scalar(conn: &dyn Connection, sql: &str) -> String {
    cell(&rows(conn, sql).await[0], 0, 0)
}

#[tokio::test]
async fn oracle_selects_types() {
    let Some(conn) = oracle().await else { return };
    assert_eq!(conn.server_info().product, "Oracle");
    assert!(
        conn.server_info().version.starts_with("23."),
        "{:?}",
        conn.server_info()
    );
    assert!(!conn.server_info().encrypted);
    let batches = rows(
        &*conn,
        "SELECT CAST(42 AS NUMBER(10)) AS i, CAST(2.5 AS NUMBER(5, 2)) AS d, 7 AS n, \
         'x' AS t, DATE '2024-01-02' AS dt, TIMESTAMP '2024-01-02 03:04:05.123456' AS ts, \
         TIMESTAMP '2024-01-02 03:04:05 +02:00' AS tz, CAST(1.5 AS BINARY_DOUBLE) AS f, \
         HEXTORAW('00FF') AS b, TO_CLOB('long text') AS c, CAST(NULL AS VARCHAR2(10)) AS z \
         FROM DUAL;",
    )
    .await;
    let batch = &batches[0];
    let names: Vec<_> = batch
        .schema()
        .fields()
        .iter()
        .map(|f| f.name().clone())
        .collect();
    assert_eq!(
        names,
        ["I", "D", "N", "T", "DT", "TS", "TZ", "F", "B", "C", "Z"]
    );
    let texts: Vec<_> = (0..10).map(|i| cell(batch, i, 0)).collect();
    assert_eq!(
        texts,
        [
            "42",
            "2.50",
            "7",
            "x",
            "2024-01-02T00:00:00",
            "2024-01-02T03:04:05.123456",
            "2024-01-02T01:04:05Z",
            "1.5",
            "00ff",
            "long text"
        ]
    );
    assert!(batch.column(10).is_null(0));
    let db_type = |i: usize| {
        batch
            .schema()
            .field(i)
            .metadata()
            .get(platypus_core::META_DB_TYPE)
            .cloned()
    };
    assert_eq!(db_type(1).as_deref(), Some("NUMBER(5,2)"));
}

#[tokio::test]
async fn oracle_streams_long_results() {
    let Some(conn) = oracle().await else { return };
    let options = ExecOptions {
        batch_size: 1000,
        paged: true,
    };
    let series = "SELECT LEVEL AS x FROM DUAL CONNECT BY LEVEL <= 100000";
    let ExecResult::Rows(mut stream) = conn.execute(series, &options).await.unwrap() else {
        panic!("no rows");
    };
    let mut total = 0;
    while let Some(batch) = stream.next().await {
        let batch = batch.unwrap();
        assert!(batch.num_rows() <= 1000);
        total += batch.num_rows();
    }
    assert_eq!(total, 100_000);
    // An abandoned result frees the session for the next statement.
    let ExecResult::Rows(mut stream) = conn.execute(series, &options).await.unwrap() else {
        panic!("no rows");
    };
    stream.next().await.unwrap().unwrap();
    drop(stream);
    assert_eq!(scalar(&*conn, "SELECT 1 FROM DUAL").await, "1");
}

#[tokio::test]
async fn oracle_errors_point_at_the_mistake() {
    let Some(conn) = oracle().await else { return };
    let err = conn
        .execute("SELECT * FROM no_such_table", &ExecOptions::default())
        .await
        .unwrap_err();
    let Error::Database(db) = &err else {
        panic!("{err}");
    };
    assert_eq!(db.code.as_deref(), Some("ORA-00942"), "{db:?}");
    assert_eq!(db.position, Some(15), "{db:?}");
    assert!(!db.message.starts_with("ORA-"), "{}", db.message);
}

#[tokio::test]
async fn oracle_runs_plsql_and_sets_the_session_up() {
    let mut config = config();
    config.init = SessionInit {
        schema: Some("SYSTEM".to_owned()),
        time_zone: Some("+03:00".to_owned()),
        statements: Vec::new(),
    };
    let Some(conn) = connect_with(&config).await else {
        return;
    };
    assert_eq!(
        scalar(
            &*conn,
            "SELECT SYS_CONTEXT('USERENV', 'CURRENT_SCHEMA') FROM DUAL"
        )
        .await,
        "SYSTEM"
    );
    assert_eq!(
        scalar(&*conn, "SELECT SESSIONTIMEZONE FROM DUAL").await,
        "+03:00"
    );
    run(&*conn, "BEGIN NULL; END;").await;
    run(&*conn, "DECLARE x NUMBER := 1; BEGIN x := x + 1; END;").await;
}

/// `ORA_PARENT` with a check and a comment, and `ORA_CHILD` pointing at it, with an index.
async fn metadata_fixture(conn: &dyn Connection) {
    run(conn, "DROP TABLE IF EXISTS ORA_CHILD").await;
    run(conn, "DROP TABLE IF EXISTS ORA_PARENT").await;
    run(
        conn,
        "CREATE TABLE ORA_PARENT (ID NUMBER(10) PRIMARY KEY, NAME VARCHAR2(20 CHAR) NOT NULL, \
         PAY NUMBER(8, 2) CONSTRAINT ORA_PAY_CHK CHECK (PAY > 0))",
    )
    .await;
    run(
        conn,
        "CREATE TABLE ORA_CHILD (ID NUMBER(10) PRIMARY KEY, PARENT_ID NUMBER(10) \
         CONSTRAINT ORA_CHILD_FK REFERENCES ORA_PARENT (ID))",
    )
    .await;
    run(
        conn,
        "CREATE INDEX ORA_CHILD_IX ON ORA_CHILD (PARENT_ID, ID)",
    )
    .await;
    run(conn, "COMMENT ON TABLE ORA_PARENT IS 'parents'").await;
}

#[tokio::test]
async fn oracle_navigates_metadata() {
    let Some(conn) = oracle().await else { return };
    let conn = &*conn;
    metadata_fixture(conn).await;

    let schemas = conn.schemas().await.unwrap();
    let mine = schemas.iter().find(|s| s.name == "PLATYPUS").unwrap();
    assert!(mine.current && !mine.system);
    assert!(schemas.iter().any(|s| s.name == "SYS" && s.system));

    let objects = conn.objects("PLATYPUS").await.unwrap();
    let parent = objects.iter().find(|o| o.name == "ORA_PARENT").unwrap();
    assert_eq!(parent.kind, ObjectKind::Table);
    assert_eq!(parent.comment.as_deref(), Some("parents"));

    let details = conn.table_details("PLATYPUS", "ORA_PARENT").await.unwrap();
    let types: Vec<_> = details
        .columns
        .iter()
        .map(|c| c.data_type.as_str())
        .collect();
    assert_eq!(types, ["NUMBER(10)", "VARCHAR2(20 CHAR)", "NUMBER(8,2)"]);
    assert!(details.columns[0].primary_key && !details.columns[1].nullable);
    assert!(
        details
            .constraints
            .iter()
            .any(|c| c.kind == ConstraintKind::Check && c.definition == "CHECK (PAY > 0)"),
        "{:?}",
        details.constraints
    );
    assert!(
        details
            .indexes
            .iter()
            .any(|i| i.primary && i.columns == ["ID"])
    );
    assert_eq!(details.referenced_by.len(), 1);
    assert_eq!(details.referenced_by[0].table, "ORA_CHILD");

    let child = conn.table_details("PLATYPUS", "ORA_CHILD").await.unwrap();
    let fk = &child.foreign_keys[0];
    assert_eq!(
        (fk.name.as_str(), fk.ref_table.as_str()),
        ("ORA_CHILD_FK", "ORA_PARENT")
    );
    assert_eq!(fk.columns, ["PARENT_ID"]);
    let index = child
        .indexes
        .iter()
        .find(|i| i.name == "ORA_CHILD_IX")
        .unwrap();
    assert_eq!(index.columns, ["PARENT_ID", "ID"]);
    assert_eq!(
        conn.referencing_keys("PLATYPUS", "ORA_PARENT")
            .await
            .unwrap()
            .len(),
        1
    );

    let catalog = conn.catalog_columns().await.unwrap();
    assert!(
        catalog
            .iter()
            .any(|c| c.table == "ORA_CHILD" && c.column == "PARENT_ID")
    );
    assert!(catalog.iter().all(|c| c.schema != "SYS"));
    let keys = conn.catalog_foreign_keys().await.unwrap();
    assert!(
        keys.iter()
            .any(|k| k.table == "ORA_CHILD" && k.ref_table == "ORA_PARENT")
    );

    let ddl = conn
        .object_ddl(&ObjectRef {
            schema: "PLATYPUS",
            name: "ORA_PARENT",
            kind: ObjectKind::Table,
            parent: None,
        })
        .await
        .unwrap();
    assert!(
        ddl.starts_with("CREATE TABLE \"PLATYPUS\".\"ORA_PARENT\""),
        "{ddl}"
    );
    run(conn, "DROP TABLE ORA_CHILD").await;
    run(conn, "DROP TABLE ORA_PARENT").await;
}

#[tokio::test]
async fn oracle_commits_and_rolls_back() {
    let Some(conn) = oracle().await else { return };
    let conn = &*conn;
    assert!(conn.capabilities().transactions && conn.capabilities().cancel);
    run(conn, "DROP TABLE IF EXISTS ORA_TX").await;
    run(conn, "CREATE TABLE ORA_TX (ID NUMBER)").await;
    let insert = |id: u32| format!("INSERT INTO ORA_TX VALUES ({id})");
    let count = "SELECT COUNT(*) FROM ORA_TX";
    assert_eq!(run(conn, &insert(1)).await, 1);
    let other = oracle().await.unwrap();
    assert_eq!(scalar(&*other, count).await, "1", "autocommit");

    conn.set_autocommit(false).await.unwrap();
    run(conn, &insert(2)).await;
    assert!(conn.transaction().open);
    assert!(conn.set_autocommit(true).await.is_err());
    assert_eq!(scalar(&*other, count).await, "1", "not committed yet");
    conn.rollback().await.unwrap();
    assert!(!conn.transaction().open);
    assert_eq!(scalar(conn, count).await, "1");
    run(conn, &insert(3)).await;
    conn.commit().await.unwrap();
    conn.set_autocommit(true).await.unwrap();
    assert_eq!(scalar(&*other, count).await, "2");

    // A typed BEGIN holds the changes until ROLLBACK, as the data editor does; Oracle by
    // itself would read it as a PL/SQL block.
    run(conn, "BEGIN").await;
    assert!(conn.transaction().open);
    run(conn, &insert(4)).await;
    run(conn, "ROLLBACK").await;
    assert!(!conn.transaction().open);
    assert_eq!(scalar(conn, count).await, "2");
    run(conn, &insert(5)).await;
    assert_eq!(scalar(&*other, count).await, "3", "autocommit is back");
    run(conn, "DROP TABLE ORA_TX").await;
}

/// The data editor's batches in Oracle's dialect: one row per `INSERT` (no multi-row
/// `VALUES` before 23ai), `OR` of composite keys and `IN` lists of single ones, and a batch
/// that finds a row fewer than it names rolls back.
#[tokio::test]
async fn oracle_saves_many_edited_rows() {
    use platypus_sql::Dialect;
    use platypus_sql::dml::{self, ApplyError, Column, Value, ValueKind};

    const ROWS: usize = 1200;
    let Some(conn) = oracle().await else { return };
    let conn = &*conn;
    let d = Dialect::Oracle;
    run(conn, "DROP TABLE IF EXISTS ORA_MANY").await;
    run(
        conn,
        "CREATE TABLE ORA_MANY (ID NUMBER NOT NULL, TAG VARCHAR2(40) NOT NULL, \
         LABEL VARCHAR2(40), PRIMARY KEY (ID, TAG))",
    )
    .await;
    let column = |name, kind| Column {
        name,
        kind,
        default: None,
    };
    let columns = [
        column("ID", ValueKind::Decimal),
        column("TAG", ValueKind::Text),
        column("LABEL", ValueKind::Text),
    ];
    let ids: Vec<String> = (1..=ROWS).map(|i| i.to_string()).collect();
    let tags: Vec<String> = (1..=ROWS).map(|i| format!("it's {}", i % 3)).collect();
    let count = "SELECT COUNT(*) FROM ORA_MANY";

    let rows: Vec<Vec<(usize, Value<'_>)>> = (0..ROWS)
        .map(|i| {
            vec![
                (0, Value::Text(&ids[i])),
                (1, Value::Text(&tags[i])),
                (2, Value::Null),
            ]
        })
        .collect();
    let inserts = dml::inserts(d, "ORA_MANY", &columns, &rows).unwrap();
    assert_eq!(inserts.len(), ROWS);
    dml::apply(conn, d, &inserts).await.unwrap();
    assert_eq!(scalar(conn, count).await, ROWS.to_string());

    let key = |i: usize| vec![(0, Some(ids[i].as_str())), (1, Some(tags[i].as_str()))];
    let keys: Vec<_> = (0..1100).map(key).collect();
    let deletes = dml::deletes(d, "ORA_MANY", &columns, &keys).unwrap();
    assert_eq!(deletes.len(), 3);
    dml::apply(conn, d, &deletes).await.unwrap();
    assert_eq!(scalar(conn, count).await, "100");

    let gone: Vec<_> = (1100..ROWS).chain([4]).map(key).collect();
    let deletes = dml::deletes(d, "ORA_MANY", &columns, &gone).unwrap();
    let err = dml::apply(conn, d, &deletes).await.unwrap_err();
    assert!(
        matches!(
            err,
            ApplyError::RowCount {
                index: 0,
                rows: 100,
                expected: 101
            }
        ),
        "{err}"
    );
    assert_eq!(scalar(conn, count).await, "100", "rolled back");

    let by_id: Vec<_> = (1100..ROWS)
        .map(|i| vec![(0, Some(ids[i].as_str()))])
        .collect();
    let deletes = dml::deletes(d, "ORA_MANY", &columns, &by_id).unwrap();
    dml::apply(conn, d, &deletes).await.unwrap();
    assert_eq!(scalar(conn, count).await, "0");
    run(conn, "DROP TABLE ORA_MANY").await;
}

/// In manual-commit mode a failed save rolls back to its savepoint only: the session's other
/// uncommitted changes stay, and so does the transaction.
#[tokio::test]
async fn oracle_keeps_uncommitted_work_when_a_save_fails() {
    use platypus_sql::Dialect;
    use platypus_sql::dml::{self, ApplyError, Column, Statement, Value, ValueKind};

    let Some(conn) = oracle().await else { return };
    let conn = &*conn;
    let d = Dialect::Oracle;
    run(conn, "DROP TABLE IF EXISTS ORA_SP").await;
    run(
        conn,
        "CREATE TABLE ORA_SP (ID NUMBER PRIMARY KEY, NAME VARCHAR2(20))",
    )
    .await;
    run(conn, "INSERT INTO ORA_SP VALUES (1, 'a')").await;
    let columns = [
        Column {
            name: "ID",
            kind: ValueKind::Decimal,
            default: None,
        },
        Column {
            name: "NAME",
            kind: ValueKind::Text,
            default: None,
        },
    ];
    let update = |id: &str, name: &str| {
        Statement::one(
            dml::update(
                d,
                "ORA_SP",
                &columns,
                &[(1, Value::Text(name))],
                &[(0, Some(id))],
            )
            .unwrap(),
        )
    };

    conn.set_autocommit(false).await.unwrap();
    run(conn, "INSERT INTO ORA_SP VALUES (2, 'b')").await;
    let err = dml::apply(conn, d, &[update("1", "x"), update("99", "y")])
        .await
        .unwrap_err();
    assert!(
        matches!(
            err,
            ApplyError::RowCount {
                index: 1,
                rows: 0,
                ..
            }
        ),
        "{err}"
    );
    assert!(conn.transaction().open, "the session's transaction goes on");
    assert_eq!(
        scalar(conn, "SELECT COUNT(*) FROM ORA_SP").await,
        "2",
        "row 2 kept"
    );
    assert_eq!(
        scalar(conn, "SELECT NAME FROM ORA_SP WHERE ID = 1").await,
        "a",
        "the save undone"
    );
    // A save that works stays uncommitted with the rest.
    dml::apply(conn, d, &[update("2", "z")]).await.unwrap();
    assert_eq!(
        scalar(conn, "SELECT NAME FROM ORA_SP WHERE ID = 2").await,
        "z"
    );
    conn.rollback().await.unwrap();
    conn.set_autocommit(true).await.unwrap();
    assert_eq!(scalar(conn, "SELECT COUNT(*) FROM ORA_SP").await, "1");
    run(conn, "DROP TABLE ORA_SP").await;
}

#[tokio::test]
async fn oracle_cancels_a_running_statement() {
    let Some(conn) = oracle().await else { return };
    let conn: Arc<dyn Connection> = conn.into();
    let cancel = conn.cancel_handle();
    let running = {
        let conn = conn.clone();
        tokio::spawn(async move {
            // Far longer than the test waits.
            conn.execute(
                "SELECT COUNT(*) FROM ALL_OBJECTS a, ALL_OBJECTS b, ALL_OBJECTS c",
                &ExecOptions::default(),
            )
            .await
            .map(|_| ())
        })
    };
    tokio::time::sleep(Duration::from_secs(1)).await;
    cancel.cancel().await.unwrap();
    let outcome = tokio::time::timeout(Duration::from_secs(10), running)
        .await
        .expect("the cancelled statement should end")
        .unwrap();
    assert!(matches!(outcome, Err(Error::Cancelled)), "{outcome:?}");
    // A break leaves the session usable.
    assert!(!conn.is_closed());
    assert_eq!(scalar(&*conn, "SELECT 1 FROM DUAL").await, "1");
}

#[tokio::test]
async fn float_columns_keep_every_digit() {
    let Some(conn) = oracle().await else { return };
    let batches = rows(
        &*conn,
        "SELECT CAST(12345678901234567890 AS FLOAT) AS f, CAST(0.5 AS FLOAT(10)) AS g FROM dual",
    )
    .await;
    assert_eq!(
        array_value_to_string(batches[0].column(0), 0).unwrap(),
        "12345678901234567890"
    );
    assert_eq!(
        batches[0].schema().field(1).data_type(),
        &arrow_schema::DataType::Float64
    );
}

/// The error of `sql`, which must fail.
async fn failure(conn: &dyn Connection, sql: &str) -> Error {
    match conn.execute(sql, &ExecOptions::default()).await {
        Ok(ExecResult::Rows(stream)) => match stream.try_collect::<Vec<_>>().await {
            Ok(_) => panic!("{sql}: no error"),
            Err(e) => e,
        },
        Ok(ExecResult::Affected { .. }) => panic!("{sql}: no error"),
        Err(e) => e,
    }
}

/// Runs `sql` without caring whether it works (dropping what may not exist).
async fn quietly(conn: &dyn Connection, sql: &str) {
    let _ = conn.execute(sql, &ExecOptions::default()).await;
}

/// Numbers without a precision are decimals that sort as numbers; `TIMESTAMP(9)` keeps
/// nanoseconds.
#[tokio::test]
async fn qa_fixora_numbers_and_nanoseconds() {
    use arrow_schema::{DataType, TimeUnit};
    let Some(conn) = oracle().await else { return };
    let batches = rows(
        &*conn,
        "SELECT LEVEL n, LEVEL / 2 h, CAST(LEVEL AS INTEGER) i FROM DUAL CONNECT BY LEVEL <= 20",
    )
    .await;
    let schema = batches[0].schema();
    assert_eq!(schema.field(0).data_type(), &DataType::Decimal128(38, 0));
    assert_eq!(schema.field(1).data_type(), &DataType::Decimal128(38, 1));
    assert_eq!(schema.field(2).data_type(), &DataType::Decimal128(38, 0));
    let last = &batches[batches.len() - 1];
    let row = last.num_rows() - 1;
    assert_eq!(cell(last, 0, row), "20");
    assert_eq!(cell(last, 1, row), "10.0");
    assert_eq!(
        scalar(&*conn, "SELECT 1/3 FROM DUAL").await,
        format!("0.{}", "3".repeat(38))
    );
    // Beyond 38 digits: the exact text.
    assert!(
        scalar(&*conn, "SELECT 1E+125 FROM DUAL")
            .await
            .starts_with('1')
    );
    let batches = rows(
        &*conn,
        "SELECT CAST(TIMESTAMP '2024-02-29 13:45:59.123456789' AS TIMESTAMP(9)) t, \
         CAST(TIMESTAMP '2024-02-29 13:45:59.123456789 +02:00' AS TIMESTAMP(9) WITH TIME ZONE) z \
         FROM DUAL",
    )
    .await;
    assert_eq!(cell(&batches[0], 0, 0), "2024-02-29T13:45:59.123456789");
    assert_eq!(cell(&batches[0], 1, 0), "2024-02-29T11:45:59.123456789Z");
    // Out of the nanoseconds' range: microseconds, as before.
    let batches = rows(
        &*conn,
        "SELECT CAST(TIMESTAMP '9999-12-31 00:00:00.5' AS TIMESTAMP(9)) t FROM DUAL",
    )
    .await;
    assert_eq!(
        batches[0].schema().field(0).data_type(),
        &DataType::Timestamp(TimeUnit::Microsecond, None)
    );
    assert_eq!(cell(&batches[0], 0, 0), "9999-12-31T00:00:00.500");
}

/// `JSON`, `VECTOR`, cursor and `BFILE` columns are read as text.
#[tokio::test]
async fn qa_fixora_json_vector_cursor_and_file_columns() {
    let Some(conn) = oracle().await else { return };
    let conn = &*conn;
    quietly(conn, "DROP TABLE QA_FIXORA_JV PURGE").await;
    run(
        conn,
        "CREATE TABLE QA_FIXORA_JV (ID NUMBER PRIMARY KEY, J JSON, V VECTOR(3, FLOAT32))",
    )
    .await;
    run(
        conn,
        "INSERT INTO QA_FIXORA_JV VALUES (1, JSON('{\"a\": 1, \"b\": [true, null, \"x\"]}'), \
         TO_VECTOR('[1.5, 2, -3]'))",
    )
    .await;
    let batches = rows(conn, "SELECT * FROM QA_FIXORA_JV").await;
    let batch = &batches[0];
    assert_eq!(cell(batch, 0, 0), "1");
    assert_eq!(cell(batch, 1, 0), "{\"a\":1,\"b\":[true,null,\"x\"]}");
    assert_eq!(cell(batch, 2, 0), "[1.5E+000,2.0E+000,-3.0E+000]");
    assert_eq!(
        batch.schema().field(1).metadata()[platypus_core::META_DB_TYPE],
        "JSON"
    );
    run(conn, "DROP TABLE QA_FIXORA_JV PURGE").await;

    assert_eq!(
        scalar(
            conn,
            "SELECT BFILENAME('DATA_PUMP_DIR', 'x.txt') f FROM DUAL"
        )
        .await,
        "BFILENAME('DATA_PUMP_DIR', 'x.txt')"
    );
    assert_eq!(
        scalar(
            conn,
            "SELECT CURSOR(SELECT 1 one, 'a' t FROM DUAL) c FROM DUAL"
        )
        .await,
        "[{\"ONE\":\"1\",\"T\":\"a\"}]"
    );
}

/// PL/SQL created with errors fails with them; `DBMS_OUTPUT` reaches the messages; SQL*Plus'
/// `EXEC` and `SHOW ERRORS` run.
#[tokio::test]
async fn qa_fixora_compile_errors_output_and_commands() {
    let Some(conn) = oracle().await else { return };
    let conn = &*conn;
    let sql = "CREATE OR REPLACE PROCEDURE QA_FIXORA_BAD AS\nBEGIN\n  x := 1;\nEND;";
    let Error::Database(e) = failure(conn, sql).await else {
        panic!("not a database error");
    };
    assert_eq!(e.code.as_deref(), Some("PLS-00201"), "{e:?}");
    let at = usize::try_from(e.position.unwrap()).unwrap() - 1;
    assert!(sql[at..].starts_with("x := 1"), "{e:?}");
    let shown = rows(conn, "SHOW ERRORS PROCEDURE QA_FIXORA_BAD").await;
    assert!(shown.iter().map(RecordBatch::num_rows).sum::<usize>() > 0);
    run(
        conn,
        "CREATE OR REPLACE PROCEDURE QA_FIXORA_BAD AS BEGIN NULL; END;",
    )
    .await;
    run(conn, "DROP PROCEDURE QA_FIXORA_BAD").await;

    conn.take_messages();
    run(
        conn,
        "DECLARE x NUMBER := 1; BEGIN DBMS_OUTPUT.PUT_LINE('qa_fixora hello ' || x); \
         DBMS_OUTPUT.PUT_LINE('second'); END;",
    )
    .await;
    assert_eq!(conn.take_messages(), ["qa_fixora hello 1", "second"]);
    run(conn, "EXEC DBMS_OUTPUT.PUT_LINE('exec')").await;
    assert_eq!(conn.take_messages(), ["exec"]);
}

/// Values the data editor, imports and copies write: long CLOB and BLOB values, dates and
/// times whatever the session's formats.
#[tokio::test]
async fn qa_fixora_long_lobs_and_times_save() {
    use platypus_sql::Dialect;
    use platypus_sql::dml::{Column, ValueKind};
    let Some(conn) = oracle().await else { return };
    let conn = &*conn;
    quietly(conn, "DROP TABLE QA_FIXORA_LV PURGE").await;
    run(
        conn,
        "CREATE TABLE QA_FIXORA_LV (ID NUMBER(5) PRIMARY KEY, C CLOB, B BLOB, D DATE, \
         T TIMESTAMP(9))",
    )
    .await;
    run(
        conn,
        "ALTER SESSION SET NLS_DATE_FORMAT = 'DD-MON-RR' \
         NLS_TIMESTAMP_FORMAT = 'DD-MON-RR HH.MI.SSXFF AM'",
    )
    .await;
    let literal = |kind, value: &str| {
        Column {
            name: "x",
            kind,
            default: None,
        }
        .literal(Dialect::Oracle, Some(value))
        .unwrap()
    };
    let text = format!("{}'!", "é".repeat(4999));
    let hex = "0aff".repeat(1500);
    let sql = format!(
        "INSERT INTO QA_FIXORA_LV VALUES (1, {}, {}, {}, {})",
        literal(ValueKind::Text, &text),
        literal(ValueKind::Binary, &hex),
        literal(ValueKind::Timestamp { zoned: false }, "2024-02-29T00:00:00"),
        literal(
            ValueKind::Timestamp { zoned: false },
            "2021-02-03 04:05:06.123456789"
        ),
    );
    assert_eq!(run(conn, &sql).await, 1);
    let update = format!(
        "UPDATE QA_FIXORA_LV SET C = {} WHERE ID = 1",
        literal(ValueKind::Text, &"y".repeat(6000))
    );
    assert_eq!(run(conn, &update).await, 1);
    assert_eq!(
        scalar(
            conn,
            "SELECT LENGTH(C) || '|' || DBMS_LOB.GETLENGTH(B) || '|' || TO_CHAR(D, 'YYYY-MM-DD') \
             || '|' || TO_CHAR(T, 'HH24:MI:SS.FF9') FROM QA_FIXORA_LV"
        )
        .await,
        "6000|3000|2024-02-29|04:05:06.123456789"
    );
    run(conn, "DROP TABLE QA_FIXORA_LV PURGE").await;
}

/// Packages and synonyms are listed, identity sequences are not, indexes show their
/// expressions, and the DDL of PL/SQL runs back as a script.
#[tokio::test]
async fn qa_fixora_navigator_objects_and_ddl() {
    let Some(conn) = oracle().await else { return };
    let conn = &*conn;
    quietly(conn, "DROP TABLE QA_FIXORA_NT PURGE").await;
    quietly(conn, "DROP SYNONYM QA_FIXORA_NS").await;
    quietly(conn, "DROP PACKAGE QA_FIXORA_NP").await;
    run(
        conn,
        "CREATE TABLE QA_FIXORA_NT (ID NUMBER GENERATED ALWAYS AS IDENTITY PRIMARY KEY, \
         NAME VARCHAR2(20), SALARY NUMBER)",
    )
    .await;
    run(
        conn,
        "CREATE INDEX QA_FIXORA_NT_UP ON QA_FIXORA_NT (UPPER(NAME))",
    )
    .await;
    run(
        conn,
        "CREATE INDEX QA_FIXORA_NT_DESC ON QA_FIXORA_NT (ID, SALARY DESC)",
    )
    .await;
    run(conn, "CREATE SYNONYM QA_FIXORA_NS FOR QA_FIXORA_NT").await;
    run(
        conn,
        "CREATE OR REPLACE PACKAGE QA_FIXORA_NP AS FUNCTION one RETURN NUMBER; END;",
    )
    .await;
    run(
        conn,
        "CREATE OR REPLACE PACKAGE BODY QA_FIXORA_NP AS FUNCTION one RETURN NUMBER IS \
         BEGIN RETURN 1; END; END;",
    )
    .await;
    run(
        conn,
        "CREATE OR REPLACE TRIGGER QA_FIXORA_NT_TRG BEFORE INSERT ON QA_FIXORA_NT \
         FOR EACH ROW\nBEGIN\n  :NEW.SALARY := NVL(:NEW.SALARY, 0);\nEND;",
    )
    .await;

    let objects = conn.objects("PLATYPUS").await.unwrap();
    let kind = |name: &str| objects.iter().find(|o| o.name == name).map(|o| o.kind);
    assert_eq!(kind("QA_FIXORA_NP"), Some(ObjectKind::Package));
    assert_eq!(kind("QA_FIXORA_NS"), Some(ObjectKind::Synonym));
    assert!(objects.iter().all(|o| !o.name.starts_with("ISEQ$$")));

    let details = conn
        .table_details("PLATYPUS", "QA_FIXORA_NT")
        .await
        .unwrap();
    let index = |name: &str| {
        details
            .indexes
            .iter()
            .find(|i| i.name == name)
            .map(|i| i.columns.clone())
    };
    assert_eq!(
        index("QA_FIXORA_NT_UP"),
        Some(vec!["UPPER(\"NAME\")".to_owned()])
    );
    assert_eq!(
        index("QA_FIXORA_NT_DESC"),
        Some(vec!["ID".to_owned(), "SALARY DESC".to_owned()])
    );
    qa_fixora_ddl_runs_back(conn).await;
    run(conn, "DROP PACKAGE QA_FIXORA_NP").await;
    run(conn, "DROP SYNONYM QA_FIXORA_NS").await;
    run(conn, "DROP TABLE QA_FIXORA_NT PURGE").await;
}

/// The DDL of the objects of [`qa_fixora_navigator_objects_and_ddl`]: PL/SQL with its `/`
/// lines, which runs back as a script leaving the objects valid; a table's without.
async fn qa_fixora_ddl_runs_back(conn: &dyn Connection) {
    use platypus_sql::Dialect;
    let ddl = |name: &'static str, kind| ObjectRef {
        schema: "PLATYPUS",
        name,
        kind,
        parent: None,
    };
    let package = conn
        .object_ddl(&ddl("QA_FIXORA_NP", ObjectKind::Package))
        .await
        .unwrap();
    assert!(package.contains("PACKAGE BODY"), "{package}");
    let synonym = conn
        .object_ddl(&ddl("QA_FIXORA_NS", ObjectKind::Synonym))
        .await
        .unwrap();
    assert!(synonym.contains("SYNONYM"), "{synonym}");
    let trigger = conn
        .object_ddl(&ddl("QA_FIXORA_NT_TRG", ObjectKind::Trigger))
        .await
        .unwrap();
    assert!(trigger.contains("\n/"), "{trigger}");
    // Run back as a script, the trigger and the package stay valid.
    for script in [&trigger, &package] {
        for statement in platypus_sql::split(script, Dialect::Oracle) {
            run(conn, statement.text(script)).await;
        }
    }
    assert_eq!(
        scalar(
            conn,
            "SELECT COUNT(*) FROM USER_OBJECTS WHERE OBJECT_NAME IN ('QA_FIXORA_NT_TRG', \
             'QA_FIXORA_NP') AND STATUS <> 'VALID'"
        )
        .await,
        "0"
    );
    // The table's DDL, without terminators, as before.
    let table = conn
        .object_ddl(&ddl("QA_FIXORA_NT", ObjectKind::Table))
        .await
        .unwrap();
    assert!(!table.trim_end().ends_with(';'), "{table}");
}

/// Explain plan, inline PL/SQL functions, `MERGE` upserts, generated columns and row counts
/// without statistics.
#[tokio::test]
async fn qa_fixora_plans_merges_and_generated_columns() {
    use platypus_sql::Dialect;
    use platypus_sql::generated::{Generated, generated_columns};
    let Some(conn) = oracle().await else { return };
    let conn = &*conn;
    quietly(conn, "DROP TABLE QA_FIXORA_PM PURGE").await;
    run(
        conn,
        "CREATE TABLE QA_FIXORA_PM (ID NUMBER(10) PRIMARY KEY, NAME VARCHAR2(20), \
         TWICE NUMBER GENERATED ALWAYS AS (ID * 2) VIRTUAL)",
    )
    .await;
    run(conn, "INSERT INTO QA_FIXORA_PM (ID, NAME) VALUES (1, 'a')").await;
    run(conn, "INSERT INTO QA_FIXORA_PM (ID, NAME) VALUES (2, 'b')").await;

    let plan = conn
        .explain("SELECT * FROM QA_FIXORA_PM WHERE ID = 1", false)
        .await
        .unwrap();
    assert!(plan.plan.contains("QA_FIXORA_PM"), "{}", plan.plan);
    assert_eq!(plan.root.operation, "SELECT STATEMENT");

    let query = "WITH FUNCTION f(x NUMBER) RETURN NUMBER IS BEGIN RETURN x * 2; END;\n\
                 SELECT f(21) v FROM DUAL";
    let statements = platypus_sql::split(query, Dialect::Oracle);
    assert_eq!(statements.len(), 1);
    assert_eq!(scalar(conn, statements[0].text(query)).await, "42");

    let merge = Dialect::Oracle
        .upsert_rows(
            "QA_FIXORA_PM",
            &["ID", "NAME"],
            &["ID"],
            "(2, 'B'), (3, 'c')",
        )
        .unwrap();
    assert_eq!(run(conn, &merge).await, 2);
    assert_eq!(
        scalar(
            conn,
            "SELECT LISTAGG(ID || NAME, ',') WITHIN GROUP (ORDER BY ID) FROM QA_FIXORA_PM"
        )
        .await,
        "1a,2B,3c"
    );

    let generated = generated_columns(conn, Dialect::Oracle, "PLATYPUS", "QA_FIXORA_PM")
        .await
        .unwrap();
    assert_eq!(generated.len(), 1);
    assert_eq!(
        (generated[0].name.as_str(), generated[0].generated),
        ("TWICE", Generated::Computed)
    );
    assert_eq!(
        Dialect::Oracle
            .estimate_rows(conn, None, "QA_FIXORA_PM")
            .await
            .unwrap(),
        Some(3)
    );
    run(conn, "DROP TABLE QA_FIXORA_PM PURGE").await;
}

/// `sys as sysdba`, as SQL*Plus takes it.
#[tokio::test]
async fn qa_fixora_connects_as_sysdba() {
    let mut config = config();
    "sys as sysdba".clone_into(&mut config.user);
    let Some(conn) = connect_with(&config).await else {
        return;
    };
    assert_eq!(scalar(&*conn, "SELECT USER FROM DUAL").await, "SYS");
}

/// Runs `DBMS_SESSION.SLEEP(20)` on `conn`, cancels it after 2 s and answers how it ended,
/// once it did within 10 s of the cancel.
async fn cancelled_sleep(conn: &Arc<dyn Connection>) -> Result<(), Error> {
    let cancel = conn.cancel_handle();
    let running = {
        let conn = Arc::clone(conn);
        tokio::spawn(async move {
            conn.execute(
                "BEGIN DBMS_SESSION.SLEEP(20); END;",
                &ExecOptions::default(),
            )
            .await
            .map(|_| ())
        })
    };
    tokio::time::sleep(Duration::from_secs(2)).await;
    let started = std::time::Instant::now();
    cancel.cancel().await.unwrap();
    let outcome = tokio::time::timeout(Duration::from_secs(15), running)
        .await
        .expect("the cancelled call should end")
        .unwrap();
    assert!(
        started.elapsed() < Duration::from_secs(10),
        "the call ran {:?} after Cancel",
        started.elapsed()
    );
    outcome
}

/// Cancel stops a PL/SQL call waiting on the server, which a break may not reach (through
/// Docker's port forwarding it does not): a user who may cancel others' SQL cancels it from
/// a second session and keeps the session; another gives the session up and is told so.
#[tokio::test]
async fn qa_fixora_cancels_plsql_sleep() {
    let Some(conn) = oracle().await else { return };
    let conn: Arc<dyn Connection> = conn.into();
    match cancelled_sleep(&conn).await {
        // The break reached the server.
        Err(Error::Cancelled) => assert!(!conn.is_closed()),
        Err(Error::Other(message)) => {
            assert!(message.contains("did not stop on Cancel"), "{message}");
            assert!(conn.is_closed(), "the app opens a new connection");
            assert!(!conn.transaction().open);
            assert!(matches!(
                conn.execute("SELECT 1 FROM DUAL", &ExecOptions::default())
                    .await,
                Err(Error::Closed)
            ));
        }
        other => panic!("{other:?}"),
    }

    let mut config = config();
    "sys as sysdba".clone_into(&mut config.user);
    let Some(sys) = connect_with(&config).await else {
        return;
    };
    let sys: Arc<dyn Connection> = sys.into();
    let outcome = cancelled_sleep(&sys).await;
    assert!(matches!(outcome, Err(Error::Cancelled)), "{outcome:?}");
    assert!(!sys.is_closed());
    assert_eq!(scalar(&*sys, "SELECT 1 FROM DUAL").await, "1");
}
