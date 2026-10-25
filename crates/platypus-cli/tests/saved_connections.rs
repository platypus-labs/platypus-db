//! `platypus --connection`: the saved connections of a profile, with the agents' rules,
//! printed as JSON.

use std::path::Path;
use std::process::Command;

use serde_json::{Value, json};

/// Runs the CLI on the profile in `dir`; returns its JSON and whether it succeeded.
fn platypus(dir: &Path, args: &[&str]) -> (Value, bool) {
    let out = Command::new(env!("CARGO_BIN_EXE_platypus"))
        .env("PLATYPUS_DATA_DIR", dir)
        .args(args)
        .output()
        .unwrap();
    let stdout = String::from_utf8(out.stdout).unwrap();
    let doc = serde_json::from_str(&stdout).unwrap_or_else(|e| panic!("{e}: {stdout}"));
    (doc, out.status.success())
}

fn profile(dir: &Path) {
    rusqlite::Connection::open(dir.join("app.db"))
        .unwrap()
        .execute_batch(
            "CREATE TABLE t (id INTEGER PRIMARY KEY, name TEXT);
             INSERT INTO t VALUES (1, 'Ada'), (2, NULL);",
        )
        .unwrap();
    std::fs::write(
        dir.join("connections.toml"),
        format!(
            "version = 4\n\n[[connections]]\nid = \"00000000-0000-4000-8000-000000000001\"\n\
             name = \"App db\"\ndriver = \"sqlite\"\ndatabase = '{}'\nconnection_type = \"dev\"\n",
            dir.join("app.db").display()
        ),
    )
    .unwrap();
}

#[test]
fn works_on_saved_connections_with_json_output() {
    let dir = tempfile::tempdir().unwrap();
    profile(dir.path());

    let (list, ok) = platypus(dir.path(), &["connections"]);
    assert!(ok, "{list}");
    assert_eq!(
        list,
        json!([{
            "name": "App db",
            "id": "00000000-0000-4000-8000-000000000001",
            "driver": "sqlite",
            "type": "dev",
            "database": dir.path().join("app.db").display().to_string(),
            "agent_access": "read_only",
        }])
    );

    let (rows, ok) = platypus(
        dir.path(),
        &[
            "--connection",
            "App db",
            "query",
            "SELECT id, name FROM t ORDER BY id",
        ],
    );
    assert!(ok, "{rows}");
    assert_eq!(rows["rows"], json!([[1, "Ada"], [2, null]]));
    assert_eq!(rows["columns"][0]["name"], "id");
    assert_eq!(rows["truncated"], false);

    let (refused, ok) = platypus(
        dir.path(),
        &[
            "query",
            "--connection",
            "App db",
            "DELETE FROM t WHERE id = 1",
        ],
    );
    assert!(!ok);
    assert_eq!(refused["error"]["kind"], "denied", "{refused}");

    let (table, ok) = platypus(
        dir.path(),
        &["-c", "App db", "table", "--schema", "main", "--table", "t"],
    );
    assert!(ok, "{table}");
    assert_eq!(table["columns"][1]["name"], "name");

    let (missing, ok) = platypus(dir.path(), &["-c", "Nope", "schemas"]);
    assert!(!ok);
    assert_eq!(missing["error"]["kind"], "not_found", "{missing}");
}
