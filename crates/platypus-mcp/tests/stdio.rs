//! Drives the `platypus-mcp` binary over stdio as an MCP client would, against a profile
//! with SQLite databases.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpListener;
use std::path::Path;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{Receiver, channel};
use std::sync::{Arc, Mutex};
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

/// What the fake app got: each request's path and body.
type Received = Arc<Mutex<Vec<(String, String)>>>;

/// Plays the open app on the bridge of the profile at `dir`: answers `/v1/confirm` with
/// `verdicts` in turn, `/v1/sessions` with one session.
fn fake_app(dir: &Path, verdicts: Vec<&'static str>) -> Received {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    std::fs::write(
        dir.join("agent-bridge.json"),
        json!({ "port": port, "token": "t0ken" }).to_string(),
    )
    .unwrap();
    let received = Received::default();
    let (got, mut verdicts) = (Arc::clone(&received), verdicts.into_iter());
    std::thread::spawn(move || {
        for socket in listener.incoming() {
            let mut socket = socket.unwrap();
            let mut reader = BufReader::new(socket.try_clone().unwrap());
            let (mut head, mut line) = (String::new(), String::new());
            while reader.read_line(&mut line).unwrap() > 2 {
                head.push_str(&line);
                line.clear();
            }
            let length = head
                .lines()
                .find_map(|l| l.strip_prefix("Content-Length: "))
                .map_or(0, |n| n.trim().parse().unwrap());
            let mut body = vec![0; length];
            reader.read_exact(&mut body).unwrap();
            let path = head.split_whitespace().nth(1).unwrap().to_owned();
            let (status, answer) = if head.contains("Authorization: Bearer t0ken") {
                match path.as_str() {
                    "/v1/confirm" | "/v1/context" => (200, verdicts.next().unwrap().to_owned()),
                    "/v1/sessions" => (
                        200,
                        r#"[{"connection":"Scratch","database":null,"purpose":"main","manual_commit":true,"transaction_open":false}]"#
                            .to_owned(),
                    ),
                    _ => (200, "{}".to_owned()),
                }
            } else {
                (401, String::new())
            };
            got.lock()
                .unwrap()
                .push((path, String::from_utf8(body).unwrap()));
            let _ = write!(
                socket,
                "HTTP/1.1 {status} X\r\nContent-Length: {}\r\n\r\n{answer}",
                answer.len()
            );
        }
    });
    received
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
        "sample_rows",
        "explain",
        "propose_sql",
        "list_open_sessions",
        "get_user_context",
        "list_scripts",
        "read_script",
        "search_history",
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

    asks_the_app_before_changes(client, dir);

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

