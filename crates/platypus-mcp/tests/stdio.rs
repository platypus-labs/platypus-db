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
        "get_er_diagram",
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
/// A profile with a table of `users`, and `extra` in its connection.
fn users_profile(dir: &Path, extra: &str) {
    sqlite(
        &dir.join("users.db"),
        "CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT, email TEXT, nick TEXT);
         INSERT INTO users VALUES (1, 'Ann', 'ann@example.com', 'annie'), (2, 'Bob', NULL, 'bo');",
    );
    std::fs::write(
        dir.join("connections.toml"),
        format!(
            "version = 4\n\n[[connections]]\nid = \"00000000-0000-4000-8000-0000000000a1\"\n\
             name = \"Users\"\ndriver = \"sqlite\"\ndatabase = '{}'\n{extra}",
            dir.join("users.db").display()
        ),
    )
    .unwrap();
}

#[test]
fn sensitive_columns_read_masked_in_results() {
    let dir = tempfile::tempdir().unwrap();
    users_profile(dir.path(), "");
    let mut client = initialized(dir.path());
    let run = |client: &mut Client, sql: &str| -> String {
        let (text, error) = client.call("run_query", &json!({ "connection": "Users", "sql": sql }));
        assert!(!error, "{text}");
        text
    };

    let (rows, error) = client.call(
        "sample_rows",
        &json!({ "connection": "Users", "schema": "main", "table": "users" }),
    );
    assert!(!error, "{rows}");
    assert!(!rows.contains("ann@example.com"), "{rows}");
    assert!(rows.contains("••••••") && rows.contains("Ann"), "{rows}");
    assert!(
        rows.contains("Sensitive columns are masked: email."),
        "{rows}"
    );

    for sql in [
        "SELECT email FROM users",
        "SELECT email AS contact FROM users",
        "SELECT upper(email) FROM users",
        "SELECT id, 'x' || email FROM users",
        "SELECT *, email AS e FROM users",
        "SELECT u.email FROM users u JOIN users v ON u.id = v.id",
    ] {
        let text = run(&mut client, sql);
        assert!(
            !text.to_lowercase().contains("ann@example.com"),
            "{sql}: {text}"
        );
        assert!(text.contains("••••••"), "{sql}: {text}");
    }

    // NULL stays NULL, and what is not sensitive stays as it is.
    let (json_rows, error) = client.call(
        "run_query",
        &json!({ "connection": "Users", "sql": "SELECT id, email, nick FROM users ORDER BY id", "format": "json" }),
    );
    assert!(!error, "{json_rows}");
    let doc: Value = serde_json::from_str(&json_rows).unwrap();
    assert_eq!(doc["rows"][0], json!([1, "••••••", "annie"]), "{doc}");
    assert_eq!(doc["rows"][1], json!([2, null, "bo"]), "{doc}");
    assert_eq!(doc["masked_columns"], json!(["email"]), "{doc}");
    let plain = run(&mut client, "SELECT id, nick FROM users");
    assert!(
        plain.contains("annie") && !plain.contains("masked"),
        "{plain}"
    );

    // An error that quotes a value of the column.
    let (failed, error) = client.call(
        "run_query",
        &json!({ "connection": "Users", "sql": "SELECT no_such FROM users WHERE email = 'ann@example.com'" }),
    );
    assert!(error && !failed.contains("ann@example.com"), "{failed}");
}

#[test]
fn the_rules_and_marks_of_the_app_decide_what_is_sensitive() {
    let dir = tempfile::tempdir().unwrap();
    users_profile(dir.path(), "");
    std::fs::write(
        dir.path().join("settings.toml"),
        "[safety]\nmask_names = \"nick\"\n",
    )
    .unwrap();
    let id = uuid::Uuid::parse_str("00000000-0000-4000-8000-0000000000a1").unwrap();
    let key = |column: &str| platypus_store::CatalogKey {
        database: String::new(),
        schema: "main".to_owned(),
        object: "users".to_owned(),
        column: column.to_owned(),
    };
    let mut catalog = platypus_store::CatalogStore::open(&dir.path().join("catalog.db")).unwrap();
    catalog.set_sensitive(id, &key("name"), Some(true)).unwrap();
    catalog
        .set_sensitive(id, &key("nick"), Some(false))
        .unwrap();
    drop(catalog);
    let mut client = initialized(dir.path());
    let (rows, error) = client.call(
        "run_query",
        &json!({ "connection": "Users", "sql": "SELECT name, nick, email FROM users", "format": "json" }),
    );
    assert!(!error, "{rows}");
    let doc: Value = serde_json::from_str(&rows).unwrap();
    // `name` is marked; `nick` is by the rule, but marked not sensitive on this table; the
    // default rule on `email` was replaced.
    assert_eq!(
        doc["rows"][0],
        json!(["••••••", "annie", "ann@example.com"]),
        "{doc}"
    );
    assert_eq!(doc["masked_columns"], json!(["name"]), "{doc}");
}

