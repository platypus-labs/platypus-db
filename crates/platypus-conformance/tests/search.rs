//! The scenarios of `conformance.rs` that apply to a search engine, on Elasticsearch or
//! `OpenSearch`: server info, SQL with types and paging, console searches that return
//! documents and page past one page, the catalog and mappings, the read-only analysis, saving
//! the grid's changes with their undo, cancel and the server's errors. Elasticsearch comes
//! from `docker/compose.yml` (`docker compose --profile search up -d --wait elasticsearch`,
//! login `elastic` / `platypus`, port 59200); `PLATYPUS_TEST_SEARCH` points at another server,
//! such as the compose `opensearch` service (`opensearch://localhost:59201`), and makes the
//! server required. Without a server the tests are skipped.
//!
//! Each test works in indices of its own (`pdb_<test>`), which it deletes first: they run in
//! parallel.

use std::time::{Duration, Instant};

use arrow_cast::display::array_value_to_string;
use futures::TryStreamExt;
use platypus_core::arrow_array::{Array, RecordBatch};
use platypus_core::arrow_schema::DataType;
use platypus_core::{
    Connection, ConnectionConfig, Driver, Error, ExecOptions, ExecResult, META_DB_TYPE, ObjectKind,
    ObjectRef,
};
use platypus_driver_search::SearchDriver;
use platypus_driver_search::command::{self, Command, Kind};
use platypus_driver_search::edit::{self, Edits, Update};

const DEFAULT_URL: &str = "elasticsearch://elastic:platypus@localhost:59200";

fn url() -> Option<(String, bool)> {
    // Another database alone (`PLATYPUS_TEST_ONLY`): not this one.
    if std::env::var("PLATYPUS_TEST_ONLY").is_ok_and(|only| only != "search") {
        return None;
    }
    Some(match std::env::var("PLATYPUS_TEST_SEARCH") {
        Ok(url) => (url, true),
        Err(_) => (DEFAULT_URL.to_owned(), false),
    })
}

async fn connect_to(url: &str) -> platypus_core::Result<Box<dyn Connection>> {
    let mut config = ConnectionConfig::from_url(url).unwrap();
    config.connect_timeout = Some(Duration::from_secs(3));
    SearchDriver.connect(&config).await
}

async fn connect() -> Option<Box<dyn Connection>> {
    let (url, required) = url()?;
    match connect_to(&url).await {
        Ok(conn) => Some(conn),
        Err(e) if !required => {
            eprintln!("skipping search: unreachable at {url}: {e}");
            None
        }
        Err(e) => panic!("cannot connect to PLATYPUS_TEST_SEARCH: {e}"),
    }
}

