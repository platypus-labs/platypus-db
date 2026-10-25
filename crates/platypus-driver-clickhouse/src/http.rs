//! Requests to the ClickHouse HTTP interface. Statements of a connection share an HTTP
//! session (`session_id`), so `SET`, `USE` and temporary tables last between them; other
//! requests (cancelling) run outside it.

use std::sync::Arc;
use std::time::Duration;

use platypus_core::{ConnectionConfig, Error, Result, Secret, SslMode};
use reqwest::header::HeaderMap;
use reqwest::{Client, Response, Url};

use crate::error::{SESSION_IS_LOCKED, ServerError, http_error, transport};

const DEFAULT_CONNECT_TIMEOUT: Duration = Duration::from_secs(30);
/// Seconds an idle session lives on the server: the server's default cap.
const SESSION_TIMEOUT: &str = "3600";
/// How long a statement waits for the session's previous request to finish on the server
/// (an abandoned result still being cancelled).
const LOCKED_RETRY: Duration = Duration::from_millis(50);
const LOCKED_ATTEMPTS: u32 = 160;

/// Settings sent with every statement.
const SETTINGS: &[(&str, &str)] = &[
    ("default_format", "ArrowStream"),
    // Uncompressed buffers: the IPC reader is built without LZ4.
    ("output_format_arrow_compression_method", "none"),
    ("output_format_arrow_string_as_string", "1"),
    ("output_format_arrow_low_cardinality_as_dictionary", "0"),
    // A dropped result stops its SELECT instead of letting it run to the end.
    ("cancel_http_readonly_queries_on_client_close", "1"),
];

/// The HTTP endpoint and credentials of a server.
#[derive(Debug, Clone)]
pub struct Http {
    client: Client,
    url: Url,
    user: String,
    password: Option<Arc<Secret>>,
}

impl Http {
    /// `https` when the TLS mode requires it: HTTP cannot negotiate TLS, so `prefer`
    /// connects in plaintext like `disable`.
    pub fn new(config: &ConnectionConfig) -> Result<Self> {
        let tls = matches!(
            config.ssl_mode,
            SslMode::Require | SslMode::VerifyCa | SslMode::VerifyFull
        );
        let host = if config.host.is_empty() {
            "localhost"
        } else {
            &config.host
        };
        let port = config.port.unwrap_or(if tls { 8443 } else { 8123 });
        let scheme = if tls { "https" } else { "http" };
        let url = Url::parse(&format!("{scheme}://{host}:{port}/"))
            .map_err(|e| Error::Config(format!("invalid host `{host}`: {e}")))?;
        let mut builder = Client::builder()
            .connect_timeout(config.connect_timeout.unwrap_or(DEFAULT_CONNECT_TIMEOUT));
        // reqwest wants a TLS configuration even for `http`: it has no provider of its own.
        let mut rustls = if tls {
            platypus_tls::client_config(config)?
                .ok_or_else(|| Error::Config("TLS is disabled".to_owned()))?
        } else {
            unused_tls()?
        };
        rustls.alpn_protocols = vec![b"http/1.1".to_vec()];
        builder = builder.use_preconfigured_tls(rustls);
        // Through a tunnel the URL (and the certificate) still name the server.
        if let Some(addr) = config.connect_addr {
            builder = builder.resolve(host, addr);
        }
        Ok(Self {
            client: builder.build().map_err(transport)?,
            url,
            user: if config.user.is_empty() {
                "default".to_owned()
            } else {
                config.user.clone()
            },
            password: config.password.clone().map(Arc::new),
        })
    }

    pub fn encrypted(&self) -> bool {
        self.url.scheme() == "https"
    }

    /// Sends `sql` with the query parameters `params`, and turns error responses into
    /// errors. A locked session is waited for.
    pub async fn send(&self, sql: &str, params: &[(&str, &str)]) -> Result<Response> {
        let mut url = self.url.clone();
        url.query_pairs_mut()
            .extend_pairs(SETTINGS)
            .extend_pairs(params);
        for _ in 0..LOCKED_ATTEMPTS {
            let mut request = self
                .client
                .post(url.clone())
                .header("X-ClickHouse-User", &self.user)
                .body(sql.to_owned());
            if let Some(password) = &self.password {
                request = request.header("X-ClickHouse-Key", password.expose());
            }
            let response = request.send().await.map_err(transport)?;
            let status = response.status();
            if status.is_success() {
                return Ok(response);
            }
            let body = response.text().await.map_err(transport)?;
            match ServerError::parse(&body) {
                Some(e) if e.code == SESSION_IS_LOCKED => tokio::time::sleep(LOCKED_RETRY).await,
                Some(e) => return Err(e.into_error(sql)),
                None => return Err(http_error(status, &body)),
            }
        }
        Err(Error::Other(
            "the session is busy with an earlier statement".to_owned(),
        ))
    }

