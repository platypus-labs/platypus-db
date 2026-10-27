//! One entry point for whatever the user pastes: a URL (`postgres://…`), a JDBC URL
//! (`jdbc:sqlserver://…`), an ADO.NET / ODBC connection string (`Server=…;Database=…`) or
//! a libpq one (`host=… dbname=…`).

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use super::{ConnectionConfig, DriverKind, JdbcUrl, keyvalue};
use crate::{Error, Result};

/// A connection read from a URL or a connection string, with what it could not carry over.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedConnection {
    pub config: ConnectionConfig,
    pub notes: Vec<ParseNote>,
}

/// Something the user should know about a pasted connection before using it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ParseNote {
    /// It logs in with Windows (integrated security, NTLM, Kerberos), which the app does
    /// not do: it needs a SQL Server login.
    WindowsAuth,
    /// It logs in with Microsoft Entra ID (Azure Active Directory), which the app does not do.
    EntraAuth,
    /// A SQL Server named instance without a port: the app connects to a port, not through
    /// the SQL Server Browser, so the port the instance listens on is needed.
    NamedInstance { instance: String },
    /// Parameters or keys the app does not use, by name only (their values may be secret).
    Ignored { keys: Vec<String> },
    /// A failover list: the app connects to the first host only, these are left out.
    OtherHosts { hosts: Vec<String> },
}

impl ParseNote {
    /// The note for the keys left out, or `None` when there are none.
    pub(super) fn ignored(keys: Vec<String>) -> Option<Self> {
        (!keys.is_empty()).then_some(Self::Ignored { keys })
    }

    /// The note for the hosts after the first, or `None` when there are none.
    pub(super) fn other_hosts(hosts: Vec<String>) -> Option<Self> {
        (!hosts.is_empty()).then_some(Self::OtherHosts { hosts })
    }
}

impl ConnectionConfig {
    /// Reads a scheme URL (as [`ConnectionConfig::from_url`], but leaving out the parameters
    /// it does not know, and MongoDB's options that hold a secret), a JDBC URL
    /// ([`JdbcUrl`]) or a key=value connection string
    /// ([`ConnectionConfig::from_connection_string`]). Errors never repeat the password, and
    /// a host that holds a login or other keys is an error: nothing but the password field
    /// gets a secret.
    pub fn parse_any(input: &str) -> Result<ParsedConnection> {
        Self::parse_any_for(input, None)
    }

    /// [`ConnectionConfig::parse_any`] for a form of the database `hint`: a connection
    /// string that does not name its driver (`Server=…;Uid=…`, as `SqlClient` and MySQL
    /// Connector/NET both write) is read as one for it, and `http(s)://` as a libSQL or
    /// ClickHouse URL. Settings the saved connection has no field for (the application
    /// name, the connect timeout) are noted as not used.
    pub fn parse_any_for(input: &str, hint: Option<DriverKind>) -> Result<ParsedConnection> {
        let mut parsed = Self::read_any(input.trim(), hint)?;
        let defaults = Self::new(parsed.config.driver);
        let mut dropped = Vec::new();
        if parsed.config.application_name != defaults.application_name {
            dropped.push("application_name".to_owned());
        }
        if parsed.config.connect_timeout != defaults.connect_timeout {
            dropped.push("connect_timeout".to_owned());
        }
        let ignored = parsed.notes.iter_mut().find_map(|n| match n {
            ParseNote::Ignored { keys } => Some(keys),
            _ => None,
        });
        match ignored {
            Some(keys) => {
                dropped.retain(|d| !keys.contains(d));
                keys.extend(dropped);
            }
            None => parsed.notes.extend(ParseNote::ignored(dropped)),
        }
        Ok(parsed)
    }

