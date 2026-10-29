//! A port forwarded by a cloud's CLI: `kubectl port-forward` to a Kubernetes service or pod,
//! or `aws ssm start-session` to a host seen from an EC2 instance. The CLI listens on a free
//! local port, which the driver connects to as it would to an SSH tunnel's; it runs as long
//! as the connection, and its end closes the connection.

use std::net::{Ipv4Addr, SocketAddr};
use std::time::{Duration, Instant};

use platypus_core::{
    AWS_PROFILE, AWS_REGION, ConnectionConfig, Error, FORWARD, K8S_CONTEXT, K8S_NAMESPACE,
    K8S_TARGET, Result, SSM_TARGET,
};
use tokio::task::JoinHandle;

use crate::cli;

/// How long the CLI may take to listen: signing in to the cluster or to AWS takes seconds.
const READY_TIMEOUT: Duration = Duration::from_secs(30);

/// The command line that forwards `local_port` to `config`'s server, or `None` when the
/// connection has no port forward.
pub(crate) fn command(
    config: &ConnectionConfig,
    local_port: u16,
) -> Result<Option<(String, Vec<String>)>> {
    let Some(kind) = config.options.get(FORWARD).filter(|k| !k.is_empty()) else {
        return Ok(None);
    };
    let option = |key: &str| {
        config
            .options
            .get(key)
            .map(|v| v.trim())
            .filter(|v| !v.is_empty())
    };
    let port = config
        .port
        .or(config.driver.default_port())
        .ok_or_else(|| Error::Config("a port forward needs a server port".to_owned()))?;
    let required = |key: &str, what: &str| {
        option(key).ok_or_else(|| Error::Config(format!("the port forward needs {what}")))
    };
    let mut args: Vec<String> = Vec::new();
    let mut push = |items: &[&str]| args.extend(items.iter().map(|&s| s.to_owned()));
    let program = match kind.as_str() {
        "kubernetes" => {
            let target = required(K8S_TARGET, "the service or pod, e.g. svc/postgres")?;
            push(&["port-forward", "--address", "127.0.0.1"]);
            if let Some(context) = option(K8S_CONTEXT) {
                push(&["--context", context]);
            }
            if let Some(namespace) = option(K8S_NAMESPACE) {
                push(&["--namespace", namespace]);
            }
            push(&[target, &format!("{local_port}:{port}")]);
            "kubectl"
        }
        "ssm" => {
            let target = required(SSM_TARGET, "the EC2 instance, e.g. i-0123456789abcdef0")?;
            push(&[
                "ssm",
                "start-session",
                "--target",
                target,
                "--document-name",
                "AWS-StartPortForwardingSessionToRemoteHost",
                "--parameters",
                &format!(
                    "host={},portNumber={port},localPortNumber={local_port}",
                    config.host
                ),
            ]);
            if let Some(region) = option(AWS_REGION) {
                push(&["--region", region]);
            }
            if let Some(profile) = option(AWS_PROFILE) {
                push(&["--profile", profile]);
            }
            "aws"
        }
        other => return Err(Error::Config(format!("unknown port forward `{other}`"))),
    };
    Ok(Some((program.to_owned(), args)))
}

/// A free port on the loopback, for the CLI to listen on.
fn free_port() -> Result<u16> {
    std::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
        .and_then(|l| l.local_addr())
        .map(|a| a.port())
        .map_err(|e| Error::Connect(format!("port forward: no free local port: {e}")))
}

/// Starts `config`'s port forward, if it has one: the local address that leads to the
/// server, and the CLI's run, which ends with it (and ends it when aborted).
pub(crate) async fn open(
    config: &ConnectionConfig,
) -> Result<Option<(SocketAddr, JoinHandle<()>)>> {
    let port = free_port()?;
    let Some((program, args)) = command(config, port)? else {
        return Ok(None);
    };
    start(&program, &args, port).await.map(Some)
}

