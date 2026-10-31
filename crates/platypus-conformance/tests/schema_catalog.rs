//! What the schema designs read from each database's catalog beyond the table details, and
//! what the schema comparison and the table editor do with it: identity and generated
//! columns, collations, indexes on expressions, with a prefix, a kind, `INCLUDE` or `WHERE`,
//! partitioned tables, ClickHouse's engine clause, PostgreSQL's user-defined types, SQLite's
//! AUTOINCREMENT counter.
//!
//! The servers come from `docker/compose.yml`, at the same addresses as `conformance.rs`
//! (and the same `PLATYPUS_TEST_*_URL` variables); one that cannot be reached is skipped.

use futures::TryStreamExt;
use platypus_core::{
    Connection, ConnectionConfig, Driver, DriverKind, Error, ExecOptions, ExecResult, TextRowExt,
};
use platypus_driver_clickhouse::ClickHouseDriver;
use platypus_driver_mssql::MssqlDriver;
use platypus_driver_mysql::MySqlDriver;
use platypus_driver_pg::PgDriver;
use platypus_driver_sqlite::SqliteDriver;
use platypus_schema::{TableDesign, compare, load, load_designs};
use platypus_sql::Dialect;

const SRC: &str = "qa4c_src";
const DST: &str = "qa4c_dst";

fn driver(kind: DriverKind) -> &'static dyn Driver {
    match kind {
        DriverKind::Postgres => &PgDriver,
        DriverKind::MySql => &MySqlDriver,
        DriverKind::SqlServer => &MssqlDriver,
        DriverKind::ClickHouse => &ClickHouseDriver,
        _ => &SqliteDriver,
    }
}

/// A connection to the server of `env` (or `default`), `None` when it cannot be reached.
async fn connect(env: &str, default: &str) -> Option<Box<dyn Connection>> {
    let url = std::env::var(env).unwrap_or_else(|_| default.to_owned());
    let config = ConnectionConfig::from_url(&url).unwrap();
    match driver(config.driver).connect(&config).await {
        Ok(conn) => Some(conn),
        Err(e) => {
            eprintln!("skipping {default}: {e}");
            None
        }
    }
}

async fn run(conn: &dyn Connection, sql: &str) -> Result<(), Error> {
    match conn.execute(sql, &ExecOptions::default()).await? {
        ExecResult::Rows(stream) => stream.try_for_each(|_| async { Ok(()) }).await,
        ExecResult::Affected { .. } => Ok(()),
    }
}

async fn run_all(conn: &dyn Connection, statements: &[String]) {
    for s in statements {
        run(conn, s).await.unwrap_or_else(|e| panic!("{s}: {e}"));
    }
}

/// The first value of the first row of `sql`.
async fn value(conn: &dyn Connection, sql: &str) -> String {
    platypus_core::query_text(conn, sql)
        .await
        .unwrap_or_else(|e| panic!("{sql}: {e}"))
        .first()
        .map(|r| r.text(0))
        .unwrap_or_default()
}

fn statements(list: &[&str], schema: &str) -> Vec<String> {
    list.iter()
        .map(|s| s.replace("S.", &format!("{schema}.")))
        .collect()
}

async fn designs(conn: &dyn Connection, d: Dialect, schema: &str) -> Vec<TableDesign> {
    load_designs(conn, d, schema)
        .await
        .unwrap_or_else(|e| panic!("{schema}: {e}"))
}

/// The comparison that makes `DST` like `SRC`, run as the SQL editor runs its text; a second
/// comparison must find nothing. Returns the text of the script.
async fn round_trip(conn: &dyn Connection, d: Dialect) -> String {
    round_trip_between(conn, d, SRC, DST).await
}

/// [`round_trip`] from the schema `src` to `dst`.
async fn round_trip_between(conn: &dyn Connection, d: Dialect, src: &str, dst: &str) -> String {
    let wanted = designs(conn, d, src).await;
    let current = designs(conn, d, dst).await;
    let script = compare(d, dst, src, &wanted, &current).unwrap();
    let text = script.text(d);
    assert!(!text.contains(src), "{d:?} names the source:\n{text}");
    for s in text.split(";\n").filter(|s| !s.trim().is_empty()) {
        run(conn, s)
            .await
            .unwrap_or_else(|e| panic!("{d:?}: {s}: {e}\n{text}"));
    }
    let again = compare(
        d,
        dst,
        src,
        &designs(conn, d, src).await,
        &designs(conn, d, dst).await,
    )
    .unwrap();
    assert_eq!(
        again.warnings,
        Vec::<String>::new(),
        "{d:?}: a second comparison warns after\n{text}"
    );
    assert_eq!(
        again.statements,
        Vec::<String>::new(),
        "{d:?}: a second comparison after\n{text}"
    );
    text
}

