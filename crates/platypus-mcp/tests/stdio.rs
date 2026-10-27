//! Drives the `platypus-mcp` binary over stdio as an MCP client would, against a profile
//! with SQLite databases.

use std::io::{BufRead, BufReader, Write};
use std::path::Path;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{Receiver, channel};
use std::time::Duration;

use serde_json::{Value, json};

struct Client {
    child: Child,
    stdin: ChildStdin,
    lines: Receiver<String>,
    next_id: u64,
}

impl Client {
    fn start(dir: &Path) -> Self {
        let mut child = Command::new(env!("CARGO_BIN_EXE_platypus-mcp"))
            .env("PLATYPUS_DATA_DIR", dir)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .unwrap();
        let stdin = child.stdin.take().unwrap();
        let stdout = child.stdout.take().unwrap();
        let (tx, lines) = channel();
        std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines() {
                let Ok(line) = line else { break };
                if tx.send(line).is_err() {
                    break;
                }
            }
        });
        Self {
            child,
            stdin,
            lines,
            next_id: 1,
        }
    }

    fn send(&mut self, message: &Value) {
        writeln!(self.stdin, "{message}").unwrap();
        self.stdin.flush().unwrap();
    }

    /// Sends a request and waits for its response, skipping notifications.
    fn request(&mut self, method: &str, params: &Value) -> Value {
        let id = self.next_id;
        self.next_id += 1;
        self.send(&json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params }));
        loop {
            let line = self
                .lines
                .recv_timeout(Duration::from_secs(60))
                .expect("no answer from platypus-mcp");
            let message: Value = serde_json::from_str(&line)
                .unwrap_or_else(|e| panic!("not JSON on stdout: {line}: {e}"));
            if message["id"] == json!(id) {
                return message;
            }
        }
    }

    /// Calls a tool; returns its text and whether it is an error.
    fn call(&mut self, tool: &str, arguments: &Value) -> (String, bool) {
        let response = self.request(
            "tools/call",
            &json!({ "name": tool, "arguments": arguments }),
        );
        let result = &response["result"];
        assert!(result.is_object(), "{tool}: {response}");
        let text = result["content"][0]["text"]
            .as_str()
            .unwrap_or_default()
            .to_owned();
        (text, result["isError"] == json!(true))
    }
}

