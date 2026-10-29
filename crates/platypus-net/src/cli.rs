//! Running the CLIs of the clouds (`aws`, `gcloud`, `az`, `kubectl`): without a console window,
//! with what they write on stderr kept for the error.

use std::process::Stdio;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use platypus_core::{Error, Result};
use tokio::io::{AsyncRead, AsyncReadExt};
use tokio::task::JoinHandle;

/// What is kept of what a command writes on stderr.
const STDERR_KEPT: usize = 64 * 1024;

/// `program` with `args`, run directly (not by a shell), ended when dropped.
pub(crate) fn command(program: &str, args: &[String]) -> tokio::process::Command {
    let mut command = tokio::process::Command::new(program);
    command
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    #[cfg(windows)]
    command.creation_flags(0x0800_0000); // CREATE_NO_WINDOW: no console flashes up.
    command
}

/// The error of `program` failing: `message`, after its name.
pub(crate) fn failed(program: &str, message: impl std::fmt::Display) -> Error {
    Error::Connect(format!("{program}: {message}"))
}

/// The error of `program` not starting: a CLI missing from the `PATH` says so.
pub(crate) fn not_started(program: &str, e: &std::io::Error) -> Error {
    if e.kind() == std::io::ErrorKind::NotFound {
        failed(program, "not found: install its CLI and put it on the PATH")
    } else {
        failed(program, e)
    }
}

/// What a command writes on stderr, read apart from its wait and to the end: a process it
/// leaves running keeps stderr open after it has ended, and would die of a closed pipe the
/// next time it writes there.
pub(crate) struct Stderr {
    text: Arc<Mutex<String>>,
    reading: JoinHandle<()>,
}

impl Stderr {
    pub(crate) fn read(stream: Option<impl AsyncRead + Unpin + Send + 'static>) -> Self {
        let text = Arc::new(Mutex::new(String::new()));
        let reading = tokio::spawn({
            let text = Arc::clone(&text);
            async move {
                let Some(mut stream) = stream else { return };
                let mut chunk = [0u8; 4096];
                while let Ok(n @ 1..) = stream.read(&mut chunk).await {
                    let mut text = text.lock().unwrap_or_else(PoisonError::into_inner);
                    if text.len() < STDERR_KEPT {
                        text.push_str(&String::from_utf8_lossy(&chunk[..n]));
                    }
                }
            }
        });
        Self { text, reading }
    }

    /// The first line written, given a moment to arrive: `status` after it, or alone.
    pub(crate) async fn explain(mut self, status: impl std::fmt::Display) -> String {
        let _ = tokio::time::timeout(Duration::from_millis(200), &mut self.reading).await;
        let text = self.text.lock().unwrap_or_else(PoisonError::into_inner);
        match text.lines().map(str::trim).find(|l| !l.is_empty()) {
            Some(line) => format!("{status}: {line}"),
            None => status.to_string(),
        }
    }
}

/// What `program` prints on stdout, once it ends well within `timeout`.
pub(crate) async fn output(program: &str, args: &[String], timeout: Duration) -> Result<String> {
    let mut child = command(program, args)
        .stdout(Stdio::piped())
        .spawn()
        .map_err(|e| not_started(program, &e))?;
    let stderr = Stderr::read(child.stderr.take());
    let mut stdout = child.stdout.take();
    let reading = tokio::spawn(async move {
        let mut text = String::new();
        if let Some(stdout) = &mut stdout {
            let _ = stdout.read_to_string(&mut text).await;
        }
        text
    });
    let Ok(status) = tokio::time::timeout(timeout, child.wait()).await else {
        let _ = child.kill().await;
        return Err(failed(
            program,
            format!("no answer in {} seconds", timeout.as_secs()),
        ));
    };
    let status = status.map_err(|e| failed(program, e))?;
    if !status.success() {
        return Err(failed(program, stderr.explain(status).await));
    }
    reading.await.map_err(|e| failed(program, e))
}
