//! The scenarios of `conformance.rs` that apply to a database without SQL, on Redis: server
//! info, capabilities, metadata (databases and the virtual table of each key type),
//! `BROWSE`, editor commands and their errors, saving the grid's changes, cancel, ACL users
//! and lost connections. Redis comes from `docker/compose.yml` (Redis Stack, with
//! `RedisJSON`), or `PLATYPUS_TEST_REDIS`; without it the tests are skipped.
//!
//! Each test works in a database of its own, which it empties first: they run in parallel.

use std::time::Duration;

use futures::TryStreamExt;
use platypus_core::arrow_array::{Array, Float64Array, Int64Array, RecordBatch, StringArray};
use platypus_core::{
    Connection, ConnectionConfig, Driver, Error, ExecOptions, ExecResult, ObjectKind, ObjectRef,
};
use platypus_driver_redis::RedisDriver;
use platypus_driver_redis::command::KeyType;
use platypus_driver_redis::edit::{self, Edits, Update};

const DEFAULT_URL: &str = "redis://:platypus@localhost:56379/0";

fn url() -> Option<(String, bool)> {
    // Another database alone (`PLATYPUS_TEST_ONLY`): not this one.
    if std::env::var("PLATYPUS_TEST_ONLY").is_ok_and(|only| only != "redis") {
        return None;
    }
    Some(match std::env::var("PLATYPUS_TEST_REDIS") {
        Ok(url) => (url, true),
        Err(_) => (DEFAULT_URL.to_owned(), false),
    })
}

async fn connect_to(url: &str) -> platypus_core::Result<Box<dyn Connection>> {
    let mut config = ConnectionConfig::from_url(url).unwrap();
    config.connect_timeout = Some(Duration::from_secs(3));
    RedisDriver.connect(&config).await
}

/// A session on database `db`, emptied.
async fn connect(db: i64) -> Option<Box<dyn Connection>> {
    let (url, required) = url()?;
    match connect_to(&url).await {
        Ok(conn) => {
            run(&*conn, &format!("SELECT {db}")).await;
            run(&*conn, "FLUSHDB").await;
            Some(conn)
        }
        Err(e) if !required => {
            eprintln!("skipping redis: unreachable at {url}: {e}");
            None
        }
        Err(e) => panic!("cannot connect to PLATYPUS_TEST_REDIS: {e}"),
    }
}

async fn run(conn: &dyn Connection, src: &str) -> Vec<RecordBatch> {
    match conn.execute(src, &ExecOptions::default()).await {
        Ok(ExecResult::Rows(stream)) => stream.try_collect().await.unwrap(),
        Ok(ExecResult::Affected { .. }) => Vec::new(),
        Err(e) => panic!("{src}: {e}"),
    }
}

async fn run_paged(conn: &dyn Connection, src: &str, batch_size: usize) -> Vec<RecordBatch> {
    let options = ExecOptions {
        batch_size,
        paged: true,
    };
    let ExecResult::Rows(stream) = conn.execute(src, &options).await.unwrap() else {
        panic!("{src}: rows expected");
    };
    stream.try_collect().await.unwrap()
}

/// The text of every cell of `batches`, by row (NULL as `None`).
fn cells(batches: &[RecordBatch]) -> Vec<Vec<Option<String>>> {
    let mut out = Vec::new();
    for batch in batches {
        for row in 0..batch.num_rows() {
            out.push(
                batch
                    .columns()
                    .iter()
                    .map(|col| {
                        if col.is_null(row) {
                            return None;
                        }
                        let any = col.as_any();
                        Some(if let Some(s) = any.downcast_ref::<StringArray>() {
                            s.value(row).to_owned()
                        } else if let Some(n) = any.downcast_ref::<Int64Array>() {
                            n.value(row).to_string()
                        } else if let Some(f) = any.downcast_ref::<Float64Array>() {
                            f.value(row).to_string()
                        } else {
                            panic!("unexpected column type {:?}", col.data_type())
                        })
                    })
                    .collect(),
            );
        }
    }
    out
}

