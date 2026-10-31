//! Environment variables in the fields of a connection: `${env:DB_PASSWORD}`, read from the
//! process's environment or from a `.env` file, so that a saved connection holds no secret
//! of a project that keeps its own (`DATABASE_URL`, `PGPASSWORD`, …).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::{ConnectionConfig, Error, Result, Secret, SshAuth, SshConfig};

/// The option naming the `.env` file of a connection.
pub const ENV_FILE: &str = "env_file";

/// Where `${env:NAME}` looks a name up: the process's environment first, then the
/// connection's `.env` file.
#[derive(Debug, Default, Clone)]
pub struct EnvVars {
    file: BTreeMap<String, String>,
    /// Stands for the process's environment in tests.
    process: Option<BTreeMap<String, String>>,
}

impl EnvVars {
    /// The process's environment, and the variables of `file` when given: an absolute path,
    /// or one starting with `~` for the home folder.
    ///
    /// # Errors
    /// When the path is relative or the file cannot be read.
    pub fn load(file: Option<&str>) -> Result<Self> {
        let file = match file.map(str::trim).filter(|f| !f.is_empty()) {
            Some(path) => {
                let path = home(path);
                if path.is_relative() {
                    return Err(Error::Config(format!(
                        "the .env file {} must be an absolute path, or start with ~",
                        path.display()
                    )));
                }
                let text = std::fs::read_to_string(&path).map_err(|e| {
                    Error::Config(format!("cannot read the .env file {}: {e}", path.display()))
                })?;
                parse_dotenv(&text)
            }
            None => BTreeMap::new(),
        };
        Ok(Self {
            file,
            process: None,
        })
    }

    /// Variables of `file` only, with `process` in place of the process's environment.
    #[must_use]
    pub fn with(process: BTreeMap<String, String>, file: BTreeMap<String, String>) -> Self {
        Self {
            file,
            process: Some(process),
        }
    }

    #[must_use]
    pub fn get(&self, name: &str) -> Option<String> {
        let process = match &self.process {
            Some(process) => process.get(name).cloned(),
            None => std::env::var(name).ok(),
        };
        process.or_else(|| self.file.get(name).cloned())
    }
}

fn home(path: &str) -> PathBuf {
    let rest = if path == "~" {
        Some("")
    } else {
        path.strip_prefix("~/").or_else(|| path.strip_prefix("~\\"))
    };
    match (
        rest,
        std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")),
    ) {
        (Some(rest), Some(home)) => Path::new(&home).join(rest),
        _ => PathBuf::from(path),
    }
}

/// The variables of a `.env` file: `NAME=value` lines, `export` before them allowed,
/// values in single quotes as they are, in double quotes with `\n`, `\t`, `\"` and `\\`,
/// unquoted up to a ` #` comment. Other lines are skipped.
#[must_use]
pub fn parse_dotenv(text: &str) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    // A byte order mark from a Windows editor would glue itself to the first name.
    for line in text.trim_start_matches('\u{feff}').lines() {
        let line = line.trim();
        let line = line.strip_prefix("export ").unwrap_or(line).trim_start();
        if line.starts_with('#') {
            continue;
        }
        let Some((name, value)) = line.split_once('=') else {
            continue;
        };
        let name = name.trim();
        if name.is_empty() || !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
            continue;
        }
        let value = value.trim();
        let value = if let Some(inner) = value
            .strip_prefix('\'')
            .and_then(|v| v.split_once('\''))
            .map(|(inner, _)| inner)
        {
            inner.to_owned()
        } else if let Some(inner) = value.strip_prefix('"').and_then(double_quoted) {
            inner
        } else {
            value
                .split_once(" #")
                .map_or(value, |(value, _)| value)
                .trim_end()
                .to_owned()
        };
        out.insert(name.to_owned(), value);
    }
    out
}