fn is_opensearch(conn: &dyn Connection) -> bool {
    conn.server_info().product == "OpenSearch"
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

/// The rows a write changed.
async fn affected(conn: &dyn Connection, src: &str) -> u64 {
    match conn.execute(src, &ExecOptions::default()).await {
        Ok(ExecResult::Affected { rows }) => rows,
        Ok(ExecResult::Rows(_)) => panic!("{src}: a count expected"),
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

fn rows(batches: &[RecordBatch]) -> usize {
    batches.iter().map(RecordBatch::num_rows).sum()
}

fn columns(batches: &[RecordBatch]) -> Vec<String> {
    batches[0]
        .schema()
        .fields()
        .iter()
        .map(|f| f.name().clone())
        .collect()
}

/// An index emptied and created with `mapping` (the body of the `mappings`), then filled with
/// `docs` through `_bulk`, visible at once.
async fn fill(conn: &dyn Connection, index: &str, mapping: &str, docs: &[String]) {
    let _ = conn
        .execute(&format!("DELETE /{index}"), &ExecOptions::default())
        .await;
    let settings = r#""settings":{"number_of_shards":1,"number_of_replicas":0}"#;
    affected_or_rows(
        conn,
        &format!("PUT /{index}\n{{{settings},\"mappings\":{mapping}}}"),
    )
    .await;
    if docs.is_empty() {
        return;
    }
    for chunk in docs.chunks(1000) {
        let mut bulk = format!("POST /{index}/_bulk?refresh=true\n");
        for doc in chunk {
            bulk.push_str("{\"index\":{}}\n");
            bulk.push_str(doc);
            bulk.push('\n');
        }
        // A blank line ends the statement.
        let ExecResult::Affected { rows } = conn
            .execute(&bulk, &ExecOptions::default())
            .await
            .unwrap_or_else(|e| panic!("bulk into {index}: {e}"))
        else {
            panic!("bulk into {index} failed");
        };
        assert_eq!(rows, u64::try_from(chunk.len()).unwrap());
    }
}

async fn affected_or_rows(conn: &dyn Connection, src: &str) {
    conn.execute(src, &ExecOptions::default())
        .await
        .unwrap_or_else(|e| panic!("{src}: {e}"));
}

fn db_type(batch: &RecordBatch, column: &str) -> String {
    batch.schema().field_with_name(column).unwrap().metadata()[META_DB_TYPE].clone()
}

#[tokio::test]
async fn reports_the_server_and_its_capabilities() {
    let Some(conn) = connect().await else { return };
    let info = conn.server_info();
    assert!(
        matches!(info.product.as_str(), "Elasticsearch" | "OpenSearch"),
        "{info:?}"
    );
    assert!(info.version_number().is_some(), "{info:?}");
    assert_eq!(info.below_minimum(), None);
    let caps = conn.capabilities();
    assert!(caps.cancel && caps.object_ddl && !caps.transactions && !caps.explain);
    assert!(!conn.is_closed());
    assert!(conn.check_alive().await);
    assert!(conn.set_autocommit(false).await.is_err());
    let schemas = conn.schemas().await.unwrap();
    assert_eq!(schemas.len(), 1);
    assert!(schemas[0].current && !schemas[0].name.is_empty());
    // The root reply is a document.
    let root = run(&*conn, "GET /").await;
    assert!(columns(&root).contains(&"cluster_name".to_owned()));
    assert_eq!(rows(&root), 1);
    // A wrong login is a failed connection.
    let (url, _) = url().unwrap();
    if url.contains("elastic:") {
        let wrong = url.replace(":platypus@", ":wrong@");
        let error = connect_to(&wrong).await.unwrap_err();
        assert!(matches!(error, Error::Connect(_)), "{error:?}");
    }
    // Nothing listens there.
    assert!(matches!(
        connect_to("elasticsearch://127.0.0.1:1").await,
        Err(Error::Connect(_))
    ));
}

#[tokio::test]
async fn runs_sql_with_types_and_pages() {
    let Some(conn) = connect().await else { return };
    let conn = &*conn;
    let docs: Vec<String> = (1..=250)
        .map(|n| {
            format!(
                r#"{{"n":{n},"price":{n}.5,"name":"item {n}","ok":{},"born":"2024-05-06T07:08:09Z"}}"#,
                n % 2 == 0
            )
        })
        .collect();
    fill(
        conn,
        "pdb_sql",
        r#"{"properties":{"n":{"type":"long"},"price":{"type":"double"},"name":{"type":"keyword"},"ok":{"type":"boolean"},"born":{"type":"date"}}}"#,
        &docs,
    )
    .await;
    let select = "SELECT n, price, name, ok, born FROM pdb_sql ORDER BY n";
    let batches = run_paged(conn, select, 100).await;
    if is_opensearch(conn) {
        // The SQL plugin pages only some statements.
        assert!(rows(&batches) >= 100);
    } else {
        assert_eq!(
            batches
                .iter()
                .map(RecordBatch::num_rows)
                .collect::<Vec<_>>(),
            [100, 100, 50]
        );
        let types: Vec<DataType> = batches[0]
            .schema()
            .fields()
            .iter()
            .map(|f| f.data_type().clone())
            .collect();
        assert_eq!(types[0], DataType::Int64);
        assert_eq!(types[1], DataType::Float64);
        assert_eq!(types[2], DataType::Utf8);
        assert_eq!(types[3], DataType::Boolean);
        assert!(
            matches!(types[4], DataType::Timestamp(_, Some(_))),
            "{types:?}"
        );
        let all = cells(&batches);
        assert_eq!(all[0][0].as_deref(), Some("1"));
        assert_eq!(all[0][1].as_deref(), Some("1.5"));
        assert_eq!(all[0][2].as_deref(), Some("item 1"));
        assert_eq!(all[1][3].as_deref(), Some("true"));
        assert!(
            all[0][4]
                .as_deref()
                .unwrap()
                .starts_with("2024-05-06T07:08:09")
        );
        assert_eq!(all[249][0].as_deref(), Some("250"));
        assert_eq!(db_type(&batches[0], "n"), "long");
    }
    // Aggregates, a statement ending with `;`, and a LIMIT.
    let count = cells(&run(conn, "SELECT COUNT(*) AS total FROM pdb_sql;").await);
    assert_eq!(count[0][0].as_deref(), Some("250"));
    let limited = run(conn, "SELECT n FROM pdb_sql ORDER BY n DESC LIMIT 3").await;
    assert_eq!(cells(&limited)[0][0].as_deref(), Some("250"));
    assert_eq!(rows(&limited), 3);
    let shown = run(conn, "SHOW TABLES LIKE 'pdb_sql'").await;
    assert!(rows(&shown) >= 1);
    // Leaving a result early closes its cursor.
    if !is_opensearch(conn) {
        let options = ExecOptions {
            batch_size: 10,
            paged: true,
        };
        let ExecResult::Rows(mut stream) = conn.execute(select, &options).await.unwrap() else {
            panic!("rows expected")
        };
        assert_eq!(stream.try_next().await.unwrap().unwrap().num_rows(), 10);
        drop(stream);
    }
    // The server's reason, placed in the statement.
    match fails(conn, "SELECT nope FROM pdb_sql").await {
        Error::Database(e) => {
            assert!(
                e.message.contains("nope")
                    || e.detail.as_deref().is_some_and(|d| d.contains("nope")),
                "{e:?}"
            );
            if !is_opensearch(conn) {
                assert_eq!(e.position, Some(8), "{e:?}");
            }
        }
        other => panic!("{other:?}"),
    }
}

#[tokio::test]
async fn searches_give_documents_and_page_past_one_page() {
    let Some(conn) = connect().await else { return };
    let conn = &*conn;
    let docs: Vec<String> = (1..=2500)
        .map(|n| {
            format!(
                r#"{{"n":{n},"name":"doc {n}","tags":["a","b"],"owner":{{"id":{n},"city":"Rome"}}}}"#
            )
        })
        .collect();
    fill(
        conn,
        "pdb_docs",
        r#"{"properties":{"n":{"type":"long"},"name":{"type":"keyword"}}}"#,
        &docs,
    )
    .await;
    // No `size`: all of them, a page at a time.
    let search = r#"GET /pdb_docs/_search
{"query":{"match_all":{}},"sort":[{"n":"asc"}]}"#;
    let options = ExecOptions {
        batch_size: 1000,
        paged: true,
    };
    let ExecResult::Rows(mut stream) = conn.execute(search, &options).await.unwrap() else {
        panic!("rows expected")
    };
    let schema = stream.schema();
    let names: Vec<_> = schema.fields().iter().map(|f| f.name().as_str()).collect();
    assert_eq!(names, ["_id", "n", "name", "tags", "owner"]);
    assert_eq!(schema.field(1).data_type(), &DataType::Int64);
    assert_eq!(schema.field(3).metadata()[META_DB_TYPE], "array");
    assert_eq!(schema.field(4).metadata()[META_DB_TYPE], "document");
    let mut sizes = Vec::new();
    let mut numbers = Vec::new();
    while let Some(batch) = stream.try_next().await.unwrap() {
        sizes.push(batch.num_rows());
        numbers.extend(cells(&[batch]).into_iter().map(|r| r[1].clone().unwrap()));
    }
    assert_eq!(sizes, [1000, 1000, 500]);
    let want: Vec<String> = (1..=2500).map(|n| n.to_string()).collect();
    assert_eq!(numbers, want, "every hit once, in order");
    // The nested values are JSON text, in the order the server wrote them.
    let first = cells(&run_paged(conn, search, 5).await);
    assert_eq!(first[0][3].as_deref(), Some(r#"["a","b"]"#));
    assert_eq!(first[0][4].as_deref(), Some(r#"{"id":1,"city":"Rome"}"#));
    // A `size` above a page is the most hits read; below, one plain request.
    let capped = run_paged(conn, "GET /pdb_docs/_search\n{\"size\":1500}", 1000).await;
    assert_eq!(rows(&capped), 1500);
    let small = run_paged(conn, "GET /pdb_docs/_search\n{\"size\":3}", 1000).await;
    assert_eq!(rows(&small), 3);
    // The server's own default, with a `from`, is not paged.
    let from = run_paged(
        conn,
        "GET /pdb_docs/_search?pretty\n{\"from\":2495,\"size\":10}",
        1000,
    )
    .await;
    assert_eq!(rows(&from), 5);
    let url_size = run_paged(conn, "GET /pdb_docs/_search?size=2", 1000).await;
    assert_eq!(rows(&url_size), 2);
    // Leaving a paged result early closes its point in time or scroll.
    let ExecResult::Rows(mut stream) = conn.execute(search, &options).await.unwrap() else {
        panic!("rows expected")
    };
    stream.try_next().await.unwrap().unwrap();
    drop(stream);
    // Aggregations come after the hits, one result each.
    let aggs = r#"POST /pdb_docs/_search
{"size":0,"aggs":{"top":{"max":{"field":"n"}},"by_name":{"terms":{"field":"name","size":2}}}}"#;
    let ExecResult::Rows(mut first) = conn.execute(aggs, &ExecOptions::default()).await.unwrap()
    else {
        panic!("rows expected")
    };
    let next = first.take_next().expect("a result per aggregation");
    let metrics: Vec<RecordBatch> = first.try_collect().await.unwrap();
    assert_eq!(columns(&metrics), ["top"]);
    assert_eq!(cells(&metrics)[0][0].as_deref(), Some("2500.0"));
    let Some(ExecResult::Rows(buckets)) = next.await.unwrap() else {
        panic!("the buckets expected")
    };
    let buckets: Vec<RecordBatch> = buckets.try_collect().await.unwrap();
    assert_eq!(columns(&buckets), ["key", "doc_count"]);
    assert_eq!(rows(&buckets), 2);
    documents_of_several_indices(conn).await;
}

/// Several indices add `_index`; `_mget`, a single document and `_msearch` are documents too.
async fn documents_of_several_indices(conn: &dyn Connection) {
    fill(
        conn,
        "pdb_docs_b",
        r#"{"properties":{"n":{"type":"long"}}}"#,
        &[r#"{"n":1}"#.to_owned()],
    )
    .await;
    let both = run(conn, "GET /pdb_docs,pdb_docs_b/_search\n{\"size\":2000}").await;
    assert_eq!(columns(&both)[..2], ["_id", "_index"]);
    let ids = cells(&run(conn, "GET /pdb_docs_b/_search").await);
    let id = ids[0][0].clone().unwrap();
    let doc = run(conn, &format!("GET /pdb_docs_b/_doc/{id}")).await;
    assert_eq!(columns(&doc), ["_id", "n"]);
    let mget = run(
        conn,
        &format!("POST /pdb_docs_b/_mget\n{{\"ids\":[\"{id}\",\"nope\"]}}"),
    )
    .await;
    assert_eq!(rows(&mget), 1);
    let multi = r#"POST /_msearch
{"index":"pdb_docs_b"}
{"query":{"match_all":{}}}
{"index":"pdb_docs"}
{"size":1}
"#;
    let ExecResult::Rows(mut one) = conn.execute(multi, &ExecOptions::default()).await.unwrap()
    else {
        panic!("rows expected")
    };
    let second = one.take_next().expect("a result per search");
    assert_eq!(rows(&one.try_collect::<Vec<_>>().await.unwrap()), 1);
    let Some(ExecResult::Rows(second)) = second.await.unwrap() else {
        panic!("the second search expected")
    };
    assert_eq!(rows(&second.try_collect::<Vec<_>>().await.unwrap()), 1);
}

#[tokio::test]
async fn reads_the_catalog_and_the_mappings() {
    let Some(conn) = connect().await else { return };
    let conn = &*conn;
    fill(
        conn,
        "pdb_meta",
        r#"{"properties":{"title":{"type":"text","fields":{"raw":{"type":"keyword"}}},"customer":{"properties":{"name":{"type":"keyword"},"address":{"properties":{"zip":{"type":"keyword"}}}}},"n":{"type":"long"},"when":{"type":"date"}}}"#,
        &[r#"{"title":"a","n":1}"#.to_owned(), r#"{"title":"b","n":2}"#.to_owned()],
    )
    .await;
    affected_or_rows(conn, "POST /_aliases\n{\"actions\":[{\"add\":{\"index\":\"pdb_meta\",\"alias\":\"pdb_meta_alias\"}}]}").await;
    let cluster = conn.schemas().await.unwrap().remove(0).name;
    let objects = conn.objects(&cluster).await.unwrap();
    let index = objects
        .iter()
        .find(|o| o.name == "pdb_meta")
        .expect("the index");
    assert_eq!(index.kind, ObjectKind::Table);
    assert_eq!(index.rows, Some(2));
    assert!(index.size.is_some_and(|s| s > 0));
    let alias = objects
        .iter()
        .find(|o| o.name == "pdb_meta_alias")
        .expect("the alias");
    assert_eq!(alias.kind, ObjectKind::View);
    assert!(alias.comment.as_deref().unwrap().contains("pdb_meta"));
    let names: Vec<_> = objects.iter().map(|o| o.name.clone()).collect();
    let mut sorted = names.clone();
    sorted.sort();
    assert_eq!(names, sorted, "sorted by name");
    let details = conn.table_details(&cluster, "pdb_meta").await.unwrap();
    let column = |name: &str| details.columns.iter().find(|c| c.name == name);
    assert_eq!(details.columns[0].name, "_id");
    assert!(details.columns[0].primary_key && !details.columns[0].nullable);
    assert_eq!(column("customer.address.zip").unwrap().data_type, "keyword");
    assert_eq!(column("title").unwrap().data_type, "text");
    assert_eq!(column("title.raw").unwrap().data_type, "keyword");
    assert_eq!(column("n").unwrap().data_type, "long");
    assert_eq!(column("when").unwrap().data_type, "date");
    assert!(column("customer").is_none(), "objects are their fields");
    assert_eq!(details.indexes.len(), 1);
    // An alias has the columns of its index.
    let through = conn
        .table_details(&cluster, "pdb_meta_alias")
        .await
        .unwrap();
    assert_eq!(through.columns.len(), details.columns.len());
    let catalog = conn.catalog_columns().await.unwrap();
    assert!(
        catalog
            .iter()
            .any(|c| c.table == "pdb_meta" && c.column == "customer.name")
    );
    assert!(
        catalog
            .iter()
            .any(|c| c.table == "pdb_meta" && c.column == "_id")
    );
    assert!(catalog.iter().all(|c| !c.table.starts_with('.')));
    // What creates the index again.
    let ddl = conn
        .object_ddl(&ObjectRef {
            schema: &cluster,
            name: "pdb_meta",
            kind: ObjectKind::Table,
            parent: None,
        })
        .await
        .unwrap();
    assert!(ddl.starts_with("PUT /pdb_meta\n{"), "{ddl}");
    assert!(
        ddl.contains("\"mappings\"") && !ddl.contains("creation_date"),
        "{ddl}"
    );
    // It is a statement of the editor, and it runs.
    let _ = conn
        .execute("DELETE /pdb_meta", &ExecOptions::default())
        .await;
    affected_or_rows(conn, &ddl).await;
    assert_eq!(
        conn.table_details(&cluster, "pdb_meta")
            .await
            .unwrap()
            .columns
            .len(),
        details.columns.len()
    );
    let properties = conn
        .object_properties(&ObjectRef {
            schema: &cluster,
            name: "pdb_meta",
            kind: ObjectKind::Table,
            parent: None,
        })
        .await
        .unwrap();
    assert!(properties.iter().any(|p| p.name == "Documents"));
    // A missing index says so.
    assert!(matches!(
        conn.table_details(&cluster, "pdb_no_such_index").await,
        Err(Error::Database(e)) if e.code.as_deref() == Some("index_not_found_exception")
    ));
}

#[tokio::test]
async fn analyses_statements_against_the_server() {
    let Some(conn) = connect().await else { return };
    let conn = &*conn;
    fill(
        conn,
        "pdb_kind",
        r#"{"properties":{"n":{"type":"long"}}}"#,
        &[r#"{"n":1}"#.to_owned()],
    )
    .await;
    let cases = [
        ("GET /pdb_kind/_search", Kind::Query),
        (
            "POST /pdb_kind/_search\n{\"query\":{\"match_all\":{}}}",
            Kind::Query,
        ),
        ("POST /pdb_kind/_count", Kind::Query),
        ("GET /_cat/indices/pdb_kind", Kind::Query),
        ("HEAD /pdb_kind", Kind::Query),
        ("SELECT COUNT(*) FROM pdb_kind", Kind::Query),
        ("PUT /pdb_kind/_doc/9\n{\"n\":9}", Kind::Dml),
        ("POST /pdb_kind/_update/9\n{\"doc\":{\"n\":10}}", Kind::Dml),
        ("DELETE /pdb_kind/_doc/9", Kind::Dml),
        (
            "POST /_bulk\n{\"delete\":{\"_index\":\"pdb_kind\",\"_id\":\"9\"}}\n",
            Kind::Dml,
        ),
        ("PUT /pdb_kind_2", Kind::Ddl),
        ("DELETE /pdb_kind", Kind::Ddl),
        ("PUT /pdb_kind/_mapping\n{\"properties\":{}}", Kind::Ddl),
        ("PUT /_cluster/settings\n{}", Kind::Other),
        ("POST /pdb_kind/_cache/clear", Kind::Other),
        ("DROP TABLE pdb_kind", Kind::Other),
    ];
    for (text, kind) in cases {
        let parsed = command::parse(text).unwrap();
        assert_eq!(parsed.kind(), kind, "{text}");
        assert_eq!(command::only_reads(text), kind == Kind::Query, "{text}");
        // What reads, reads on the real server too.
        if kind == Kind::Query {
            conn.execute(text, &ExecOptions::default())
                .await
                .unwrap_or_else(|e| panic!("{text}: {e}"));
        }
    }
    // A script is read-only only when every statement is.
    assert!(command::only_reads("GET /pdb_kind/_search\n\nSELECT 1"));
    assert!(!command::only_reads(
        "GET /pdb_kind/_search\n\nDELETE /pdb_kind"
    ));
    assert!(!command::only_reads("GET /pdb_kind/_search\n{oops"));
    assert!(!command::only_reads(""));
    // The write the check refused did not run.
    let after = cells(&run(conn, "POST /pdb_kind/_count").await);
    assert_eq!(after[0][0].as_deref(), Some("1"));
    // The editor cuts a script into the same statements.
    let script = "GET /a\n\nSELECT 1;\n# c\nDELETE /b\n";
    let kinds: Vec<Kind> = command::split(script)
        .into_iter()
        .map(|r| command::parse(&script[r]).unwrap().kind())
        .collect();
    assert_eq!(kinds, [Kind::Query, Kind::Query, Kind::Ddl]);
    assert!(matches!(
        command::parse("GET /a").unwrap(),
        Command::Request(_)
    ));
}

/// The columns and cells of the documents of `pdb_edit`, by `qty`.
async fn read_edit(conn: &dyn Connection) -> (Vec<String>, Vec<Vec<Option<String>>>) {
    let batches = run(
        conn,
        "GET /pdb_edit/_search
{\"sort\":[{\"qty\":\"asc\"}]}",
    )
    .await;
    (columns(&batches), cells(&batches))
}

/// A document with an `_id` that exists is refused, and the run stops there.
async fn refuses_an_existing_id(conn: &dyn Connection, edits: &Edits<'_>) {
    let again = edit::script(&Edits {
        inserts: vec![vec![
            (0, Some("fixed".to_owned())),
            (1, Some("dup".to_owned())),
        ]],
        updates: Vec::new(),
        deletes: Vec::new(),
        ..edits.clone()
    })
    .unwrap();
    let commands: Vec<&str> = again.commands.iter().map(String::as_str).collect();
    let ran = conn.execute_all(&commands, &[]).await;
    assert_eq!(ran.rows.len(), 0);
    match ran.error {
        Some(Error::Database(e)) => {
            assert_eq!(
                e.code.as_deref(),
                Some("version_conflict_engine_exception"),
                "{e:?}"
            );
        }
        other => panic!("{other:?}"),
    }
}

#[tokio::test]
async fn saves_the_grids_changes_and_undoes_them() {
    let Some(conn) = connect().await else { return };
    let conn = &*conn;
    let docs: Vec<String> = ["a", "b", "c"]
        .iter()
        .enumerate()
        .map(|(i, name)| {
            format!(
                r#"{{"name":"{name}","qty":{},"ok":true,"meta":{{"x":1}}}}"#,
                i + 1
            )
        })
        .collect();
    fill(
        conn,
        "pdb_edit",
        r#"{"properties":{"name":{"type":"keyword"},"qty":{"type":"long"},"ok":{"type":"boolean"},"meta":{"properties":{"x":{"type":"long"},"y":{"type":"long"}}}}}"#,
        &docs,
    )
    .await;
    let read = |conn| async move {
        let batches = run(
            conn,
            "GET /pdb_edit/_search\n{\"sort\":[{\"qty\":\"asc\"}]}",
        )
        .await;
        (columns(&batches), cells(&batches))
    };
    let (names, before) = read(conn).await;
    assert_eq!(names, ["_id", "name", "qty", "ok", "meta"]);
    let types: Vec<String> = ["string", "string", "long", "boolean", "document"]
        .map(str::to_owned)
        .to_vec();
    let id = |row: usize| before[row][0].clone().unwrap();
    let edits = Edits {
        index: "pdb_edit",
        columns: &names,
        types: &types,
        updates: vec![Update {
            loaded: before[0].clone(),
            set: vec![
                (1, Some("renamed".to_owned())),
                (2, Some("41".to_owned())),
                (4, Some(r#"{"y":2}"#.to_owned())),
            ],
        }],
        inserts: vec![
            vec![
                (0, Some("fixed".to_owned())),
                (1, Some("new".to_owned())),
                (2, Some("7".to_owned())),
            ],
            vec![(1, Some("generated".to_owned()))],
        ],
        deletes: vec![before[2].clone()],
    };
    let script = edit::script(&edits).unwrap();
    assert_eq!(script.commands.len(), 4);
    let commands: Vec<&str> = script.commands.iter().map(String::as_str).collect();
    let ran = conn.execute_all(&commands, &[]).await;
    assert!(ran.error.is_none(), "{:?}", ran.error);
    assert_eq!(ran.rows, [1, 1, 1, 1]);
    let (_, after) = read_edit(conn).await;
    let by_name = |rows: &[Vec<Option<String>>], name: &str| {
        rows.iter().find(|r| r[1].as_deref() == Some(name)).cloned()
    };
    let renamed = by_name(&after, "renamed").expect("the update");
    assert_eq!(renamed[0], Some(id(0)));
    assert_eq!(renamed[2].as_deref(), Some("41"));
    // An object replaces the old one instead of merging with it.
    assert_eq!(renamed[4].as_deref(), Some(r#"{"y":2}"#));
    assert_eq!(by_name(&after, "new").unwrap()[0].as_deref(), Some("fixed"));
    assert!(by_name(&after, "generated").is_some());
    assert!(by_name(&after, "c").is_none(), "the delete");
    assert_eq!(after.len(), 4);
    refuses_an_existing_id(conn, &edits).await;
    undoes_the_changes(conn, &script).await;
    // Deleting a document that is not there changes nothing.
    assert_eq!(affected(conn, "DELETE /pdb_edit/_doc/nope").await, 0);
}

/// The requests that undo the changes: the update, the insert with an `_id` and the delete.
async fn undoes_the_changes(conn: &dyn Connection, script: &edit::Script) {
    let undo: Vec<&str> = script
        .undo
        .iter()
        .filter(|l| !l.starts_with('#'))
        .map(String::as_str)
        .collect();
    assert_eq!(undo.len(), 3);
    assert!(
        script
            .undo
            .iter()
            .any(|l| l.starts_with("# the new document"))
    );
    let ran = conn.execute_all(&undo, &[]).await;
    assert!(ran.error.is_none(), "{:?}", ran.error);
    let (_, restored) = read_edit(conn).await;
    // The generated one stays: it could not be undone by id.
    let mut kept: Vec<_> = restored
        .iter()
        .filter(|r| r[1].as_deref() != Some("generated"))
        .collect();
    kept.sort_by_key(|r| r[2].clone());
    assert_eq!(kept.len(), 3);
    assert_eq!(kept[0][1].as_deref(), Some("a"));
    assert_eq!(kept[0][2].as_deref(), Some("1"));
    // The old object comes back whole with the replacing script.
    assert_eq!(kept[0][4].as_deref(), Some(r#"{"x":1}"#));
    assert_eq!(kept[2][1].as_deref(), Some("c"));
}

#[tokio::test]
async fn cancels_a_slow_search_and_goes_on() {
    let Some(conn) = connect().await else { return };
    let conn = &*conn;
    let docs: Vec<String> = (1..=4000).map(|n| format!(r#"{{"n":{n}}}"#)).collect();
    fill(
        conn,
        "pdb_slow",
        r#"{"properties":{"n":{"type":"long"}}}"#,
        &docs,
    )
    .await;
    // About a million loop turns per document.
    let slow = "POST /pdb_slow/_search\n{\"size\":1,\"query\":{\"script\":{\"script\":{\"source\":\"long s = 0; for (long i = 0; i < 900000; i++) { s += i % 7; } return s > 0 && doc['n'].value > 0;\"}}}}";
    let handle = conn.cancel_handle();
    let started = Instant::now();
    let cancel = tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(700)).await;
        handle.cancel().await
    });
    let result = tokio::time::timeout(
        Duration::from_secs(20),
        conn.execute(slow, &ExecOptions::default()),
    )
    .await
    .expect("cancel did not stop the search");
    assert!(matches!(result, Err(Error::Cancelled)), "{result:?}");
    cancel.await.unwrap().unwrap();
    assert!(started.elapsed() < Duration::from_secs(10));
    // The session goes on.
    assert!(!conn.is_closed());
    let count = cells(&run(conn, "POST /pdb_slow/_count").await);
    assert_eq!(count[0][0].as_deref(), Some("4000"));
    // The server stopped it too: no search of the index is left running.
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        let tasks = run(conn, "GET /_tasks?actions=*search*&detailed=true").await;
        let text = format!("{:?}", cells(&tasks));
        if !text.contains("pdb_slow") {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "the search still runs on the server"
        );
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
}

#[tokio::test]
async fn reports_errors_with_the_servers_reason() {
    let Some(conn) = connect().await else { return };
    let conn = &*conn;
    fill(
        conn,
        "pdb_err",
        r#"{"properties":{"n":{"type":"long"}}}"#,
        &[r#"{"n":1}"#.to_owned()],
    )
    .await;
    let database = |e: Error| match e {
        Error::Database(e) => e,
        other => panic!("{other:?}"),
    };
    let missing = database(fails(conn, "GET /pdb_no_such_index/_search").await);
    assert_eq!(missing.code.as_deref(), Some("index_not_found_exception"));
    assert!(missing.message.contains("no such index"), "{missing:?}");
    let unknown = database(fails(conn, "POST /pdb_err/_search\n{\"query\":{\"nope\":{}}}").await);
    assert!(unknown.message.contains("nope"), "{unknown:?}");
    let mapping = database(fails(conn, "POST /pdb_err/_doc\n{\"n\":\"not a number\"}").await);
    assert!(
        mapping.message.contains("failed to parse") || mapping.message.contains("number"),
        "{mapping:?}"
    );
    let conflict = database(fails(conn, "PUT /pdb_err").await);
    assert_eq!(
        conflict.code.as_deref(),
        Some("resource_already_exists_exception")
    );
    // Statements that do not parse never reach the server.
    let body = database(fails(conn, "POST /pdb_err/_doc\n{\"n\": }").await);
    assert!(body.message.contains("invalid JSON body"), "{body:?}");
    assert!(matches!(
        conn.execute("   \n# nothing\n", &ExecOptions::default())
            .await,
        Err(Error::Database(_))
    ));
    // After errors the session works.
    assert!(!conn.is_closed());
    assert_eq!(
        cells(&run(conn, "POST /pdb_err/_count").await)[0][0].as_deref(),
        Some("1")
    );
}