#[tokio::test]
async fn postgres_compares_identity_generated_columns_rich_indexes_types_and_partitions() {
    let Some(conn) = connect(
        "PLATYPUS_TEST_PG_URL",
        "postgres://platypus:platypus@localhost:55432/platypus?sslmode=disable",
    )
    .await
    else {
        return;
    };
    let conn = &*conn;
    let d = Dialect::Postgres;
    for schema in [SRC, DST] {
        run_all(
            conn,
            &[
                format!("DROP SCHEMA IF EXISTS {schema} CASCADE"),
                format!("CREATE SCHEMA {schema}"),
            ],
        )
        .await;
    }
    let source = [
        "CREATE TYPE S.mood AS ENUM ('sad', 'ok')",
        "CREATE DOMAIN S.positive AS integer CHECK (VALUE > 0)",
        "CREATE TABLE S.t (id int GENERATED ALWAYS AS IDENTITY (START WITH 10 INCREMENT BY 5) PRIMARY KEY,
            a int NOT NULL, g int GENERATED ALWAYS AS (a * 2) STORED, name text COLLATE \"C\",
            ts timestamptz, m S.mood NOT NULL DEFAULT 'ok', p S.positive)",
        "CREATE TABLE S.u (id bigint GENERATED BY DEFAULT AS IDENTITY PRIMARY KEY)",
        "CREATE INDEX t_lower ON S.t (lower(name))",
        "CREATE UNIQUE INDEX t_part ON S.t (name DESC) INCLUDE (a) WHERE id > 5",
        "CREATE INDEX t_brin ON S.t USING brin (ts)",
        "CREATE TABLE S.ev (id int, ts date NOT NULL, PRIMARY KEY (id, ts)) PARTITION BY RANGE (ts)",
        "CREATE TABLE S.ev_2024 PARTITION OF S.ev FOR VALUES FROM ('2024-01-01') TO ('2025-01-01')",
        "CREATE TABLE S.ev_2025 PARTITION OF S.ev FOR VALUES FROM ('2025-01-01') TO ('2026-01-01')",
    ];
    run_all(conn, &statements(&source, SRC)).await;

    postgres_source_design(conn).await;

    let text = round_trip(conn, d).await;
    run(conn, &format!("INSERT INTO {DST}.t (a, p) VALUES (3, 1)"))
        .await
        .unwrap_or_else(|e| panic!("{e}\n{text}"));
    assert_eq!(
        value(
            conn,
            &format!("SELECT id || ':' || g || ':' || m FROM {DST}.t")
        )
        .await,
        "10:6:ok"
    );
    assert_eq!(
        value(
            conn,
            &format!(
                "SELECT string_agg(relname || ':' || relkind::text, ',' ORDER BY relname) FROM pg_class
                 WHERE relnamespace = '{DST}'::regnamespace AND relname LIKE 'ev%' AND relkind IN ('r', 'p')"
            )
        )
        .await,
        "ev:p,ev_2024:r,ev_2025:r"
    );
    assert_eq!(
        value(
            conn,
            &format!(
                "SELECT count(*) || ':' || (SELECT format_type(atttypid, NULL) FROM pg_attribute
                     WHERE attrelid = '{DST}.t'::regclass AND attname = 'm')
                 FROM pg_index WHERE indrelid = '{DST}.t'::regclass AND NOT indisprimary"
            )
        )
        .await,
        format!("3:{DST}.mood"),
        "three indexes, and the type in the target"
    );
    // The target depends on nothing of the source: dropping it leaves every column.
    run_all(conn, &[format!("DROP SCHEMA {SRC} CASCADE")]).await;
    assert_eq!(
        value(
            conn,
            &format!("SELECT count(*) FROM information_schema.columns WHERE table_schema = '{DST}' AND table_name = 't'")
        )
        .await,
        "7"
    );
    postgres_editor_keeps_identity_and_collation(conn).await;
    run_all(conn, &[format!("DROP SCHEMA {DST} CASCADE")]).await;
}

/// What the design of the PostgreSQL source reads beyond the table details.
async fn postgres_source_design(conn: &dyn Connection) {
    let d = Dialect::Postgres;
    let t = load(conn, d, SRC, "t").await.unwrap();
    let column = |name: &str| t.columns.iter().find(|c| c.name == name).unwrap();
    assert_eq!(
        column("id").extra.as_deref(),
        Some("GENERATED ALWAYS AS IDENTITY (START WITH 10 INCREMENT BY 5)")
    );
    assert_eq!(
        column("g").generated.as_deref(),
        Some("GENERATED ALWAYS AS (a * 2) STORED")
    );
    assert_eq!(column("g").default, None);
    assert_eq!(column("name").collation.as_deref(), Some("\"C\""));
    let index = |name: &str| t.indexes.iter().find(|i| i.name == name).unwrap();
    assert_eq!(index("t_lower").parts, ["lower(name)"]);
    assert_eq!(index("t_brin").method.as_deref(), Some("brin"));
    assert_eq!(index("t_part").include, ["a"]);
    assert_eq!(index("t_part").predicate.as_deref(), Some("(id > 5)"));
    let types: Vec<&str> = t.types.iter().map(|u| u.name.as_str()).collect();
    assert_eq!(types, ["mood", "positive"]);
    let ev = load(conn, d, SRC, "ev").await.unwrap();
    assert_eq!(ev.options.as_deref(), Some("PARTITION BY RANGE (ts)"));
    assert_eq!(ev.partitions.len(), 2);
}

/// The table editor keeps the identity and the collation of the columns it does not touch.
async fn postgres_editor_keeps_identity_and_collation(conn: &dyn Connection) {
    let d = Dialect::Postgres;
    let old = load(conn, d, DST, "t").await.unwrap();
    let mut new = old.clone();
    let name = new.columns.iter_mut().find(|c| c.name == "name").unwrap();
    "varchar(40)".clone_into(&mut name.data_type);
    let script = platypus_schema::script(d, DST, Some(&old), &new).unwrap();
    platypus_schema::apply(conn, &script).await.unwrap();
    let after = load(conn, d, DST, "t").await.unwrap();
    let name = after.columns.iter().find(|c| c.name == "name").unwrap();
    assert_eq!(name.collation.as_deref(), Some("\"C\""), "{script:?}");
    assert_eq!(after.columns[0].extra, old.columns[0].extra);
}
#[tokio::test]
async fn postgres_row_triggers_on_delete_and_another_event_let_the_delete_happen() {
    let Some(conn) = connect(
        "PLATYPUS_TEST_PG_URL",
        "postgres://platypus:platypus@localhost:55432/platypus?sslmode=disable",
    )
    .await
    else {
        return;
    };
    let conn = &*conn;
    let schema = "qa4c_trigger";
    run_all(
        conn,
        &[
            format!("DROP SCHEMA IF EXISTS {schema} CASCADE"),
            format!("CREATE SCHEMA {schema}"),
            format!("CREATE TABLE {schema}.t (id int PRIMARY KEY, v text)"),
            format!("CREATE TABLE {schema}.audit (op text)"),
            format!("INSERT INTO {schema}.t VALUES (1, 'a'), (2, 'b')"),
        ],
    )
    .await;
    let form = platypus_schema::ObjectForm::Trigger {
        name: "t_audit".into(),
        table: "t".into(),
        timing: platypus_schema::Timing::Before,
        events: vec![
            platypus_schema::Event::Insert,
            platypus_schema::Event::Delete,
        ],
        for_each_row: true,
        when: None,
        body: format!("INSERT INTO {schema}.audit VALUES (TG_OP)"),
        replace: false,
    };
    let sql = platypus_schema::create_object(Dialect::Postgres, schema, &form).unwrap();
    run_all(conn, &sql).await;
    run_all(
        conn,
        &[
            format!("DELETE FROM {schema}.t WHERE id = 1"),
            format!("INSERT INTO {schema}.t VALUES (3, 'c')"),
        ],
    )
    .await;
    assert_eq!(
        value(
            conn,
            &format!("SELECT string_agg(id::text, ',' ORDER BY id) FROM {schema}.t")
        )
        .await,
        "2,3"
    );
    assert_eq!(
        value(
            conn,
            &format!("SELECT string_agg(op, ',' ORDER BY op) FROM {schema}.audit")
        )
        .await,
        "DELETE,INSERT"
    );
    run_all(conn, &[format!("DROP SCHEMA {schema} CASCADE")]).await;
}