#[test]
fn the_values_show_only_as_the_user_approved_in_the_app() {
    let dir = tempfile::tempdir().unwrap();
    users_profile(dir.path(), "agent_values = \"shown\"\n");
    // No seal: the file is taken as it is.
    let mut client = initialized(dir.path());
    let (rows, error) = client.call(
        "run_query",
        &json!({ "connection": "Users", "sql": "SELECT email FROM users ORDER BY id" }),
    );
    assert!(!error && rows.contains("ann@example.com"), "{rows}");
    assert!(!rows.contains("masked"), "{rows}");
}

/// The limits of a connection lower what agents ask for, and say so.
#[test]
fn a_connection_lowers_rows_and_says_so() {
    let dir = tempfile::tempdir().unwrap();
    users_profile(
        dir.path(),
        "
[connections.agent_limits]
max_rows = 1
",
    );
    let mut client = initialized(dir.path());
    let (asked, error) = client.call(
        "run_query",
        &json!({ "connection": "Users", "sql": "SELECT id FROM users ORDER BY id", "max_rows": 500, "format": "json" }),
    );
    assert!(!error, "{asked}");
    let doc: Value = serde_json::from_str(&asked).unwrap();
    assert_eq!(doc["row_count"], 1, "{doc}");
    assert_eq!(doc["truncated_by"], "max_rows", "{doc}");
    assert!(
        doc["notes"][0]
            .as_str()
            .unwrap()
            .contains("max_rows was lowered to 1"),
        "{doc}"
    );
    // The default is lowered too, with no note: the agent asked for nothing.
    let (default, error) = client.call(
        "run_query",
        &json!({ "connection": "Users", "sql": "SELECT id FROM users ORDER BY id" }),
    );
    assert!(!error && default.contains("(1 row"), "{default}");
    assert!(!default.contains("lowered"), "{default}");
    // Asking for no more than the limit says nothing.
    let (fits, error) = client.call(
        "sample_rows",
        &json!({ "connection": "Users", "schema": "main", "table": "users", "max_rows": 1 }),
    );
    assert!(!error && !fits.contains("lowered"), "{fits}");
}

#[test]
fn calls_per_minute_are_counted_per_connection() {
    let dir = tempfile::tempdir().unwrap();
    users_profile(
        dir.path(),
        "
[connections.agent_limits]
calls_per_minute = 2
",
    );
    let mut client = initialized(dir.path());
    let select = json!({ "connection": "Users", "sql": "SELECT 1" });
    let (first, error) = client.call("run_query", &select);
    assert!(!error, "{first}");
    let (second, error) = client.call(
        "sample_rows",
        &json!({ "connection": "Users", "schema": "main", "table": "users" }),
    );
    assert!(!error, "{second}");
    for (tool, args) in [
        ("run_query", select),
        (
            "explain",
            json!({ "connection": "Users", "sql": "SELECT 1" }),
        ),
    ] {
        let (refused, error) = client.call(tool, &args);
        assert!(
            error && refused.contains("2 calls a minute") && refused.contains("try again in"),
            "{tool}: {refused}"
        );
    }
    // Metadata calls are not counted.
    let (schemas, error) = client.call("list_schemas", &json!({ "connection": "Users" }));
    assert!(!error, "{schemas}");
}

