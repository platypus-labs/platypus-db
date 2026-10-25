//! What each database's ADBC driver expects: the connection's fields as database options.
//! Checked against each driver's documentation: Snowflake (Apache Arrow ADBC), `BigQuery`,
//! Trino and Databricks (ADBC Driver Foundry).

use std::fmt::Write as _;

use percent_encoding::{NON_ALPHANUMERIC, utf8_percent_encode};
use platypus_core::{ConnectionConfig, DriverKind, Error, Result, SslMode};

/// Canonical ADBC keys.
pub const URI: &str = "uri";
pub const USERNAME: &str = "username";
pub const PASSWORD: &str = "password";

/// The `options` key of the generic driver that names its library; not passed on.
pub const DRIVER_FILE: &str = "driver_file";

fn option<'a>(config: &'a ConnectionConfig, key: &str) -> Option<&'a str> {
    config
        .options
        .get(key)
        .map(|v| v.trim())
        .filter(|v| !v.is_empty())
}

fn password(config: &ConnectionConfig) -> Option<String> {
    config
        .password
        .as_ref()
        .map(|p| p.expose().to_owned())
        .filter(|p| !p.is_empty())
}

fn encode(s: &str) -> String {
    utf8_percent_encode(s, NON_ALPHANUMERIC).to_string()
}

/// The database options that open `config` with its driver's ADBC library.
pub fn options(config: &ConnectionConfig) -> Result<Vec<(String, String)>> {
    let mut out = Vec::new();
    let mut push = |k: &str, v: &str| out.push((k.to_owned(), v.to_owned()));
    match config.driver {
        DriverKind::Snowflake => {
            if config.host.trim().is_empty() {
                return Err(Error::Config(
                    "the Snowflake account is required".to_owned(),
                ));
            }
            push("adbc.snowflake.sql.account", config.host.trim());
            if !config.user.is_empty() {
                push(USERNAME, &config.user);
            }
            if let Some(p) = password(config) {
                push(PASSWORD, &p);
            }
            if !config.database.is_empty() {
                push("adbc.snowflake.sql.db", &config.database);
            }
            for key in ["schema", "warehouse", "role"] {
                if let Some(v) = option(config, key) {
                    push(&format!("adbc.snowflake.sql.{key}"), v);
                }
            }
        }
        DriverKind::BigQuery => {
            let project = option(config, "project")
                .ok_or_else(|| Error::Config("the BigQuery project is required".to_owned()))?;
            push("bigquery.project_id", project);
            if let Some(dataset) = option(config, "dataset") {
                push("bigquery.dataset_id", dataset);
            }
            match option(config, "credentials_file") {
                Some(file) => {
                    push("bigquery.auth_type", "json_credential_file");
                    push("bigquery.auth.credentials", file);
                }
                None => push("bigquery.auth_type", "app_default_credentials"),
            }
        }
        DriverKind::Trino => push(URI, &trino_uri(config)?),
        DriverKind::Databricks => push(URI, &databricks_uri(config)?),
        DriverKind::Adbc => {
            if let Some(uri) = option(config, URI) {
                push(URI, uri);
            }
            if !config.user.is_empty() {
                push(USERNAME, &config.user);
            }
            if let Some(p) = password(config) {
                push(PASSWORD, &p);
            }
            for (k, v) in &config.options {
                if k != DRIVER_FILE && k != URI && k != "dialect" && !v.trim().is_empty() {
                    push(k, v.trim());
                }
            }
        }
        other => {
            return Err(Error::Config(format!(
                "{} does not use an ADBC driver",
                other.name()
            )));
        }
    }
    Ok(out)
}

