//! ADO.NET and ODBC connection strings: `Server=tcp:db,1433;Database=shop;User Id=sa;…`.
//!
//! Keys are matched without case or spaces (`User Id`, `UID`, `UserID`). A value may be
//! quoted with `"…"` or `'…'` (a doubled quote inside is one quote) or, as ODBC does, with
//! `{…}` (`}}` is one brace); empty pairs (`;;`) are skipped.
//!
//! The driver is the one `Driver={…}` (ODBC) or `Provider=…` (OLE DB) names: SQL Server,
//! PostgreSQL, MySQL/MariaDB and ClickHouse drivers are read into their own fields, any
//! other ODBC driver (or a `DSN=`) becomes a generic ODBC connection with the string as it
//! is. Without either, `Host=` (Npgsql's key) means PostgreSQL and anything else SQL Server
//! (`SqlClient`'s `Server=` / `Data Source=`): MySQL Connector/NET strings, which use the
//! same keys as ``SqlClient``, need a `Driver=` to be told apart.
//!
//! libpq's own strings (`host=db port=5432 dbname=shop`, separated by spaces) are read
//! as PostgreSQL ones (see `libpq`).

use std::time::Duration;

use super::any::{ParseNote, ParsedConnection, login_note};
use super::jdbc::{host_port, sqlserver_flag, sqlserver_ssl, strip_tcp};
use super::{ConnectionConfig, DriverKind, JdbcUrl, Secret, SslMode, libpq, secret_key};
use crate::{Error, Result};

/// The ODBC driver option keys of the generic driver (see `platypus-driver-odbc`).
const ODBC_DRIVER: &str = "driver";
const ODBC_CONNECTION_STRING: &str = "connection_string";

impl ConnectionConfig {
    /// Parses an ADO.NET or ODBC connection string such as
    /// `Server=tcp:db.example.com,1433;Initial Catalog=shop;User Id=sa;Password=…;Encrypt=True`
    /// or `Host=db;Port=5433;Database=shop;Username=app;Password=…` (Npgsql). See the module
    /// documentation for how the driver is chosen. Errors name keys, never values, and a
    /// host that holds a login or other keys is an error.
    pub fn from_connection_string(input: &str) -> Result<Self> {
        parse(input, None).map(|parsed| parsed.config)
    }
}

/// One `key=value` of the string: the key as written and its matching form.
struct Pair {
    key: String,
    /// Lower case, without spaces.
    norm: String,
    value: String,
}

pub(super) fn parse(input: &str, hint: Option<DriverKind>) -> Result<ParsedConnection> {
    let parsed = match libpq::parse(input) {
        Some(parsed) => parsed?,
        None => read(input, hint)?,
    };
    parsed.config.check_host()?;
    Ok(parsed)
}

fn read(input: &str, hint: Option<DriverKind>) -> Result<ParsedConnection> {
    let pairs = pairs(input)?;
    if pairs.is_empty() {
        return Err(Error::Config(
            "the connection string has no key=value pairs".to_owned(),
        ));
    }
    let last = |key: &str| {
        pairs
            .iter()
            .rev()
            .find(|p| p.norm == key)
            .map(|p| p.value.as_str())
    };
    let driver = match (last("driver"), last("provider")) {
        (Some(name), _) => by_name(name),
        (None, Some(provider)) if !provider.to_ascii_lowercase().starts_with("msdasql") => {
            Some(by_name(provider).ok_or_else(|| {
                Error::Config(format!("unsupported provider `{}`", provider.trim()))
            })?)
        }
        _ if last("dsn").is_some() => None,
        _ if last("host").is_some() => Some(DriverKind::Postgres),
        // The form the string is pasted in says which database it is for.
        _ if hint.is_some_and(|h| by_name(h.name()).is_some()) => hint,
        // `Port=` is no `SqlClient` key (it puts the port after a comma): Connector/NET.
        _ if last("port").is_some() => Some(DriverKind::MySql),
        _ => Some(DriverKind::SqlServer),
    };
    match driver {
        Some(driver) => Native::read(driver, &pairs),
        None => Ok(odbc(&pairs)),
    }
}

