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
    /// The process's environment, and the variables of `file` when given (`~` is the home
    /// folder).
    ///
    /// # Errors
    /// When the file cannot be read.
    pub fn load(file: Option<&str>) -> Result<Self> {
        let file = match file.map(str::trim).filter(|f| !f.is_empty()) {
            Some(path) => {
                let path = home(path);
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
    match (
        path.strip_prefix("~/"),
        std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")),
    ) {
        (Some(rest), Some(home)) => Path::new(&home).join(rest),
        _ => PathBuf::from(path),
    }
}

/// The variables of a `.env` file: `NAME=value` lines, `export` before them allowed,
/// values in single quotes as they are, in double quotes with `\n` and `\"`, unquoted up to a
/// ` #` comment. Other lines are skipped.
#[must_use]
pub fn parse_dotenv(text: &str) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    for line in text.lines() {
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
        let value = if let Some(inner) = value.strip_prefix('\'').and_then(|v| v.rsplit_once('\''))
        {
            inner.0.to_owned()
        } else if let Some(inner) = value.strip_prefix('"').and_then(|v| v.rsplit_once('"')) {
            inner
                .0
                .replace("\\n", "\n")
                .replace("\\\"", "\"")
                .replace("\\\\", "\\")
        } else {
            value
                .split(" #")
                .next()
                .unwrap_or(value)
                .trim_end()
                .to_owned()
        };
        out.insert(name.to_owned(), value);
    }
    out
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
        let end = inner
            .find('}')
            .ok_or_else(|| Error::Config(format!("`{}` is not closed by `}}`", &rest[at..])))?;
        let reference = &inner[..end];
        let Some(name) = reference.strip_prefix("env:").map(str::trim) else {
            return Err(Error::Config(format!(
                "`${{{reference}}}`: only environment variables are read, as `${{env:NAME}}`"
            )));
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

impl ConnectionConfig {
    /// The settings with the environment variables of their text fields, password and SSH
    /// included, replaced. Reads the `.env` file of the `env_file` option.
    ///
    /// # Errors
    /// When a variable is not set or the `.env` file cannot be read.
    pub fn with_env(self) -> Result<Self> {
        let vars = EnvVars::load(self.options.get(ENV_FILE).map(String::as_str))?;
        self.with_variables(&vars)
    }

    /// As [`with_env`](Self::with_env), with the variables of `vars`.
    ///
    /// # Errors
    /// When a variable is not set.
    pub fn with_variables(mut self, vars: &EnvVars) -> Result<Self> {
        let text = |s: &mut String| -> Result<()> {
            *s = expand(s, vars)?;
            Ok(())
        };
        text(&mut self.host)?;
        text(&mut self.database)?;
        text(&mut self.user)?;
        if let Some(password) = &self.password {
            self.password = Some(Secret::new(expand(password.expose(), vars)?));
        }
        for path in [
            &mut self.ssl_root_cert,
            &mut self.ssl_client_cert,
            &mut self.ssl_client_key,
        ]
        .into_iter()
        .flatten()
        {
            *path = PathBuf::from(expand(&path.to_string_lossy(), vars)?);
        }
        for (key, value) in &mut self.options {
            if key != ENV_FILE {
                text(value)?;
            }
        }
        if let Some(schema) = &mut self.init.schema {
            text(schema)?;
        }
        if let Some(ssh) = &mut self.ssh {
            ssh_with_variables(ssh, vars)?;
        }
        Ok(self)
    }
}

fn ssh_with_variables(ssh: &mut SshConfig, vars: &EnvVars) -> Result<()> {
    ssh.host = expand(&ssh.host, vars)?;
    ssh.user = expand(&ssh.user, vars)?;
    match &mut ssh.auth {
        SshAuth::Password(secret) => *secret = Secret::new(expand(secret.expose(), vars)?),
        SshAuth::Key { path, passphrase } => {
            *path = PathBuf::from(expand(&path.to_string_lossy(), vars)?);
            if let Some(p) = passphrase {
                *p = Secret::new(expand(p.expose(), vars)?);
            }
        }
        SshAuth::Agent => {}
    }
    if let Some(jump) = &mut ssh.jump {
        ssh_with_variables(jump, vars)?;
    }
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
        let ssh = config.ssh.unwrap();
        assert_eq!(ssh.host, "b.example");
        assert!(matches!(ssh.auth, SshAuth::Password(p) if p.expose() == "spw"));
    }

    #[test]
    fn a_missing_env_file_is_an_error() {
        let error = EnvVars::load(Some("/no/such/.env"))
            .unwrap_err()
            .to_string();
        assert!(error.contains("cannot read the .env file"), "{error}");
        assert!(EnvVars::load(Some("  ")).is_ok());
    }
}