/// The value of a double-quoted string up to its closing quote, escapes resolved; `None`
/// when it is not closed.
fn double_quoted(text: &str) -> Option<String> {
    let mut out = String::new();
    let mut chars = text.chars();
    while let Some(c) = chars.next() {
        match c {
            '"' => return Some(out),
            '\\' => match chars.next()? {
                'n' => out.push('\n'),
                't' => out.push('\t'),
                'r' => out.push('\r'),
                other @ ('"' | '\\') => out.push(other),
                other => {
                    out.push('\\');
                    out.push(other);
                }
            },
            c => out.push(c),
        }
    }
    None
}

/// `text` with each `${env:NAME}` replaced by the variable's value; `$${` is a plain `${`.
///
/// # Errors
/// When a variable is not set, or a reference is not closed.
pub fn expand(text: &str, vars: &EnvVars) -> Result<String> {
    if !text.contains("${") {
        return Ok(text.to_owned());
    }
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(at) = rest.find("${") {
        // `$${`: the reference written out.
        if rest[..at].ends_with('$') {
            out.push_str(&rest[..at - 1]);
            out.push_str("${");
            rest = &rest[at + 2..];
            continue;
        }
        out.push_str(&rest[..at]);
        let inner = &rest[at + 2..];
        // The messages name no part of the text: it may be a password.
        let end = inner
            .find('}')
            .ok_or_else(|| Error::Config("a `${` is not closed by `}`".to_owned()))?;
        let reference = &inner[..end];
        let Some(name) = reference.strip_prefix("env:").map(str::trim) else {
            return Err(Error::Config(
                "only environment variables are read, as `${env:NAME}`".to_owned(),
            ));
        };
        let value = vars
            .get(name)
            .ok_or_else(|| Error::Config(format!("the environment variable {name} is not set")))?;
        out.push_str(&value);
        rest = &inner[end + 1..];
    }
    out.push_str(rest);
    Ok(out)
}

/// `text` with each `${…}` reference replaced by `x`: a stand-in to check a field, such as
/// the host, before its variables are read.
#[must_use]
pub fn without_references(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(at) = rest.find("${") {
        out.push_str(&rest[..at]);
        out.push('x');
        rest = rest[at..].find('}').map_or("", |end| &rest[at + end + 1..]);
    }
    out.push_str(rest);
    out
}

impl ConnectionConfig {
    /// The settings with the environment variables of their text fields, password and SSH
    /// included, replaced. Reads the `.env` file of the `env_file` option, and drops the
    /// option: it is not a setting of the driver.
    ///
    /// # Errors
    /// When a variable is not set or the `.env` file cannot be read.
    pub fn with_env(mut self) -> Result<Self> {
        let vars = EnvVars::load(self.options.remove(ENV_FILE).as_deref())?;
        self.with_variables(&vars)
    }

    /// As [`with_env`](Self::with_env), with the variables of `vars`.
    ///
    /// # Errors
    /// When a variable is not set.
    pub fn with_variables(mut self, vars: &EnvVars) -> Result<Self> {
        expand_text(&mut self.host, vars)?;
        expand_text(&mut self.database, vars)?;
        expand_text(&mut self.user, vars)?;
        if let Some(password) = &mut self.password {
            expand_secret(password, vars)?;
        }
        for path in [
            &mut self.ssl_root_cert,
            &mut self.ssl_client_cert,
            &mut self.ssl_client_key,
        ]
        .into_iter()
        .flatten()
        {
            expand_path(path, vars)?;
        }
        for (key, value) in &mut self.options {
            if ![ENV_FILE, crate::BEFORE_CONNECT, crate::AFTER_DISCONNECT].contains(&key.as_str()) {
                expand_text(value, vars)?;
            }
        }
        if let Some(schema) = &mut self.init.schema {
            expand_text(schema, vars)?;
        }
        if let Some(ssh) = &mut self.ssh {
            ssh_with_variables(ssh, vars)?;
        }
        Ok(self)
    }
}

fn ssh_with_variables(ssh: &mut SshConfig, vars: &EnvVars) -> Result<()> {
    expand_text(&mut ssh.host, vars)?;
    expand_text(&mut ssh.user, vars)?;
    match &mut ssh.auth {
        SshAuth::Password(secret) => expand_secret(secret, vars)?,
        SshAuth::Key { path, passphrase } => {
            expand_path(path, vars)?;
            if let Some(passphrase) = passphrase {
                expand_secret(passphrase, vars)?;
            }
        }
        SshAuth::Agent => {}
    }
    if let Some(jump) = &mut ssh.jump {
        ssh_with_variables(jump, vars)?;
    }
    Ok(())
}

