//! What masking adds to a `run_query`, measured on a SQLite connection with rules and marks:
//! `cargo test --release -p platypus-mcp --test masking_cost -- --ignored --nocapture`.

use std::path::Path;
use std::sync::Arc;
use std::time::Instant;

use platypus_mcp::masking_seal::SealedMarks;
use platypus_mcp::policy::{AgentPolicy, Seal};
use platypus_mcp::{Agent, Profile, QueryArgs};
use platypus_store::{CatalogKey, CatalogStore, MemorySecrets, SecretStore};
use uuid::Uuid;

const ID: &str = "00000000-0000-4000-8000-0000000000a1";

/// A profile with a table of 50 rows, rules of the user's, and `marks` marks on the connection.
fn profile(dir: &Path, marks: usize) {
    let db = rusqlite::Connection::open(dir.join("people.db")).unwrap();
    db.execute_batch(
        "CREATE TABLE people (id INTEGER PRIMARY KEY, name TEXT, email TEXT, nick TEXT);",
    )
    .unwrap();
    for i in 0..50 {
        db.execute(
            "INSERT INTO people VALUES (?1, ?2, ?3, ?4)",
            rusqlite::params![i, format!("n{i}"), format!("e{i}@example.com"), "x"],
        )
        .unwrap();
    }
    drop(db);
    std::fs::write(
        dir.join("connections.toml"),
        format!(
            "version = 4\n\n[[connections]]\nid = \"{ID}\"\nname = \"People\"\n\
             driver = \"sqlite\"\ndatabase = '{}'\n",
            dir.join("people.db").display()
        ),
    )
    .unwrap();
    std::fs::write(
        dir.join("settings.toml"),
        "[safety]\nmask_names = \"*email*, *secret*, *token*, *phone*, *salary*\"\n\
         mask_types = \"inet, bytea\"\n",
    )
    .unwrap();
    let mut catalog = CatalogStore::open(&dir.join("catalog.db")).unwrap();
    let id = Uuid::parse_str(ID).unwrap();
    for i in 0..marks {
        let key = CatalogKey {
            database: String::new(),
            schema: "main".to_owned(),
            object: format!("table_{:04}", i / 10),
            column: format!("column_{:02}", i % 10),
        };
        catalog.set_sensitive(id, &key, Some(i % 7 != 0)).unwrap();
    }
}

async fn measure(marks: usize, runs: usize) {
    let dir = tempfile::tempdir().unwrap();
    profile(dir.path(), marks);
    let secrets: Arc<dyn SecretStore> = Arc::new(MemorySecrets::default());
    let seal = Seal::with(dir.path(), secrets);
    let at = Profile::at(dir.path());
    let store = at.connections().unwrap();
    let id = Uuid::parse_str(ID).unwrap();
    let catalog = CatalogStore::open(&dir.path().join("catalog.db")).unwrap();
    let sealed = SealedMarks::from_catalog([(id, catalog.sensitive_columns(id).unwrap())]);
    drop(catalog);
    seal.write_with_marks(
        &AgentPolicy::current(store.list(), at.settings().as_ref()),
        &sealed,
    )
    .unwrap();
    let agent = Agent::new(at).with_seal(seal);
    let args = QueryArgs {
        connection: "People".to_owned(),
        sql: "SELECT id, name, email, nick FROM people WHERE id < 20".to_owned(),
        max_rows: None,
        max_bytes: None,
        timeout_seconds: None,
        format: None,
    };
    for _ in 0..10 {
        agent.run_query(&args, "cost").await.unwrap();
    }
    let mut times = Vec::with_capacity(runs);
    for _ in 0..runs {
        let start = Instant::now();
        agent.run_query(&args, "cost").await.unwrap();
        times.push(start.elapsed().as_micros());
    }
    times.sort_unstable();
    let total: u128 = times.iter().sum();
    println!(
        "marks {marks:>6}: median {} us, p10 {} us, mean {} us over {runs} calls",
        times[runs / 2],
        times[runs / 10],
        total / runs as u128,
    );
}

#[tokio::test]
#[ignore = "a measurement, not a check"]
async fn cost_of_a_run_query_with_rules_and_marks() {
    for (marks, runs) in [(0, 300), (100, 300), (10_000, 100)] {
        measure(marks, runs).await;
    }
}
