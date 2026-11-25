//! The bridge process: started with the driver's jars on its class path, written requests on
//! its standard input and read answers (JSON lines) from its standard output.

use std::io::{BufRead as _, BufReader, Read as _, Write as _};
use std::path::Path;
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use platypus_core::{DbError, Error, Result};

use crate::command::Interrupt;
use serde_json::Value;

/// The request that cancels the running statement; the bridge answers nothing to it.
const CANCEL: &str = "0\tCANCEL\n";

/// How much of the bridge's standard error is kept, for the error of a bridge that stops.
const STDERR_KEPT: usize = 4096;

pub struct Bridge {
    child: Child,
    /// Shared with [`Bridge::interrupter`]: a cancel is written while the worker waits.
    stdin: Arc<Mutex<Option<ChildStdin>>>,
    stdout: BufReader<ChildStdout>,
    stderr: Arc<Mutex<String>>,
    next: u64,
}

impl Bridge {
    pub fn start(java: &Path, class_path: &str) -> Result<Self> {
        let mut command = Command::new(java);
        command
            .args(["-Dfile.encoding=UTF-8", "-cp", class_path, "Bridge"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt as _;
            /// No console window for the JVM.
            const CREATE_NO_WINDOW: u32 = 0x0800_0000;
            command.creation_flags(CREATE_NO_WINDOW);
        }
        let mut child = command
            .spawn()
            .map_err(|e| Error::Connect(format!("cannot start {}: {e}", java.display())))?;
        let stdin = child.stdin.take();
        let stdout = child.stdout.take().map(BufReader::new);
        let (Some(stdin), Some(stdout)) = (stdin, stdout) else {
            return Err(Error::Connect("the JDBC bridge has no pipes".to_owned()));
        };
        let stderr = Arc::new(Mutex::new(String::new()));
        if let Some(mut pipe) = child.stderr.take() {
            let kept = Arc::clone(&stderr);
            std::thread::spawn(move || {
                let mut buffer = [0_u8; 1024];
                while let Ok(n) = pipe.read(&mut buffer) {
                    if n == 0 {
                        break;
                    }
                    let mut text = kept
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner);
                    text.push_str(&String::from_utf8_lossy(&buffer[..n]));
                    if text.len() > STDERR_KEPT {
                        let cut = text.len() - STDERR_KEPT;
                        let cut = (cut..text.len())
                            .find(|i| text.is_char_boundary(*i))
                            .unwrap_or(0);
                        text.drain(..cut);
                    }
                }
            });
        }
        Ok(Self {
            child,
            stdin: Arc::new(Mutex::new(Some(stdin))),
            stdout,
            stderr,
            next: 0,
        })
    }

    /// Sends `command` with `args` and returns its id.
    pub fn send(&mut self, command: &str, args: &[&str]) -> Result<u64> {
        self.next += 1;
        let mut line = format!("{}\t{command}", self.next);
        for arg in args {
            line.push('\t');
            line.push_str(&STANDARD.encode(arg));
        }
        line.push('\n');
        self.write(&line)?;
        Ok(self.next)
    }

    /// Asks the bridge to cancel the statement it is running. It answers nothing.
    pub fn cancel(&mut self) {
        let _ = self.write(CANCEL);
    }

    /// What cancels the running statement from another thread than the worker's, which is
    /// waiting for the bridge's answer.
    pub fn interrupter(&self) -> Interrupt {
        let stdin = Arc::clone(&self.stdin);
        Interrupt(Arc::new(move || {
            if let Some(stdin) = stdin
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .as_mut()
            {
                let _ = stdin
                    .write_all(CANCEL.as_bytes())
                    .and_then(|()| stdin.flush());
            }
        }))
    }

    fn write(&mut self, line: &str) -> Result<()> {
        let written = {
            let mut stdin = self
                .stdin
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let stdin = stdin.as_mut().ok_or(Error::Closed)?;
            stdin
                .write_all(line.as_bytes())
                .and_then(|()| stdin.flush())
        };
        written.map_err(|_| self.stopped())
    }

    /// The next answer of the bridge.
    pub fn read(&mut self) -> Result<Value> {
        let mut line = String::new();
        match self.stdout.read_line(&mut line) {
            Ok(n) if n > 0 => serde_json::from_str(&line).map_err(|e| {
                Error::Other(format!("the JDBC bridge sent something unexpected: {e}"))
            }),
            _ => Err(self.stopped()),
        }
    }

    /// The error of a bridge that is gone: what the JVM said before it stopped, when it said.
    fn stopped(&self) -> Error {
        // The standard error may still be on its way: the JVM has just exited.
        std::thread::sleep(Duration::from_millis(100));
        let said = self
            .stderr
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .trim()
            .to_owned();
        if said.is_empty() {
            Error::Closed
        } else {
            Error::Connect(format!("the JDBC bridge stopped: {said}"))
        }
    }
}

impl Drop for Bridge {
    /// Closing the input ends the bridge, which closes the database connection; a bridge
    /// still there after a moment is killed.
    fn drop(&mut self) {
        drop(
            self.stdin
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .take(),
        );
        let until = Instant::now() + Duration::from_secs(2);
        while Instant::now() < until {
            if matches!(self.child.try_wait(), Ok(Some(_))) {
                return;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// The database error of an answer with an `error`, or what the bridge says is wrong.
pub fn error_of(answer: &Value) -> Option<Error> {
    let error = answer.get("error")?;
    let message = error["message"]
        .as_str()
        .unwrap_or("unknown error")
        .to_owned();
    Some(Error::Database(DbError {
        code: error["code"].as_str().map(str::to_owned),
        message,
        detail: None,
        hint: None,
        position: None,
    }))
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn an_error_answer_is_a_database_error_with_its_sqlstate() {
        let answer =
            json!({"id": 5, "error": {"code": "42S02", "vendor": 1, "message": "no table"}});
        let Some(Error::Database(e)) = error_of(&answer) else {
            panic!("not a database error");
        };
        assert_eq!(e.code.as_deref(), Some("42S02"));
        assert_eq!(e.message, "no table");
        assert!(error_of(&json!({"id": 5, "ok": true})).is_none());
    }
}