/// The database a driver or provider name is for, among those read into their own fields.
fn by_name(name: &str) -> Option<DriverKind> {
    let name = name.to_ascii_lowercase();
    [
        ("sql server", DriverKind::SqlServer),
        ("sqlserver", DriverKind::SqlServer),
        ("sqloledb", DriverKind::SqlServer),
        ("msoledbsql", DriverKind::SqlServer),
        ("sqlncli", DriverKind::SqlServer),
        ("postgres", DriverKind::Postgres),
        ("npgsql", DriverKind::Postgres),
        ("mysql", DriverKind::MySql),
        ("mariadb", DriverKind::MySql),
        ("clickhouse", DriverKind::ClickHouse),
    ]
    .into_iter()
    .find_map(|(word, kind)| name.contains(word).then_some(kind))
}

/// Splits the string into its pairs, honouring quoted values.
fn pairs(input: &str) -> Result<Vec<Pair>> {
    let mut out = Vec::new();
    let mut rest = input;
    loop {
        rest = rest.trim_start_matches(|c: char| c == ';' || c.is_whitespace());
        if rest.is_empty() {
            return Ok(out);
        }
        let end = rest.find(';').unwrap_or(rest.len());
        let Some(eq) = rest[..end].find('=') else {
            return Err(Error::Config(
                "a part of the connection string has no `=`".to_owned(),
            ));
        };
        let key = rest[..eq].trim().to_owned();
        let (value, after) = value(&rest[eq + 1..], &key)?;
        out.push(Pair {
            norm: key
                .chars()
                .filter(|c| !c.is_whitespace())
                .flat_map(char::to_lowercase)
                .collect(),
            key,
            value,
        });
        rest = after;
    }
}

/// The value at the start of `text` and what follows it (from its `;`).
fn value<'a>(text: &'a str, key: &str) -> Result<(String, &'a str)> {
    let text = text.trim_start();
    let close = match text.chars().next() {
        Some(q @ ('"' | '\'')) => q,
        Some('{') => '}',
        _ => {
            let end = text.find(';').unwrap_or(text.len());
            return Ok((text[..end].trim().to_owned(), &text[end..]));
        }
    };
    let mut value = String::new();
    let mut chars = text.char_indices().skip(1).peekable();
    while let Some((i, c)) = chars.next() {
        if c != close {
            value.push(c);
        } else if chars.peek().is_some_and(|&(_, next)| next == close) {
            // Doubled: one of it, inside the value.
            value.push(c);
            chars.next();
        } else {
            let after = text[i + c.len_utf8()..].trim_start();
            if after.is_empty() || after.starts_with(';') {
                return Ok((value, after));
            }
            return Err(Error::Config(format!(
                "unexpected text after the quoted value of `{key}`"
            )));
        }
    }
    Err(Error::Config(format!(
        "the quoted value of `{key}` is not closed"
    )))
}

/// A string for one of the drivers read into their own fields.
#[derive(Default)]
struct Native {
    instance: Option<String>,
    port_given: bool,
    encrypt: Option<bool>,
    trust: bool,
    ssl: Option<SslMode>,
    ignored: Vec<String>,
}

impl Native {
    fn read(driver: DriverKind, pairs: &[Pair]) -> Result<ParsedConnection> {
        let mut config = ConnectionConfig::new(driver);
        let mut state = Self::default();
        let mut props = std::collections::BTreeMap::new();
        for pair in pairs {
            props.insert(pair.norm.clone(), pair.value.clone());
            state.apply(&mut config, pair)?;
        }
        if let Some(mode) = state
            .ssl
            .or_else(|| sqlserver_ssl(state.encrypt, state.trust))
        {
            config.ssl_mode = mode;
        }
        let mut notes = Vec::new();
        if let Some(instance) = state.instance.filter(|_| !state.port_given) {
            notes.push(ParseNote::NamedInstance { instance });
        }
        notes.extend(login_note(&props));
        notes.extend(ParseNote::ignored(state.ignored));
        Ok(ParsedConnection { config, notes })
    }

