//! Runs a script with the database's own command-line client found on the PATH (`psql`,
//! `mysql` or `sqlite3`), with the settings and password of the editor's session, for
//! scripts that rely on the client's own commands such as `\copy` or `source`.

use std::process::Stdio;
use std::time::Instant;

use platypus_core::{ConnectionConfig, DriverKind, SslMode};
use platypus_net::Tunnel;
use serde::Serialize;
use tauri::State;
use tokio::io::AsyncWriteExt;
use tokio::process::Command;

use super::AppState;
use crate::error::{CommandError, CommandResult};

#[derive(Debug, Serialize)]
pub struct CliOutput {
    /// The command line, without the password (which goes in the environment).
    command: String,
    /// The exit code; `None` when the client was stopped by a signal.
    status: Option<i32>,
    /// What the client printed, standard output then standard error.
    output: String,
    elapsed_ms: u64,
}

fn program(driver: DriverKind) -> &'static str {
    match driver {
        DriverKind::Postgres => "psql",
        DriverKind::MySql => "mysql",
        DriverKind::Sqlite => "sqlite3",
    }
}

/// The client's arguments and environment for `config`, reached at `host:port`.
fn invocation(
    config: &ConnectionConfig,
    host: &str,
    port: Option<u16>,
) -> (Vec<String>, Vec<(&'static str, String)>) {
    let mut args: Vec<String> = Vec::new();
    let mut env = Vec::new();
    let password = config.password.as_ref().map(|p| p.expose().to_owned());
    match config.driver {
        DriverKind::Postgres => {
            // No prompts (stdin is the script), stop at the first error, no ~/.psqlrc.
            args.extend(
                [
                    "-X",
                    "-w",
                    "-v",
                    "ON_ERROR_STOP=1",
                    "-h",
                    host,
                    "-U",
                    config.user.as_str(),
                    "-d",
                    config.database.as_str(),
                ]
                .map(str::to_owned),
            );
            if let Some(port) = port {
                args.extend(["-p".to_owned(), port.to_string()]);
            }
            let mode = match config.ssl_mode {
                SslMode::Disable => "disable",
                SslMode::Prefer => "prefer",
                SslMode::Require => "require",
                SslMode::VerifyCa => "verify-ca",
                SslMode::VerifyFull => "verify-full",
            };
            env.push(("PGSSLMODE", mode.to_owned()));
            env.push(("PGCLIENTENCODING", "UTF8".to_owned()));
            if let Some(cert) = &config.ssl_root_cert {
                env.push(("PGSSLROOTCERT", cert.display().to_string()));
            }
            env.extend(password.map(|p| ("PGPASSWORD", p)));
        }
        DriverKind::MySql => {
            args.extend(
                [
                    "--table",
                    "--default-character-set=utf8mb4",
                    "-h",
                    host,
                    "-u",
                    config.user.as_str(),
                ]
                .map(str::to_owned),
            );
            if let Some(port) = port {
                args.push(format!("--port={port}"));
            }
            let mode = match config.ssl_mode {
                SslMode::Disable => "DISABLED",
                SslMode::Prefer => "PREFERRED",
                SslMode::Require => "REQUIRED",
                SslMode::VerifyCa => "VERIFY_CA",
                SslMode::VerifyFull => "VERIFY_IDENTITY",
            };
            args.push(format!("--ssl-mode={mode}"));
            if let Some(cert) = &config.ssl_root_cert {
                args.push(format!("--ssl-ca={}", cert.display()));
            }
            if !config.database.is_empty() {
                args.push(config.database.clone());
            }
            env.extend(password.map(|p| ("MYSQL_PWD", p)));
        }
        DriverKind::Sqlite => {
            args.extend(
                ["-bail", "-header", "-column", config.database.as_str()].map(str::to_owned),
            );
        }
    }
    (args, env)
}

/// Runs `script` with the client of the session's database and waits for it to end.
#[tauri::command]
pub async fn run_cli(
    state: State<'_, AppState>,
    session_id: u64,
    script: String,
) -> CommandResult<CliOutput> {
    let config = state.session(session_id)?.config;
    let program = program(config.driver);
    // The tunnel stays open while the client runs.
    let tunnel = match (&config.ssh, config.port.or(config.driver.default_port())) {
        (Some(ssh), Some(port)) => Some(Tunnel::open(ssh, &config.host, port, None).await?),
        _ => None,
    };
    let (host, port) = match &tunnel {
        Some(t) => ("127.0.0.1".to_owned(), Some(t.local_addr().port())),
        None => (config.host.clone(), config.port),
    };
    let (args, env) = invocation(&config, &host, port);
    let mut command = Command::new(program);
    command
        .args(&args)
        .envs(env)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    #[cfg(windows)]
    command.creation_flags(0x0800_0000); // CREATE_NO_WINDOW: no console flashes up.
    let started = Instant::now();
    let mut child = command.spawn().map_err(|e| {
        CommandError::new(
            "cli",
            if e.kind() == std::io::ErrorKind::NotFound {
                format!("{program} was not found: install the client and put it on the PATH")
            } else {
                format!("cannot start {program}: {e}")
            },
        )
    })?;
    if let Some(mut stdin) = child.stdin.take() {
        // Written alongside the reads, so a client that prints a lot cannot block the script;
        // one that exits early closes its input, and its output says why.
        tauri::async_runtime::spawn(async move {
            let _ = stdin.write_all(script.as_bytes()).await;
        });
    }
    let out = child
        .wait_with_output()
        .await
        .map_err(|e| CommandError::new("cli", format!("{program}: {e}")))?;
    drop(tunnel);
    let mut output = String::from_utf8_lossy(&out.stdout).into_owned();
    output.push_str(&String::from_utf8_lossy(&out.stderr));
    Ok(CliOutput {
        command: format!("{program} {}", args.join(" ")),
        status: out.status.code(),
        output,
        elapsed_ms: u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX),
    })
}

#[cfg(test)]
mod tests {
    use platypus_core::Secret;

    use super::*;

    #[test]
    fn passes_the_password_in_the_environment() {
        let mut config = ConnectionConfig::new(DriverKind::Postgres);
        config.user = "app".to_owned();
        config.database = "shop".to_owned();
        config.password = Some(Secret::new("s3cret"));
        let (args, env) = invocation(&config, "db", Some(5433));
        assert!(args.windows(2).any(|w| w == ["-p", "5433"]));
        assert!(!args.iter().any(|a| a.contains("s3cret")));
        assert!(env.contains(&("PGPASSWORD", "s3cret".to_owned())));

        config.driver = DriverKind::MySql;
        let (args, env) = invocation(&config, "db", None);
        assert_eq!(args.last().map(String::as_str), Some("shop"));
        assert!(env.contains(&("MYSQL_PWD", "s3cret".to_owned())));
    }
}
