//! Shell commands around a connection, e.g. bringing a VPN up and down: one before it
//! connects, which must end well for the connection to be made, and one after it closes,
//! which runs on its own. They are options of the connection, run by the system's shell
//! (`cmd /C` on Windows, `sh -c` elsewhere), and never reach the driver.

use std::process::Stdio;
use std::time::Duration;

use platypus_core::{ConnectionConfig, Error, Result};
use tokio::io::AsyncReadExt;

/// The option with the command run before connecting.
pub const BEFORE_CONNECT: &str = "before_connect";
/// The option with the command run after the connection closes.
pub const AFTER_DISCONNECT: &str = "after_disconnect";

/// How long the command before connecting may run: bringing a VPN up takes seconds.
const BEFORE_TIMEOUT: Duration = Duration::from_secs(60);

/// The two commands of `config`, taken out of its options.
pub(crate) fn take(config: &mut ConnectionConfig) -> (Option<String>, Option<String>) {
    let mut take = |key: &str| config.options.remove(key).filter(|c| !c.trim().is_empty());
    (take(BEFORE_CONNECT), take(AFTER_DISCONNECT))
}

fn shell(command: &str) -> std::process::Command {
    let (program, flags): (&str, &[&str]) = if cfg!(windows) {
        ("cmd", &["/D", "/C"])
    } else {
        ("sh", &["-c"])
    };
    let mut shell = std::process::Command::new(program);
    shell.args(flags).arg(command).stdin(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt as _;
        shell.creation_flags(0x0800_0000); // CREATE_NO_WINDOW: no console flashes up.
    }
    shell
}

/// Runs the command before connecting and waits for its end: an exit other than 0, or no
/// end within a minute, is an error that quotes the first line it wrote on stderr.
pub(crate) async fn before(command: &str) -> Result<()> {
    let failed = |message: String| Error::Connect(format!("command before connecting: {message}"));
    let mut child = tokio::process::Command::from(shell(command))
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .map_err(|e| failed(e.to_string()))?;
    // Read apart from the wait: a process the command leaves running (`vpn &`) may keep
    // stderr open after the command itself has ended.
    let mut stderr = child.stderr.take();
    let reading = tokio::spawn(async move {
        let mut text = String::new();
        if let Some(stderr) = &mut stderr {
            let _ = stderr.take(64 * 1024).read_to_string(&mut text).await;
        }
        text
    });
    let Ok(status) = tokio::time::timeout(BEFORE_TIMEOUT, child.wait()).await else {
        let _ = child.kill().await;
        return Err(failed(format!(
            "no end in {} seconds",
            BEFORE_TIMEOUT.as_secs()
        )));
    };
    let status = status.map_err(|e| failed(e.to_string()))?;
    if status.success() {
        reading.abort();
        return Ok(());
    }
    let text = tokio::time::timeout(Duration::from_millis(200), reading)
        .await
        .ok()
        .and_then(Result::ok)
        .unwrap_or_default();
    Err(failed(
        match text.lines().map(str::trim).find(|l| !l.is_empty()) {
            Some(line) => format!("{status}: {line}"),
            None => status.to_string(),
        },
    ))
}

/// Starts the command after the connection closed, without waiting for it; how it ends is
/// not reported, as nothing waits for it any more. Runs outside the async runtime: a
/// connection may close as the app quits.
pub(crate) fn after(command: &str) {
    let spawned = shell(command)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn();
    if let Ok(mut child) = spawned {
        // Reaped when it ends, on a thread of its own.
        std::thread::spawn(move || child.wait());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Whether `file` appears within 5 s, written by a command that runs on its own.
    #[cfg(unix)]
    fn appears(file: &std::path::Path) -> bool {
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        while !file.exists() && std::time::Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(50));
        }
        file.exists()
    }

    #[tokio::test]
    async fn a_command_that_ends_well_lets_the_connection_go_on() {
        before("exit 0").await.unwrap();
    }

    #[tokio::test]
    async fn a_failed_command_says_why() {
        let print = if cfg!(windows) {
            "echo VPN refused 1>&2 && exit 3"
        } else {
            "echo VPN refused >&2; exit 3"
        };
        let error = before(print).await.unwrap_err().to_string();
        assert!(error.contains("command before connecting"), "{error}");
        assert!(error.contains("VPN refused"), "{error}");
        assert!(error.contains('3'), "{error}");
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn a_process_left_running_does_not_hold_the_connection() {
        let started = std::time::Instant::now();
        before("sleep 30 &").await.unwrap();
        assert!(started.elapsed() < Duration::from_secs(5));
    }

    #[test]
    fn the_commands_leave_the_driver_options() {
        let mut config = ConnectionConfig::new(platypus_core::DriverKind::Postgres);
        config
            .options
            .insert(BEFORE_CONNECT.to_owned(), "vpn up".to_owned());
        config
            .options
            .insert(AFTER_DISCONNECT.to_owned(), "  ".to_owned());
        config
            .options
            .insert("pooler".to_owned(), "transaction".to_owned());
        let (before, after) = take(&mut config);
        assert_eq!(before.as_deref(), Some("vpn up"));
        assert_eq!(after, None, "a blank command is none");
        assert_eq!(config.options.len(), 1);
    }

    #[tokio::test]
    async fn a_failed_command_before_stops_the_connection() {
        let mut config = ConnectionConfig::new(platypus_core::DriverKind::Postgres);
        config
            .options
            .insert(BEFORE_CONNECT.to_owned(), "exit 1".to_owned());
        let called = std::sync::atomic::AtomicBool::new(false);
        let result = crate::connect(&config, None, |_| async {
            called.store(true, std::sync::atomic::Ordering::SeqCst);
            Err(Error::Connect("unreachable".to_owned()))
        })
        .await;
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("command before connecting")
        );
        assert!(
            !called.load(std::sync::atomic::Ordering::SeqCst),
            "the driver is not asked"
        );
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn the_command_after_runs_when_the_connection_fails() {
        let dir = tempfile::tempdir().unwrap();
        let mark = dir.path().join("down");
        let mut config = ConnectionConfig::new(platypus_core::DriverKind::Postgres);
        config
            .options
            .insert(BEFORE_CONNECT.to_owned(), "true".to_owned());
        config.options.insert(
            AFTER_DISCONNECT.to_owned(),
            format!("touch '{}'", mark.display()),
        );
        let result = crate::connect(&config, None, |c| async move {
            // The driver never sees the commands.
            assert!(c.options.is_empty());
            Err(Error::Connect("refused".to_owned()))
        })
        .await;
        assert_eq!(
            result.unwrap_err().to_string(),
            "connection failed: refused"
        );
        assert!(appears(&mark), "the command after ran");
    }

    #[cfg(unix)]
    #[test]
    fn the_command_after_runs_on_its_own() {
        let dir = tempfile::tempdir().unwrap();
        let mark = dir.path().join("down");
        after(&format!("touch '{}'", mark.display()));
        assert!(appears(&mark));
    }
}