    fn read_any(input: &str, hint: Option<DriverKind>) -> Result<ParsedConnection> {
        if input
            .get(..5)
            .is_some_and(|p| p.eq_ignore_ascii_case("jdbc:"))
        {
            let jdbc = JdbcUrl::parse(&format!("jdbc:{}", &input[5..]))?;
            let notes = jdbc_notes(&jdbc);
            return Ok(ParsedConnection {
                config: jdbc.config,
                notes,
            });
        }
        if has_scheme(input) {
            let mut read = Self::read_url(input, hint)?;
            if read.config.driver == DriverKind::Mongo {
                read.unknown.extend(read.config.drop_secret_options());
            }
            let mut notes: Vec<ParseNote> = ParseNote::ignored(read.unknown).into_iter().collect();
            notes.extend(ParseNote::other_hosts(read.other_hosts));
            return Ok(ParsedConnection {
                config: read.config,
                notes,
            });
        }
        if input.contains('=') {
            return keyvalue::parse(input, hint);
        }
        Err(Error::Config(
            "not a connection URL (postgres://…, jdbc:…) or connection string (Server=…;Database=…)"
                .to_owned(),
        ))
    }
}

/// `scheme://`, the scheme made of the characters a URL scheme may have.
fn has_scheme(input: &str) -> bool {
    input.split_once("://").is_some_and(|(scheme, _)| {
        scheme.starts_with(|c: char| c.is_ascii_alphabetic())
            && scheme
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'))
    })
}

/// The logins and the named instance of a SQL Server JDBC URL that the app cannot use as
/// they are.
fn jdbc_notes(jdbc: &JdbcUrl) -> Vec<ParseNote> {
    let mut notes = Vec::new();
    if let Some(instance) = jdbc.instance.as_ref().filter(|_| !jdbc.port_given) {
        notes.push(ParseNote::NamedInstance {
            instance: instance.clone(),
        });
    }
    notes.extend(login_note(&jdbc.properties));
    notes.extend(ParseNote::other_hosts(jdbc.other_hosts.clone()));
    notes
}

