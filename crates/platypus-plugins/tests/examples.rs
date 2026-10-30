//! The example plugins of `examples/plugins`, built to WebAssembly and committed with their
//! sources, installed and run as the app does.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use arrow_array::cast::AsArray;
use arrow_array::types::Int64Type;
use futures::TryStreamExt;
use platypus_core::{
    ConnectionConfig, Driver, DriverKind, ExecOptions, ExecResult, ObjectKind, Secret, SslMode,
};
use platypus_plugins::{ActionOutcome, PLUGIN_OPTION, PluginDriver, Plugins};
use serde_json::{Value, json};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

/// Installs `source` as the app does: inspected, then installed with the reviewed hash.
async fn install(
    plugins: &Plugins,
    source: &Path,
) -> Result<platypus_plugins::Installed, platypus_plugins::PluginError> {
    let reviewed = plugins.inspect(source).await?;
    plugins.install(source, &reviewed.hash).await
}

fn example(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../examples/plugins")
        .join(name)
}

#[tokio::test(flavor = "multi_thread")]
async fn sql_tools_actions() {
    let data = tempfile::tempdir().unwrap();
    let plugins = Plugins::new(data.path());
    let installed = install(&plugins, &example("sql-tools")).await.unwrap();
    assert!(installed.approved.clipboard);
    assert_eq!(installed.description.actions.len(), 3);

    let upper = plugins
        .run_action(
            "sql-tools",
            "uppercase",
            json!({ "text": "select 'from' as \"order\" from t -- where\nwhere x is null", "selection": null, "driver": "sqlite" }),
        )
        .await
        .unwrap();
    assert_eq!(
        upper,
        ActionOutcome::Replace {
            text: "SELECT 'from' AS \"order\" FROM t -- where\nWHERE x IS NULL".to_owned()
        }
    );
    // The selection wins over the whole text.
    let selected = plugins
        .run_action(
            "sql-tools",
            "uppercase",
            json!({ "text": "select 1", "selection": { "from": 0, "to": 6, "text": "select" } }),
        )
        .await
        .unwrap();
    assert_eq!(
        selected,
        ActionOutcome::Replace {
            text: "SELECT".to_owned()
        }
    );

    let table = plugins
        .run_action(
            "sql-tools",
            "markdown",
            json!({
                "columns": [{ "name": "id", "type": "INTEGER" }, { "name": "name", "type": "TEXT" }],
                "rows": [[1, "a|b"], [2, null]],
            }),
        )
        .await
        .unwrap();
    assert_eq!(
        table,
        ActionOutcome::Copy {
            text: "| id | name |\n| --- | --- |\n| 1 | a\\|b |\n| 2 | NULL |\n".to_owned()
        }
    );
    let none = plugins
        .run_action(
            "sql-tools",
            "markdown",
            json!({ "columns": [], "rows": [] }),
        )
        .await
        .unwrap();
    assert!(matches!(none, ActionOutcome::Message { .. }), "{none:?}");

    let count = plugins
        .run_action(
            "sql-tools",
            "count",
            json!({ "target": "\"main\".\"items\"", "object": { "name": "items", "kind": "table" } }),
        )
        .await
        .unwrap();
    assert_eq!(
        count,
        ActionOutcome::NewTab {
            text: "SELECT COUNT(*) FROM \"main\".\"items\";".to_owned()
        }
    );
    let function = plugins
        .run_action(
            "sql-tools",
            "count",
            json!({ "target": "f", "object": { "name": "f", "kind": "function" } }),
        )
        .await
        .unwrap();
    assert!(
        matches!(function, ActionOutcome::Message { .. }),
        "{function:?}"
    );
    // A guest error reaches the app as the guest wrote it.
    let e = plugins
        .run_action("sql-tools", "count", json!({}))
        .await
        .unwrap_err();
    assert_eq!(e.to_string(), "no object");
}

/// The requests the toy server saw: method, path, `Authorization` and body.
type Seen = Arc<Mutex<Vec<(String, String, String, String)>>>;

/// A database speaking the toy driver's HTTP API, with one table and the token `t0ken`.
async fn toy_server(seen: Seen) -> u16 {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(async move {
        while let Ok((mut socket, _)) = listener.accept().await {
            let seen = Arc::clone(&seen);
            tokio::spawn(async move {
                let (method, path, auth, body) = read_request(&mut socket).await;
                seen.lock().unwrap().push((
                    method.clone(),
                    path.clone(),
                    auth.clone(),
                    body.clone(),
                ));
                let (status, reply) = answer(&method, &path, &auth, &body);
                let response = format!(
                    "HTTP/1.1 {status} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{reply}",
                    reply.len()
                );
                let _ = socket.write_all(response.as_bytes()).await;
            });
        }
    });
    port
}

fn answer(method: &str, path: &str, auth: &str, body: &str) -> (u16, String) {
    if auth != "Bearer t0ken" {
        return (401, json!({ "error": "bad token" }).to_string());
    }
    let reply = match (method, path) {
        ("GET", "/shop/hello") => json!({ "product": "ToyDB", "version": "1.2" }),
        ("POST", "/shop/query") => {
            let sql = serde_json::from_str::<Value>(body).unwrap()["sql"]
                .as_str()
                .unwrap()
                .to_owned();
            if sql.starts_with("SELECT") {
                json!({
                    "columns": [{ "name": "id", "type": "int" }, { "name": "name" }],
                    "rows": [[1, "apple"], [2, "pear"], [3, null]],
                })
            } else if sql.starts_with("DELETE") {
                json!({ "affected": 2 })
            } else {
                return (
                    400,
                    json!({ "error": format!("syntax error near {sql:?}") }).to_string(),
                );
            }
        }
        ("GET", "/shop/tables") => json!(["items"]),
        ("GET", "/shop/tables/items") => json!({ "columns": [
            { "name": "id", "type": "int", "nullable": false, "primary_key": true },
            { "name": "name", "type": "text" },
        ] }),
        _ => return (404, json!({ "error": "not found" }).to_string()),
    };
    (200, reply.to_string())
}

