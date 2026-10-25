//! Passwords that are not kept by PlatypusDB: looked up in `.pgpass`, or printed by a
//! shell command such as a password manager's CLI.

use std::io::Read as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use platypus_core::Secret;

use crate::{Result, SavedConnection, StoreError};

/// How long a password command may run.
const COMMAND_TIMEOUT: Duration = Duration::from_secs(30);

/// Where libpq reads `.pgpass`: `PGPASSFILE`, else `%APPDATA%\postgresql\pgpass.conf` on
/// Windows and `~/.pgpass` elsewhere.
#[must_use]
pub fn pgpass_path() -> Option<PathBuf> {
    if let Some(path) = std::env::var_os("PGPASSFILE") {
        return Some(path.into());
    }
    if cfg!(windows) {
        std::env::var_os("APPDATA").map(|d| Path::new(&d).join("postgresql").join("pgpass.conf"))
    } else {
        std::env::var_os("HOME").map(|d| Path::new(&d).join(".pgpass"))
    }
}

/// The password of the first `.pgpass` line matching the server, or `None` when no line
/// does or the file does not exist.
pub fn pgpass_password(
    path: &Path,
    host: &str,
    port: u16,
    database: &str,
    user: &str,
) -> Result<Option<Secret>> {
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(source) => {
            return Err(StoreError::Io {
                path: path.to_owned(),
                source,
            });
        }
    };
    let port = port.to_string();
    let wanted = [host, port.as_str(), database, user];
    Ok(text.lines().find_map(|line| {
        let fields = pgpass_fields(line)?;
        let matches = fields[..4]
            .iter()
            .zip(wanted)
            .all(|(field, value)| field == "*" || field == value);
        let [.., password] = fields;
        matches.then(|| Secret::new(password))
    }))
}

/// The five fields of a `.pgpass` line, with `\:` and `\\` unescaped; `None` for comments
/// and malformed lines.
fn pgpass_fields(line: &str) -> Option<[String; 5]> {
    if line.trim_start().starts_with('#') {
        return None;
    }
    let mut fields: Vec<String> = vec![String::new()];
    let mut chars = line.chars();
    while let Some(c) = chars.next() {
        match c {
            '\\' => fields.last_mut()?.push(chars.next().unwrap_or('\\')),
            // The password is the rest of the line, colons included.
            ':' if fields.len() < 5 => fields.push(String::new()),
            c => fields.last_mut()?.push(c),
        }
    }
    fields.try_into().ok()
}

/// The password from the connection's command or `.pgpass`, if it uses one of them; `None`
/// when it uses neither. Runs the command, so call it off the async runtime.
pub fn external_password(saved: &SavedConnection) -> Result<Option<Secret>> {
    if let Some(command) = &saved.password_command {
        return command_password(command).map(Some);
    }
    if !saved.pgpass {
        return Ok(None);
    }
    let path = pgpass_path()
        .ok_or_else(|| StoreError::Invalid("no .pgpass location: set PGPASSFILE".to_owned()))?;
    let port = saved
        .port
        .or(saved.driver.default_port())
        .unwrap_or_default();
    pgpass_password(&path, &saved.host, port, &saved.database, &saved.user)?
        .map(Some)
        .ok_or_else(|| {
            StoreError::Invalid(format!(
                "no line of {} matches {}:{port}:{}:{}",
                path.display(),
                saved.host,
                saved.database,
                saved.user
            ))
        })
}

/// Runs `command` in the system shell and returns the first line it prints.
pub fn command_password(command: &str) -> Result<Secret> {
    let failed = |message: String| StoreError::Invalid(format!("password command: {message}"));
    let (shell, flags): (_, &[&str]) = if cfg!(windows) {
        ("cmd", &["/D", "/C"])
    } else {
        ("sh", &["-c"])
    };
    let mut child = Command::new(shell)
        .args(flags)
        .arg(command)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| failed(e.to_string()))?;
    // Read while it runs: a full pipe would block the command.
    let (Some(mut stdout), Some(mut stderr)) = (child.stdout.take(), child.stderr.take()) else {
        return Err(failed("no output".to_owned()));
    };
    let out = std::thread::spawn(move || {
        let mut s = String::new();
        stdout.read_to_string(&mut s).map(|_| s)
    });
    let err = std::thread::spawn(move || {
        let mut s = String::new();
        let _ = stderr.read_to_string(&mut s);
        s
    });
    let start = Instant::now();
    let status = loop {
        if let Some(status) = child.try_wait().map_err(|e| failed(e.to_string()))? {
            break status;
        }
        if start.elapsed() > COMMAND_TIMEOUT {
            let _ = child.kill();
            return Err(failed(format!(
                "no answer in {} seconds",
                COMMAND_TIMEOUT.as_secs()
            )));
        }
        std::thread::sleep(Duration::from_millis(20));
    };
    let stdout = out
        .join()
        .map_err(|_| failed("output lost".to_owned()))?
        .map_err(|e| failed(e.to_string()))?;
    if !status.success() {
        let stderr = err.join().unwrap_or_default();
        return Err(match stderr.lines().next().map(str::trim) {
            Some(detail) if !detail.is_empty() => failed(format!("{status}: {detail}")),
            _ => failed(status.to_string()),
        });
    }
    let password = stdout.lines().next().unwrap_or("").trim_end_matches('\r');
    if password.is_empty() {
        return Err(failed("printed nothing".to_owned()));
    }
    Ok(Secret::new(password))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_pgpass_like_libpq() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("pgpass.conf");
        std::fs::write(
            &path,
            "# comment\n\
             db.example.com:5432:sales:ada:first\n\
             *:5432:*:ada:with\\:colon:and:more\n\
             localhost:*:*:*:back\\\\slash\n\
             malformed line\n",
        )
        .unwrap();
        let find = |host, port, db, user| {
            pgpass_password(&path, host, port, db, user)
                .unwrap()
                .map(|s| s.expose().to_owned())
        };
        assert_eq!(
            find("db.example.com", 5432, "sales", "ada").as_deref(),
            Some("first")
        );
        assert_eq!(
            find("other", 5432, "x", "ada").as_deref(),
            Some("with:colon:and:more")
        );
        assert_eq!(
            find("localhost", 6000, "x", "bob").as_deref(),
            Some("back\\slash")
        );
        assert_eq!(find("other", 6000, "x", "bob"), None);
        assert_eq!(
            pgpass_password(&dir.path().join("missing"), "h", 1, "d", "u").unwrap(),
            None
        );
    }

    #[test]
    fn external_passwords_come_from_the_command_first() {
        let mut saved = SavedConnection::new("x", platypus_core::DriverKind::Postgres);
        assert_eq!(external_password(&saved).unwrap(), None);
        saved.pgpass = true;
        saved.password_command = Some("echo from-command".to_owned());
        assert_eq!(
            external_password(&saved).unwrap().unwrap().expose(),
            "from-command"
        );
    }

    #[test]
    fn runs_password_commands() {
        assert_eq!(command_password("echo s3cret").unwrap().expose(), "s3cret");
        assert!(command_password("exit 3").is_err());
        let empty = if cfg!(windows) { "echo." } else { "echo" };
        assert!(command_password(empty).is_err());
    }
}
