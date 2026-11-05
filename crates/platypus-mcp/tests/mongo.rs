//! Agents against the MongoDB of `docker/compose.yml` (profile `mongo`), or the server at
//! `PLATYPUS_TEST_MONGO`: nested fields of real documents must stay masked on the whole path.
//! Skipped when the default server is unreachable.

use arrow_array::RecordBatch;
use futures::TryStreamExt;
use platypus_core::{Connection, ConnectionConfig, Driver, ExecOptions, ExecResult, META_DB_TYPE};
use platypus_driver_mongo::MongoDriver;
use platypus_mcp::{Agent, Format, Profile, ProfileArgs, QueryArgs, SampleArgs};

const DEFAULT_URL: &str = "mongodb://platypus:platypus@localhost:57017/?authSource=admin";
const COLLECTION: &str = "mcp_masking_probe";
const SECRETS: [&str; 2] = ["s3cret-a", "s3cret-b"];

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
        Ok(_) => Vec::new(),
        Err(e) => panic!("{src}: {e}"),
    }
}

/// The `db_type` the driver reports for each column of the probe collection, by name.
async fn db_types(conn: &dyn Connection) -> Vec<(String, String)> {
    let batches = rows(conn, &format!("db.{COLLECTION}.find({{}})")).await;
    batches[0]
        .schema()
        .fields()
        .iter()
        .map(|f| {
            let t = f.metadata().get(META_DB_TYPE).cloned().unwrap_or_default();
            (f.name().clone(), t)
        })
        .collect()
}

fn agent(dir: &std::path::Path) -> Agent {
    std::fs::write(
        dir.join("connections.toml"),
        "version = 4\n\n[[connections]]\nid = \"00000000-0000-4000-8000-0000000000b1\"\n\
         name = \"Mongo\"\ndriver = \"mongo\"\nhost = \"localhost\"\nport = 57017\n\
         user = \"platypus\"\ndatabase = \"platypus?authSource=admin\"\n\
         password_command = \"echo platypus\"\n",
    )
    .unwrap();
    Agent::new(Profile::at(dir))
}

fn query(sql: &str) -> QueryArgs {
    QueryArgs {
        connection: "Mongo".to_owned(),
        sql: sql.to_owned(),
        max_rows: None,
        max_bytes: None,
        timeout_seconds: None,
        format: Some(Format::Json),
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn nested_secrets_of_real_documents_never_reach_agents() {
    let Some(conn) = connect().await else {
        return;
    };
    rows(&*conn, "use platypus").await;
    rows(&*conn, &format!("db.{COLLECTION}.drop()")).await;
    rows(
        &*conn,
        &format!(
            "db.{COLLECTION}.insertOne({{name: \"Ann\", \
             user: {{password: \"s3cret-a\", city: \"Rome\"}}, \
             logins: [{{ip: \"10.0.0.1\", token: \"s3cret-b\"}}], \
             matrix: [[{{token: \"s3cret-b\", ip: \"10.0.0.2\"}}]], note: \"plain\"}})"
        ),
    )
    .await;

    // What the driver really reports for an object, an array and a nested array.
    let types = db_types(&*conn).await;
    eprintln!("db_type of the columns: {types:?}");
    let type_of = |n: &str| types.iter().find(|(c, _)| c == n).unwrap().1.clone();
    assert_eq!(type_of("user"), "document");
    assert_eq!(type_of("logins"), "array");
    assert_eq!(type_of("matrix"), "array");

    let dir = tempfile::tempdir().unwrap();
    let agent = agent(dir.path());
    // A mark on a path and a mark on the nested name `token`.
    let id = uuid::Uuid::parse_str("00000000-0000-4000-8000-0000000000b1").unwrap();
    let key = |column: &str| platypus_store::CatalogKey {
        database: String::new(),
        schema: "platypus".to_owned(),
        object: COLLECTION.to_owned(),
        column: column.to_owned(),
    };
    let mut catalog = platypus_store::CatalogStore::open(&dir.path().join("catalog.db")).unwrap();
    for column in ["user.password", "token"] {
        catalog.set_sensitive(id, &key(column), Some(true)).unwrap();
    }
    drop(catalog);

    let found = agent
        .run_query(&query(&format!("db.{COLLECTION}.find({{}})")), "test")
        .await
        .unwrap();
    let sample = agent
        .sample_rows_until(
            &SampleArgs {
                connection: "Mongo".to_owned(),
                schema: "platypus".to_owned(),
                table: COLLECTION.to_owned(),
                max_rows: None,
                max_bytes: None,
                timeout_seconds: None,
                format: Some(Format::Json),
            },
            "test",
            std::future::pending(),
        )
        .await
        .unwrap();
    let profile = agent
        .profile_table_until(
            &ProfileArgs {
                connection: "Mongo".to_owned(),
                schema: "platypus".to_owned(),
                table: COLLECTION.to_owned(),
                sample_rows: None,
                max_bytes: None,
                timeout_seconds: None,
            },
            std::future::pending(),
        )
        .await
        .unwrap();
    rows(&*conn, &format!("db.{COLLECTION}.drop()")).await;

    let answers = [
        ("run_query", found.render(Format::Json)),
        ("sample_rows", sample.render(Format::Json)),
        ("profile_table", profile),
    ];
    for (what, text) in &answers {
        eprintln!("{what}: {text}");
        for secret in SECRETS {
            assert!(!text.contains(secret), "{what} leaks {secret}: {text}");
        }
        for visible in ["Rome", "10.0.0.1", "plain"] {
            assert!(text.contains(visible), "{what} hides {visible}: {text}");
        }
    }
}
