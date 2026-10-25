//! Databases reached through the ODBC bridge, with the drivers installed in the system:
//! SQL Server through Windows' own `SQL Server` driver (generic ODBC), on the container of
//! the `sqlserver` profile, and Oracle through Oracle's driver (Instant Client), on the
//! container of the `oracle` profile (`docker compose --profile oracle up -d --wait`).
//! Each is skipped when its driver is not installed or its container is unreachable.

use std::sync::Arc;
use std::time::Duration;

use arrow_array::RecordBatch;
use arrow_cast::display::array_value_to_string;
use futures::{StreamExt as _, TryStreamExt as _};
use platypus_core::{
    Connection, ConnectionConfig, Driver as _, DriverKind, Error, ExecOptions, ExecResult,
    ObjectKind, Secret,
};
use platypus_driver_odbc::{OdbcDriver, profile, system};

/// One database to run the scenarios on.
struct Target {
    name: &'static str,
    config: ConnectionConfig,
    /// The schema the session works in, as the catalog names it.
    schema: &'static str,
    /// Names the scenarios create, as the database stores them: Oracle in upper case.
    upper: bool,
    /// 100 000 rows, one column.
    series_sql: &'static str,
    /// A statement that runs for 20 seconds before answering.
    sleep_sql: &'static str,
    types_sql: &'static str,
}

impl Target {
    fn ident(&self, name: &str) -> String {
        if self.upper {
            name.to_ascii_uppercase()
        } else {
            name.to_owned()
        }
    }

    async fn connect(&self) -> Box<dyn Connection> {
        OdbcDriver
            .connect(&self.config)
            .await
            .unwrap_or_else(|e| panic!("{}: cannot connect: {e}", self.name))
    }
}

async fn reachable(port: u16) -> bool {
    tokio::time::timeout(
        Duration::from_secs(2),
        tokio::net::TcpStream::connect(("127.0.0.1", port)),
    )
    .await
    .is_ok_and(|c| c.is_ok())
}

fn installed(name: &str) -> bool {
    system::list().is_ok_and(|s| s.drivers.iter().any(|d| d.name == name))
}

async fn sql_server() -> Option<Target> {
    if !installed("SQL Server") {
        println!("skipping SQL Server through ODBC: no `SQL Server` ODBC driver");
        return None;
    }
    if !reachable(51433).await {
        println!("skipping SQL Server through ODBC: unreachable at 127.0.0.1:51433");
        return None;
    }
    let mut config = ConnectionConfig::new(DriverKind::Odbc);
    config
        .options
        .insert(profile::DRIVER.to_owned(), "SQL Server".to_owned());
    config.options.insert(
        profile::CONNECTION_STRING.to_owned(),
        "Server=127.0.0.1,51433;Database=platypus".to_owned(),
    );
    "sa".clone_into(&mut config.user);
    config.password = Some(Secret::new("Platypus-2026"));
    Some(Target {
        name: "sqlserver-odbc",
        config,
        schema: "dbo",
        upper: false,
        series_sql: "WITH d (n) AS (SELECT 0 UNION ALL SELECT n + 1 FROM d WHERE n < 9) \
                     SELECT a.n + 10 * b.n + 100 * c.n + 1000 * e.n + 10000 * f.n AS x \
                     FROM d a, d b, d c, d e, d f",
        sleep_sql: "WAITFOR DELAY '00:00:20'",
        types_sql: "SELECT 1 AS i, CAST(2.5 AS DECIMAL(5, 2)) AS d, N'x' AS t, \
                    CAST('2024-01-02' AS DATE) AS dt, CAST(NULL AS VARCHAR(10)) AS n",
    })
}

async fn oracle() -> Option<Target> {
    let Ok(Some(driver)) = platypus_driver_odbc::vendor_driver_installed(DriverKind::Oracle) else {
        println!("skipping Oracle: no Oracle ODBC driver installed");
        return None;
    };
    if !reachable(51521).await {
        println!("skipping Oracle: unreachable at 127.0.0.1:51521");
        return None;
    }
    println!("Oracle through `{driver}`");
    let mut config = ConnectionConfig::new(DriverKind::Oracle);
    "127.0.0.1".clone_into(&mut config.host);
    config.port = Some(51521);
    "FREEPDB1".clone_into(&mut config.database);
    "platypus".clone_into(&mut config.user);
    config.password = Some(Secret::new("platypus"));
    Some(Target {
        name: "oracle",
        config,
        schema: "PLATYPUS",
        upper: true,
        series_sql: "SELECT LEVEL AS x FROM DUAL CONNECT BY LEVEL <= 100000",
        sleep_sql: "BEGIN DBMS_SESSION.SLEEP(20); END;",
        types_sql: "SELECT 1 AS i, CAST(2.5 AS NUMBER(5, 2)) AS d, 'x' AS t, \
                    DATE '2024-01-02' AS dt, CAST(NULL AS VARCHAR2(10)) AS n FROM DUAL;",
    })
}