/// Runs `program` and waits until something listens on `port`: an end of the CLI before
/// that, or no port open within the time allowed, is an error that quotes its stderr.
pub(crate) async fn start(
    program: &str,
    args: &[String],
    port: u16,
) -> Result<(SocketAddr, JoinHandle<()>)> {
    let mut child = cli::command(program, args)
        .spawn()
        .map_err(|e| cli::not_started(program, &e))?;
    let stderr = cli::Stderr::read(child.stderr.take());
    let local = SocketAddr::from((Ipv4Addr::LOCALHOST, port));
    let deadline = Instant::now() + READY_TIMEOUT;
    loop {
        if let Some(status) = child.try_wait().map_err(|e| cli::failed(program, e))? {
            return Err(cli::failed(program, stderr.explain(status).await));
        }
        if tokio::net::TcpStream::connect(local).await.is_ok() {
            break;
        }
        if Instant::now() > deadline {
            // Dropped, the CLI ends.
            return Err(cli::failed(
                program,
                format!(
                    "no forwarded port after {} seconds",
                    READY_TIMEOUT.as_secs()
                ),
            ));
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    let running = tokio::spawn(async move {
        let _ = child.wait().await;
    });
    Ok((local, running))
}

#[cfg(test)]
mod tests {
    use super::*;
    use platypus_core::DriverKind;

    fn config(options: &[(&str, &str)]) -> ConnectionConfig {
        let mut config = ConnectionConfig::new(DriverKind::Postgres);
        config.host = "db.internal".to_owned();
        for (key, value) in options {
            config
                .options
                .insert((*key).to_owned(), (*value).to_owned());
        }
        config
    }

    #[test]
    fn kubectl_forwards_the_local_port_to_the_server_port() {
        let config = config(&[
            (FORWARD, "kubernetes"),
            (K8S_TARGET, "svc/postgres"),
            (K8S_NAMESPACE, "data"),
            (K8S_CONTEXT, "prod"),
        ]);
        let (program, args) = command(&config, 40000).unwrap().unwrap();
        assert_eq!(program, "kubectl");
        assert_eq!(
            args.join(" "),
            "port-forward --address 127.0.0.1 --context prod --namespace data svc/postgres 40000:5432"
        );
    }

    #[test]
    fn ssm_forwards_to_the_host_seen_from_the_instance() {
        let config = config(&[
            (FORWARD, "ssm"),
            (SSM_TARGET, "i-0abc"),
            (AWS_PROFILE, "prod"),
        ]);
        let (program, args) = command(&config, 40000).unwrap().unwrap();
        assert_eq!(program, "aws");
        let args = args.join(" ");
        assert!(
            args.starts_with("ssm start-session --target i-0abc"),
            "{args}"
        );
        assert!(
            args.contains("host=db.internal,portNumber=5432,localPortNumber=40000"),
            "{args}"
        );
        assert!(args.ends_with("--profile prod"), "{args}");
    }

    #[test]
    fn a_forward_needs_its_target() {
        assert!(command(&config(&[]), 1).unwrap().is_none());
        let error = command(&config(&[(FORWARD, "kubernetes")]), 1)
            .unwrap_err()
            .to_string();
        assert!(error.contains("svc/postgres"), "{error}");
        assert!(command(&config(&[(FORWARD, "teleport")]), 1).is_err());
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn the_forward_is_ready_when_its_port_listens_and_ends_with_its_run() {
        let port = free_port().unwrap();
        // A CLI that listens after a moment, as `kubectl` does once it reached the cluster.
        let script = format!(
            "sleep 0.3; exec python3 -c 'import socket,time; s=socket.socket(); s.bind((\"127.0.0.1\",{port})); s.listen(); time.sleep(60)'"
        );
        let (local, running) = start("sh", &["-c".to_owned(), script], port).await.unwrap();
        assert_eq!(local.port(), port);
        assert!(!running.is_finished());
        running.abort();
        let _ = running.await;
        // Aborted, the run killed the CLI: the port no longer listens.
        let deadline = Instant::now() + Duration::from_secs(5);
        while tokio::net::TcpStream::connect(local).await.is_ok() && Instant::now() < deadline {
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        assert!(tokio::net::TcpStream::connect(local).await.is_err());
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn a_cli_that_ends_before_listening_says_why() {
        let script = "echo 'error: pods \"pg\" not found' >&2; exit 1".to_owned();
        let error = start("sh", &["-c".to_owned(), script], free_port().unwrap())
            .await
            .unwrap_err()
            .to_string();
        assert!(error.contains("not found"), "{error}");
    }
}
