//! One entry point for whatever the user pastes: a URL (`postgres://…`), a JDBC URL
//! (`jdbc:sqlserver://…`) or an ADO.NET / ODBC connection string (`Server=…;Database=…`).

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use super::{ConnectionConfig, JdbcUrl, keyvalue};
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
}

impl ConnectionConfig {
    /// Reads a scheme URL (as [`ConnectionConfig::from_url`], but leaving out the parameters
    /// it does not know), a JDBC URL ([`JdbcUrl`]) or a key=value connection string
    /// ([`ConnectionConfig::from_connection_string`]). Errors never repeat the password.
    pub fn parse_any(input: &str) -> Result<ParsedConnection> {
        let input = input.trim();
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
            let (config, unknown) = Self::read_url(input)?;
            let notes = if unknown.is_empty() {
                Vec::new()
            } else {
                vec![ParseNote::Ignored { keys: unknown }]
            };
            return Ok(ParsedConnection { config, notes });
        }
        if input.contains('=') {
            return keyvalue::parse(input);
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
    fn notes_serialize_by_kind() {
        let json = serde_json::to_string(&ParseNote::NamedInstance {
            instance: "X".to_owned(),
        })
        .unwrap();
        assert_eq!(json, r#"{"kind":"named_instance","instance":"X"}"#);
    }
}
