//! Shell commands around connections, e.g. bringing a VPN up and down: one before the
//! first connection that has it, which must end well for the connection to be made, and one
//! after the last of them closes, which runs on its own. Connections with the same two
//! commands share them, as they share the VPN: closing one of several sessions, or a test
//! next to an open editor, leaves it up. They are options of the connection, run by the
//! system's shell (`cmd /C` on Windows, `sh -c` elsewhere), and never reach the driver.

use std::collections::HashMap;
use std::process::Stdio;
use std::sync::{LazyLock, Mutex, PoisonError};
use std::time::Duration;

pub use platypus_core::{AFTER_DISCONNECT, BEFORE_CONNECT};
use platypus_core::{ConnectionConfig, Error, Result};

/// How long the command before connecting may run: bringing a VPN up takes seconds.
const BEFORE_TIMEOUT: Duration = Duration::from_secs(60);

/// A connection's two commands.
type Commands = (Option<String>, Option<String>);

/// The connections open with each pair of commands.
static USERS: LazyLock<Mutex<HashMap<Commands, usize>>> = LazyLock::new(Mutex::default);

/// Held while a command before connecting runs: a second connection waits for it, then
/// finds it done.
static STARTING: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

/// The two commands of `config`, taken out of its options.
pub(crate) fn take(config: &mut ConnectionConfig) -> Commands {
    let mut take = |key: &str| config.options.remove(key).filter(|c| !c.trim().is_empty());
    (take(BEFORE_CONNECT), take(AFTER_DISCONNECT))
}

/// A connection's use of its commands: the last one to go runs the command after.
#[derive(Debug)]
pub(crate) struct Use(Commands);

impl Drop for Use {
    fn drop(&mut self) {
        let mut users = USERS.lock().unwrap_or_else(PoisonError::into_inner);
        let Some(count) = users.get_mut(&self.0) else {
            return;
        };
        *count -= 1;
        if *count == 0 {
            users.remove(&self.0);
            if let Some(command) = &self.0.1 {
                after(command);
            }
        }
    }
}

/// Takes `config`'s commands out of its options and, when no other connection with them is
/// open, runs the one before connecting. The returned use lasts as long as the connection.
pub(crate) async fn acquire(config: &mut ConnectionConfig) -> Result<Option<Use>> {
    let commands = take(config);
    if commands == (None, None) {
        return Ok(None);
    }
    let _starting = STARTING.lock().await;
    let first = !USERS
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .contains_key(&commands);
    if first && let Some(command) = &commands.0 {
        before(command).await?;
    }
    *USERS
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .entry(commands.clone())
        .or_default() += 1;
    Ok(Some(Use(commands)))
}

fn shell(command: &str) -> std::process::Command {
    #[cfg(windows)]
    let shell = {
        use std::os::windows::process::CommandExt as _;
        let mut shell = std::process::Command::new("cmd");
        // As written, in quotes `cmd /S` takes off: `arg` would escape its quotes as `\"`,
        // which `cmd` does not read, and `rasdial "Office VPN"` would break.
        shell
            .args(["/D", "/S", "/C"])
            .raw_arg(format!("\"{command}\""))
            .creation_flags(0x0800_0000); // CREATE_NO_WINDOW: no console flashes up.
        shell
    };
    #[cfg(not(windows))]
    let shell = {
        let mut shell = std::process::Command::new("sh");
        shell.arg("-c").arg(command);
        shell
    };
    let mut shell = shell;
    shell.stdin(Stdio::null());
    shell
}

/// Runs the command before connecting and waits for its end: an exit other than 0, or no
/// end within a minute, is an error that quotes the first line it wrote on stderr.
async fn before(command: &str) -> Result<()> {
    let failed = |message: String| Error::Connect(format!("command before connecting: {message}"));
    let mut child = tokio::process::Command::from(shell(command))
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .map_err(|e| failed(e.to_string()))?;
    let stderr = crate::cli::Stderr::read(child.stderr.take());
    let Ok(status) = tokio::time::timeout(BEFORE_TIMEOUT, child.wait()).await else {
        let _ = child.kill().await;
        return Err(failed(format!(
            "no end in {} seconds",
            BEFORE_TIMEOUT.as_secs()
        )));
    };
    let status = status.map_err(|e| failed(e.to_string()))?;
    if status.success() {
        // The reading goes on, on its own.
        return Ok(());
    }
    Err(failed(stderr.explain(status).await))
}

/// Starts the command after the connection closed, without waiting for it; how it ends is
/// not reported, as nothing waits for it any more. Runs outside the async runtime: a
/// connection may close as the app quits.
fn after(command: &str) {
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
    #[tokio::test]
    async fn a_background_process_can_still_write_on_stderr() {
        let dir = tempfile::tempdir().unwrap();
        let mark = dir.path().join("alive");
        // Writes on stderr after the command ended: with the pipe closed, SIGPIPE kills it.
        before(&format!(
            "(sleep 0.3; echo log >&2; echo log >&2; touch '{}') &",
            mark.display()
        ))
        .await
        .unwrap();
        assert!(appears(&mark), "the process lived on");
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn connections_with_the_same_commands_share_them() {
        let dir = tempfile::tempdir().unwrap();
        let ups = dir.path().join("ups");
        let down = dir.path().join("down");
        let mut config = ConnectionConfig::new(platypus_core::DriverKind::Postgres);
        config.options.insert(
            BEFORE_CONNECT.to_owned(),
            format!("echo up >> '{}'", ups.display()),
        );
        config.options.insert(
            AFTER_DISCONNECT.to_owned(),
            format!("touch '{}'", down.display()),
        );
        let first = acquire(&mut config.clone()).await.unwrap();
        let second = acquire(&mut config.clone()).await.unwrap();
        assert_eq!(std::fs::read_to_string(&ups).unwrap(), "up\n", "run once");
        drop(first);
        std::thread::sleep(Duration::from_millis(300));
        assert!(!down.exists(), "the other connection still uses it");
        drop(second);
        assert!(appears(&down), "the last one ran the command after");
        // Once all are gone, the next connection runs the command before again.
        let again = acquire(&mut config).await.unwrap();
        assert_eq!(std::fs::read_to_string(&ups).unwrap(), "up\nup\n");
        drop(again);
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