/// `trino://user:password@host:port/catalog/schema?SSL=…`. Without TLS (`sslmode=disable`)
/// the driver speaks plain HTTP; `require` skips the certificate check. The driver cannot
/// fall back from one to the other, so `prefer` (the default) picks by port: HTTPS on 443,
/// plain HTTP elsewhere, like Trino's own default port 8080.
fn trino_uri(config: &ConnectionConfig) -> Result<String> {
    if config.host.trim().is_empty() {
        return Err(Error::Config("the Trino host is required".to_owned()));
    }
    let mut uri = String::from("trino://");
    if !config.user.is_empty() {
        uri.push_str(&encode(&config.user));
        if let Some(p) = password(config) {
            uri.push(':');
            uri.push_str(&encode(&p));
        }
        uri.push('@');
    }
    uri.push_str(config.host.trim());
    if let Some(port) = config.port {
        let _ = write!(uri, ":{port}");
    }
    if !config.database.is_empty() {
        uri.push('/');
        uri.push_str(&encode(&config.database));
        if let Some(schema) = option(config, "schema") {
            uri.push('/');
            uri.push_str(&encode(schema));
        }
    }
    uri.push_str(match config.ssl_mode {
        SslMode::Prefer if config.port == Some(443) => "?SSL=true",
        SslMode::Disable | SslMode::Prefer => "?SSL=false",
        SslMode::Require => "?SSL=true&SSLVerification=NONE",
        SslMode::VerifyCa | SslMode::VerifyFull => "?SSL=true",
    });
    Ok(uri)
}

/// `databricks://token:<token>@host:443/<http path>?catalog=…&schema=…`.
fn databricks_uri(config: &ConnectionConfig) -> Result<String> {
    let host = config.host.trim();
    let path = option(config, "http_path").map(|p| p.trim_start_matches('/'));
    let (false, Some(path)) = (host.is_empty(), path) else {
        return Err(Error::Config(
            "the Databricks host and HTTP path are required".to_owned(),
        ));
    };
    let token =
        password(config).ok_or_else(|| Error::Config("the access token is required".to_owned()))?;
    let mut uri = format!(
        "databricks://token:{}@{host}:{}/{path}",
        encode(&token),
        config.port.unwrap_or(443)
    );
    let mut params = Vec::new();
    if !config.database.is_empty() {
        params.push(format!("catalog={}", encode(&config.database)));
    }
    if let Some(schema) = option(config, "schema") {
        params.push(format!("schema={}", encode(schema)));
    }
    if !params.is_empty() {
        uri.push('?');
        uri.push_str(&params.join("&"));
    }
    Ok(uri)
}

#[cfg(test)]
mod tests {
    use platypus_core::Secret;

    use super::*;

    fn config(driver: DriverKind, options: &[(&str, &str)]) -> ConnectionConfig {
        let mut c = ConnectionConfig::new(driver);
        c.host = String::new();
        c.options = options
            .iter()
            .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
            .collect();
        c
    }