fn text(rows: &[Vec<Option<String>>]) -> Vec<Vec<&str>> {
    rows.iter()
        .map(|r| r.iter().map(|c| c.as_deref().unwrap_or("NULL")).collect())
        .collect()
}

async fn rows_of(conn: &dyn Connection, src: &str) -> Vec<Vec<Option<String>>> {
    let mut rows = cells(&run(conn, src).await);
    rows.sort();
    rows
}

async fn err(conn: &dyn Connection, src: &str) -> Error {
    match conn.execute(src, &ExecOptions::default()).await {
        Ok(_) => panic!("{src}: expected an error"),
        Err(e) => e,
    }
}

async fn seed(conn: &dyn Connection) {
    for line in [
        "SET greeting hello",
        "SET temp soon EX 300",
        "SET \"bin:\\xff\" \"\\x00\\xfe\"",
        "HSET user:1 name Ada lang en",
        "HSET user:2 name Bob",
        "RPUSH queue a b c",
        "SADD tags red green",
        "ZADD board 1.5 alice 3 bob",
        "JSON.SET doc $ '{\"a\":[1,2],\"b\":{\"c\":true}}'",
        "XADD events 1-1 kind click",
    ] {
        run(conn, line).await;
    }
}

#[tokio::test]
async fn connects_and_reports_the_server() {
    let Some(conn) = connect(1).await else { return };
    let info = conn.server_info();
    assert_eq!(info.product, "Redis");
    assert!(
        info.at_least(platypus_core::ServerVersion::new(6, 0, 0)),
        "{}",
        info.version
    );
    assert!(!info.encrypted);
    assert!(!conn.is_closed());
    assert!(conn.check_alive().await);
    assert!(conn.transaction().autocommit && !conn.transaction().open);
}

#[tokio::test]
async fn capabilities_match_optional_methods() {
    let Some(conn) = connect(2).await else { return };
    let caps = conn.capabilities();
    assert!(caps.cancel && !caps.transactions && !caps.explain && !caps.object_ddl);
    assert!(matches!(
        conn.set_autocommit(false).await,
        Err(Error::Unsupported(_))
    ));
    assert!(conn.set_autocommit(true).await.is_ok());
    assert!(matches!(
        conn.explain("GET a", false).await,
        Err(Error::Unsupported(_))
    ));
    let object = ObjectRef {
        schema: "db2",
        name: "hash",
        kind: ObjectKind::Table,
        parent: None,
    };
    assert!(matches!(
        conn.object_ddl(&object).await,
        Err(Error::Unsupported(_))
    ));
}

#[tokio::test]
async fn navigates_databases_and_key_types() {
    let Some(conn) = connect(3).await else { return };
    seed(&*conn).await;
    let schemas = conn.schemas().await.unwrap();
    let current: Vec<_> = schemas
        .iter()
        .filter(|s| s.current)
        .map(|s| s.name.as_str())
        .collect();
    assert_eq!(current, ["db3"]);
    let objects = conn.objects("db3").await.unwrap();
    let names: Vec<_> = objects.iter().map(|o| o.name.as_str()).collect();
    assert_eq!(
        names,
        ["string", "hash", "list", "set", "zset", "json", "stream"]
    );
    let count = |name: &str| {
        objects
            .iter()
            .find(|o| o.name == name)
            .unwrap()
            .comment
            .clone()
    };
    assert_eq!(count("string").as_deref(), Some("3 keys"));
    assert_eq!(count("hash").as_deref(), Some("2 keys"));
    assert_eq!(count("json").as_deref(), Some("1 key"));
    let details = conn.table_details("db3", "hash").await.unwrap();
    let keys: Vec<_> = details
        .columns
        .iter()
        .filter(|c| c.primary_key)
        .map(|c| c.name.as_str())
        .collect();
    assert_eq!(keys, ["key", "field"]);
    assert!(conn.table_details("db3", "nope").await.is_err());
    assert!(conn.objects("nope").await.is_err());
    let catalog = conn.catalog_columns().await.unwrap();
    assert!(
        catalog
            .iter()
            .any(|c| c.schema == "db3" && c.table == "zset" && c.column == "score")
    );
}