#[tokio::test]
async fn mysql_and_mariadb_compare_prefix_and_fulltext_indexes_json_and_widen_keys() {
    for (env, url) in [
        (
            "PLATYPUS_TEST_MYSQL_ROOT_URL",
            "mysql://root:platypus@localhost:53306/platypus",
        ),
        (
            "PLATYPUS_TEST_MARIADB_ROOT_URL",
            "mysql://root:platypus@localhost:53307/platypus",
        ),
    ] {
        let Some(conn) = connect(env, url).await else {
            continue;
        };
        let conn = &*conn;
        let d = Dialect::MySql;
        for schema in [SRC, DST] {
            run_all(
                conn,
                &[
                    format!("DROP DATABASE IF EXISTS {schema}"),
                    format!("CREATE DATABASE {schema}"),
                ],
            )
            .await;
        }
        let source = [
            "CREATE TABLE S.d (id int NOT NULL AUTO_INCREMENT PRIMARY KEY, a int NOT NULL DEFAULT 0,
                body text, j json, KEY d_body (body(50)), FULLTEXT KEY d_ft (body))",
        ];
        run_all(conn, &statements(&source, SRC)).await;
        let text = round_trip(conn, d).await;
        assert_eq!(
            value(
                conn,
                &format!(
                    "SELECT GROUP_CONCAT(CONCAT(INDEX_NAME, ':', INDEX_TYPE, ':', IFNULL(SUB_PART, '-'))
                     ORDER BY INDEX_NAME) FROM information_schema.STATISTICS
                     WHERE TABLE_SCHEMA = '{DST}' AND TABLE_NAME = 'd' AND INDEX_NAME <> 'PRIMARY'"
                )
            )
            .await,
            "d_body:BTREE:50,d_ft:FULLTEXT:-",
            "{url}\n{text}"
        );

        // Widening the key of an AUTO_INCREMENT column, as the table editor does.
        let old = load(conn, d, DST, "d").await.unwrap();
        let mut new = old.clone();
        new.columns
            .iter_mut()
            .find(|c| c.name == "a")
            .unwrap()
            .primary_key = true;
        let script = platypus_schema::script(d, DST, Some(&old), &new).unwrap();
        assert_eq!(script.statements.len(), 1, "{script:?}");
        platypus_schema::apply(conn, &script)
            .await
            .unwrap_or_else(|e| panic!("{url}: {}: {}", e.statement, e.error));
        assert_eq!(
            value(
                conn,
                &format!(
                    "SELECT GROUP_CONCAT(COLUMN_NAME ORDER BY ORDINAL_POSITION) FROM information_schema.KEY_COLUMN_USAGE
                     WHERE TABLE_SCHEMA = '{DST}' AND TABLE_NAME = 'd' AND CONSTRAINT_NAME = 'PRIMARY'"
                )
            )
            .await,
            "id,a"
        );
        for schema in [SRC, DST] {
            run_all(conn, &[format!("DROP DATABASE {schema}")]).await;
        }
    }
}