/// On a read-write connection a change waits for the user's yes in the open app; with the
/// app closed it is refused.
fn asks_the_app_before_changes(client: &mut Client, dir: &Path) {
    let insert = json!({ "connection": "Scratch", "sql": "INSERT INTO t VALUES (3, 'Cy')" });
    let (closed, error) = client.call("run_query", &insert);
    assert!(
        error && closed.contains("cannot ask: PlatypusDB is not open"),
        "{closed}"
    );
    let (sessions, error) = client.call("list_open_sessions", &json!({}));
    assert!(error && sessions.contains("not open"), "{sessions}");
    let app = fake_app(
        dir,
        vec![
            r#"{"answer":"deny","reason":"not now"}"#,
            r#"{"answer":"session"}"#,
        ],
    );
    let (denied, error) = client.call("run_query", &insert);
    assert!(
        error && denied.contains("the user refused this change: not now"),
        "{denied}"
    );
    assert_eq!(count(&dir.join("scratch.db")), 2);
    let (inserted, error) = client.call("run_query", &insert);
    assert!(!error && inserted == "1 row affected.", "{inserted}");
    // Allowed for the session: the next change on the connection runs without asking.
    let (again, error) = client.call(
        "run_query",
        &json!({ "connection": "Scratch", "sql": "UPDATE t SET name = 'Cyd' WHERE id = 3" }),
    );
    assert!(!error && again == "1 row affected.", "{again}");
    let asked: Vec<Value> = app
        .lock()
        .unwrap()
        .iter()
        .filter(|(path, _)| path == "/v1/confirm")
        .map(|(_, body)| serde_json::from_str(body).unwrap())
        .collect();
    assert_eq!(asked.len(), 2, "{asked:?}");
    assert_eq!(asked[0]["agent"], "test-agent");
    assert_eq!(asked[0]["connection"], "Scratch");
    assert_eq!(asked[0]["statement"], "INSERT INTO t VALUES (3, 'Cy')");
    assert!(
        asked[0]["plan"].is_string(),
        "SQLite explains it: {}",
        asked[0]
    );
    assert_eq!(asked[0]["session"], asked[1]["session"]);
    let (sessions, error) = client.call("list_open_sessions", &json!({}));
    assert!(!error, "{sessions}");
    let sessions: Value = serde_json::from_str(&sessions).unwrap();
    assert_eq!(sessions[0]["connection"], "Scratch");
    assert_eq!(sessions[0]["manual_commit"], true);
    // The server told the app it is connected.
    assert!(
        app.lock()
            .unwrap()
            .iter()
            .any(|(path, body)| path == "/v1/hello" && body.contains("test-agent"))
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
    assert_eq!(recorded.len(), 8, "{recorded:?}");
    assert!(recorded.iter().all(|(_, _, origin)| origin == "test-agent"));
    assert_eq!(recorded[2].0, "DELETE FROM t WHERE id = 1");
    assert_eq!(recorded[2].1, "error");

    // Every tool call is in the agents' call log, in one session, with its arguments.
    let log = rusqlite::Connection::open(dir.path().join("agent-calls.db")).unwrap();
    let calls: Vec<(String, String, Option<String>, String, String)> = log
        .prepare("SELECT session, tool, connection, arguments, status FROM calls ORDER BY id")
        .unwrap()
        .query_map([], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?))
        })
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    let tools: Vec<&str> = calls.iter().map(|c| c.1.as_str()).collect();
    assert_eq!(
        tools,
        [
            "list_connections",
            "run_query",
            "run_query",
            "run_query",
            "run_query",
            "list_open_sessions",
            "run_query",
            "run_query",
            "run_query",
            "list_open_sessions",
            "run_query",
            "run_query",
            "describe_table",
            "get_ddl",
            "search_objects",
            "describe_table",
            "list_objects",
        ]
    );
    assert!(calls.iter().all(|c| c.0 == calls[0].0), "one session");
    assert_eq!(calls[0].2, None);
    assert_eq!(calls[0].3, "{}");
    assert_eq!(calls[1].2.as_deref(), Some("App db"));
    assert_eq!(
        calls[1].3,
        r#"{"connection":"App db","sql":"SELECT id, name FROM t ORDER BY id"}"#
    );
    assert_eq!(calls[1].4, "ok");
    assert_eq!(calls[3].4, "error", "the refused DELETE");
    let agents: i64 = log
        .query_row(
            "SELECT count(*) FROM calls WHERE agent = 'test-agent'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(agents, 17);
}

/// `get_user_context` asks the app: closed it is an error, a refusal carries the user's
/// reason, a share is returned within the limits, and a tab on a connection closed to
/// agents is not returned at all.
#[test]
fn shares_what_the_user_works_on_only_with_consent() {
    let dir = tempfile::tempdir().unwrap();
    profile(dir.path());
    let mut client = initialized(dir.path());
    let (closed, error) = client.call("get_user_context", &json!({}));
    assert!(error && closed.contains("not open"), "{closed}");

    // As the page sends it: the first 100 rows of a result of 150.
    let rows: Vec<String> = (0..100).map(|i| format!("[\"{i}\",null]")).collect();
    let shared = Box::leak(
        format!(
            r#"{{"answer":"once","context":{{"connection":"App db","connection_id":"00000000-0000-4000-8000-000000000001","tab":"Orders","editor":"SELECT 1","selection":"1","result":{{"columns":["n","m"],"rows":[{}],"truncated":true,"total_rows":150}}}}}}"#,
            rows.join(",")
        )
        .into_boxed_str(),
    );
    let app = fake_app(
        dir.path(),
        vec![
            r#"{"answer":"deny","reason":"private"}"#,
            shared,
            r#"{"answer":"session","context":{"connection":"Hidden","connection_id":"00000000-0000-4000-8000-000000000003","editor":"SELECT secret"}}"#,
        ],
    );
    let (denied, error) = client.call("get_user_context", &json!({}));
    assert!(error && denied.contains("private"), "{denied}");

    let (text, error) = client.call(
        "get_user_context",
        &json!({ "parts": ["editor", "result"] }),
    );
    assert!(!error, "{text}");
    let context: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(context["tab"], "Orders");
    assert_eq!(context["editor"], "SELECT 1");
    assert_eq!(context["selection"], Value::Null, "not asked for");
    assert_eq!(context["result"]["rows"].as_array().unwrap().len(), 100);
    assert_eq!(context["result"]["truncated"], true);
    assert_eq!(context["result"]["rows"][1], json!(["1", null]));
    assert!(
        context["notes"][0]
            .as_str()
            .unwrap()
            .contains("cut to 100 of its 150 rows"),
        "{text}"
    );

    let (hidden, error) = client.call("get_user_context", &json!({}));
    assert!(error && hidden.contains("not open to agents"), "{hidden}");
    assert!(!hidden.contains("secret"), "{hidden}");

    let asked: Vec<Value> = app
        .lock()
        .unwrap()
        .iter()
        .filter(|(path, _)| path == "/v1/context")
        .map(|(_, body)| serde_json::from_str(body).unwrap())
        .collect();
    assert_eq!(asked.len(), 3, "{asked:?}");
    assert_eq!(asked[0]["agent"], "test-agent");
    assert_eq!(asked[0]["parts"], json!(["editor", "selection", "result"]));
    assert_eq!(asked[1]["parts"], json!(["editor", "result"]));
    assert_eq!(asked[0]["session"], asked[1]["session"]);
}