async fn targets() -> Vec<Target> {
    let mut out = Vec::new();
    out.extend(sql_server().await);
    out.extend(oracle().await);
    out
}

async fn rows(conn: &dyn Connection, sql: &str) -> Vec<RecordBatch> {
    match conn.execute(sql, &ExecOptions::default()).await {
        Ok(ExecResult::Rows(stream)) => stream.try_collect().await.unwrap(),
        Ok(ExecResult::Affected { .. }) => panic!("{sql}: no rows"),
        Err(e) => panic!("{sql}: {e}"),
    }
}

async fn run(conn: &dyn Connection, sql: &str) {
    if let Err(e) = conn.execute(sql, &ExecOptions::default()).await {
        panic!("{sql}: {e}");
    }
}

fn cell(batch: &RecordBatch, column: usize, row: usize) -> String {
    array_value_to_string(batch.column(column), row).unwrap()
}

async fn count(conn: &dyn Connection, table: &str) -> String {
    cell(
        &rows(conn, &format!("SELECT COUNT(*) FROM {table}")).await[0],
        0,
        0,
    )
}

#[tokio::test]
async fn odbc_selects_types() {
    for t in targets().await {
        let conn = t.connect().await;
        let batches = rows(&*conn, t.types_sql).await;
        let batch = &batches[0];
        assert_eq!(
            (batch.num_rows(), batch.num_columns()),
            (1, 5),
            "{}",
            t.name
        );
        let names: Vec<_> = batch
            .schema()
            .fields()
            .iter()
            .map(|f| f.name().to_ascii_lowercase())
            .collect();
        assert_eq!(names, ["i", "d", "t", "dt", "n"], "{}", t.name);
        assert_eq!(cell(batch, 0, 0), "1", "{}", t.name);
        assert_eq!(cell(batch, 1, 0), "2.50", "{}", t.name);
        assert_eq!(cell(batch, 2, 0), "x", "{}", t.name);
        assert!(cell(batch, 3, 0).starts_with("2024-01-02"), "{}", t.name);
        assert!(batch.column(4).is_null(0), "{}", t.name);
        if t.config.driver == DriverKind::Oracle {
            assert_eq!(conn.server_info().product, "Oracle");
            assert!(
                conn.server_info().version.starts_with("23"),
                "{:?}",
                conn.server_info()
            );
        }
    }
}

#[tokio::test]
async fn odbc_streams_long_results() {
    for t in targets().await {
        let conn = t.connect().await;
        let options = ExecOptions {
            batch_size: 1000,
            paged: true,
        };
        let ExecResult::Rows(mut stream) = conn.execute(t.series_sql, &options).await.unwrap()
        else {
            panic!("{}: no rows", t.name);
        };
        let mut total = 0;
        while let Some(batch) = stream.next().await {
            let batch = batch.unwrap();
            assert!(batch.num_rows() <= 1000, "{}", t.name);
            total += batch.num_rows();
        }
        assert_eq!(total, 100_000, "{}", t.name);
        // An abandoned result frees the session for the next statement.
        let ExecResult::Rows(mut stream) = conn.execute(t.series_sql, &options).await.unwrap()
        else {
            panic!("{}: no rows", t.name);
        };
        stream.next().await.unwrap().unwrap();
        drop(stream);
        assert_eq!(
            rows(&*conn, t.types_sql).await[0].num_rows(),
            1,
            "{}",
            t.name
        );
    }
}

#[tokio::test]
async fn odbc_errors_carry_the_sqlstate() {
    for t in targets().await {
        let conn = t.connect().await;
        let err = conn
            .execute("SELEC 1", &ExecOptions::default())
            .await
            .unwrap_err();
        let Error::Database(db) = &err else {
            panic!("{}: {err}", t.name);
        };
        assert!(
            db.code.as_ref().is_some_and(|c| c.len() == 5),
            "{}: {db:?}",
            t.name
        );
        assert!(!db.message.starts_with('['), "{}: {}", t.name, db.message);
    }
}