fn expand_text(text: &mut String, vars: &EnvVars) -> Result<()> {
    *text = expand(text, vars)?;
    Ok(())
}

/// A secret is read from the environment only when it is a whole `${env:NAME}`: any other
/// one, `${` or `$${` included, is the password as written.
fn expand_secret(secret: &mut Secret, vars: &EnvVars) -> Result<()> {
    let name = secret
        .expose()
        .strip_prefix("${env:")
        .and_then(|rest| rest.strip_suffix('}'))
        .filter(|name| !name.contains('}'))
        .map(|name| name.trim().to_owned());
    if let Some(name) = name {
        let value = vars
            .get(&name)
            .ok_or_else(|| Error::Config(format!("the environment variable {name} is not set")))?;
        *secret = Secret::new(value);
    }
    Ok(())
}

fn expand_path(path: &mut PathBuf, vars: &EnvVars) -> Result<()> {
    *path = PathBuf::from(expand(&path.to_string_lossy(), vars)?);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::DriverKind;

    fn vars(process: &[(&str, &str)], file: &[(&str, &str)]) -> EnvVars {
        let map = |l: &[(&str, &str)]| {
            l.iter()
                .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
                .collect()
        };
        EnvVars::with(map(process), map(file))
    }

    #[test]
    fn dotenv_with_byte_order_mark() {
        let parsed = parse_dotenv("\u{feff}FIRST=1\nSECOND=2\n");
        assert_eq!(parsed.get("FIRST").map(String::as_str), Some("1"));
        assert_eq!(parsed.len(), 2);
    }

    #[test]
    fn expands_references() {
        let v = vars(
            &[("HOST", "db.local"), ("SHARED", "process")],
            &[("SHARED", "file"), ("PW", "s3cret")],
        );
        assert_eq!(expand("${env:HOST}:5432", &v).unwrap(), "db.local:5432");
        assert_eq!(
            expand("${env:SHARED}", &v).unwrap(),
            "process",
            "the process wins"
        );
        assert_eq!(expand("${env:PW}", &v).unwrap(), "s3cret");
        assert_eq!(expand("plain", &v).unwrap(), "plain");
        assert_eq!(expand("cost $${env:HOST}", &v).unwrap(), "cost ${env:HOST}");
        assert!(
            expand("${env:MISSING}", &v)
                .unwrap_err()
                .to_string()
                .contains("MISSING is not set")
        );
        assert!(expand("${env:HOST", &v).is_err());
        assert!(expand("${HOST}", &v).is_err());
    }

    #[test]
    fn references_are_replaced_by_a_stand_in() {
        assert_eq!(without_references("${env:HOST}.example"), "x.example");
        assert_eq!(without_references("a${env:A}b${env:B}"), "axbx");
        assert_eq!(without_references("db@${env:H}"), "db@x");
        assert_eq!(without_references("open${env"), "openx");
        assert_eq!(without_references("plain"), "plain");
    }

    #[test]
    fn errors_do_not_show_the_text() {
        let v = vars(&[], &[]);
        for text in ["pa${ss", "pa${secret}"] {
            let error = expand(text, &v).unwrap_err().to_string();
            assert!(
                !error.contains("ss") && !error.contains("secret"),
                "{error}"
            );
        }
    }

    #[test]
    fn a_password_is_read_only_when_it_is_a_whole_reference() {
        let v = vars(&[("PW", "s3cret")], &[]);
        for (written, expected) in [
            ("${env:PW}", "s3cret"),
            ("a${b", "a${b"),
            ("a$${env:PW}", "a$${env:PW}"),
            ("x${env:PW}", "x${env:PW}"),
            ("${env:PW}}", "${env:PW}}"),
        ] {
            let mut secret = Secret::new(written);
            expand_secret(&mut secret, &v).unwrap();
            assert_eq!(secret.expose(), expected, "{written}");
        }
        let mut missing = Secret::new("${env:NOPE}");
        assert!(expand_secret(&mut missing, &v).is_err());
    }

    #[test]
    fn reads_dotenv_files() {
        let parsed = parse_dotenv(
            "# a comment\nexport DB_HOST=localhost\nDB_PASSWORD='p#ss w$rd'\nNOTE=\"two\\nlines\"\nPORT=5432 # the port\nnot a line\n=x\n",
        );
        assert_eq!(parsed.get("DB_HOST").map(String::as_str), Some("localhost"));
        assert_eq!(
            parsed.get("DB_PASSWORD").map(String::as_str),
            Some("p#ss w$rd")
        );
        assert_eq!(parsed.get("NOTE").map(String::as_str), Some("two\nlines"));
        assert_eq!(parsed.get("PORT").map(String::as_str), Some("5432"));
        assert_eq!(parsed.len(), 4);
    }

    #[test]
    fn reads_quoted_dotenv_values() {
        let parsed =
            parse_dotenv("A='one' # it's a comment\nB=\"a\\\\nb\\tc\\\"d\" # \"x\"\nC=\"open\n");
        assert_eq!(parsed.get("A").map(String::as_str), Some("one"));
        assert_eq!(parsed.get("B").map(String::as_str), Some("a\\nb\tc\"d"));
        assert_eq!(parsed.get("C").map(String::as_str), Some("\"open"));
    }

    #[test]
    fn expands_the_settings_of_a_connection() {
        let mut config = ConnectionConfig::new(DriverKind::Postgres);
        config.host = "${env:DB_HOST}".to_owned();
        config.user = "${env:DB_USER}".to_owned();
        config.password = Some(Secret::new("${env:DB_PASSWORD}"));
        config
            .options
            .insert("warehouse".into(), "${env:WH}".into());
        config
            .options
            .insert(ENV_FILE.into(), "${env:not_expanded}".into());
        // The shell's own variables, which the shell replaces.
        config
            .options
            .insert(crate::BEFORE_CONNECT.into(), "vpn up \"${VPN}\"".into());
        config.ssh = Some(SshConfig {
            host: "${env:BASTION}".to_owned(),
            port: 22,
            user: "ops".to_owned(),
            auth: SshAuth::Password(Secret::new("${env:SSH_PW}")),
            jump: None,
        });
        let v = vars(
            &[
                ("DB_HOST", "db"),
                ("DB_USER", "app"),
                ("WH", "wh1"),
                ("BASTION", "b.example"),
            ],
            &[("DB_PASSWORD", "pw"), ("SSH_PW", "spw")],
        );
        let config = config.with_variables(&v).unwrap();
        assert_eq!((config.host.as_str(), config.user.as_str()), ("db", "app"));
        assert_eq!(config.password.unwrap().expose(), "pw");
        assert_eq!(config.options["warehouse"], "wh1");
        assert_eq!(config.options[ENV_FILE], "${env:not_expanded}");
        assert_eq!(config.options[crate::BEFORE_CONNECT], "vpn up \"${VPN}\"");
        let ssh = config.ssh.unwrap();
        assert_eq!(ssh.host, "b.example");
        assert!(matches!(ssh.auth, SshAuth::Password(p) if p.expose() == "spw"));
    }

    #[test]
    fn a_missing_env_file_is_an_error() {
        // Absolute on every system: `/no/such` is not on Windows.
        let missing = std::env::temp_dir().join("platypus-no-such").join(".env");
        let error = EnvVars::load(missing.to_str()).unwrap_err().to_string();
        assert!(error.contains("cannot read the .env file"), "{error}");
        assert!(EnvVars::load(Some("  ")).is_ok());
        let error = EnvVars::load(Some("project/.env")).unwrap_err().to_string();
        assert!(error.contains("absolute path"), "{error}");
    }

    #[test]
    fn the_env_file_option_is_not_passed_on() {
        let mut config = ConnectionConfig::new(DriverKind::Postgres);
        config.options.insert(ENV_FILE.into(), String::new());
        assert!(!config.with_env().unwrap().options.contains_key(ENV_FILE));
    }
}