    fn apply(&mut self, config: &mut ConnectionConfig, pair: &Pair) -> Result<()> {
        let value = pair.value.as_str();
        match pair.norm.as_str() {
            "server" | "servername" | "datasource" | "address" | "addr" | "networkaddress"
            | "host" | "hostname" => self.address(config, value)?,
            "port" => {
                config.port = Some(number(&pair.key, value)?);
                self.port_given = true;
            }
            "database" | "initialcatalog" | "dbname" => value.clone_into(&mut config.database),
            "userid" | "uid" | "user" | "username" => value.clone_into(&mut config.user),
            "password" | "pwd" => config.password = Some(Secret::new(value)),
            "encrypt" => self.encrypt = sqlserver_flag(value),
            "trustservercertificate" => self.trust = sqlserver_flag(value) == Some(true),
            "sslmode" => self.ssl = Some(value.trim().parse()?),
            "applicationname" | "app" => config.application_name = Some(value.to_owned()),
            "connecttimeout" | "connectiontimeout" | "timeout" => {
                config.connect_timeout = Some(Duration::from_secs(number(&pair.key, value)?));
            }
            // Read by the caller.
            "driver" | "provider" | "integratedsecurity" | "trusted_connection"
            | "authentication" => {}
            _ => self.ignored.push(pair.key.clone()),
        }
        Ok(())
    }

    /// `tcp:host\instance,port`, `host,port`, `host:port`, `[::1],port`, `(local)` or `.`.
    /// Named pipes (`np:`) and shared memory (`lpc:`) are not network addresses.
    fn address(&mut self, config: &mut ConnectionConfig, value: &str) -> Result<()> {
        let value = value.trim();
        let protocol = value.split_once(':').map(|(p, _)| p.to_ascii_lowercase());
        if matches!(protocol.as_deref(), Some("np" | "lpc")) {
            return Err(Error::Config(
                "named pipes and shared memory are not supported: the server needs a TCP address"
                    .to_owned(),
            ));
        }
        let value = strip_tcp(value);
        let (address, port) = match value.rsplit_once(',') {
            Some((a, p)) if p.trim().parse::<u16>().is_ok() => (a, p.trim().parse().ok()),
            _ => (value, None),
        };
        let (host, instance) = JdbcUrl::split_instance(address);
        let (host, port) = match port {
            Some(port) => (host.trim_matches(['[', ']']).to_owned(), Some(port)),
            None => host_port(&host),
        };
        config.host = match host.as_str() {
            "(local)" | "." | "" => "localhost".to_owned(),
            _ => host,
        };
        if let Some(port) = port {
            config.port = Some(port);
            self.port_given = true;
        }
        self.instance = instance;
        Ok(())
    }
}

pub(super) fn number<T: std::str::FromStr>(key: &str, value: &str) -> Result<T> {
    value
        .trim()
        .parse()
        .map_err(|_| Error::Config(format!("`{key}` is not a valid number")))
}

/// Another ODBC driver, or a data source name: the generic ODBC connection, with the
/// string as it is but for the driver, user and password, which have their own fields.
/// Other keys that hold a secret (`token=`, `PrivateKeyPwd=`) are left out, with a note:
/// the string is saved as it is, not in the keychain.
fn odbc(pairs: &[Pair]) -> ParsedConnection {
    let mut config = ConnectionConfig::new(DriverKind::Odbc);
    config.host.clear();
    config.port = None;
    let mut written = Vec::new();
    let mut secret = Vec::new();
    for pair in pairs {
        match pair.norm.as_str() {
            "driver" => {
                let name = pair.value.trim().to_owned();
                config.options.insert(ODBC_DRIVER.to_owned(), name);
            }
            "uid" | "userid" | "user" => pair.value.clone_into(&mut config.user),
            "pwd" | "password" => config.password = Some(Secret::new(pair.value.as_str())),
            key if secret_key(key) => secret.push(pair.key.clone()),
            _ => written.push(format!("{}={}", pair.key, braced(&pair.value))),
        }
    }
    if !written.is_empty() {
        config
            .options
            .insert(ODBC_CONNECTION_STRING.to_owned(), written.join(";"));
    }
    ParsedConnection {
        config,
        notes: ParseNote::ignored(secret).into_iter().collect(),
    }
}