#[tokio::test]
async fn odbc_navigates_metadata() {
    for t in targets().await {
        let conn = t.connect().await;
        let conn = &*conn;
        let (parent, child) = (t.ident("odbc_parent"), t.ident("odbc_child"));
        run(conn, &format!("DROP TABLE IF EXISTS {child}")).await;
        run(conn, &format!("DROP TABLE IF EXISTS {parent}")).await;
        run(
            conn,
            &format!("CREATE TABLE {parent} (id INT PRIMARY KEY, name VARCHAR(20) NOT NULL)"),
        )
        .await;
        run(
            conn,
            &format!(
                "CREATE TABLE {child} (id INT PRIMARY KEY, parent_id INT \
                 CONSTRAINT odbc_child_fk REFERENCES {parent} (id))"
            ),
        )
        .await;

        let schemas = conn.schemas().await.unwrap();
        let schema = schemas
            .iter()
            .find(|s| s.name == t.schema)
            .unwrap_or_else(|| panic!("{}: no {} in {schemas:?}", t.name, t.schema));
        assert!(!schema.system, "{}", t.name);
        if t.upper {
            assert!(schema.current, "{}: the session's schema", t.name);
            assert!(schemas.iter().any(|s| s.name == "SYS" && s.system));
        }
        let objects = conn.objects(t.schema).await.unwrap();
        let found = objects.iter().find(|o| o.name == parent).unwrap();
        assert_eq!(found.kind, ObjectKind::Table, "{}", t.name);

        let details = conn.table_details(t.schema, &child).await.unwrap();
        let columns: Vec<_> = details
            .columns
            .iter()
            .map(|c| c.name.to_ascii_lowercase())
            .collect();
        assert_eq!(columns, ["id", "parent_id"], "{}", t.name);
        assert!(details.columns[0].primary_key && !details.columns[0].nullable);
        let fk = &details.foreign_keys[0];
        assert_eq!(fk.name.to_ascii_lowercase(), "odbc_child_fk", "{}", t.name);
        assert_eq!(fk.ref_table, parent, "{}", t.name);
        assert_eq!(fk.ref_columns, [t.ident("id")], "{}", t.name);

        let parent_details = conn.table_details(t.schema, &parent).await.unwrap();
        assert!(
            parent_details.columns[1].data_type.contains("20"),
            "{parent_details:?}"
        );
        assert!(!parent_details.columns[1].nullable);
        let referencing = conn.referencing_keys(t.schema, &parent).await.unwrap();
        assert_eq!(referencing.len(), 1, "{}", t.name);
        assert_eq!(referencing[0].table, child, "{}", t.name);
        assert_eq!(parent_details.referenced_by, referencing, "{}", t.name);

        let catalog = conn.catalog_columns().await.unwrap();
        assert!(
            catalog.iter().any(|c| c.schema == t.schema
                && c.table == child
                && c.column == t.ident("parent_id")),
            "{}",
            t.name
        );
        run(conn, &format!("DROP TABLE {child}")).await;
        run(conn, &format!("DROP TABLE {parent}")).await;
    }
}

#[tokio::test]
async fn odbc_commits_and_rolls_back() {
    for t in targets().await {
        let conn = t.connect().await;
        let conn = &*conn;
        assert!(conn.capabilities().transactions && conn.capabilities().cancel);
        let table = t.ident("odbc_tx");
        run(conn, &format!("DROP TABLE IF EXISTS {table}")).await;
        run(conn, &format!("CREATE TABLE {table} (id INT)")).await;
        let insert = |id: u32| format!("INSERT INTO {table} VALUES ({id})");

        conn.set_autocommit(false).await.unwrap();
        run(conn, &insert(1)).await;
        assert!(conn.transaction().open, "{}", t.name);
        assert!(conn.set_autocommit(true).await.is_err(), "{}", t.name);
        conn.rollback().await.unwrap();
        assert!(!conn.transaction().open, "{}", t.name);
        assert_eq!(count(conn, &table).await, "0", "{}", t.name);
        run(conn, &insert(2)).await;
        conn.commit().await.unwrap();
        conn.set_autocommit(true).await.unwrap();
        assert_eq!(count(conn, &table).await, "1", "{}", t.name);

        // A typed BEGIN in autocommit mode opens a transaction until ROLLBACK, as the data
        // editor does; Oracle would read it as a PL/SQL block.
        run(conn, "BEGIN").await;
        assert!(conn.transaction().open, "{}", t.name);
        run(conn, &insert(3)).await;
        run(conn, "ROLLBACK").await;
        assert!(!conn.transaction().open, "{}", t.name);
        assert_eq!(count(conn, &table).await, "1", "{}", t.name);
        // Autocommit is back: this insert stays without a COMMIT.
        run(conn, &insert(4)).await;
        let other = t.connect().await;
        assert_eq!(count(&*other, &table).await, "2", "{}", t.name);
        run(conn, &format!("DROP TABLE {table}")).await;
    }
}

#[tokio::test]
async fn odbc_cancel_gives_up_a_waiting_statement() {
    for t in targets().await {
        let conn: Arc<dyn Connection> = t.connect().await.into();
        let cancel = conn.cancel_handle();
        let running = {
            let conn = conn.clone();
            let sql = t.sleep_sql;
            tokio::spawn(
                async move { conn.execute(sql, &ExecOptions::default()).await.map(|_| ()) },
            )
        };
        tokio::time::sleep(Duration::from_secs(1)).await;
        cancel.cancel().await.unwrap();
        let outcome = tokio::time::timeout(Duration::from_secs(5), running)
            .await
            .expect("the cancelled statement should end")
            .unwrap();
        assert!(
            matches!(outcome, Err(Error::Cancelled)),
            "{}: {outcome:?}",
            t.name
        );
        // The session waits on the given-up statement: the app opens another one.
        assert!(conn.is_closed(), "{}", t.name);
        let fresh = t.connect().await;
        assert_eq!(
            rows(&*fresh, t.types_sql).await[0].num_rows(),
            1,
            "{}",
            t.name
        );
    }
}
