//! [`ConnectionConfig`] to client options: a connection string built from the fields, with
//! the options carried in `database` after a `?` (see [`DriverKind::Mongo`]).
//!
//! [`DriverKind::Mongo`]: platypus_core::DriverKind::Mongo

use std::time::Duration;

use mongodb::options::{ClientOptions, Credential, Tls, TlsOptions};
use platypus_core::{ConnectionConfig, Error, Result, SslMode};

/// What [`client_options`] makes of a config.
pub struct Target {
    pub options: ClientOptions,
    /// The database statements run in until `use` changes it.
    pub database: String,
    pub encrypted: bool,
}

/// The database name and connection string options of `config.database`, without `srv`;
/// and whether `srv=true` was among them.
fn split_database(database: &str) -> (&str, Vec<&str>, bool) {
    let (name, query) = database.split_once('?').unwrap_or((database, ""));
    let mut srv = false;
    let options = query
        .split('&')
        .filter(|p| !p.is_empty())
        .filter(|p| match p.strip_prefix("srv=") {
            Some(value) => {
                srv = value == "true";
                false
            }
            None => true,
        })
        .collect();
    (name, options, srv)
}

/// The host part of the connection string.
fn hosts(config: &ConnectionConfig, srv: bool) -> String {
    if let Some(addr) = config.connect_addr {
        return addr.to_string();
    }
    let host = config.host.trim();
    let host = if host.is_empty() { "localhost" } else { host };
    if srv || host.contains(',') {
        return host.to_owned();
    }
    let host = if host.contains(':') && !host.starts_with('[') {
        format!("[{host}]")
    } else {
        host.to_owned()
    };
    format!("{host}:{}", config.port.unwrap_or(27017))
}

pub async fn client_options(config: &ConnectionConfig) -> Result<Target> {
    let (name, mut query, srv) = split_database(&config.database);
    if config.connect_addr.is_some() {
        // Through an SSH tunnel only the one server behind it can be reached.
        query.push("directConnection=true");
    }
    let has_tls_option = query
        .iter()
        .any(|p| p.starts_with("tls=") || p.starts_with("ssl="));
    let scheme = if srv && config.connect_addr.is_none() {
        "mongodb+srv"
    } else {
        "mongodb"
    };
    let uri = format!("{scheme}://{}/?{}", hosts(config, srv), query.join("&"));
    let mut options = ClientOptions::parse(&uri)
        .await
        .map_err(|e| Error::Config(crate::error::message(&e)))?;

    if !config.user.is_empty() {
        let mut credential = options
            .credential
            .take()
            .unwrap_or_else(|| Credential::builder().build());
        credential.username = Some(config.user.clone());
        credential.password = config.password.as_ref().map(|p| p.expose().to_owned());
        // Without a user the connection string's `authSource` is not kept.
        let source = query.iter().find_map(|p| p.strip_prefix("authSource="));
        credential.source = Some(source.unwrap_or("admin").to_owned());
        options.credential = Some(credential);
    }
    if let Some(app) = &config.application_name {
        options.app_name = Some(app.clone());
    }
    let timeout = config.connect_timeout.unwrap_or(Duration::from_secs(10));
    options.connect_timeout = Some(timeout);
    options.server_selection_timeout = Some(timeout);
    if !has_tls_option {
        options.tls = tls(config, options.tls.take())?;
    }
    let encrypted = matches!(options.tls, Some(Tls::Enabled(_)));
    let database = if name.is_empty() {
        options
            .default_database
            .clone()
            .unwrap_or_else(|| "test".to_owned())
    } else {
        name.to_owned()
    };
    Ok(Target {
        options,
        database,
        encrypted,
    })
}

/// TLS from the SSL mode; `Prefer` keeps what the connection string implies (on for
/// `mongodb+srv`, off otherwise), as MongoDB has no opportunistic TLS.
fn tls(config: &ConnectionConfig, parsed: Option<Tls>) -> Result<Option<Tls>> {
    let mut options = match config.ssl_mode {
        SslMode::Disable => return Ok(Some(Tls::Disabled)),
        SslMode::Prefer => match parsed {
            Some(Tls::Enabled(options)) => options,
            _ => return Ok(parsed),
        },
        SslMode::Require => TlsOptions::builder()
            .allow_invalid_certificates(true)
            .build(),
        // rustls cannot skip only the host name check: `verify-ca` verifies it too.
        SslMode::VerifyCa | SslMode::VerifyFull => TlsOptions::builder().build(),
    };
    if let Some(ca) = &config.ssl_root_cert {
        options.ca_file_path = Some(ca.clone());
    }
    match (&config.ssl_client_cert, &config.ssl_client_key) {
        (Some(cert), key) if key.is_none() || key.as_ref() == Some(cert) => {
            options.cert_key_file_path = Some(cert.clone());
        }
        (None, None) => {}
        _ => {
            return Err(Error::Config(
                "MongoDB takes the client certificate and its key in one PEM file".to_owned(),
            ));
        }
    }
    Ok(Some(Tls::Enabled(options)))
}

#[cfg(test)]
mod tests {
    use platypus_core::{DriverKind, Secret};

    use super::*;

    fn config(database: &str) -> ConnectionConfig {
        let mut c = ConnectionConfig::new(DriverKind::Mongo);
        c.database = database.to_owned();
        c
    }

    #[test]
    fn splits_options_from_the_database() {
        assert_eq!(split_database("shop"), ("shop", vec![], false));
        assert_eq!(
            split_database("shop?srv=true&authSource=x"),
            ("shop", vec!["authSource=x"], true)
        );
        assert_eq!(
            split_database("?replicaSet=rs"),
            ("", vec!["replicaSet=rs"], false)
        );
    }

    #[test]
    fn builds_hosts() {
        let mut c = config("");
        assert_eq!(hosts(&c, false), "localhost:27017");
        c.host = "a:1,b:2".into();
        assert_eq!(hosts(&c, false), "a:1,b:2");
        c.host = "::1".into();
        c.port = Some(5);
        assert_eq!(hosts(&c, false), "[::1]:5");
        c.host = "cluster.example.net".into();
        assert_eq!(hosts(&c, true), "cluster.example.net");
    }

    #[tokio::test]
    async fn makes_client_options() {
        let mut c = config("shop?authSource=users&replicaSet=rs0");
        c.user = "u".into();
        c.password = Some(Secret::new("p"));
        c.ssl_mode = SslMode::Require;
        let target = client_options(&c).await.unwrap();
        assert_eq!(target.database, "shop");
        assert!(target.encrypted);
        let credential = target.options.credential.unwrap();
        assert_eq!(credential.source.as_deref(), Some("users"));
        assert_eq!(credential.password.as_deref(), Some("p"));
        assert_eq!(target.options.repl_set_name.as_deref(), Some("rs0"));

        let mut c = config("");
        c.user = "u".into();
        let target = client_options(&c).await.unwrap();
        assert_eq!(target.database, "test");
        assert!(!target.encrypted);
        assert_eq!(
            target.options.credential.unwrap().source.as_deref(),
            Some("admin")
        );
    }
}