impl Drop for Client {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn sqlite(path: &Path, sql: &str) {
    rusqlite::Connection::open(path)
        .unwrap()
        .execute_batch(sql)
        .unwrap();
}

fn count(path: &Path) -> i64 {
    rusqlite::Connection::open(path)
        .unwrap()
        .query_row("SELECT count(*) FROM t", [], |r| r.get(0))
        .unwrap()
}

/// A profile with a read-only, a read-write, a hidden and a remote connection.
fn profile(dir: &Path) {
    for name in ["app.db", "scratch.db", "hidden.db"] {
        sqlite(
            &dir.join(name),
            "CREATE TABLE t (id INTEGER PRIMARY KEY, name TEXT);
             INSERT INTO t VALUES (1, 'Ada'), (2, 'Bob');",
        );
    }
    let sqlite_connection = |id: u32, name: &str, file: &str, extra: &str| {
        format!(
            "[[connections]]\nid = \"00000000-0000-4000-8000-00000000000{id}\"\nname = \"{name}\"\n\
             driver = \"sqlite\"\ndatabase = '{}'\n{extra}\n",
            dir.join(file).display()
        )
    };
    let toml = format!(
        "version = 4\n\n{}{}{}{}",
        sqlite_connection(1, "App db", "app.db", ""),
        sqlite_connection(2, "Scratch", "scratch.db", "agent_access = \"read_write\""),
        sqlite_connection(3, "Hidden", "hidden.db", "agent_access = \"off\""),
        "[[connections]]\nid = \"00000000-0000-4000-8000-000000000004\"\nname = \"Remote PG\"\n\
         driver = \"postgres\"\nhost = \"db.secret-host.example\"\nport = 5432\ndatabase = \"sales\"\n\
         user = \"ada_admin\"\npassword_command = \"echo hunter2\"\nconnection_type = \"prod\"\n\
         agent_access = \"read_write\"\n\n[connections.ssh]\nhost = \"bastion.example\"\nuser = \"ops\"\n\
         auth = \"agent\"\nsecret_id = \"00000000-0000-4000-8000-000000000009\"\n",
    );
    std::fs::write(dir.join("connections.toml"), toml).unwrap();
}

/// Starts the server and introduces the client as `test-agent`.
fn initialized(dir: &Path) -> Client {
    let mut client = Client::start(dir);
    let init = client.request(
        "initialize",
        &json!({
            "protocolVersion": "2025-06-18",
            "capabilities": {},
            "clientInfo": { "name": "test-agent", "version": "1.0" },
        }),
    );
    assert_eq!(
        init["result"]["serverInfo"]["name"], "platypus-mcp",
        "{init}"
    );
    assert!(
        init["result"]["capabilities"]["tools"].is_object(),
        "{init}"
    );
    client.send(&json!({ "jsonrpc": "2.0", "method": "notifications/initialized" }));
    client
}

fn lists_tools_and_connections_without_secrets(client: &mut Client) {
    let tools = client.request("tools/list", &json!({}));
    let names: Vec<&str> = tools["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["name"].as_str().unwrap())
        .collect();
    for tool in [
        "list_connections",
        "list_schemas",
        "list_objects",
        "describe_table",
        "get_ddl",
        "search_objects",
        "run_query",
    ] {
        assert!(names.contains(&tool), "{tool} missing from {names:?}");
    }

    // The hidden connection is left out, and nothing secret shows.
    let (list, error) = client.call("list_connections", &json!({}));
    assert!(!error, "{list}");
    let connections: Value = serde_json::from_str(&list).unwrap();
    let listed: Vec<(&str, &str)> = connections
        .as_array()
        .unwrap()
        .iter()
        .map(|c| {
            (
                c["name"].as_str().unwrap(),
                c["agent_access"].as_str().unwrap(),
            )
        })
        .collect();
    assert_eq!(
        listed,
        [
            ("App db", "read_only"),
            ("Scratch", "read_write"),
            ("Remote PG", "read_only")
        ],
        "production is read-only whatever was chosen"
    );
    for secret in [
        "secret-host",
        "ada_admin",
        "hunter2",
        "bastion",
        "echo",
        "5432",
    ] {
        assert!(!list.contains(secret), "{secret} in {list}");
    }
}

fn runs_queries_within_the_access(client: &mut Client, dir: &Path) {
    let (rows, error) = client.call(
        "run_query",
        &json!({ "connection": "App db", "sql": "SELECT id, name FROM t ORDER BY id" }),
    );
    assert!(!error, "{rows}");
    assert!(
        rows.starts_with(
            "id | name
---+-----
1  | Ada
2  | Bob
(2 rows"
        ),
        "{rows}"
    );

    let (json_rows, _) = client.call(
        "run_query",
        &json!({ "connection": "App db", "sql": "SELECT name FROM t ORDER BY id", "format": "json", "max_rows": 1 }),
    );
    let doc: Value = serde_json::from_str(&json_rows).unwrap();
    assert_eq!(doc["rows"], json!([["Ada"]]), "{doc}");
    assert_eq!(doc["truncated_by"], "max_rows", "{doc}");

    // Read-only: a write is refused before it reaches the database.
    let (refused, error) = client.call(
        "run_query",
        &json!({ "connection": "App db", "sql": "DELETE FROM t WHERE id = 1" }),
    );
    assert!(
        error && refused.contains("read-only for agents"),
        "{refused}"
    );
    assert_eq!(count(&dir.join("app.db")), 2);

    // Read-write: changes run, but not one that hits every row.
    let (inserted, error) = client.call(
        "run_query",
        &json!({ "connection": "Scratch", "sql": "INSERT INTO t VALUES (3, 'Cy')" }),
    );
    assert!(!error && inserted == "1 row affected.", "{inserted}");
    let (every, error) = client.call(
        "run_query",
        &json!({ "connection": "Scratch", "sql": "DELETE FROM t" }),
    );
    assert!(error && every.contains("every row"), "{every}");
    assert_eq!(count(&dir.join("scratch.db")), 3);

    let (hidden, error) = client.call(
        "run_query",
        &json!({ "connection": "Hidden", "sql": "SELECT 1" }),
    );
    assert!(
        error && hidden.contains("no connection named `Hidden`"),
        "{hidden}"
    );
}

fn describes_the_schema(client: &mut Client) {
    let (schema, error) = client.call(
        "describe_table",
        &json!({ "connection": "App db", "schema": "main", "table": "t" }),
    );
    assert!(!error, "{schema}");
    let details: Value = serde_json::from_str(&schema).unwrap();
    assert_eq!(details["columns"][0]["name"], "id");
    assert_eq!(details["columns"][0]["primary_key"], true);
    let (ddl, error) = client.call(
        "get_ddl",
        &json!({ "connection": "App db", "schema": "main", "name": "t" }),
    );
    assert!(!error && ddl.contains("CREATE TABLE t"), "{ddl}");
    let (found, error) = client.call(
        "search_objects",
        &json!({ "connection": "App db", "text": "T" }),
    );
    assert!(!error && found.contains("\"name\":\"t\""), "{found}");
    // What does not exist is an error, not an empty answer.
    let (missing, error) = client.call(
        "describe_table",
        &json!({ "connection": "App db", "schema": "main", "table": "nope" }),
    );
    assert!(
        error && missing.contains("no table or view `main.nope`"),
        "{missing}"
    );
    let (missing, error) = client.call(
        "list_objects",
        &json!({ "connection": "App db", "schema": "nope" }),
    );
    // SQLite fails on its own; other databases list nothing, which is reported as missing.
    assert!(error, "{missing}");
}

#[test]
fn serves_the_tools_over_stdio() {
    let dir = tempfile::tempdir().unwrap();
    profile(dir.path());
    let mut client = initialized(dir.path());
    lists_tools_and_connections_without_secrets(&mut client);
    runs_queries_within_the_access(&mut client, dir.path());
    describes_the_schema(&mut client);

    // The queries are in the history, with the agent's name; the hidden connection's is
    // not, since agents cannot reach it.
    drop(client);
    let history = rusqlite::Connection::open(dir.path().join("history.db")).unwrap();
    let recorded: Vec<(String, String, String)> = history
        .prepare("SELECT sql, status, origin FROM history ORDER BY id")
        .unwrap()
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    assert_eq!(recorded.len(), 5, "{recorded:?}");
    assert!(recorded.iter().all(|(_, _, origin)| origin == "test-agent"));
    assert_eq!(recorded[2].0, "DELETE FROM t WHERE id = 1");
    assert_eq!(recorded[2].1, "error");
}