#[tokio::test]
async fn sqlserver_compares_identity_computed_collated_columns_and_rich_indexes() {
    let Some(conn) = connect(
        "PLATYPUS_TEST_MSSQL_URL",
        "sqlserver://sa:Platypus-2026@localhost:51433/platypus?sslmode=require",
    )
    .await
    else {
        return;
    };
    let conn = &*conn;
    let d = Dialect::SqlServer;
    for schema in [SRC, DST] {
        run_all(
            conn,
            &[
                format!(
                    "IF OBJECT_ID('{schema}.t') IS NOT NULL DROP TABLE {schema}.t;
                     IF SCHEMA_ID('{schema}') IS NOT NULL DROP SCHEMA {schema}"
                ),
                format!("CREATE SCHEMA {schema}"),
            ],
        )
        .await;
    }
    let source = [
        "CREATE TABLE S.t (id int IDENTITY(10,5) PRIMARY KEY NONCLUSTERED, a int NOT NULL,
            g AS (a * 2) PERSISTED, h AS (a + 1),
            d varchar(10) COLLATE Latin1_General_CS_AS NULL, q int NOT NULL DEFAULT 5)",
        "CREATE CLUSTERED INDEX t_a ON S.t (a)",
        "CREATE INDEX t_d ON S.t (d DESC) INCLUDE (q) WHERE d IS NOT NULL",
    ];
    run_all(conn, &statements(&source, SRC)).await;
    let text = round_trip(conn, d).await;
    assert_eq!(
        value(
            conn,
            &format!(
                "SELECT STRING_AGG(name + ':' + CAST(is_identity AS varchar) + CAST(is_computed AS varchar)
                   + ':' + ISNULL(collation_name, '-'), ',') WITHIN GROUP (ORDER BY column_id)
                 FROM sys.columns WHERE object_id = OBJECT_ID('{DST}.t')"
            )
        )
        .await,
        "id:10:-,a:00:-,g:01:-,h:01:-,d:00:Latin1_General_CS_AS,q:00:-",
        "{text}"
    );
    assert_eq!(
        value(
            conn,
            &format!(
                "SELECT STRING_AGG(name COLLATE DATABASE_DEFAULT + ':' + type_desc COLLATE DATABASE_DEFAULT + ':'
                   + ISNULL(filter_definition, '-') COLLATE DATABASE_DEFAULT, ',')
                   WITHIN GROUP (ORDER BY name)
                 FROM sys.indexes WHERE object_id = OBJECT_ID('{DST}.t') AND is_primary_key = 0"
            )
        )
        .await,
        "t_a:CLUSTERED:-,t_d:NONCLUSTERED:([d] IS NOT NULL)",
        "{text}"
    );

    // The table editor keeps the collation and moves the default out of the type's way.
    // (SQL Server alters no column an index is on.)
    run_all(conn, &[format!("DROP INDEX t_d ON {DST}.t")]).await;
    let old = load(conn, d, DST, "t").await.unwrap();
    let mut new = old.clone();
    for c in &mut new.columns {
        match c.name.as_str() {
            "d" => "varchar(20)".clone_into(&mut c.data_type),
            "q" => "bigint".clone_into(&mut c.data_type),
            _ => {}
        }
    }
    let script = platypus_schema::script(d, DST, Some(&old), &new).unwrap();
    platypus_schema::apply(conn, &script)
        .await
        .unwrap_or_else(|e| panic!("{}: {}", e.statement, e.error));
    assert_eq!(
        value(
            conn,
            &format!(
                "SELECT c.collation_name + ':' + TYPE_NAME(q.system_type_id) + ':' + dc.definition
                 FROM sys.columns c, sys.columns q
                 JOIN sys.default_constraints dc ON dc.object_id = q.default_object_id
                 WHERE c.object_id = OBJECT_ID('{DST}.t') AND c.name = 'd'
                   AND q.object_id = c.object_id AND q.name = 'q'"
            )
        )
        .await,
        "Latin1_General_CS_AS:bigint:((5))"
    );
    for schema in [SRC, DST] {
        run_all(
            conn,
            &[
                format!("DROP TABLE {schema}.t"),
                format!("DROP SCHEMA {schema}"),
            ],
        )
        .await;
    }
}

#[tokio::test]
async fn clickhouse_compares_the_engine_clause_and_skipping_indexes() {
    let Some(conn) = connect(
        "PLATYPUS_TEST_CLICKHOUSE_URL",
        "clickhouse://platypus:platypus@localhost:58123/platypus",
    )
    .await
    else {
        return;
    };
    let conn = &*conn;
    let d = Dialect::ClickHouse;
    for schema in [SRC, DST] {
        run_all(
            conn,
            &[
                format!("DROP DATABASE IF EXISTS {schema}"),
                format!("CREATE DATABASE {schema}"),
            ],
        )
        .await;
    }
    let source = [
        "CREATE TABLE S.t (id UInt64, ts DateTime, s String, INDEX s_bloom s TYPE bloom_filter GRANULARITY 1)
         ENGINE = MergeTree PARTITION BY toYYYYMM(ts) PRIMARY KEY id ORDER BY (id, ts)
         TTL ts + INTERVAL 30 DAY",
    ];
    run_all(conn, &statements(&source, SRC)).await;
    let t = load(conn, d, SRC, "t").await.unwrap();
    assert!(
        t.indexes.iter().all(|i| i.name != "ORDER BY"),
        "{:?}",
        t.indexes
    );
    let text = round_trip(conn, d).await;
    let keys = |db: &str| {
        format!(
            "SELECT concat(sorting_key, '|', partition_key, '|', primary_key, '|',
                    toString(length(extractAll(create_table_query, 'TTL'))))
             FROM system.tables WHERE database = '{db}' AND name = 't'"
        )
    };
    assert_eq!(
        value(conn, &keys(DST)).await,
        value(conn, &keys(SRC)).await,
        "{text}"
    );
    assert_eq!(
        value(
            conn,
            &format!(
                "SELECT concat(name, ':', type) FROM system.data_skipping_indices WHERE database = '{DST}'"
            )
        )
        .await,
        "s_bloom:bloom_filter"
    );
    for schema in [SRC, DST] {
        run_all(conn, &[format!("DROP DATABASE {schema}")]).await;
    }
}

#[tokio::test]
async fn sqlite_rebuilds_keep_the_autoincrement_counter() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("seq.db");
    std::fs::File::create(&path).unwrap();
    let mut config = ConnectionConfig::new(DriverKind::Sqlite);
    config.database = path.to_string_lossy().into_owned();
    let conn = SqliteDriver.connect(&config).await.unwrap();
    let conn = &*conn;
    run_all(
        conn,
        &[
            "CREATE TABLE t (id INTEGER PRIMARY KEY AUTOINCREMENT, v TEXT)".to_owned(),
            "INSERT INTO t (v) VALUES ('a'), ('b'), ('c')".to_owned(),
            "DELETE FROM t WHERE id = 3".to_owned(),
        ],
    )
    .await;
    let d = Dialect::Sqlite;
    let old = load(conn, d, "main", "t").await.unwrap();
    let mut new = old.clone();
    new.columns.push(platypus_schema::ColumnDesign {
        name: "z".to_owned(),
        data_type: "TEXT".to_owned(),
        default: Some("'q'".to_owned()),
        ..platypus_schema::ColumnDesign::default()
    });
    let script = platypus_schema::script(d, "main", Some(&old), &new).unwrap();
    platypus_schema::apply(conn, &script)
        .await
        .unwrap_or_else(|e| panic!("{}: {}", e.statement, e.error));
    run_all(conn, &["INSERT INTO t (v) VALUES ('d')".to_owned()]).await;
    assert_eq!(
        value(conn, "SELECT max(id) FROM t").await,
        "4",
        "{script:?}"
    );
    assert_eq!(
        value(
            conn,
            "SELECT count(*) FROM sqlite_master WHERE name LIKE '_platypus%'"
        )
        .await,
        "0"
    );
}

