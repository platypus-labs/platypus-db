//! libpq's connection strings: `host=db port=5433 dbname=shop user=app password='a b'`.
//!
//! Pairs are separated by spaces; a value may be in single quotes, and `\` escapes the
//! next character. Keys are libpq's, in lower case: a string with any other key, or with a
//! `;` outside quotes, is not one of these (it may be an ADO.NET or ODBC string).

use std::path::PathBuf;
use std::time::Duration;

use super::any::{ParseNote, ParsedConnection};
use super::keyvalue::number;
use super::{ConnectionConfig, DriverKind, Secret};
use crate::Result;

/// The keywords libpq knows.
const KEYS: &[&str] = &[
    "host",
    "hostaddr",
    "port",
    "dbname",
    "user",
    "password",
    "passfile",
    "require_auth",
    "channel_binding",
    "connect_timeout",
    "client_encoding",
    "options",
    "application_name",
    "fallback_application_name",
    "keepalives",
    "keepalives_idle",
    "keepalives_interval",
    "keepalives_count",
    "tcp_user_timeout",
    "replication",
    "gssencmode",
    "sslmode",
    "sslnegotiation",
    "requiressl",
    "sslcompression",
    "sslcert",
    "sslkey",
    "sslpassword",
    "sslcertmode",
    "sslrootcert",
    "sslcrl",
    "sslcrldir",
    "sslsni",
    "requirepeer",
    "ssl_min_protocol_version",
    "ssl_max_protocol_version",
    "krbsrvname",
    "gsslib",
    "gssdelegation",
    "service",
    "target_session_attrs",
    "load_balance_hosts",
];

/// The connection a libpq string describes, or `None` when the input is not one.
pub(super) fn parse(input: &str) -> Option<Result<ParsedConnection>> {
    pairs(input).map(|pairs| read(&pairs))
}

/// The `key=value` pairs of a libpq string, or `None` when it is not one.
fn pairs(input: &str) -> Option<Vec<(String, String)>> {
    let mut out = Vec::new();
    let mut chars = input.chars().peekable();
    loop {
        while chars.next_if(|c| c.is_whitespace()).is_some() {}
        if chars.peek().is_none() {
            break;
        }
        let mut key = String::new();
        while let Some(c) = chars.next_if(|c| c.is_ascii_lowercase() || *c == '_') {
            key.push(c);
        }
        if !KEYS.contains(&key.as_str()) {
            return None;
        }
        while chars.next_if(|c| c.is_whitespace()).is_some() {}
        chars.next_if_eq(&'=')?;
        while chars.next_if(|c| c.is_whitespace()).is_some() {}
        let quoted = chars.next_if_eq(&'\'').is_some();
        let mut value = String::new();
        loop {
            match chars.next() {
                Some('\\') => value.push(chars.next()?),
                Some('\'') if quoted => break,
                Some(c) if !quoted && c.is_whitespace() => break,
                // `;` belongs to the ADO.NET and ODBC strings.
                Some(';') if !quoted => return None,
                Some(c) => value.push(c),
                // An unclosed quote.
                None if quoted => return None,
                None => break,
            }
        }
        out.push((key, value));
    }
    (!out.is_empty()).then_some(out)
}

/// The first of a comma-separated list (libpq tries several hosts in order).
fn first(value: &str) -> &str {
    value.split(',').next().unwrap_or_default().trim()
}

fn read(pairs: &[(String, String)]) -> Result<ParsedConnection> {
    let mut config = ConnectionConfig::new(DriverKind::Postgres);
    let (mut host, mut hostaddr) = (None, None);
    let mut ignored = Vec::new();
    for (key, value) in pairs {
        let value = value.as_str();
        match key.as_str() {
            "host" => host = Some(first(value)),
            "hostaddr" => hostaddr = Some(first(value)),
            "port" if !first(value).is_empty() => config.port = Some(number(key, first(value))?),
            "dbname" => value.clone_into(&mut config.database),
            "user" => value.clone_into(&mut config.user),
            "password" => config.password = Some(Secret::new(value)),
            "sslmode" => config.ssl_mode = value.parse()?,
            "sslrootcert" => config.ssl_root_cert = Some(PathBuf::from(value)),
            "sslcert" => config.ssl_client_cert = Some(PathBuf::from(value)),
            "sslkey" => config.ssl_client_key = Some(PathBuf::from(value)),
            "application_name" => config.application_name = Some(value.to_owned()),
            "connect_timeout" => {
                config.connect_timeout = Some(Duration::from_secs(number(key, value)?));
            }
            _ => ignored.push(key.clone()),
        }
    }
    if let Some(host) = host.filter(|h| !h.is_empty()).or(hostaddr) {
        host.clone_into(&mut config.host);
    }
    Ok(ParsedConnection {
        config,
        notes: ParseNote::ignored(ignored).into_iter().collect(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::SslMode;

    fn parse_ok(input: &str) -> ParsedConnection {
        parse(input).unwrap().unwrap()
    }

    #[test]
    fn reads_libpq_strings() {
        let p = parse_ok(
            "host=pg.example port=5433 dbname=shop user=app password=hunter2 sslmode=require",
        );
        let c = &p.config;
        assert_eq!(c.driver, DriverKind::Postgres);
        assert_eq!((c.host.as_str(), c.port), ("pg.example", Some(5433)));
        assert_eq!((c.database.as_str(), c.user.as_str()), ("shop", "app"));
        assert_eq!(c.password, Some(Secret::new("hunter2")));
        assert_eq!(c.ssl_mode, SslMode::Require);
        assert_eq!(p.notes, []);

        let p = parse_ok(
            r"host = h1,h2 port=5432,5433 password='it\'s a\\b' options='-c x=1' dbname=d",
        );
        assert_eq!((p.config.host.as_str(), p.config.port), ("h1", Some(5432)));
        assert_eq!(p.config.password, Some(Secret::new(r"it's a\b")));
        assert_eq!(
            p.notes,
            [ParseNote::Ignored {
                keys: vec!["options".to_owned()]
            }]
        );
        assert_eq!(
            parse_ok("hostaddr=10.0.0.5 dbname=x").config.host,
            "10.0.0.5"
        );
    }

    #[test]
    fn leaves_other_strings_alone() {
        for other in [
            "Host=h;Port=5432",
            "host=h;port=5432",
            "server=h database=x",
            "host=h password='open",
            "",
        ] {
            assert!(parse(other).is_none(), "{other}");
        }
        let err = parse("host=h port=hunter2").unwrap().err().unwrap();
        assert!(!err.to_string().contains("hunter2"), "{err}");
    }
}