    /// Stops the statement `query_id`, from outside the session it runs in.
    pub async fn kill(&self, query_id: &str) -> Result<()> {
        let sql = format!(
            "KILL QUERY WHERE query_id = {} ASYNC FORMAT Null",
            platypus_sql::Dialect::ClickHouse.quote_literal(query_id)
        );
        let response = self.send(&sql, &[]).await?;
        response.bytes().await.map_err(transport)?;
        Ok(())
    }
}

/// A configuration that trusts nothing, for plaintext clients.
fn unused_tls() -> Result<rustls::ClientConfig> {
    let provider = std::sync::Arc::new(rustls::crypto::ring::default_provider());
    Ok(rustls::ClientConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .map_err(|e| Error::Config(format!("TLS setup failed: {e}")))?
        .with_root_certificates(rustls::RootCertStore::empty())
        .with_no_client_auth())
}

/// The rows a statement inserted, from the `X-ClickHouse-Summary` header.
pub fn written_rows(headers: &HeaderMap) -> u64 {
    headers
        .get("X-ClickHouse-Summary")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| serde_json::from_str::<serde_json::Value>(v).ok())
        .and_then(|v| v.get("written_rows")?.as_str()?.parse().ok())
        .unwrap_or(0)
}

/// The output format of a statement's result, or `None` when it returns no rows.
pub fn result_format(headers: &HeaderMap) -> Option<&str> {
    headers.get("X-ClickHouse-Format")?.to_str().ok()
}

/// The query parameters of a statement in the session `session_id`. `check` fails the
/// statement when the session expired, instead of starting a new, empty one.
pub fn session_params<'a>(
    session_id: &'a str,
    query_id: &'a str,
    check: bool,
) -> Vec<(&'static str, &'a str)> {
    let mut params = vec![
        ("session_id", session_id),
        ("session_timeout", SESSION_TIMEOUT),
        ("query_id", query_id),
    ];
    if check {
        params.push(("session_check", "1"));
    }
    params
}

#[cfg(test)]
mod tests {
    use platypus_core::DriverKind;
    use reqwest::header::HeaderValue;

    use super::*;

    #[test]
    fn picks_the_scheme_from_the_tls_mode() {
        let mut config = ConnectionConfig::new(DriverKind::ClickHouse);
        config.host = "ch.example".to_owned();
        let http = Http::new(&config).unwrap();
        assert_eq!(http.url.as_str(), "http://ch.example:8123/");
        assert!(!http.encrypted());
        assert_eq!(http.user, "default");
        config.ssl_mode = SslMode::Require;
        config.port = None;
        let http = Http::new(&config).unwrap();
        assert_eq!(http.url.as_str(), "https://ch.example:8443/");
        assert!(http.encrypted());
        config.ssl_client_cert = Some("client.crt".into());
        assert!(Http::new(&config).is_err(), "a certificate without its key");
    }

    #[test]
    fn reads_response_headers() {
        let mut headers = HeaderMap::new();
        assert_eq!(written_rows(&headers), 0);
        assert_eq!(result_format(&headers), None);
        headers.insert(
            "X-ClickHouse-Summary",
            HeaderValue::from_static(r#"{"read_rows":"0","written_rows":"3"}"#),
        );
        headers.insert(
            "X-ClickHouse-Format",
            HeaderValue::from_static("ArrowStream"),
        );
        assert_eq!(written_rows(&headers), 3);
        assert_eq!(result_format(&headers), Some("ArrowStream"));
        let params = session_params("s", "q", true);
        assert!(params.contains(&("session_check", "1")));
        assert!(!session_params("s", "q", false).contains(&("session_check", "1")));
    }
}