#[tokio::test]
async fn postgres_compare_detaches_a_partition_the_source_has_not() {
    let Some(conn) = connect(
        "PLATYPUS_TEST_PG_URL",
        "postgres://platypus:platypus@localhost:55432/platypus?sslmode=disable",
    )
    .await
    else {
        return;
    };
    let conn = &*conn;
    let d = Dialect::Postgres;
    let (src, dst) = ("qa4c_part_src", "qa4c_part_dst");
    for schema in [src, dst] {
        run_all(
            conn,
            &[
                format!("DROP SCHEMA IF EXISTS {schema} CASCADE"),
                format!("CREATE SCHEMA {schema}"),
                format!(
                    "CREATE TABLE {schema}.ev (id int, ts date NOT NULL) PARTITION BY RANGE (ts)"
                ),
                format!(
                    "CREATE TABLE {schema}.ev_2024 PARTITION OF {schema}.ev
                     FOR VALUES FROM ('2024-01-01') TO ('2025-01-01')"
                ),
            ],
        )
        .await;
    }
    run_all(
        conn,
        &[
            format!(
                "CREATE TABLE {dst}.ev_2025 PARTITION OF {dst}.ev
                 FOR VALUES FROM ('2025-01-01') TO ('2026-01-01')"
            ),
            format!("INSERT INTO {dst}.ev VALUES (1, '2024-06-01'), (2, '2025-06-01')"),
        ],
    )
    .await;
    let script = compare(
        d,
        dst,
        src,
        &designs(conn, d, src).await,
        &designs(conn, d, dst).await,
    )
    .unwrap();
    assert_eq!(
        script.statements,
        [format!(
            "ALTER TABLE {dst}.ev DETACH PARTITION {dst}.ev_2025"
        )]
    );
    assert!(
        script.warnings[0].starts_with("Detaches partition ev_2025"),
        "{script:?}"
    );
    platypus_schema::apply(conn, &script)
        .await
        .unwrap_or_else(|e| panic!("{}: {}", e.statement, e.error));
    // The partition's rows stay, in a table of its own.
    assert_eq!(
        value(
            conn,
            &format!(
                "SELECT (SELECT count(*) FROM {dst}.ev) || ':' || (SELECT count(*) FROM {dst}.ev_2025)
                 || ':' || (SELECT relispartition FROM pg_class WHERE oid = '{dst}.ev_2025'::regclass)"
            )
        )
        .await,
        "1:1:false"
    );
    for schema in [src, dst] {
        run_all(conn, &[format!("DROP SCHEMA {schema} CASCADE")]).await;
    }
}

#[tokio::test]
async fn mysql_and_mariadb_take_a_column_out_of_the_key_and_make_it_nullable() {
    for (env, url) in [
        (
            "PLATYPUS_TEST_MYSQL_ROOT_URL",
            "mysql://root:platypus@localhost:53306/platypus",
        ),
        (
            "PLATYPUS_TEST_MARIADB_ROOT_URL",
            "mysql://root:platypus@localhost:53307/platypus",
        ),
    ] {
        let Some(conn) = connect(env, url).await else {
            continue;
        };
        let conn = &*conn;
        let d = Dialect::MySql;
        let db = "qa4c_pk";
        run_all(
            conn,
            &[
                format!("DROP DATABASE IF EXISTS {db}"),
                format!("CREATE DATABASE {db}"),
                format!(
                    "CREATE TABLE {db}.t (id int NOT NULL AUTO_INCREMENT, k int NOT NULL,
                     PRIMARY KEY (id, k))"
                ),
                format!("INSERT INTO {db}.t (k) VALUES (1), (2)"),
            ],
        )
        .await;
        let old = load(conn, d, db, "t").await.unwrap();
        let mut new = old.clone();
        let k = new.columns.iter_mut().find(|c| c.name == "k").unwrap();
        k.primary_key = false;
        k.nullable = true;
        let script = platypus_schema::script(d, db, Some(&old), &new).unwrap();
        platypus_schema::apply(conn, &script)
            .await
            .unwrap_or_else(|e| panic!("{url}: {}: {}\n{script:?}", e.statement, e.error));
        assert_eq!(
            value(
                conn,
                &format!(
                    "SELECT CONCAT((SELECT GROUP_CONCAT(COLUMN_NAME) FROM information_schema.KEY_COLUMN_USAGE
                       WHERE TABLE_SCHEMA = '{db}' AND TABLE_NAME = 't' AND CONSTRAINT_NAME = 'PRIMARY'),
                     ':', (SELECT IS_NULLABLE FROM information_schema.COLUMNS
                       WHERE TABLE_SCHEMA = '{db}' AND TABLE_NAME = 't' AND COLUMN_NAME = 'k'))"
                )
            )
            .await,
            "id:YES",
            "{url}"
        );
        run_all(conn, &[format!("DROP DATABASE {db}")]).await;
    }
}