/// `sample_rows` reads the first rows of a table within the limits; `explain` shows the
/// plan of a statement allowed by the access, running it only with analyze.
#[test]
fn samples_rows_and_explains_statements() {
    let dir = tempfile::tempdir().unwrap();
    profile(dir.path());
    let mut client = initialized(dir.path());

    let (rows, error) = client.call(
        "sample_rows",
        &json!({ "connection": "App db", "schema": "main", "table": "t" }),
    );
    assert!(!error && rows.starts_with("id | name\n"), "{rows}");
    assert!(rows.contains("(2 rows"), "{rows}");
    let (one, error) = client.call(
        "sample_rows",
        &json!({ "connection": "App db", "schema": "main", "table": "t", "max_rows": 1, "format": "json" }),
    );
    assert!(!error, "{one}");
    let doc: Value = serde_json::from_str(&one).unwrap();
    assert_eq!(doc["row_count"], 1, "{doc}");
    assert_eq!(doc["truncated_by"], "max_rows", "{doc}");
    // A name that needs quotes is quoted, not run as SQL.
    let (missing, error) = client.call(
        "sample_rows",
        &json!({ "connection": "App db", "schema": "main", "table": "t; DROP TABLE t" }),
    );
    assert!(error && missing.contains("no such table"), "{missing}");
    assert_eq!(count(&dir.path().join("app.db")), 2);
    let (empty, error) = client.call(
        "sample_rows",
        &json!({ "connection": "App db", "schema": "main", "table": " " }),
    );
    assert!(error && empty.contains("needed"), "{empty}");
    let (zero, error) = client.call(
        "sample_rows",
        &json!({ "connection": "App db", "schema": "main", "table": "t", "max_rows": 0 }),
    );
    assert!(error && zero.contains("max_rows"), "{zero}");

    let (plan, error) = client.call(
        "explain",
        &json!({ "connection": "App db", "sql": "SELECT name FROM t WHERE id = 1" }),
    );
    assert!(!error && plan.starts_with("-> "), "{plan}");
    assert!(plan.contains("Estimated plan"), "{plan}");
    let (json_plan, error) = client.call(
        "explain",
        &json!({ "connection": "App db", "sql": "SELECT * FROM t", "format": "json" }),
    );
    assert!(!error, "{json_plan}");
    let doc: Value = serde_json::from_str(&json_plan).unwrap();
    assert!(doc["root"]["operation"].is_string(), "{doc}");
    assert_eq!(doc["analyzed"], false);
    // SQLite's analyze runs in the read-only transaction.
    let (analyzed, error) = client.call(
        "explain",
        &json!({ "connection": "App db", "sql": "SELECT * FROM t", "analyze": true }),
    );
    assert!(!error, "{analyzed}");
    // The access applies as in run_query.
    let (refused, error) = client.call(
        "explain",
        &json!({ "connection": "App db", "sql": "DELETE FROM t WHERE id = 1" }),
    );
    assert!(
        error && refused.contains("read-only for agents"),
        "{refused}"
    );
    let (two, error) = client.call(
        "explain",
        &json!({ "connection": "App db", "sql": "SELECT 1; SELECT 2" }),
    );
    assert!(error && two.contains("one statement at a time"), "{two}");
    // Without analyze nothing runs, so the Safety rule on every row does not apply.
    let (every, error) = client.call(
        "explain",
        &json!({ "connection": "Scratch", "sql": "DELETE FROM t" }),
    );
    assert!(!error, "{every}");
    let (every, error) = client.call(
        "explain",
        &json!({ "connection": "Scratch", "sql": "DELETE FROM t", "analyze": true }),
    );
    assert!(error && every.contains("every row"), "{every}");
    let (small, error) = client.call(
        "explain",
        &json!({ "connection": "App db", "sql": "SELECT * FROM t", "max_bytes": 1 }),
    );
    assert!(
        !error && small.contains("left out") && !small.contains("->"),
        "{small}"
    );
    assert_eq!(count(&dir.path().join("scratch.db")), 2);
}