/// SQLite gives no estimate: the cost guard lets the read run and says it could not look.
#[test]
fn the_cost_guard_goes_ahead_without_an_estimate() {
    let dir = tempfile::tempdir().unwrap();
    users_profile(
        dir.path(),
        "
[connections.agent_limits]
max_estimated_rows = 1
max_plan_cost = 1
",
    );
    let mut client = initialized(dir.path());
    let (rows, error) = client.call(
        "run_query",
        &json!({ "connection": "Users", "sql": "SELECT id FROM users ORDER BY id" }),
    );
    assert!(!error && rows.contains("(2 rows"), "{rows}");
    assert!(rows.contains("could not be checked"), "{rows}");
    // Without a threshold nothing is checked, so nothing is said.
    let plain = tempfile::tempdir().unwrap();
    users_profile(plain.path(), "");
    let mut client = initialized(plain.path());
    let (rows, error) = client.call(
        "run_query",
        &json!({ "connection": "Users", "sql": "SELECT id FROM users" }),
    );
    assert!(!error && !rows.contains("checked"), "{rows}");
}

/// `get_er_diagram` draws the tables of a schema, or those near some tables, as Mermaid.
#[test]
fn draws_er_diagrams_from_the_catalog() {
    let dir = tempfile::tempdir().unwrap();
    profile(dir.path());
    sqlite(
        &dir.path().join("app.db"),
        "CREATE TABLE customer (id INTEGER PRIMARY KEY, name TEXT);
         CREATE TABLE \"sales order\" (id INTEGER PRIMARY KEY, customer_id INTEGER,
             CONSTRAINT fk_customer FOREIGN KEY (customer_id) REFERENCES customer (id));
         CREATE TABLE item (id INTEGER PRIMARY KEY, order_id INTEGER, product_id INTEGER,
             FOREIGN KEY (order_id) REFERENCES \"sales order\" (id),
             FOREIGN KEY (product_id) REFERENCES product (id));
         CREATE TABLE product (id INTEGER PRIMARY KEY, label TEXT);
         CREATE TABLE tag (id INTEGER PRIMARY KEY, parent INTEGER REFERENCES tag (id));
         CREATE VIEW names AS SELECT name FROM customer;",
    );
    let mut client = initialized(dir.path());
    let mut draw = |args: Value| client.call("get_er_diagram", &args);

    let (all, error) = draw(json!({ "connection": "App db", "schema": "main" }));
    assert!(
        !error
            && all.starts_with(
                "erDiagram
"
            ),
        "{all}"
    );
    // Tables with primary keys, names with spaces quoted, the view left out.
    assert!(all.contains("\"customer\" {"), "{all}");
    assert!(all.contains("\"sales order\" {"), "{all}");
    assert!(all.contains("INTEGER id PK"), "{all}");
    assert!(!all.contains("names"), "{all}");
    assert!(
        all.contains("\"sales order\" }o--|| \"customer\" : "),
        "{all}"
    );
    assert!(all.contains("\"item\" }o--|| \"sales order\""), "{all}");
    assert!(all.contains("\"tag\" }o--|| \"tag\""), "{all}");
    assert!(!all.contains("cut:"), "{all}");

    // One hop by default, in both directions: the customer, and the items that point at it.
    let (near, error) = draw(json!({
        "connection": "App db", "schema": "main", "tables": ["sales order"]
    }));
    assert!(!error, "{near}");
    assert!(
        near.contains("\"customer\" {") && near.contains("\"item\" {"),
        "{near}"
    );
    assert!(
        !near.contains("\"product\"") && !near.contains("\"tag\""),
        "{near}"
    );
    // The key from item to product is left out: product is not drawn.
    assert!(!near.contains("}o--|| \"product\""), "{near}");

    let (alone, _) = draw(json!({
        "connection": "App db", "schema": "main", "tables": ["sales order"], "depth": 0
    }));
    assert!(
        !alone.contains("\"customer\" {") && !alone.contains("}o--||"),
        "{alone}"
    );
    assert!(alone.contains("no foreign keys"), "{alone}");
    let (deep, _) = draw(json!({
        "connection": "App db", "schema": "main", "tables": ["customer"], "depth": 3
    }));
    assert!(
        deep.contains("\"product\" {") && !deep.contains("\"tag\" {"),
        "{deep}"
    );

    // Only real tables, in a real schema, on a connection open to agents.
    let (missing, error) = draw(json!({
        "connection": "App db", "schema": "main", "tables": ["customer", "nope"]
    }));
    assert!(
        error && missing.contains("no table `main.nope`"),
        "{missing}"
    );
    let (view, error) = draw(json!({
        "connection": "App db", "schema": "main", "tables": ["names"]
    }));
    assert!(
        error && view.contains("`main.names` is not a table"),
        "{view}"
    );
    let (schema, error) = draw(json!({ "connection": "App db", "schema": "nope" }));
    // SQLite fails on its own; other databases list nothing, which is reported as missing.
    assert!(error && schema.contains("nope"), "{schema}");
    let (off, error) = draw(json!({ "connection": "Hidden", "schema": "main" }));
    assert!(error && off.contains("available to agents"), "{off}");
    let (nowhere, error) = draw(json!({ "connection": "Nowhere", "schema": "main" }));
    assert!(error, "{nowhere}");

    // A database without tables or keys still answers.
    sqlite(&dir.path().join("scratch.db"), "DROP TABLE t;");
    let (empty, error) = draw(json!({ "connection": "Scratch", "schema": "main" }));
    assert!(!error && empty.contains("no tables"), "{empty}");
}