#[tokio::test]
async fn sqlserver_compare_leaves_out_xml_and_spatial_indexes() {
    let Some(conn) = connect(
        "PLATYPUS_TEST_MSSQL_URL",
        "sqlserver://sa:Platypus-2026@localhost:51433/platypus?sslmode=require",
    )
    .await
    else {
        return;
    };
    let conn = &*conn;
    let d = Dialect::SqlServer;
    let (src, dst) = ("qa4c_xml_src", "qa4c_xml_dst");
    for schema in [src, dst] {
        run_all(
            conn,
            &[
                format!(
                    "IF OBJECT_ID('{schema}.x') IS NOT NULL DROP TABLE {schema}.x;
                     IF SCHEMA_ID('{schema}') IS NOT NULL DROP SCHEMA {schema}"
                ),
                format!("CREATE SCHEMA {schema}"),
            ],
        )
        .await;
    }
    run_all(
        conn,
        &[
            format!("CREATE TABLE {src}.x (id int PRIMARY KEY CLUSTERED, doc xml, g geometry)"),
            format!("CREATE PRIMARY XML INDEX x_doc ON {src}.x (doc)"),
            format!(
                "CREATE SPATIAL INDEX x_geo ON {src}.x (g) USING GEOMETRY_GRID
                 WITH (BOUNDING_BOX = (0, 0, 100, 100))"
            ),
        ],
    )
    .await;
    let script = compare(
        d,
        dst,
        src,
        &designs(conn, d, src).await,
        &designs(conn, d, dst).await,
    )
    .unwrap();
    platypus_schema::apply(conn, &script)
        .await
        .unwrap_or_else(|e| panic!("{}: {}\n{script:?}", e.statement, e.error));
    for index in ["x_doc", "x_geo"] {
        assert!(
            script.warnings.contains(&format!(
                "Index {index} is an XML or spatial index: create it again by hand."
            )),
            "{script:?}"
        );
    }
    assert_eq!(
        value(
            conn,
            &format!("SELECT count(*) FROM sys.columns WHERE object_id = OBJECT_ID('{dst}.x')")
        )
        .await,
        "3"
    );
    for schema in [src, dst] {
        run_all(
            conn,
            &[
                format!("DROP TABLE {schema}.x"),
                format!("DROP SCHEMA {schema}"),
            ],
        )
        .await;
    }
}

/// A SQLite database in `dir`, created empty.
async fn sqlite_file(dir: &std::path::Path, name: &str) -> Box<dyn Connection> {
    let path = dir.join(name);
    std::fs::File::create(&path).unwrap();
    let mut config = ConnectionConfig::new(DriverKind::Sqlite);
    config.database = path.to_string_lossy().into_owned();
    SqliteDriver.connect(&config).await.unwrap()
}

/// The comparison that makes the SQLite database `dst` like `src`, run on `dst`; a second
/// comparison must find nothing. Returns the text of the script.
async fn sqlite_round_trip(src: &dyn Connection, dst: &dyn Connection) -> String {
    let d = Dialect::Sqlite;
    let script = compare(
        d,
        "main",
        "main",
        &designs(src, d, "main").await,
        &designs(dst, d, "main").await,
    )
    .unwrap();
    platypus_schema::apply(dst, &script)
        .await
        .unwrap_or_else(|e| panic!("{}: {}\n{script:?}", e.statement, e.error));
    let again = compare(
        d,
        "main",
        "main",
        &designs(src, d, "main").await,
        &designs(dst, d, "main").await,
    )
    .unwrap();
    assert_eq!(again.statements, Vec::<String>::new(), "after {script:?}");
    script.text(d)
}

#[tokio::test]
async fn sqlite_compare_and_rebuild_keep_expressions_conditions_deferral_and_conflicts() {
    let dir = tempfile::tempdir().unwrap();
    let (src, dst) = (
        sqlite_file(dir.path(), "src.db").await,
        sqlite_file(dir.path(), "dst.db").await,
    );
    let (src, dst) = (&*src, &*dst);
    let d = Dialect::Sqlite;
    run_all(
        src,
        &[
            "CREATE TABLE f0 (id INTEGER PRIMARY KEY)".to_owned(),
            "CREATE TABLE f1 (id INTEGER PRIMARY KEY, created TEXT DEFAULT (datetime('now')),
               r INT REFERENCES f0(id) ON DELETE CASCADE DEFERRABLE INITIALLY DEFERRED,
               u TEXT UNIQUE ON CONFLICT REPLACE, n INT, name TEXT, live INT)"
                .to_owned(),
            "CREATE INDEX f1_e ON f1 (lower(name), n DESC)".to_owned(),
            "CREATE UNIQUE INDEX f1_p ON f1 (n) WHERE live = 1".to_owned(),
        ],
    )
    .await;
    let text = sqlite_round_trip(src, dst).await;
    let sql = |name: &str| format!("SELECT sql FROM sqlite_master WHERE name = '{name}'");
    let f1 = value(dst, &sql("f1")).await;
    for kept in [
        "DEFAULT (datetime('now'))",
        "DEFERRABLE INITIALLY DEFERRED",
        "ON CONFLICT REPLACE",
    ] {
        assert!(f1.contains(kept), "{kept}: {f1}\n{text}");
    }
    assert!(
        value(dst, &sql("f1_e"))
            .await
            .contains("lower(name), n DESC"),
        "{text}"
    );
    // The partial unique index lets rows that are not live repeat.
    run_all(
        dst,
        &["INSERT INTO f1 (id, n, live) VALUES (1, 5, 0), (2, 5, 0)".to_owned()],
    )
    .await;

    // A changed condition is seen, and the index created again.
    run_all(
        src,
        &[
            "DROP INDEX f1_p".to_owned(),
            "CREATE UNIQUE INDEX f1_p ON f1 (n) WHERE live = 2".to_owned(),
        ],
    )
    .await;
    let text = sqlite_round_trip(src, dst).await;
    assert!(text.contains("WHERE live = 2"), "{text}");

    // The table editor creates the table again with all of it.
    let old = load(dst, d, "main", "f1").await.unwrap();
    let mut new = old.clone();
    let n = new.columns.iter_mut().find(|c| c.name == "n").unwrap();
    "TEXT".clone_into(&mut n.data_type);
    n.nullable = true;
    let name = new.columns.iter_mut().find(|c| c.name == "name").unwrap();
    name.default = Some("lower('X')".to_owned());
    let script = platypus_schema::script(d, "main", Some(&old), &new).unwrap();
    platypus_schema::apply(dst, &script)
        .await
        .unwrap_or_else(|e| panic!("{}: {}\n{script:?}", e.statement, e.error));
    let f1 = value(dst, &sql("f1")).await;
    for kept in [
        "DEFAULT (datetime('now'))",
        "DEFAULT (lower('X'))",
        "DEFERRABLE INITIALLY DEFERRED",
        "ON CONFLICT REPLACE",
    ] {
        assert!(f1.contains(kept), "{kept}: {f1}\n{script:?}");
    }
    assert_eq!(value(dst, "SELECT count(*) FROM f1").await, "2");
    assert!(
        value(dst, &sql("f1_p")).await.contains("WHERE live = 2"),
        "{script:?}"
    );
}