/// An ODBC value, in braces when it holds what would end or split it.
fn braced(v: &str) -> String {
    if v.contains([';', '{', '}', '=']) || v.trim() != v {
        format!("{{{}}}", v.replace('}', "}}"))
    } else {
        v.to_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(input: &str) -> Result<ParsedConnection> {
        super::parse(input, None)
    }

    fn parse_ok(input: &str) -> ParsedConnection {
        parse(input).unwrap()
    }

    fn at(c: &ConnectionConfig) -> (DriverKind, &str, Option<u16>, &str, &str) {
        (
            c.driver,
            c.host.as_str(),
            c.port,
            c.database.as_str(),
            c.user.as_str(),
        )
    }

    #[test]
    fn sqlclient_strings() {
        let p = parse_ok(
            "Server=tcp:db.example.com,1444;Initial Catalog=shop;User ID=sa;Password=pw;\
             Encrypt=True;TrustServerCertificate=True;MultipleActiveResultSets=False",
        );
        let c = &p.config;
        assert_eq!(
            at(c),
            (
                DriverKind::SqlServer,
                "db.example.com",
                Some(1444),
                "shop",
                "sa"
            )
        );
        assert_eq!(c.password, Some(Secret::new("pw")));
        assert_eq!(c.ssl_mode, SslMode::Require);
        assert_eq!(
            p.notes,
            [ParseNote::Ignored {
                keys: vec!["MultipleActiveResultSets".to_owned()]
            }]
        );
        let c = parse_ok("Data Source=db;Database=x;Encrypt=yes").config;
        assert_eq!(c.ssl_mode, SslMode::VerifyFull);
        let c = parse_ok("Data Source=db;Database=x;Encrypt=False").config;
        assert_eq!(c.ssl_mode, SslMode::Disable);
        assert_eq!(c.port, Some(1433));
        let c = parse_ok("Server=(local);Database=x").config;
        assert_eq!(c.host, "localhost");
        let c = parse_ok("Server=tcp:[::1],1433;Database=x").config;
        assert_eq!((c.host.as_str(), c.port), ("::1", Some(1433)));
        assert!(parse(r"Server=np:\\srv\pipe\sql\query;Database=x").is_err());
        assert!(parse("Server=lpc:srv;Database=x").is_err());
        // A host that holds other keys, or a login: an error, never a host.
        for bad in [
            "Server=h port=5433 password=hunter2",
            "Server=sa:hunter2@h;Database=x",
            "Server=h:hunter2;Database=x",
        ] {
            let err = parse(bad).unwrap_err().to_string();
            assert!(!err.contains("hunter2"), "{err}");
        }
    }

    #[test]
    fn named_instances_and_windows_logins() {
        let p = parse_ok(r"Server=SRV\SQLEXPRESS;Database=App;Integrated Security=SSPI");
        assert_eq!(
            at(&p.config),
            (DriverKind::SqlServer, "SRV", Some(1433), "App", "")
        );
        assert_eq!(
            p.notes,
            [
                ParseNote::NamedInstance {
                    instance: "SQLEXPRESS".to_owned()
                },
                ParseNote::WindowsAuth
            ]
        );
        // With a port, the instance is not looked up.
        let p = parse_ok(r"Server=SRV\PROD,14330;Trusted_Connection=yes");
        assert_eq!(p.config.port, Some(14330));
        assert_eq!(p.notes, [ParseNote::WindowsAuth]);
        let p = parse_ok("Server=h;Authentication=Active Directory Password;User Id=a@b");
        assert_eq!(p.notes, [ParseNote::EntraAuth]);
    }

    #[test]
    fn npgsql_and_driver_named_strings() {
        let p = parse_ok(
            "Host=pg.local;Port=5433;Database=shop;Username=app;Password=pw;SSL Mode=VerifyFull",
        );
        assert_eq!(
            at(&p.config),
            (DriverKind::Postgres, "pg.local", Some(5433), "shop", "app")
        );
        assert_eq!(p.config.ssl_mode, SslMode::VerifyFull);
        assert_eq!(p.notes, []);

        let c = parse_ok("Driver={MySQL ODBC 8.0 Unicode Driver};Server=m;Port=3307;Database=d;UID=root;PWD=x;SslMode=REQUIRED").config;
        assert_eq!(at(&c), (DriverKind::MySql, "m", Some(3307), "d", "root"));
        assert_eq!(c.ssl_mode, SslMode::Require);
        let c = parse_ok("Driver={ODBC Driver 18 for SQL Server};Server=s;Database=d").config;
        assert_eq!(c.driver, DriverKind::SqlServer);
        let c = parse_ok("Driver={PostgreSQL Unicode};Servername=pg.example;Port=5433;Database=d")
            .config;
        assert_eq!(
            at(&c),
            (DriverKind::Postgres, "pg.example", Some(5433), "d", "")
        );
        let c = parse_ok("Provider=MSOLEDBSQL;Data Source=s;Initial Catalog=d").config;
        assert_eq!(at(&c), (DriverKind::SqlServer, "s", Some(1433), "d", ""));
        assert!(parse("Provider=Microsoft.ACE.OLEDB.12.0;Data Source=x.accdb").is_err());
    }

    #[test]
    fn other_odbc_drivers_keep_the_string() {
        let c = parse_ok(
            "Driver={Snowflake};Server=acme.snowflakecomputing.com;UID=me;PWD=pw;Warehouse=WH",
        )
        .config;
        assert_eq!(c.driver, DriverKind::Odbc);
        assert_eq!(
            (c.user.as_str(), c.password),
            ("me", Some(Secret::new("pw")))
        );
        assert_eq!(
            c.options.get("driver").map(String::as_str),
            Some("Snowflake")
        );
        assert_eq!(
            c.options.get("connection_string").map(String::as_str),
            Some("Server=acme.snowflakecomputing.com;Warehouse=WH")
        );
        let p = parse_ok("Driver={Snowflake};Server=s;token=hunter2;PrivateKeyPwd=hunter2;Role=R");
        assert_eq!(
            p.config
                .options
                .get("connection_string")
                .map(String::as_str),
            Some("Server=s;Role=R")
        );
        assert_eq!(
            p.notes,
            [ParseNote::Ignored {
                keys: vec!["token".to_owned(), "PrivateKeyPwd".to_owned()]
            }]
        );
        let c = parse_ok("DSN=Sales;UID=me").config;
        assert_eq!(c.driver, DriverKind::Odbc);
        assert_eq!(
            c.options.get("connection_string").map(String::as_str),
            Some("DSN=Sales")
        );
    }

    #[test]
    fn the_form_tells_strings_without_a_driver_apart() {
        let string = "Server=h;Database=app;Uid=root;Pwd=x";
        let read = |hint| super::parse(string, hint).unwrap().config;
        assert_eq!(read(None).driver, DriverKind::SqlServer);
        let c = read(Some(DriverKind::MySql));
        assert_eq!(at(&c), (DriverKind::MySql, "h", Some(3306), "app", "root"));
        assert_eq!(c.password, Some(Secret::new("x")));
        // A driver named in the string wins; so does Npgsql's `Host=`.
        let c = super::parse(
            "Driver={ODBC Driver 18 for SQL Server};Server=s",
            Some(DriverKind::MySql),
        )
        .unwrap()
        .config;
        assert_eq!(c.driver, DriverKind::SqlServer);
        let c = super::parse("Host=pg;Database=d", Some(DriverKind::SqlServer))
            .unwrap()
            .config;
        assert_eq!(c.driver, DriverKind::Postgres);
        // A hint without a connection string of its own is no hint.
        assert_eq!(
            super::parse(string, Some(DriverKind::Mongo))
                .unwrap()
                .config
                .driver,
            DriverKind::SqlServer
        );
    }

    #[test]
    fn quoted_values_and_empty_pairs() {
        let c =
            parse_ok(r#"Server=h;;Password="a;b""c";User Id='o''neil'; Database={x}}y};"#).config;
        assert_eq!(c.password, Some(Secret::new(r#"a;b"c"#)));
        assert_eq!((c.user.as_str(), c.database.as_str()), ("o'neil", "x}y"));
        for bad in [
            r#"Server=h;Password="hunter2"#,
            r#"Server=h;Password="hunter2" x"#,
            "Server=h;hunter2",
            "Server=h;Port=hunter2",
        ] {
            let err = parse(bad).unwrap_err().to_string();
            assert!(!err.contains("hunter2"), "{err}");
        }
        assert!(ConnectionConfig::from_connection_string(";;").is_err());
        // Connector/NET writes `Port=`, which `SqlClient` does not know.
        let c = parse_ok("Server=h;Port=3306;Database=app;Uid=root;Pwd=x;").config;
        assert_eq!(at(&c), (DriverKind::MySql, "h", Some(3306), "app", "root"));
        assert_eq!(
            ConnectionConfig::from_connection_string("Server=h")
                .unwrap()
                .driver,
            DriverKind::SqlServer
        );
    }
}