/// A Windows or Entra ID login among the properties (keys in lower case, without spaces).
pub(super) fn login_note(props: &BTreeMap<String, String>) -> Option<ParseNote> {
    let prop = |k: &str| {
        props
            .get(k)
            .map(|v| v.trim().to_ascii_lowercase())
            .unwrap_or_default()
    };
    if prop("authentication")
        .replace(' ', "")
        .starts_with("activedirectory")
    {
        return Some(ParseNote::EntraAuth);
    }
    let on = |v: String| matches!(v.as_str(), "true" | "yes" | "sspi");
    let windows = on(prop("integratedsecurity"))
        || on(prop("trusted_connection"))
        || matches!(
            prop("authenticationscheme").as_str(),
            "ntlm" | "javakerberos" | "nativeauthentication"
        );
    windows.then_some(ParseNote::WindowsAuth)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{DriverKind, Secret, SslMode};

    fn parse(input: &str) -> ParsedConnection {
        ConnectionConfig::parse_any(input).unwrap()
    }

    #[test]
    fn scheme_urls_leave_out_unknown_parameters() {
        let p =
            parse("postgres://u:p@db.example:6543/shop?sslmode=require&channel_binding=require");
        let c = &p.config;
        assert_eq!(c.driver, DriverKind::Postgres);
        assert_eq!((c.host.as_str(), c.port), ("db.example", Some(6543)));
        assert_eq!((c.user.as_str(), c.database.as_str()), ("u", "shop"));
        assert_eq!(c.password, Some(Secret::new("p")));
        assert_eq!(c.ssl_mode, SslMode::Require);
        assert_eq!(
            p.notes,
            [ParseNote::Ignored {
                keys: vec!["channel_binding".to_owned()]
            }]
        );
        // MySQL's own names of the TLS modes.
        let my = parse("mysql://root@h/app?ssl-mode=REQUIRED");
        assert_eq!(my.config.ssl_mode, SslMode::Require);
        assert_eq!(my.notes, []);
        assert_eq!(
            parse("mongodb+srv://u@cluster0.example.net/app")
                .config
                .driver,
            DriverKind::Mongo
        );
        let mongo = parse("mongodb://u:p@h/db?tls=true&tlsCertificateKeyFilePassword=hunter2");
        assert!(!mongo.config.database.contains("hunter2"));
        assert_eq!(
            mongo.notes,
            [ParseNote::Ignored {
                keys: vec!["tlsCertificateKeyFilePassword".to_owned()]
            }]
        );
    }

    #[test]
    fn no_secret_ends_up_in_the_host() {
        let my = parse("jdbc:mysql://sandy:hunter2@myhost:3307/db").config;
        assert_eq!(
            (my.host.as_str(), my.port, my.user.as_str()),
            ("myhost", Some(3307), "sandy")
        );
        assert_eq!(my.password, Some(Secret::new("hunter2")));
        let pg = parse("host=h port=5432 dbname=d user=u password='hunter 2' sslmode=require");
        assert_eq!(pg.config.driver, DriverKind::Postgres);
        assert_eq!(
            (
                pg.config.host.as_str(),
                pg.config.port,
                pg.config.user.as_str()
            ),
            ("h", Some(5432), "u")
        );
        assert_eq!(pg.config.password, Some(Secret::new("hunter 2")));
        assert_eq!(pg.config.ssl_mode, SslMode::Require);
        for bad in [
            "jdbc:mysql://sandy:hunter2/db",
            "Host=h port=5432 password=hunter2",
            "host=h password=hunter2;x",
            "Server=sa:hunter2@h;Database=x",
        ] {
            let err = ConnectionConfig::parse_any(bad).unwrap_err().to_string();
            assert!(!err.contains("hunter2"), "{err}");
        }
    }

    #[test]
    fn jdbc_urls_note_the_named_instance_and_windows_login() {
        let p = parse(r"jdbc:sqlserver://SRV\SQLEXPRESS;databaseName=x;integratedSecurity=true");
        assert_eq!(p.config.driver, DriverKind::SqlServer);
        assert_eq!(
            p.notes,
            [
                ParseNote::NamedInstance {
                    instance: "SQLEXPRESS".to_owned()
                },
                ParseNote::WindowsAuth
            ]
        );
        let p = parse("JDBC:postgresql://h/db");
        assert_eq!(p.config.driver, DriverKind::Postgres);
        assert_eq!(p.notes, []);
        let p = parse("jdbc:sqlserver://h:1433;authentication=ActiveDirectoryPassword");
        assert_eq!(p.notes, [ParseNote::EntraAuth]);
    }

    #[test]
    fn connection_strings_and_errors() {
        let p = parse("Server=db;Database=x;User Id=sa;Password=pw");
        assert_eq!(p.config.driver, DriverKind::SqlServer);
        for bad in [
            "",
            "hello",
            "ftp://u:hunter2@h/x",
            "jdbc:db2://h;password=hunter2",
        ] {
            let err = ConnectionConfig::parse_any(bad).unwrap_err().to_string();
            assert!(!err.contains("hunter2"), "{err}");
        }
        let err = ConnectionConfig::parse_any("postgres://u:hunter2@h:99999/x")
            .unwrap_err()
            .to_string();
        assert!(!err.contains("hunter2"), "{err}");
    }

    #[test]
    fn notes_what_the_saved_connection_cannot_keep() {
        let p = parse("jdbc:postgresql://h1:5432,h2:5432/app");
        assert_eq!(p.config.host, "h1");
        assert_eq!(
            p.notes,
            [ParseNote::OtherHosts {
                hosts: vec!["h2:5432".to_owned()]
            }]
        );
        let p = parse("postgres://u@h/db?application_name=foo&connect_timeout=5&x=1");
        assert_eq!(
            p.notes,
            [ParseNote::Ignored {
                keys: vec![
                    "x".to_owned(),
                    "application_name".to_owned(),
                    "connect_timeout".to_owned()
                ]
            }]
        );
        let p = parse("Server=db;Database=x;Application Name=app");
        assert_eq!(
            p.notes,
            [ParseNote::Ignored {
                keys: vec!["application_name".to_owned()]
            }]
        );
        assert_eq!(parse("postgres://u@h/db").notes, []);
    }

    #[test]
    fn notes_serialize_by_kind() {
        let json = serde_json::to_string(&ParseNote::NamedInstance {
            instance: "X".to_owned(),
        })
        .unwrap();
        assert_eq!(json, r#"{"kind":"named_instance","instance":"X"}"#);
    }
}