#[tokio::test]
async fn postgres_compare_and_editor_keep_deferral_match_nulls_and_exclusions() {
    let Some(conn) = connect(
        "PLATYPUS_TEST_PG_URL",
        "postgres://platypus:platypus@localhost:55432/platypus?sslmode=disable",
    )
    .await
    else {
        return;
    };
    let conn = &*conn;
    let d = Dialect::Postgres;
    let (src, dst) = ("qa5c_pg_src", "qa5c_pg_dst");
    for schema in [src, dst] {
        run_all(
            conn,
            &[
                format!("DROP SCHEMA IF EXISTS {schema} CASCADE"),
                format!("CREATE SCHEMA {schema}"),
            ],
        )
        .await;
    }
    let source = [
        "CREATE TABLE S.a (id int PRIMARY KEY)",
        "CREATE TABLE S.p (a int, b int, PRIMARY KEY (a, b))",
        "CREATE TABLE S.b (id int PRIMARY KEY,
            a_id int REFERENCES S.a(id) ON DELETE SET NULL DEFERRABLE INITIALLY DEFERRED,
            x int, y int, FOREIGN KEY (x, y) REFERENCES S.p(a, b) MATCH FULL,
            u1 int, u2 int, CONSTRAINT b_u UNIQUE NULLS NOT DISTINCT (u1, u2),
            r int4range, CONSTRAINT b_ex EXCLUDE USING gist (r WITH &&))",
    ];
    run_all(conn, &statements(&source, src)).await;
    let text = round_trip_between(conn, d, src, dst).await;
    let constraints = |schema: &str| {
        format!(
            "SELECT string_agg(contype::text || ':' || condeferrable || condeferred || ':'
                    || confmatchtype::text || ':' || pg_get_constraintdef(oid), ' | ' ORDER BY contype, conname)
             FROM pg_constraint WHERE conrelid = '{schema}.b'::regclass AND contype IN ('f', 'u', 'x')"
        )
    };
    assert_eq!(
        value(conn, &constraints(dst)).await,
        value(conn, &constraints(src)).await.replace(src, dst),
        "{text}"
    );

    // The table editor changes the action and keeps the deferral.
    let old = load(conn, d, dst, "b").await.unwrap();
    let mut new = old.clone();
    let key = new
        .foreign_keys
        .iter_mut()
        .find(|f| f.columns == ["a_id"])
        .unwrap();
    key.on_delete = Some("CASCADE".to_owned());
    let script = platypus_schema::script(d, dst, Some(&old), &new).unwrap();
    platypus_schema::apply(conn, &script)
        .await
        .unwrap_or_else(|e| panic!("{}: {}", e.statement, e.error));
    assert_eq!(
        value(
            conn,
            &format!(
                "SELECT confdeltype::text || condeferrable || condeferred FROM pg_constraint
                 WHERE conrelid = '{dst}.b'::regclass AND contype = 'f' AND confrelid = '{dst}.a'::regclass"
            )
        )
        .await,
        "ctruetrue",
        "{script:?}"
    );
    for schema in [src, dst] {
        run_all(conn, &[format!("DROP SCHEMA {schema} CASCADE")]).await;
    }
}

#[tokio::test]
async fn mysql_and_mariadb_compare_table_options_hidden_indexes_and_unenforced_checks() {
    for (env, url, mariadb) in [
        (
            "PLATYPUS_TEST_MYSQL_ROOT_URL",
            "mysql://root:platypus@localhost:53306/platypus",
            false,
        ),
        (
            "PLATYPUS_TEST_MARIADB_ROOT_URL",
            "mysql://root:platypus@localhost:53307/platypus",
            true,
        ),
    ] {
        let Some(conn) = connect(env, url).await else {
            continue;
        };
        let conn = &*conn;
        let d = Dialect::MySql;
        let (src, dst) = ("qa5c_my_src", "qa5c_my_dst");
        for schema in [src, dst] {
            run_all(
                conn,
                &[
                    format!("DROP DATABASE IF EXISTS {schema}"),
                    format!("CREATE DATABASE {schema}"),
                ],
            )
            .await;
        }
        let source: &[&str] = if mariadb {
            &[
                "CREATE TABLE S.sv (id int PRIMARY KEY, v int, KEY sv_v (v) IGNORED) WITH SYSTEM VERSIONING",
                "CREATE TABLE S.a (id int PRIMARY KEY, v varchar(10)) ENGINE=MyISAM DEFAULT CHARSET=latin1
                 COLLATE=latin1_general_ci",
            ]
        } else {
            &[
                "CREATE TABLE S.a (id int PRIMARY KEY, v varchar(10)) ENGINE=MyISAM DEFAULT CHARSET=latin1
                 COLLATE=latin1_general_ci",
                "CREATE TABLE S.pt (id int, d date, PRIMARY KEY (id, d)) PARTITION BY RANGE (YEAR(d))
                 (PARTITION p0 VALUES LESS THAN (2020), PARTITION p1 VALUES LESS THAN MAXVALUE)",
                "CREATE TABLE S.c (id int PRIMARY KEY, w int, b int, KEY c_b (b) INVISIBLE,
                 CONSTRAINT w_chk CHECK (w < 100) NOT ENFORCED)",
            ]
        };
        run_all(conn, &statements(source, src)).await;
        let text = round_trip_between(conn, d, src, dst).await;
        let tables = |schema: &str| {
            format!(
                "SELECT GROUP_CONCAT(CONCAT(TABLE_NAME, ':', ENGINE, ':', TABLE_COLLATION, ':', TABLE_TYPE,
                        ':', CREATE_OPTIONS) ORDER BY TABLE_NAME)
                 FROM information_schema.TABLES WHERE TABLE_SCHEMA = '{schema}'"
            )
        };
        assert_eq!(
            value(conn, &tables(dst)).await,
            value(conn, &tables(src)).await,
            "{url}\n{text}"
        );
        let hidden = if mariadb {
            format!("SHOW CREATE TABLE {dst}.sv")
        } else {
            format!("SHOW CREATE TABLE {dst}.c")
        };
        let create = platypus_core::query_text(conn, &hidden).await.unwrap()[0].text(1);
        let words: &[&str] = if mariadb {
            &["IGNORED", "WITH SYSTEM VERSIONING"]
        } else {
            &["INVISIBLE", "NOT ENFORCED"]
        };
        for word in words {
            assert!(create.contains(word), "{url}: {word}\n{create}\n{text}");
        }
        for schema in [src, dst] {
            run_all(conn, &[format!("DROP DATABASE {schema}")]).await;
        }
    }
}