#[tokio::test]
async fn browses_every_type() {
    let Some(conn) = connect(4).await else { return };
    seed(&*conn).await;
    let c = &*conn;
    let strings = rows_of(c, "BROWSE string").await;
    let strings = text(&strings);
    assert_eq!(strings[0][..2], [r#""bin:\xff""#, r#""\x00\xfe""#]);
    assert_eq!(strings[1], ["greeting", "hello", "NULL"]);
    assert_eq!(strings[2][..2], ["temp", "soon"]);
    let ttl: i64 = strings[2][2].parse().unwrap();
    assert!((1..=300).contains(&ttl));
    assert_eq!(
        text(&rows_of(c, "BROWSE hash").await),
        [
            ["user:1", "lang", "en"],
            ["user:1", "name", "Ada"],
            ["user:2", "name", "Bob"]
        ]
    );
    assert_eq!(
        text(&rows_of(c, "BROWSE list").await),
        [
            ["queue", "0", "a"],
            ["queue", "1", "b"],
            ["queue", "2", "c"]
        ]
    );
    assert_eq!(
        text(&rows_of(c, "BROWSE set").await),
        [["tags", "green"], ["tags", "red"]]
    );
    assert_eq!(
        text(&rows_of(c, "BROWSE zset").await),
        [["board", "alice", "1.5"], ["board", "bob", "3"]]
    );
    let json = rows_of(c, "BROWSE json").await;
    assert_eq!(json[0][0].as_deref(), Some("doc"));
    assert_eq!(json[0][1].as_deref(), Some(r#"{"a":[1,2],"b":{"c":true}}"#));
    assert_eq!(
        text(&rows_of(c, "BROWSE stream").await),
        [["events", "1-1", r#"{"kind":"click"}"#]]
    );
    // Another database, by name, and an empty result.
    run(c, "SELECT 5").await;
    assert_eq!(rows_of(c, "BROWSE db4.set").await.len(), 2);
    assert_eq!(rows_of(c, "BROWSE set").await.len(), 0);
}

#[tokio::test]
async fn filters_limits_and_pages_big_keys() {
    let Some(conn) = connect(6).await else { return };
    let c = &*conn;
    for i in 0..30 {
        run(c, &format!("SET k:{i:02} v{i}")).await;
    }
    run(c, "SET other x").await;
    let items: Vec<String> = (0..2500).map(|i| format!("i{i}")).collect();
    for chunk in items.chunks(500) {
        run(c, &format!("RPUSH big {}", chunk.join(" "))).await;
    }
    let fields: Vec<String> = (0..1500).map(|i| format!("f{i} {i}")).collect();
    run(c, &format!("HSET wide {}", fields.join(" "))).await;
    assert_eq!(rows_of(c, "BROWSE string MATCH k:*").await.len(), 30);
    assert_eq!(rows_of(c, "BROWSE string MATCH k:1?").await.len(), 10);
    assert_eq!(
        rows_of(c, "BROWSE string MATCH 'k:0*' LIMIT 4").await.len(),
        4
    );
    assert_eq!(rows_of(c, "BROWSE string MATCH nothing*").await.len(), 0);
    let batches = run_paged(c, "BROWSE list", 1000).await;
    assert_eq!(
        batches
            .iter()
            .map(RecordBatch::num_rows)
            .collect::<Vec<_>>(),
        [1000, 1000, 500]
    );
    let last = cells(&batches[2..]);
    assert_eq!(text(&last[499..])[0], ["big", "2499", "i2499"]);
    let batches = run_paged(c, "BROWSE hash LIMIT 1200", 500).await;
    assert_eq!(
        batches.iter().map(RecordBatch::num_rows).sum::<usize>(),
        1200
    );
    let batches = run_paged(c, "BROWSE hash", 500).await;
    assert_eq!(
        batches.iter().map(RecordBatch::num_rows).sum::<usize>(),
        1500
    );
}

#[tokio::test]
async fn runs_commands_and_reports_errors() {
    let Some(conn) = connect(7).await else { return };
    let c = &*conn;
    assert_eq!(text(&rows_of(c, "SET a 1").await), [["OK"]]);
    assert_eq!(text(&rows_of(c, "GET a").await), [["1"]]);
    assert_eq!(text(&rows_of(c, "GET missing").await), [["NULL"]]);
    assert_eq!(text(&rows_of(c, "INCR a").await), [["2"]]);
    run(c, "HSET h f1 v1 f2 v2").await;
    assert_eq!(
        text(&rows_of(c, "HGETALL h").await),
        [["f1", "v1"], ["f2", "v2"]]
    );
    run(c, "RPUSH l x y").await;
    assert_eq!(
        text(&rows_of(c, "LRANGE l 0 -1").await),
        [["0", "x"], ["1", "y"]]
    );
    let scan = rows_of(c, "SCAN 0").await;
    assert_eq!(scan.len(), 2);
    // Several lines run in order; the last one's result is returned.
    assert_eq!(text(&rows_of(c, "SET b 5\n# note\nGET b").await), [["5"]]);
    // Errors from the server keep their code.
    let Error::Database(e) = err(c, "NOSUCHCOMMAND x").await else {
        panic!()
    };
    assert_eq!(e.code.as_deref(), Some("ERR"));
    let Error::Database(e) = err(c, "LPUSH h x").await else {
        panic!()
    };
    assert_eq!(e.code.as_deref(), Some("WRONGTYPE"));
    assert!(matches!(
        err(c, "SET \"unbalanced").await,
        Error::Database(_)
    ));
    assert!(
        err(c, "SUBSCRIBE ch")
            .await
            .to_string()
            .contains("not available")
    );
    assert!(
        err(c, "SELECT 99999")
            .await
            .to_string()
            .contains("DB index")
    );
    assert!(err(c, "EXEC").await.to_string().contains("without MULTI"));
    // The session goes on after errors, in its database.
    assert_eq!(text(&rows_of(c, "GET b").await), [["5"]]);
    // MULTI queues on the client, EXEC sends the transaction.
    assert_eq!(text(&rows_of(c, "MULTI").await), [["OK"]]);
    assert_eq!(text(&rows_of(c, "INCR a").await), [["QUEUED"]]);
    assert_eq!(text(&rows_of(c, "LPUSH h x").await), [["QUEUED"]]);
    assert!(
        err(c, "SELECT 1")
            .await
            .to_string()
            .contains("inside MULTI")
    );
    let exec = rows_of(c, "EXEC").await;
    let replies = text(&exec).concat();
    assert_eq!(replies[..2], ["0", "3"]);
    assert!(replies[3].starts_with("WRONGTYPE"), "{replies:?}");
    run(c, "MULTI").await;
    run(c, "SET a 100").await;
    run(c, "DISCARD").await;
    assert_eq!(text(&rows_of(c, "GET a").await), [["3"]]);
    // JSON.GET reads as JSON for the document panel.
    run(c, "JSON.SET j $ '{\"n\":1}'").await;
    let schema = match conn
        .execute("JSON.GET j", &ExecOptions::default())
        .await
        .unwrap()
    {
        ExecResult::Rows(stream) => stream.schema(),
        ExecResult::Affected { .. } => panic!(),
    };
    assert_eq!(
        schema.field(0).metadata()[platypus_core::META_DB_TYPE],
        "mixed"
    );
}

/// The rows of `browse`, as the grid loads them, for an edit.
async fn loaded(conn: &dyn Connection, browse: &str) -> Vec<Vec<Option<String>>> {
    rows_of(conn, browse).await
}

async fn save(conn: &dyn Connection, edits: &Edits<'_>) -> edit::Script {
    let script = edit::script(edits).unwrap();
    let statements: Vec<&str> = script.commands.iter().map(String::as_str).collect();
    let ran = conn.execute_all(&statements, &[]).await;
    assert!(
        ran.error.is_none(),
        "{:?}: {:?}",
        script.commands,
        ran.error
    );
    assert_eq!(ran.rows.len(), statements.len());
    script
}

fn columns(kind: KeyType) -> Vec<String> {
    kind.columns()
        .iter()
        .map(|(n, _)| (*n).to_owned())
        .collect()
}

/// The string part of `saves_grid_edits_and_undoes_them`: a key renamed with a new value
/// and TTL, a binary one changed, one deleted and one added, then the undo script run.
async fn save_string_edits(c: &dyn Connection) {
    // The edits go to their database, whichever the session is in.
    run(c, "SELECT 0").await;
    let cols = columns(KeyType::String);
    let rows = loaded(c, "BROWSE db8.string").await;
    let greeting = rows
        .iter()
        .find(|r| r[0].as_deref() == Some("greeting"))
        .unwrap()
        .clone();
    let binary = rows
        .iter()
        .find(|r| r[0].as_deref() == Some(r#""bin:\xff""#))
        .unwrap()
        .clone();
    let edits = Edits {
        db: 8,
        kind: KeyType::String,
        columns: &cols,
        updates: vec![
            Update {
                loaded: greeting,
                set: vec![
                    (0, Some("salute".into())),
                    (1, Some("ciao mondo".into())),
                    (2, Some("120".into())),
                ],
            },
            Update {
                loaded: binary,
                set: vec![(1, Some(r#""\x01\x02""#.into()))],
            },
        ],
        inserts: vec![vec![(0, Some("fresh".into())), (1, Some(String::new()))]],
        deletes: vec![
            rows.iter()
                .find(|r| r[0].as_deref() == Some("temp"))
                .unwrap()
                .clone(),
        ],
    };
    let script = save(c, &edits).await;
    let after = text(&rows_of(c, "BROWSE db8.string").await)
        .into_iter()
        .map(|r| r.into_iter().map(str::to_owned).collect::<Vec<_>>())
        .collect::<Vec<_>>();
    assert_eq!(after.len(), 3);
    assert_eq!(after[0][..2], [r#""bin:\xff""#, r#""\x01\x02""#]);
    assert_eq!(after[1][..], ["fresh", "", "NULL"]);
    assert_eq!(after[2][..2], ["salute", "ciao mondo"]);
    assert!((1..=120).contains(&after[2][2].parse::<i64>().unwrap()));
    // The undo script puts everything back.
    let undo: Vec<&str> = script
        .undo
        .iter()
        .map(String::as_str)
        .filter(|l| !l.starts_with('#'))
        .collect();
    assert!(c.execute_all(&undo, &[]).await.error.is_none());
    let back = text(&rows_of(c, "BROWSE db8.string").await)
        .into_iter()
        .map(|r| r[0].to_owned())
        .collect::<Vec<_>>();
    assert_eq!(back, [r#""bin:\xff""#, "greeting", "temp"]);
    assert_eq!(
        text(&rows_of(c, "SELECT 8\nGET greeting").await),
        [["hello"]]
    );
}

/// The JSON part of `saves_grid_edits_and_undoes_them`: a document replaced, then invalid
/// JSON, which the server refuses.
async fn save_json_edits(c: &dyn Connection) {
    // JSON: a document replaced; invalid JSON is the server's error.
    let cols = columns(KeyType::Json);
    let rows = loaded(c, "BROWSE json").await;
    let mut edits = Edits {
        db: 8,
        kind: KeyType::Json,
        columns: &cols,
        updates: vec![Update {
            loaded: rows[0].clone(),
            set: vec![(1, Some(r#"{"z":0}"#.into()))],
        }],
        inserts: Vec::new(),
        deletes: Vec::new(),
    };
    save(c, &edits).await;
    assert_eq!(text(&rows_of(c, "BROWSE json").await)[0][1], r#"{"z":0}"#);
    edits.updates[0].set = vec![(1, Some("{not json".into()))];
    let script = edit::script(&edits).unwrap();
    let statements: Vec<&str> = script.commands.iter().map(String::as_str).collect();
    let ran = c.execute_all(&statements, &[]).await;
    assert!(ran.error.is_some());
    assert_eq!(text(&rows_of(c, "BROWSE json").await)[0][1], r#"{"z":0}"#);
}

#[tokio::test]
async fn saves_grid_edits_and_undoes_them() {
    let Some(conn) = connect(8).await else { return };
    let c = &*conn;
    seed(c).await;
    save_string_edits(c).await;

    // Hash: a field renamed, a value changed, a field deleted, one added.
    let cols = columns(KeyType::Hash);
    let rows = loaded(c, "BROWSE hash").await;
    let mut edits = Edits {
        db: 8,
        kind: KeyType::Hash,
        columns: &cols,
        updates: vec![
            Update {
                loaded: rows[0].clone(),
                set: vec![(1, Some("language".into()))],
            },
            Update {
                loaded: rows[1].clone(),
                set: vec![(2, Some("Ada L.".into()))],
            },
        ],
        inserts: vec![vec![
            (0, Some("user:3".into())),
            (1, Some("name".into())),
            (2, Some("Cy".into())),
        ]],
        deletes: vec![rows[2].clone()],
    };
    save(c, &edits).await;
    assert_eq!(
        text(&rows_of(c, "BROWSE hash").await),
        [
            ["user:1", "language", "en"],
            ["user:1", "name", "Ada L."],
            ["user:3", "name", "Cy"]
        ]
    );

    // List: an item changed and one deleted by index, one appended.
    let cols = columns(KeyType::List);
    let rows = loaded(c, "BROWSE list").await;
    edits = Edits {
        db: 8,
        kind: KeyType::List,
        columns: &cols,
        updates: vec![Update {
            loaded: rows[2].clone(),
            set: vec![(2, Some("C".into()))],
        }],
        inserts: vec![vec![(0, Some("queue".into())), (2, Some("d".into()))]],
        deletes: vec![rows[0].clone()],
    };
    save(c, &edits).await;
    assert_eq!(
        text(&rows_of(c, "BROWSE list").await),
        [
            ["queue", "0", "b"],
            ["queue", "1", "C"],
            ["queue", "2", "d"]
        ]
    );

    // Sorted set: a score and a member changed.
    let cols = columns(KeyType::Zset);
    let rows = loaded(c, "BROWSE zset").await;
    edits = Edits {
        db: 8,
        kind: KeyType::Zset,
        columns: &cols,
        updates: vec![
            Update {
                loaded: rows[0].clone(),
                set: vec![(2, Some("10".into()))],
            },
            Update {
                loaded: rows[1].clone(),
                set: vec![(1, Some("robert".into()))],
            },
        ],
        inserts: Vec::new(),
        deletes: Vec::new(),
    };
    save(c, &edits).await;
    assert_eq!(
        text(&rows_of(c, "BROWSE zset").await),
        [["board", "alice", "10"], ["board", "robert", "3"]]
    );

    save_json_edits(c).await;
}

#[tokio::test]
async fn cancels_blocking_commands() {
    let Some(conn) = connect(9).await else { return };
    let handle = conn.cancel_handle();
    let cancel = tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(300)).await;
        handle.cancel().await
    });
    let result = tokio::time::timeout(
        Duration::from_secs(10),
        conn.execute("BLPOP never:filled 0", &ExecOptions::default()),
    )
    .await
    .expect("cancel did not stop the command");
    assert!(matches!(result, Err(Error::Cancelled)), "{result:?}");
    cancel.await.unwrap().unwrap();
    // The connection serves the next command.
    assert_eq!(text(&rows_of(&*conn, "PING").await), [["PONG"]]);
}

#[tokio::test]
async fn signs_in_with_acl_users_and_selects_the_database() {
    let Some(conn) = connect(10).await else {
        return;
    };
    run(
        &*conn,
        "ACL SETUSER platy_reader on >s3cret ~* &* +@read +@connection -@dangerous +info",
    )
    .await;
    run(&*conn, "SET mine 1").await;
    let (url, _) = url().unwrap();
    let base = url.split('@').nth(1).unwrap().split('/').next().unwrap();
    let reader = connect_to(&format!("redis://platy_reader:s3cret@{base}/10"))
        .await
        .unwrap();
    assert_eq!(text(&rows_of(&*reader, "GET mine").await), [["1"]]);
    let Error::Database(e) = err(&*reader, "SET mine 2").await else {
        panic!()
    };
    assert_eq!(e.code.as_deref(), Some("NOPERM"));
    let wrong = connect_to(&format!("redis://platy_reader:wrong@{base}/10")).await;
    assert!(matches!(wrong, Err(Error::Connect(_))), "{wrong:?}");
    assert!(
        connect_to(&format!("redis://:platypus@{base}/x"))
            .await
            .is_err()
    );
    // The session's schema from its settings.
    let mut config = ConnectionConfig::from_url(&url).unwrap();
    config.init.schema = Some("db10".to_owned());
    let conn2 = RedisDriver.connect(&config).await.unwrap();
    assert_eq!(text(&rows_of(&*conn2, "GET mine").await), [["1"]]);
    run(&*conn, "ACL DELUSER platy_reader").await;
}

#[tokio::test]
async fn reopens_a_lost_connection() {
    let Some(conn) = connect(11).await else {
        return;
    };
    run(&*conn, "SET k v").await;
    let id = text(&rows_of(&*conn, "CLIENT ID").await)[0][0].to_owned();
    let other = connect_to(&url().unwrap().0).await.unwrap();
    run(&*other, &format!("CLIENT KILL ID {id}")).await;
    // The first command after the kill fails or reconnects; either way the session recovers.
    let mut ok = false;
    for _ in 0..3 {
        if conn
            .execute("SELECT 11\nGET k", &ExecOptions::default())
            .await
            .is_ok()
        {
            ok = true;
            break;
        }
    }
    assert!(ok, "the session did not reconnect");
    assert!(!conn.is_closed());
}

/// The error of saving `edits`, which must fail.
async fn refused(conn: &dyn Connection, edits: &Edits<'_>) -> (String, usize) {
    let script = edit::script(edits).unwrap();
    let statements: Vec<&str> = script.commands.iter().map(String::as_str).collect();
    let ran = conn.execute_all(&statements, &[]).await;
    (
        ran.error.expect("the save must fail").to_string(),
        ran.rows.len(),
    )
}

#[tokio::test]
async fn refuses_edits_onto_existing_keys() {
    let Some(conn) = connect(12).await else {
        return;
    };
    let c = &*conn;
    run(c, "SET a 1").await;
    run(c, "SET b 2").await;
    run(c, "HSET h f 1 g 2").await;
    let cols = columns(KeyType::String);
    let rows = loaded(c, "BROWSE db12.string").await;
    // A key renamed onto another: refused before anything runs.
    let edits = Edits {
        db: 12,
        kind: KeyType::String,
        columns: &cols,
        updates: vec![Update {
            loaded: rows[0].clone(),
            set: vec![(0, Some("b".into())), (1, Some("changed".into()))],
        }],
        inserts: Vec::new(),
        deletes: Vec::new(),
    };
    let (error, ran) = refused(c, &edits).await;
    assert!(error.contains("the key `b` already exists"), "{error}");
    // `SELECT`, then the command refused: the second.
    assert_eq!(ran, 1);
    let strings = [["a", "1", "NULL"], ["b", "2", "NULL"]];
    assert_eq!(text(&rows_of(c, "BROWSE string").await), strings);
    // A new row with the key of one that exists: its TTL stays as it was.
    let edits = Edits {
        updates: Vec::new(),
        inserts: vec![vec![
            (0, Some("a".into())),
            (1, Some("x".into())),
            (2, Some("10".into())),
        ]],
        ..edits
    };
    assert!(refused(c, &edits).await.0.contains("the key `a`"));
    assert_eq!(text(&rows_of(c, "BROWSE string").await), strings);

    // A field renamed onto another is refused, unless the same save deletes that one.
    let cols = columns(KeyType::Hash);
    let rows = loaded(c, "BROWSE hash").await;
    let mut edits = Edits {
        db: 12,
        kind: KeyType::Hash,
        columns: &cols,
        updates: vec![Update {
            loaded: rows[0].clone(),
            set: vec![(1, Some("g".into()))],
        }],
        inserts: Vec::new(),
        deletes: Vec::new(),
    };
    assert!(refused(c, &edits).await.0.contains("the field `g` of `h`"));
    edits.deletes.push(rows[1].clone());
    save(c, &edits).await;
    assert_eq!(text(&rows_of(c, "BROWSE hash").await), [["h", "g", "1"]]);
    // The E2E scenario `redis-browse` reads this database as empty.
    run(c, "FLUSHDB").await;
}