    fn get<'a>(options: &'a [(String, String)], key: &str) -> Option<&'a str> {
        options
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.as_str())
    }

    #[test]
    fn snowflake_options() {
        let mut c = config(
            DriverKind::Snowflake,
            &[
                ("warehouse", "COMPUTE_WH"),
                ("role", " "),
                ("schema", "PUBLIC"),
            ],
        );
        assert!(options(&c).is_err());
        c.host = "xy12345.eu-west-1".to_owned();
        c.user = "luca".to_owned();
        c.password = Some(Secret::new("s3cret"));
        c.database = "SALES".to_owned();
        let o = options(&c).unwrap();
        assert_eq!(
            get(&o, "adbc.snowflake.sql.account"),
            Some("xy12345.eu-west-1")
        );
        assert_eq!(get(&o, USERNAME), Some("luca"));
        assert_eq!(get(&o, PASSWORD), Some("s3cret"));
        assert_eq!(get(&o, "adbc.snowflake.sql.db"), Some("SALES"));
        assert_eq!(get(&o, "adbc.snowflake.sql.warehouse"), Some("COMPUTE_WH"));
        assert_eq!(get(&o, "adbc.snowflake.sql.schema"), Some("PUBLIC"));
        // Empty fields are left out.
        assert_eq!(get(&o, "adbc.snowflake.sql.role"), None);
    }

    #[test]
    fn bigquery_options() {
        assert!(options(&config(DriverKind::BigQuery, &[])).is_err());
        let o = options(&config(
            DriverKind::BigQuery,
            &[("project", "my-proj"), ("dataset", "sales")],
        ))
        .unwrap();
        assert_eq!(get(&o, "bigquery.project_id"), Some("my-proj"));
        assert_eq!(get(&o, "bigquery.dataset_id"), Some("sales"));
        assert_eq!(
            get(&o, "bigquery.auth_type"),
            Some("app_default_credentials")
        );
        let o = options(&config(
            DriverKind::BigQuery,
            &[("project", "p"), ("credentials_file", "C:/keys/sa.json")],
        ))
        .unwrap();
        assert_eq!(get(&o, "bigquery.auth_type"), Some("json_credential_file"));
        assert_eq!(
            get(&o, "bigquery.auth.credentials"),
            Some("C:/keys/sa.json")
        );
    }

    #[test]
    fn databricks_options() {
        let mut c = config(
            DriverKind::Databricks,
            &[
                ("http_path", "/sql/1.0/warehouses/abc"),
                ("schema", "default"),
            ],
        );
        c.host = "dbc-1.cloud.databricks.com".to_owned();
        assert!(options(&c).is_err(), "no token");
        c.password = Some(Secret::new("dapi/1+2"));
        c.database = "main".to_owned();
        let o = options(&c).unwrap();
        assert_eq!(
            get(&o, URI),
            Some(
                "databricks://token:dapi%2F1%2B2@dbc-1.cloud.databricks.com:443/sql/1.0/warehouses/abc?catalog=main&schema=default"
            )
        );
    }

    #[test]
    fn trino_options() {
        let mut c = config(DriverKind::Trino, &[("schema", "default")]);
        assert!(options(&c).is_err());
        c.host = "localhost".to_owned();
        c.user = "luca".to_owned();
        c.database = "memory".to_owned();
        c.ssl_mode = SslMode::Disable;
        assert_eq!(
            get(&options(&c).unwrap(), URI),
            Some("trino://luca@localhost:8080/memory/default?SSL=false")
        );
        c.password = Some(Secret::new("p@ss"));
        c.ssl_mode = SslMode::Require;
        assert_eq!(
            get(&options(&c).unwrap(), URI),
            Some("trino://luca:p%40ss@localhost:8080/memory/default?SSL=true&SSLVerification=NONE")
        );
        c.password = None;
        c.ssl_mode = SslMode::VerifyFull;
        assert_eq!(
            get(&options(&c).unwrap(), URI),
            Some("trino://luca@localhost:8080/memory/default?SSL=true")
        );
    }

    #[test]
    fn trino_prefers_tls_on_the_https_port_only() {
        let mut c = config(DriverKind::Trino, &[]);
        c.host = "trino.example.com".to_owned();
        assert_eq!(c.ssl_mode, SslMode::Prefer);
        assert_eq!(c.port, Some(8080));
        assert_eq!(
            get(&options(&c).unwrap(), URI),
            Some("trino://trino.example.com:8080?SSL=false")
        );
        c.port = Some(443);
        assert_eq!(
            get(&options(&c).unwrap(), URI),
            Some("trino://trino.example.com:443?SSL=true")
        );
        c.port = None;
        assert_eq!(
            get(&options(&c).unwrap(), URI),
            Some("trino://trino.example.com?SSL=false")
        );
    }

    #[test]
    fn generic_options() {
        let mut c = config(
            DriverKind::Adbc,
            &[
                (DRIVER_FILE, "C:/drivers/x.dll"),
                (URI, "grpc://host:1234"),
                ("adbc.flight.sql.rpc.call_header.x", "1"),
                ("dialect", "ansi"),
            ],
        );
        c.user = "u".to_owned();
        let o = options(&c).unwrap();
        assert_eq!(get(&o, URI), Some("grpc://host:1234"));
        assert_eq!(get(&o, USERNAME), Some("u"));
        assert_eq!(get(&o, "adbc.flight.sql.rpc.call_header.x"), Some("1"));
        assert_eq!(get(&o, DRIVER_FILE), None);
        assert_eq!(get(&o, "dialect"), None);
        assert!(options(&config(DriverKind::Postgres, &[])).is_err());
    }
}