#[tokio::test]
async fn sqlserver_compare_keeps_disabled_constraints_and_temporal_tables() {
    let Some(conn) = connect(
        "PLATYPUS_TEST_MSSQL_URL",
        "sqlserver://sa:Platypus-2026@localhost:51433/platypus?sslmode=require",
    )
    .await
    else {
        return;
    };
    let conn = &*conn;
    let d = Dialect::SqlServer;
    let (src, dst) = ("qa5c_ms_src", "qa5c_ms_dst");
    let clear = |schema: &str| {
        format!(
            "IF OBJECTPROPERTY(OBJECT_ID('{schema}.tmp'), 'TableTemporalType') = 2
                 ALTER TABLE {schema}.tmp SET (SYSTEM_VERSIONING = OFF);
             DROP TABLE IF EXISTS {schema}.tmp, {schema}.tmp_hist, {schema}.b, {schema}.a;
             IF SCHEMA_ID('{schema}') IS NOT NULL DROP SCHEMA {schema}"
        )
    };
    for schema in [src, dst] {
        run_all(conn, &[clear(schema), format!("CREATE SCHEMA {schema}")]).await;
    }
    let source = [
        "CREATE TABLE S.a (id int PRIMARY KEY)",
        "CREATE TABLE S.b (id int PRIMARY KEY, a_id int CONSTRAINT fk_b REFERENCES S.a(id),
            w int CONSTRAINT w_chk CHECK (w < 100))",
        "ALTER TABLE S.b NOCHECK CONSTRAINT fk_b",
        "ALTER TABLE S.b NOCHECK CONSTRAINT w_chk",
        "CREATE TABLE S.tmp (id int PRIMARY KEY, v int,
            vf datetime2 GENERATED ALWAYS AS ROW START HIDDEN NOT NULL DEFAULT SYSUTCDATETIME(),
            vt datetime2 GENERATED ALWAYS AS ROW END HIDDEN NOT NULL
               DEFAULT CONVERT(datetime2, '9999-12-31 23:59:59.9999999'),
            PERIOD FOR SYSTEM_TIME (vf, vt)) WITH (SYSTEM_VERSIONING = ON (HISTORY_TABLE = S.tmp_hist))",
    ];
    run_all(conn, &statements(&source, src)).await;
    let text = round_trip_between(conn, d, src, dst).await;
    let disabled = format!(
        "SELECT STRING_AGG(name + ':' + CAST(is_disabled AS varchar), ',') WITHIN GROUP (ORDER BY name)
         FROM (SELECT name, is_disabled FROM sys.foreign_keys WHERE parent_object_id = OBJECT_ID('{dst}.b')
               UNION ALL SELECT name, is_disabled FROM sys.check_constraints
               WHERE parent_object_id = OBJECT_ID('{dst}.b')) k"
    );
    assert_eq!(value(conn, &disabled).await, "fk_b:1,w_chk:1", "{text}");
    let versioned = format!(
        "SELECT t.temporal_type_desc COLLATE DATABASE_DEFAULT + ':' + h.name COLLATE DATABASE_DEFAULT
         FROM sys.tables t
         JOIN sys.tables h ON h.object_id = t.history_table_id WHERE t.object_id = OBJECT_ID('{dst}.tmp')"
    );
    assert_eq!(
        value(conn, &versioned).await,
        "SYSTEM_VERSIONED_TEMPORAL_TABLE:tmp_hist",
        "{text}"
    );
    let hidden = format!(
        "SELECT STRING_AGG(name + ':' + CAST(is_hidden AS varchar), ',') WITHIN GROUP (ORDER BY name)
         FROM sys.columns WHERE object_id = OBJECT_ID('{dst}.tmp') AND generated_always_type <> 0"
    );
    assert_eq!(value(conn, &hidden).await, "vf:1,vt:1", "{text}");
    // Gone from the source, the versioned table and its history go from the target.
    run_all(conn, &[clear(src), format!("CREATE SCHEMA {src}")]).await;
    let script = compare(
        d,
        dst,
        src,
        &designs(conn, d, src).await,
        &designs(conn, d, dst).await,
    )
    .unwrap();
    platypus_schema::apply(conn, &script)
        .await
        .unwrap_or_else(|e| panic!("{}: {}\n{script:?}", e.statement, e.error));
    assert_eq!(
        value(
            conn,
            &format!("SELECT count(*) FROM sys.tables WHERE schema_id = SCHEMA_ID('{dst}')")
        )
        .await,
        "0",
        "{script:?}"
    );
    for schema in [src, dst] {
        run_all(conn, &[clear(schema)]).await;
    }
}
