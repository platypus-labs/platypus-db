//! Cloud sign-in and port forwards as `platypus-net` runs them, against PostgreSQL and MySQL
//! from `docker/compose.yml`: a fake `aws` prints the server's password as its IAM token, and
//! a fake `kubectl` forwards its local port to the server, as the real ones do for RDS and
//! for a Kubernetes service. An unreachable server is skipped unless its
//! `PLATYPUS_TEST_*_URL` is set.
#![cfg(unix)]

use std::os::unix::fs::PermissionsExt as _;
use std::path::Path;
use std::time::{Duration, Instant};

use platypus_core::{
    AUTH, Connection, ConnectionConfig, Driver, DriverKind, ExecOptions, ExecResult, FORWARD,
    K8S_TARGET,
};
use platypus_driver_mysql::MySqlDriver;
use platypus_driver_pg::PgDriver;

/// Forwards `127.0.0.1:<argv 1>` to `127.0.0.1:<argv 2>`, as `kubectl port-forward` does to
/// a pod.
const FORWARDER: &str = r#"
import socket, sys, threading
local, remote = int(sys.argv[1]), int(sys.argv[2])
server = socket.socket()
server.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
server.bind(("127.0.0.1", local))
server.listen()
print(f"Forwarding from 127.0.0.1:{local} -> {remote}", flush=True)
def pipe(a, b):
    try:
        while data := a.recv(65536):
            b.sendall(data)
    except OSError:
        pass
    for s in (a, b):
        try:
            s.shutdown(socket.SHUT_RDWR)
        except OSError:
            pass
while True:
    client, _ = server.accept()
    upstream = socket.create_connection(("127.0.0.1", remote))
    for a, b in ((client, upstream), (upstream, client)):
        threading.Thread(target=pipe, args=(a, b), daemon=True).start()
"#;

fn script(dir: &Path, name: &str, text: &str) {
    let path = dir.join(name);
    std::fs::write(&path, text).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
}

/// Puts the fake CLIs first on the `PATH`, before any thread starts: the test's only one.
#[allow(unsafe_code)]
fn fake_clis(dir: &Path) {
    script(dir, "forward.py", FORWARDER);
    // The token: the password, with the arguments kept to check them.
    script(
        dir,
        "aws",
        &format!(
            "#!/bin/sh\necho \"$@\" >> '{}'\necho platypus\n",
            dir.join("aws.args").display()
        ),
    );
    // `kubectl port-forward … <local>:<remote>`: the last argument says both ports.
    script(
        dir,
        "kubectl",
        &format!(
            "#!/bin/sh\nfor a; do last=$a; done\nexec python3 '{}' \"${{last%%:*}}\" \"${{last##*:}}\"\n",
            dir.join("forward.py").display()
        ),
    );
    let path = std::env::var("PATH").unwrap_or_default();
    // SAFETY: set before the runtime, and any other thread, starts.
    unsafe { std::env::set_var("PATH", format!("{}:{path}", dir.display())) };
}

fn driver(kind: DriverKind) -> &'static dyn Driver {
    match kind {
        DriverKind::Postgres => &PgDriver,
        DriverKind::MySql => &MySqlDriver,
        other => unreachable!("{other:?}"),
    }
}

async fn connect(config: &ConnectionConfig) -> platypus_core::Result<Box<dyn Connection>> {
    platypus_net::connect(config, None, |config| async move {
        driver(config.driver).connect(&config).await
    })
    .await
}

async fn answers(conn: &dyn Connection) {
    let result = conn
        .execute("SELECT 1", &ExecOptions::default())
        .await
        .unwrap();
    assert!(
        matches!(result, ExecResult::Rows { .. }),
        "SELECT 1 gave {result:?}"
    );
}

/// The server at `env`'s URL or `default`, with its password; `None` when it is not
/// running and not required.
async fn server(env: &str, default: &str) -> Option<ConnectionConfig> {
    let (url, required) = match std::env::var(env) {
        Ok(url) => (url, true),
        Err(_) => (default.to_owned(), false),
    };
    let config = ConnectionConfig::from_url(&url).unwrap();
    match driver(config.driver).connect(&config).await {
        Ok(_) => Some(config),
        Err(e) if !required => {
            eprintln!("skipping {default}: {e}");
            None
        }
        Err(e) => panic!("{url}: {e}"),
    }
}

async fn signs_in_with_the_token(dir: &Path, mut config: ConnectionConfig) {
    let port = config.port.unwrap();
    config.password = None;
    config.options.insert(AUTH.to_owned(), "aws_iam".to_owned());
    let conn = connect(&config).await.unwrap();
    answers(conn.as_ref()).await;
    let args = std::fs::read_to_string(dir.join("aws.args")).unwrap();
    let asked = format!(
        "rds generate-db-auth-token --hostname {} --port {port} --username {}",
        config.host, config.user
    );
    assert!(args.contains(&asked), "{args}");
}

async fn reaches_through_the_forward(mut config: ConnectionConfig) {
    config
        .options
        .insert(FORWARD.to_owned(), "kubernetes".to_owned());
    config
        .options
        .insert(K8S_TARGET.to_owned(), "svc/db".to_owned());
    let conn = connect(&config).await.unwrap();
    answers(conn.as_ref()).await;
    drop(conn);
    // The forward ends with the connection: `kubectl` is gone, a new connection starts
    // another one.
    let again = connect(&config).await.unwrap();
    answers(again.as_ref()).await;
}

#[test]
fn tokens_and_forwards_reach_the_servers() {
    let dir = tempfile::tempdir().unwrap();
    fake_clis(dir.path());
    let runtime = tokio::runtime::Runtime::new().unwrap();
    runtime.block_on(async {
        let started = Instant::now();
        for (env, default) in [
            (
                "PLATYPUS_TEST_PG_URL",
                "postgres://platypus:platypus@localhost:55432/platypus?sslmode=disable",
            ),
            (
                "PLATYPUS_TEST_MYSQL_URL",
                "mysql://platypus:platypus@localhost:53306/platypus",
            ),
        ] {
            let Some(config) = server(env, default).await else {
                continue;
            };
            signs_in_with_the_token(dir.path(), config.clone()).await;
            reaches_through_the_forward(config).await;
        }
        assert!(started.elapsed() < Duration::from_secs(120));
    });
}