/// `propose_sql` keeps the SQL for the app to open, whatever the access, and runs nothing.
#[test]
fn proposes_sql_for_the_app_to_open() {
    let dir = tempfile::tempdir().unwrap();
    profile(dir.path());
    let mut client = initialized(dir.path());
    let (done, error) = client.call(
        "propose_sql",
        &json!({ "connection": "App db", "sql": " DELETE FROM t WHERE id = 1 ", "title": "Remove Ada\n" }),
    );
    assert!(!error && done.contains("Nothing ran"), "{done}");
    assert_eq!(count(&dir.path().join("app.db")), 2);
    let (empty, error) = client.call(
        "propose_sql",
        &json!({ "connection": "App db", "sql": "  " }),
    );
    assert!(error && empty.contains("no SQL"), "{empty}");
    let (hidden, error) = client.call(
        "propose_sql",
        &json!({ "connection": "Hidden", "sql": "SELECT 1" }),
    );
    assert!(error && hidden.contains("no connection named"), "{hidden}");
    drop(client);

    let log = rusqlite::Connection::open(dir.path().join("agent-calls.db")).unwrap();
    let proposals: Vec<(String, String, Option<String>, String)> = log
        .prepare("SELECT agent, connection_id, title, sql FROM proposals")
        .unwrap()
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    assert_eq!(
        proposals,
        [(
            "test-agent".to_owned(),
            "00000000-0000-4000-8000-000000000001".to_owned(),
            Some("Remove Ada".to_owned()),
            "DELETE FROM t WHERE id = 1".to_owned()
        )]
    );
}

/// `notifications/cancelled` cancels the running statement, which frees the connection.
#[test]
fn a_cancelled_request_cancels_its_statement() {
    let dir = tempfile::tempdir().unwrap();
    profile(dir.path());
    let mut client = initialized(dir.path());
    let id = client.next_id;
    client.next_id += 1;
    client.send(&json!({
        "jsonrpc": "2.0", "id": id, "method": "tools/call",
        "params": { "name": "run_query", "arguments": {
            "connection": "App db",
            "sql": "WITH RECURSIVE c(x) AS (SELECT 1 UNION ALL SELECT x + 1 FROM c) SELECT count(*) FROM c",
            "timeout_seconds": 60,
        }},
    }));
    std::thread::sleep(Duration::from_secs(1));
    client.send(&json!({
        "jsonrpc": "2.0", "method": "notifications/cancelled",
        "params": { "requestId": id, "reason": "interrupted" },
    }));
    let start = std::time::Instant::now();
    let (rows, error) = client.call(
        "run_query",
        &json!({ "connection": "App db", "sql": "SELECT 2 AS two" }),
    );
    assert!(!error && rows.contains('2'), "{rows}");
    assert!(
        start.elapsed() < Duration::from_secs(20),
        "the cancelled statement held the connection for {:?}",
        start.elapsed()
    );
}

/// A line that is not JSON is skipped without an answer, as the official MCP SDKs do (an
/// answer to garbage can start a storm of errors with a peer that echoes it,
/// modelcontextprotocol/rust-sdk#938), and the server goes on.
#[test]
fn a_line_that_is_not_json_is_skipped() {
    let dir = tempfile::tempdir().unwrap();
    profile(dir.path());
    let mut client = initialized(dir.path());
    writeln!(client.stdin, "{{this is not json").unwrap();
    client.stdin.flush().unwrap();
    client.send(&json!({ "jsonrpc": "2.0", "id": 900, "method": "ping" }));
    let line = client
        .lines
        .recv_timeout(Duration::from_secs(10))
        .expect("no answer to the ping after a line that is not JSON");
    let answer: Value = serde_json::from_str(&line).unwrap();
    assert_eq!(
        answer["id"], 900,
        "the first line out answers the ping: {answer}"
    );
    assert!(answer["result"].is_object(), "{answer}");
}

/// An argument the server does not take fails with the usage, instead of serving.
#[test]
fn an_unknown_argument_prints_the_usage_and_fails() {
    let dir = tempfile::tempdir().unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_platypus-mcp"))
        .arg("--bogus")
        .env("PLATYPUS_DATA_DIR", dir.path())
        .stdin(Stdio::piped())
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("--bogus") && stderr.contains("Usage"),
        "{stderr}"
    );
    assert_eq!(output.stdout, b"");
}