/// A schema too big for the byte limit is cut at whole tables, with a note.
#[test]
fn a_big_er_diagram_is_cut_with_a_note() {
    use std::fmt::Write as _;

    let dir = tempfile::tempdir().unwrap();
    profile(dir.path());
    let mut columns = String::new();
    for i in 0..30 {
        let _ = write!(columns, ", column_number_{i} INTEGER");
    }
    let mut big = String::new();
    for i in 0..80 {
        let _ = write!(
            big,
            "CREATE TABLE big_table_{i:02} (id INTEGER PRIMARY KEY{columns});"
        );
    }
    sqlite(&dir.path().join("scratch.db"), &big);
    let mut client = initialized(dir.path());
    let (cut, error) = client.call(
        "get_er_diagram",
        &json!({ "connection": "Scratch", "schema": "main" }),
    );
    assert!(!error && cut.len() < 33_000, "{}", cut.len());
    assert!(
        cut.contains("%% cut: ") && cut.contains("of 81 tables"),
        "{cut}"
    );
    assert!(cut.contains("Pass `tables`"), "{cut}");
    assert_eq!(cut.matches('{').count(), cut.matches('}').count());
    let (some, error) = client.call(
        "get_er_diagram",
        &json!({ "connection": "Scratch", "schema": "main", "tables": ["big_table_07"] }),
    );
    assert!(
        !error && !some.contains("cut:") && some.contains("big_table_07"),
        "{some}"
    );
}

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
    let proposals = || -> Vec<(String, String, Option<String>, String)> {
        log.prepare("SELECT agent, connection_id, title, sql FROM proposals")
            .unwrap()
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap()
    };
    assert_eq!(
        proposals(),
        [(
            "test-agent".to_owned(),
            "00000000-0000-4000-8000-000000000001".to_owned(),
            Some("Remove Ada".to_owned()),
            "DELETE FROM t WHERE id = 1".to_owned()
        )]
    );

    // A flood of proposals stops at the bound, with a reason the agent can act on.
    let mut client = initialized(dir.path());
    let mut refused = None;
    for i in 1..=platypus_store::MAX_PENDING_PROPOSALS {
        let (text, error) = client.call(
            "propose_sql",
            &json!({ "connection": "App db", "sql": format!("SELECT {i}") }),
        );
        if error {
            refused = Some((i, text));
            break;
        }
    }
    let (at, text) = refused.expect("the proposals are bounded");
    assert_eq!(at, platypus_store::MAX_PENDING_PROPOSALS, "{text}");
    assert!(text.contains("wait for the user"), "{text}");
    drop(client);
    assert_eq!(
        proposals().len(),
        platypus_store::MAX_PENDING_PROPOSALS as usize
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

/// Saves `notes` as `(connection digit, object, column, note, tags)` in the schema `main` of
/// the app's `catalog.db` in `dir`, and marks `marks` (object, column) of the first connection
/// sensitive.
fn catalog_notes(dir: &Path, notes: &[(&str, &str, &str, &str, &[&str])], marks: &[(&str, &str)]) {
    use platypus_store::{CatalogKey, CatalogStore};
    let mut catalog = CatalogStore::open(&dir.join("catalog.db")).unwrap();
    let key = |object: &str, column: &str| CatalogKey {
        database: String::new(),
        schema: "main".to_owned(),
        object: object.to_owned(),
        column: column.to_owned(),
    };
    let id = |n: &str| {
        uuid::Uuid::parse_str(&format!("00000000-0000-4000-8000-00000000000{n}")).unwrap()
    };
    for (connection, object, column, note, tags) in notes {
        let tags: Vec<String> = tags.iter().map(|t| (*t).to_owned()).collect();
        catalog
            .save(id(connection), &key(object, column), note, &tags)
            .unwrap();
    }
    for (object, column) in marks {
        catalog
            .set_sensitive(id("1"), &key(object, column), Some(true))
            .unwrap();
    }
}

fn json_of(client: &mut Client, tool: &str, args: &Value) -> Value {
    let (text, error) = client.call(tool, args);
    assert!(!error, "{tool}: {text}");
    serde_json::from_str(&text).unwrap_or_else(|e| panic!("{tool}: {text}: {e}"))
}

/// What `search_objects` matched: name, what matched and the column.
fn search_hits(client: &mut Client, text: &str) -> Vec<(String, String, Option<String>)> {
    let found = json_of(
        client,
        "search_objects",
        &json!({ "connection": "App db", "text": text }),
    );
    found["matches"]
        .as_array()
        .unwrap()
        .iter()
        .map(|m| {
            (
                m["name"].as_str().unwrap().to_owned(),
                m["matched"].as_str().unwrap().to_owned(),
                m["column"].as_str().map(str::to_owned),
            )
        })
        .collect()
}

/// `search_objects` by note, by the note of a column and by `#tag`.
fn finds_by_notes_and_tags(client: &mut Client) {
    let hit =
        |n: &str, m: &str, c: Option<&str>| (n.to_owned(), m.to_owned(), c.map(str::to_owned));
    // By note and by the note of a column; the dropped table finds nothing.
    assert_eq!(
        search_hits(client, "bills sent"),
        [hit("invoice", "note", None)]
    );
    assert_eq!(
        search_hits(client, "VAT"),
        [hit("invoice", "note", Some("total"))]
    );
    assert_eq!(search_hits(client, "ledger").len(), 0);
    assert_eq!(
        search_hits(client, "invoice"),
        [hit("invoice", "name", None)]
    );
    // By tag: only tags, as in the app's navigator.
    assert_eq!(
        search_hits(client, "#billing"),
        [hit("invoice", "tag", None)]
    );
    assert_eq!(
        search_hits(client, "#BILLING sent"),
        [hit("invoice", "tag", None)]
    );
    assert_eq!(search_hits(client, "#billing nothing"), []);
    assert_eq!(search_hits(client, "billing"), []);
    // A `#word` that is no tag is plain text.
    assert_eq!(search_hits(client, "#nope"), []);
}

#[test]
fn agents_read_the_notes_tags_and_marks_of_the_catalog() {
    let dir = tempfile::tempdir().unwrap();
    profile(dir.path());
    sqlite(
        &dir.path().join("app.db"),
        "CREATE TABLE invoice (id INTEGER PRIMARY KEY, total REAL, iban TEXT);
         CREATE TABLE plain (id INTEGER PRIMARY KEY);",
    );
    sqlite(
        &dir.path().join("hidden.db"),
        "CREATE TABLE secret_plans (id INTEGER PRIMARY KEY);",
    );
    let nasty = "Bills sent to customers. \",\"sensitive\":false,\"columns\":[]";
    catalog_notes(
        dir.path(),
        &[
            ("1", "invoice", "", nasty, &["Billing", "core"]),
            ("1", "invoice", "total", "gross, VAT included", &[]),
            ("1", "invoice", "iban", "", &["pii"]),
            // The table was dropped: the note stays in the catalog.
            ("1", "dropped_table", "", "old ledger", &["billing"]),
            ("3", "secret_plans", "", "ledger of the plans", &["billing"]),
        ],
        &[("invoice", "iban")],
    );
    let mut client = initialized(dir.path());
    let table = json_of(
        &mut client,
        "describe_table",
        &json!({ "connection": "App db", "schema": "main", "table": "invoice" }),
    );
    assert_eq!(table["note"], nasty, "{table}");
    assert_eq!(table["tags"], json!(["billing", "core"]));
    assert_eq!(table["columns"].as_array().unwrap().len(), 3, "{table}");
    assert_eq!(table["columns"][1]["note"], "gross, VAT included");
    assert_eq!(table["columns"][2]["tags"], json!(["pii"]));
    assert_eq!(table["columns"][2]["sensitive"], true);
    // The note stayed one string: it added no field, and only the marked column has a mark.
    assert!(table["columns"][0].get("note").is_none(), "{table}");
    assert!(table["columns"][0].get("sensitive").is_none(), "{table}");
    assert!(table["columns"][1].get("sensitive").is_none(), "{table}");

    // An object without entries has the fields it always had.
    let plain = json_of(
        &mut client,
        "describe_table",
        &json!({ "connection": "App db", "schema": "main", "table": "plain" }),
    );
    for field in ["note", "tags", "note_cut", "catalog_unavailable"] {
        assert!(plain.get(field).is_none(), "{field}: {plain}");
    }
    let id_column = &plain["columns"][0];
    assert_eq!(id_column["name"], "id");
    assert!(id_column.get("note").is_none() && id_column.get("sensitive").is_none());

    let objects = json_of(
        &mut client,
        "list_objects",
        &json!({ "connection": "App db", "schema": "main" }),
    );
    let by_name = |n: &str| {
        objects
            .as_array()
            .unwrap()
            .iter()
            .find(|o| o["name"] == n)
            .unwrap()
    };
    assert_eq!(by_name("invoice")["tags"], json!(["billing", "core"]));
    assert_eq!(by_name("invoice")["note"], nasty);
    assert!(by_name("plain").get("tags").is_none() && by_name("plain").get("note").is_none());

    // The resources are the same descriptions.
    let read = client.request(
        "resources/read",
        &json!({ "uri": "platypus://App%20db/main/invoice" }),
    );
    let text = read["result"]["contents"][0]["text"].as_str().unwrap();
    assert_eq!(serde_json::from_str::<Value>(text).unwrap(), table);
    let read = client.request(
        "resources/read",
        &json!({ "uri": "platypus://App%20db/main" }),
    );
    let text = read["result"]["contents"][0]["text"].as_str().unwrap();
    assert!(text.contains(r#""tags":["billing","core"]"#), "{text}");

    finds_by_notes_and_tags(&mut client);

    // A connection agents may not use does not leak its notes.
    let (hidden, error) = client.call(
        "search_objects",
        &json!({ "connection": "Hidden", "text": "#billing" }),
    );
    assert!(error && !hidden.contains("ledger"), "{hidden}");
    let (hidden, error) = client.call(
        "describe_table",
        &json!({ "connection": "Hidden", "schema": "main", "table": "secret_plans" }),
    );
    assert!(error && !hidden.contains("plans"), "{hidden}");
}

#[test]
fn search_by_a_hash_word_works_without_a_catalog() {
    let dir = tempfile::tempdir().unwrap();
    profile(dir.path());
    sqlite(
        &dir.path().join("app.db"),
        "CREATE TABLE \"#temp\" (id INTEGER PRIMARY KEY);",
    );
    let mut client = initialized(dir.path());
    let found = search_hits(&mut client, "#temp");
    assert_eq!(found[0].0, "#temp", "{found:?}");
    let table = json_of(
        &mut client,
        "describe_table",
        &json!({ "connection": "App db", "schema": "main", "table": "t" }),
    );
    assert!(table.get("note").is_none() && table.get("catalog_unavailable").is_none());
}

#[test]
fn an_unreadable_catalog_does_not_break_describe_table() {
    let dir = tempfile::tempdir().unwrap();
    profile(dir.path());
    std::fs::write(
        dir.path().join("catalog.db"),
        b"this is not a database at all",
    )
    .unwrap();
    let mut client = initialized(dir.path());
    let table = json_of(
        &mut client,
        "describe_table",
        &json!({ "connection": "App db", "schema": "main", "table": "t" }),
    );
    assert_eq!(table["columns"][0]["name"], "id");
    assert!(
        table["catalog_unavailable"]
            .as_str()
            .unwrap()
            .contains("could not be read"),
        "{table}"
    );
    let objects = json_of(
        &mut client,
        "list_objects",
        &json!({ "connection": "App db", "schema": "main" }),
    );
    // A plain list stays a list, as clients read it.
    assert_eq!(objects[0]["name"], "t", "{objects}");
}