async fn read_request(socket: &mut tokio::net::TcpStream) -> (String, String, String, String) {
    let mut data = Vec::new();
    let mut buf = [0u8; 4096];
    let end = loop {
        let n = socket.read(&mut buf).await.unwrap();
        data.extend_from_slice(&buf[..n]);
        if let Some(end) = data.windows(4).position(|w| w == b"\r\n\r\n") {
            break end;
        }
        assert!(n > 0, "the request ended early");
    };
    let head = String::from_utf8_lossy(&data[..end]).into_owned();
    let header = |name: &str| {
        head.lines()
            .find_map(|l| {
                let (k, v) = l.split_once(':')?;
                k.eq_ignore_ascii_case(name).then(|| v.trim().to_owned())
            })
            .unwrap_or_default()
    };
    let length: usize = header("content-length").parse().unwrap_or(0);
    while data.len() < end + 4 + length {
        let n = socket.read(&mut buf).await.unwrap();
        if n == 0 {
            break;
        }
        data.extend_from_slice(&buf[..n]);
    }
    let mut first = head.lines().next().unwrap().split(' ');
    (
        first.next().unwrap().to_owned(),
        first.next().unwrap().to_owned(),
        header("authorization"),
        String::from_utf8_lossy(&data[end + 4..]).into_owned(),
    )
}

fn config(port: u16, token: &str) -> ConnectionConfig {
    let mut config = ConnectionConfig::new(DriverKind::Plugin);
    "127.0.0.1".clone_into(&mut config.host);
    config.port = Some(port);
    "shop".clone_into(&mut config.database);
    config.ssl_mode = SslMode::Disable;
    config.password = Some(Secret::new(token));
    config
        .options
        .insert(PLUGIN_OPTION.to_owned(), "toy-http-driver".to_owned());
    config
}

#[tokio::test(flavor = "multi_thread")]
async fn toy_http_driver() {
    let data = tempfile::tempdir().unwrap();
    let plugins = Plugins::new(data.path());
    install(&plugins, &example("toy-http-driver"))
        .await
        .unwrap();
    let seen = Seen::default();
    let port = toy_server(Arc::clone(&seen)).await;
    let driver = PluginDriver::new(plugins.clone());

    let conn = driver.connect(&config(port, "t0ken")).await.unwrap();
    assert_eq!(conn.server_info().product, "ToyDB");
    assert_eq!(conn.server_info().version, "1.2");

    let ExecResult::Rows(stream) = conn
        .execute(
            "SELECT * FROM items",
            &ExecOptions {
                batch_size: 2,
                paged: false,
            },
        )
        .await
        .unwrap()
    else {
        panic!("expected rows");
    };
    assert_eq!(stream.schema().field(0).name(), "id");
    let batches: Vec<_> = stream.try_collect().await.unwrap();
    assert_eq!(batches.len(), 2);
    assert_eq!(batches[0].column(0).as_primitive::<Int64Type>().value(1), 2);
    assert_eq!(batches[0].column(1).as_string::<i32>().value(0), "apple");
    assert!(batches[1].column(1).is_null(0));

    let affected = conn
        .execute("DELETE FROM items", &ExecOptions::default())
        .await
        .unwrap();
    assert!(
        matches!(affected, ExecResult::Affected { rows: 2 }),
        "{affected:?}"
    );
    let e = conn
        .execute("NONSENSE", &ExecOptions::default())
        .await
        .unwrap_err();
    assert!(e.to_string().contains("syntax error"), "{e}");
    assert!(
        !conn.is_closed(),
        "a database error leaves the connection open"
    );

    let schemas = conn.schemas().await.unwrap();
    assert_eq!(schemas[0].name, "main");
    let objects = conn.objects("main").await.unwrap();
    assert_eq!(objects[0].name, "items");
    assert_eq!(objects[0].kind, ObjectKind::Table);
    let details = conn.table_details("main", "items").await.unwrap();
    assert_eq!(details.columns.len(), 2);
    assert!(details.columns[0].primary_key);
    assert!(!details.columns[0].nullable);
    assert!(conn.set_autocommit(false).await.is_err());

    // The token goes to the server in each request, and nowhere in the connect config.
    assert!(
        seen.lock()
            .unwrap()
            .iter()
            .all(|(_, _, auth, _)| auth == "Bearer t0ken")
    );
    drop(conn);

    let e = driver.connect(&config(port, "wrong")).await.unwrap_err();
    assert!(e.to_string().contains("bad token"), "{e}");

    let mut no_plugin = config(port, "t0ken");
    no_plugin.options.clear();
    assert!(driver.connect(&no_plugin).await.is_err());

    plugins.set_enabled("toy-http-driver", false).unwrap();
    let e = driver.connect(&config(port, "t0ken")).await.unwrap_err();
    assert!(e.to_string().contains("turned off"), "{e}");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_driver_may_reach_the_host_of_its_connection() {
    let data = tempfile::tempdir().unwrap();
    let plugins = Plugins::new(data.path());
    install(&plugins, &example("toy-http-driver"))
        .await
        .unwrap();
    let port = toy_server(Seen::default()).await;
    // The plugin asks for no host in its manifest: the one the user typed is allowed.
    let mut config = config(port, "t0ken");
    "localhost".clone_into(&mut config.host);
    let conn = PluginDriver::new(plugins).connect(&config).await.unwrap();
    assert_eq!(conn.server_info().product, "ToyDB");
}