/// Connections, schemas and tables are MCP resources: listed without opening a database,
/// read as the JSON the tools return, and logged like a tool call.
#[test]
fn serves_schemas_and_tables_as_resources() {
    let dir = tempfile::tempdir().unwrap();
    profile(dir.path());
    sqlite(
        &dir.path().join("app.db"),
        r#"CREATE TABLE "my table/x" (k INTEGER PRIMARY KEY, "odd col" TEXT);"#,
    );
    let mut client = initialized(dir.path());

    let list = client.request("resources/list", &json!({}));
    let resources = list["result"]["resources"].as_array().unwrap();
    let listed: Vec<(&str, &str)> = resources
        .iter()
        .map(|r| (r["name"].as_str().unwrap(), r["uri"].as_str().unwrap()))
        .collect();
    assert_eq!(
        listed,
        [
            ("App db", "platypus://App%20db"),
            ("Scratch", "platypus://Scratch"),
            ("Remote PG", "platypus://Remote%20PG"),
        ],
        "the hidden connection is left out: {list}"
    );
    assert_eq!(resources[0]["mimeType"], "application/json");
    assert!(
        resources[0]["description"]
            .as_str()
            .unwrap()
            .contains("sqlite"),
        "{list}"
    );

    let templates = client.request("resources/templates/list", &json!({}));
    let templates: Vec<&str> = templates["result"]["resourceTemplates"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["uriTemplate"].as_str().unwrap())
        .collect();
    assert_eq!(
        templates,
        [
            "platypus://{connection}/{schema}",
            "platypus://{connection}/{schema}/{table}"
        ]
    );

    let mut read = |uri: &str| {
        let response = client.request("resources/read", &json!({ "uri": uri }));
        match response["result"]["contents"][0].clone() {
            Value::Null => Err(response["error"]["message"].as_str().unwrap().to_owned()),
            content => {
                assert_eq!(content["mimeType"], "application/json", "{response}");
                assert_eq!(content["uri"], uri, "{response}");
                Ok(content["text"].as_str().unwrap().to_owned())
            }
        }
    };
    let schemas = read("platypus://App%20db").unwrap();
    assert!(schemas.contains(r#""name":"main""#), "{schemas}");
    let objects = read("platypus://App%20db/main").unwrap();
    assert!(objects.contains(r#""name":"t""#), "{objects}");
    assert!(objects.contains("my table/x"), "{objects}");
    let table: Value = serde_json::from_str(&read("platypus://App%20db/main/t").unwrap()).unwrap();
    assert_eq!(table["columns"][0]["name"], "id");
    assert_eq!(table["columns"][1]["name"], "name");
    // A name that needs escapes round-trips.
    let odd: Value =
        serde_json::from_str(&read("platypus://App%20db/main/my%20table%2Fx").unwrap()).unwrap();
    assert_eq!(odd["columns"][1]["name"], "odd col");

    // A bad uri names the forms; an agent error is passed on as its text.
    for bad in [
        "file:///etc/passwd",
        "platypus://App%20db/main/t/extra",
        "platypus://",
    ] {
        let error = read(bad).unwrap_err();
        assert!(error.contains("platypus://<connection>"), "{bad}: {error}");
    }
    let hidden = read("platypus://Hidden").unwrap_err();
    assert!(hidden.contains("Hidden"), "{hidden}");
    let missing = read("platypus://App%20db/main/nope").unwrap_err();
    assert!(
        missing.contains("no table or view `main.nope`"),
        "{missing}"
    );

    drop(client);
    resource_reads_are_logged(dir.path());
}

/// Each read of a resource is in the call log, as `read_resource` with its connection.
fn resource_reads_are_logged(dir: &Path) {
    let log = rusqlite::Connection::open(dir.join("agent-calls.db")).unwrap();
    let calls: Vec<(String, Option<String>, String, String)> = log
        .prepare("SELECT tool, connection, arguments, status FROM calls ORDER BY id")
        .unwrap()
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    // Unparsable uris are refused before they count as a call.
    assert_eq!(calls.len(), 6, "{calls:?}");
    assert!(calls.iter().all(|c| c.0 == "read_resource"));
    assert_eq!(calls[0].1.as_deref(), Some("App db"));
    assert_eq!(
        calls[0].2,
        r#"{"uri":"platypus://App%20db","connection":"App db"}"#
    );
    assert_eq!(calls[0].3, "ok");
    assert_eq!(calls[4].1.as_deref(), Some("Hidden"));
    assert_eq!(calls[4].3, "error");
    assert_eq!(calls[5].3, "error");
}

/// Connections that share a name get their id in the resource uri; one name stays a name.
#[test]
fn resources_of_connections_with_the_same_name_use_the_id() {
    use std::fmt::Write as _;
    let dir = tempfile::tempdir().unwrap();
    profile(dir.path());
    sqlite(
        &dir.path().join("dup.db"),
        "CREATE TABLE only_in_dup (id INTEGER PRIMARY KEY);",
    );
    let toml = dir.path().join("connections.toml");
    let mut text = std::fs::read_to_string(&toml).unwrap();
    write!(
        text,
        "\n[[connections]]\nid = \"00000000-0000-4000-8000-000000000005\"\nname = \"Scratch\"\n\
         driver = \"sqlite\"\ndatabase = '{}'\n",
        dir.path().join("dup.db").display()
    )
    .unwrap();
    std::fs::write(&toml, text).unwrap();
    let mut client = initialized(dir.path());

    let list = client.request("resources/list", &json!({}));
    let uris: Vec<&str> = list["result"]["resources"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["uri"].as_str().unwrap())
        .collect();
    assert_eq!(
        uris,
        [
            "platypus://App%20db",
            "platypus://00000000-0000-4000-8000-000000000002",
            "platypus://Remote%20PG",
            "platypus://00000000-0000-4000-8000-000000000005",
        ],
        "{list}"
    );
    let mut read = |uri: &str| {
        let response = client.request("resources/read", &json!({ "uri": uri }));
        response["result"]["contents"][0]["text"]
            .as_str()
            .map(str::to_owned)
            .ok_or_else(|| response["error"]["message"].as_str().unwrap().to_owned())
    };
    let first = read(uris[1]).unwrap();
    let second = read(uris[3]).unwrap();
    assert!(first.contains("main") && second.contains("main"));
    let tables = read(&format!("{}/main", uris[3])).unwrap();
    assert!(tables.contains("only_in_dup"), "{tables}");
    let tables = read(&format!("{}/main", uris[1])).unwrap();
    assert!(!tables.contains("only_in_dup"), "{tables}");
    let by_name = read("platypus://Scratch").unwrap_err();
    assert!(by_name.contains("several"), "{by_name}");
}

/// A linked script is as hidden under any alias of its file as under its own path.
#[cfg(unix)]
#[test]
fn a_symlink_to_a_hidden_script_does_not_read() {
    let dir = tempfile::tempdir().unwrap();
    profile(dir.path());
    scripts(dir.path());
    let all = dir.path().join("scripts/all");
    let link =
        |target: &str, name: &str| std::os::unix::fs::symlink(target, all.join(name)).unwrap();
    link("hidden.sql", "alias.sql");
    link("private", "dirlink");
    link(".", "self");
    link("top.sql", "visible-alias.sql");
    let mut client = initialized(dir.path());
    for path in [
        "alias.sql",
        "dirlink/secret.sql",
        "self/hidden.sql",
        "self/self/hidden.sql",
    ] {
        let (refused, error) = client.call("read_script", &json!({ "path": path }));
        assert!(error, "{path}: {refused}");
        assert!(
            !refused.contains("SELECT 'hidden'") && !refused.contains("SELECT 'secret'"),
            "{path}"
        );
    }
    // The alias of a script agents may read still reads, with its connection.
    let (read, error) = client.call("read_script", &json!({ "path": "visible-alias.sql" }));
    assert!(!error, "{read}");
    let read: Value = serde_json::from_str(&read).unwrap();
    assert_eq!(read["text"], "SELECT * FROM t");
    assert_eq!(read["connection"], "App db");
}

/// The statements are redacted like the errors are.
#[test]
fn history_sql_is_redacted() {
    use platypus_store::{HistoryStatus, HistoryStore, NewHistoryEntry};
    let dir = tempfile::tempdir().unwrap();
    profile(dir.path());
    let history = HistoryStore::open(&dir.path().join("history.db")).unwrap();
    for sql in ["SELECT 'echo hunter2'", "SELECT 2"] {
        history
            .record(&NewHistoryEntry {
                connection_id: uuid::Uuid::parse_str("00000000-0000-4000-8000-000000000004")
                    .unwrap(),
                sql: sql.to_owned(),
                started_at: std::time::UNIX_EPOCH + Duration::from_secs(1_700_000_000),
                duration: Duration::from_millis(1),
                rows: Some(1),
                status: HistoryStatus::Ok,
                error: None,
                origin: None,
            })
            .unwrap();
    }
    drop(history);
    let mut client = initialized(dir.path());
    let (text, error) = client.call("search_history", &json!({}));
    assert!(!error, "{text}");
    assert!(!text.contains("echo hunter2"), "{text}");
    assert!(
        text.contains("SELECT '***'") && text.contains("SELECT 2"),
        "{text}"
    );
}

const SALES: &str = "00000000-0000-4000-8000-0000000000a1";

/// The saved scripts of the profile: All connections has scripts on every kind of
/// connection, and the project Sales one on the production connection.
fn scripts(dir: &Path) {
    use std::fmt::Write as _;
    let toml = dir.join("connections.toml");
    let mut text = std::fs::read_to_string(&toml).unwrap();
    write!(
        text,
        "\n[[projects]]\nid = \"{SALES}\"\nname = \"Sales\"\ncolor = \"#d08f20\"\n"
    )
    .unwrap();
    std::fs::write(&toml, text).unwrap();
    let id = |n: u32| format!("00000000-0000-4000-8000-00000000000{n}");
    let all = dir.join("scripts/all");
    for (rel, text) in [
        ("top.sql", "SELECT * FROM t"),
        ("plain.sql", "SELECT 1"),
        ("hidden.sql", "SELECT 'hidden'"),
        ("gone.sql", "SELECT 'deleted connection'"),
        ("reports/monthly.sql", "SELECT count(*) FROM t"),
        ("private/secret.sql", "SELECT 'secret'"),
        ("notes.txt", "not a script"),
        (".draft.sql", "SELECT 'draft'"),
    ] {
        let path = all.join(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }
    // Past the cap of 256 KiB, with a two-byte letter on the boundary.
    std::fs::write(all.join("big.sql"), "é".repeat(140_000)).unwrap();
    let links = json!({
        "top.sql": id(1),
        "hidden.sql": id(3),
        "gone.sql": "00000000-0000-4000-8000-0000000000ff",
        "reports/monthly.sql": id(2),
        "private/secret.sql": id(3),
    });
    std::fs::write(all.join(".links.json"), links.to_string()).unwrap();
    let sales = dir.join("scripts").join(SALES);
    std::fs::create_dir_all(&sales).unwrap();
    std::fs::write(sales.join("pipeline.sql"), "SELECT 'pipeline'").unwrap();
    std::fs::write(
        sales.join(".links.json"),
        json!({ "pipeline.sql": id(4) }).to_string(),
    )
    .unwrap();
}

fn entries(list: &str) -> Vec<(String, bool, Option<String>)> {
    let list: Value = serde_json::from_str(list).unwrap();
    list.as_array()
        .unwrap()
        .iter()
        .map(|e| {
            (
                e["path"].as_str().unwrap().to_owned(),
                e["folder"].as_bool().unwrap(),
                e["connection"].as_str().map(str::to_owned),
            )
        })
        .collect()
}

fn lists_and_reads_saved_scripts(client: &mut Client) {
    let some = |name: &str| Some(name.to_owned());
    let (list, error) = client.call("list_scripts", &json!({}));
    assert!(!error, "{list}");
    // Scripts of an access-off or deleted connection are gone, with the folder they emptied.
    assert_eq!(
        entries(&list),
        [
            ("reports".to_owned(), true, None),
            ("reports/monthly.sql".to_owned(), false, some("Scratch")),
            ("big.sql".to_owned(), false, None),
            ("plain.sql".to_owned(), false, None),
            ("top.sql".to_owned(), false, some("App db")),
        ]
    );
    for hidden in ["hidden", "gone", "private", "secret", "notes", "draft"] {
        assert!(!list.contains(hidden), "{hidden} in {list}");
    }

    let (list, error) = client.call("list_scripts", &json!({ "project": "sales" }));
    assert!(!error, "{list}");
    assert_eq!(
        entries(&list),
        [("pipeline.sql".to_owned(), false, some("Remote PG"))]
    );
    let (unknown, error) = client.call("list_scripts", &json!({ "project": "Nope" }));
    assert!(error && unknown.contains("`Sales`"), "{unknown}");

    let (read, error) = client.call("read_script", &json!({ "path": "reports/monthly.sql" }));
    assert!(!error, "{read}");
    let read: Value = serde_json::from_str(&read).unwrap();
    assert_eq!(read["text"], "SELECT count(*) FROM t");
    assert_eq!(read["connection"], "Scratch");
    assert_eq!(read["truncated"], false);
    let (read, _) = client.call(
        "read_script",
        &json!({ "path": "pipeline.sql", "project": "Sales" }),
    );
    assert!(read.contains("SELECT 'pipeline'"), "{read}");

    let (big, error) = client.call("read_script", &json!({ "path": "big.sql" }));
    assert!(!error);
    let big: Value = serde_json::from_str(&big).unwrap();
    assert_eq!(big["truncated"], true);
    assert_eq!(big["text"].as_str().unwrap().len(), 262_144);
    assert!(big["note"].as_str().unwrap().contains("cut"), "{big}");

    // Refused: the hidden and the deleted connections' scripts, paths that leave the
    // folder, the links file, a file that is no script and one that is not there.
    for path in [
        "hidden.sql",
        "gone.sql",
        "private/secret.sql",
        "../all/top.sql",
        "reports/../top.sql",
        "/etc/passwd",
        ".links.json",
        ".draft.sql",
        "notes.txt",
        "missing.sql",
    ] {
        let (refused, error) = client.call("read_script", &json!({ "path": path }));
        assert!(error, "{path}: {refused}");
        assert!(!refused.contains("hidden'"), "{path}: {refused}");
    }
    let (other, error) = client.call(
        "read_script",
        &json!({ "path": "top.sql", "project": "Sales" }),
    );
    assert!(
        error,
        "a script of All connections is not in Sales: {other}"
    );
}

fn searches_the_history(dir: &Path, client: &mut Client) {
    use platypus_store::{HistoryStatus, HistoryStore, NewHistoryEntry};
    let history = HistoryStore::open(&dir.join("history.db")).unwrap();
    let id =
        |n: u32| uuid::Uuid::parse_str(&format!("00000000-0000-4000-8000-00000000000{n}")).unwrap();
    let mut when = std::time::UNIX_EPOCH + Duration::from_secs(1_700_000_000);
    let mut add = |connection: u32, sql: &str, origin: Option<&str>, error: Option<&str>| {
        when += Duration::from_secs(60);
        history
            .record(&NewHistoryEntry {
                connection_id: id(connection),
                sql: sql.to_owned(),
                started_at: when,
                duration: Duration::from_millis(12),
                rows: error.is_none().then_some(2),
                status: if error.is_none() {
                    HistoryStatus::Ok
                } else {
                    HistoryStatus::Error
                },
                error: error.map(str::to_owned),
                origin: origin.map(str::to_owned),
            })
            .unwrap();
    };
    add(1, "SELECT * FROM t", None, None);
    add(3, "SELECT 'from the hidden one'", None, None);
    add(1, "SELECT name FROM t", Some("claude-code"), None);
    add(2, "UPDATE t SET name = 'x'", None, None);
    add(
        4,
        "SELECT 1 FROM sales",
        None,
        Some("cannot open postgres://ada:s3cr@t@db/x"),
    );
    drop(history);

    let search = |client: &mut Client, args: Value| -> Vec<Value> {
        let (text, error) = client.call("search_history", &args);
        assert!(!error, "{text}");
        serde_json::from_str(&text).unwrap()
    };
    let sqls = |list: &[Value]| -> Vec<String> {
        list.iter()
            .map(|e| e["sql"].as_str().unwrap().to_owned())
            .collect()
    };

    // Newest first, and nothing of the connection that is off.
    let all = search(client, json!({}));
    assert_eq!(
        sqls(&all),
        [
            "SELECT 1 FROM sales",
            "UPDATE t SET name = 'x'",
            "SELECT name FROM t",
            "SELECT * FROM t"
        ]
    );
    let first = &all[0];
    assert_eq!(first["connection"], "Remote PG");
    assert_eq!(first["status"], "error");
    assert_eq!(first["origin"], "app");
    assert_eq!(first["duration_ms"], 12);
    assert_eq!(first["started_at"], "2023-11-14T22:18:20.000Z");
    assert_eq!(
        first["error"], "cannot open postgres://ada:***@db/x",
        "redacted"
    );
    assert_eq!(all[2]["origin"], "claude-code");
    assert_eq!(all[2]["rows"], 2);

    let app_db = search(client, json!({ "connection": "App db" }));
    assert_eq!(sqls(&app_db), ["SELECT name FROM t", "SELECT * FROM t"]);
    let by_agent = search(client, json!({ "origin": "agent" }));
    assert_eq!(sqls(&by_agent), ["SELECT name FROM t"]);
    let by_app = search(client, json!({ "origin": "app", "connection": "App db" }));
    assert_eq!(sqls(&by_app), ["SELECT * FROM t"]);
    let text = search(client, json!({ "text": "UPDATE" }));
    assert_eq!(sqls(&text), ["UPDATE t SET name = 'x'"]);
    assert_eq!(search(client, json!({ "limit": 2 })).len(), 2);

    let (refused, error) = client.call("search_history", &json!({ "connection": "Hidden" }));
    assert!(error, "{refused}");
    assert!(!refused.contains("from the hidden one"));
}

#[test]
fn reads_saved_scripts_and_the_history() {
    let dir = tempfile::tempdir().unwrap();
    profile(dir.path());
    scripts(dir.path());
    let mut client = initialized(dir.path());
    lists_and_reads_saved_scripts(&mut client);
    searches_the_history(dir.path(), &mut client);
    drop(client);

    // Each call is in the call log with its arguments and outcome.
    let log = rusqlite::Connection::open(dir.path().join("agent-calls.db")).unwrap();
    let calls: Vec<(String, Option<String>, String, String)> = log
        .prepare("SELECT tool, connection, arguments, status FROM calls ORDER BY id")
        .unwrap()
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    let count = |tool: &str| calls.iter().filter(|c| c.0 == tool).count();
    assert_eq!(count("list_scripts"), 3);
    assert_eq!(count("read_script"), 14);
    assert_eq!(count("search_history"), 7);
    assert_eq!(
        (calls[0].0.as_str(), calls[0].2.as_str()),
        ("list_scripts", "{}")
    );
    assert_eq!(calls[2].3, "error", "unknown project");
    assert_eq!(calls[3].2, r#"{"path":"reports/monthly.sql"}"#);
    let last = calls.last().unwrap();
    assert_eq!(last.1.as_deref(), Some("Hidden"));
    assert_eq!(last.3, "error");
}
