//! The HTTP client and the server-side stream statements run on.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use platypus_core::{ConnectionConfig, Error, Result, SslMode};
use platypus_driver_sqlite::columns::Owned;

use crate::hrana::{
    PipelineRequest, PipelineResponse, Stmt, StmtResult, StreamRequest, StreamResponse,
    StreamResult,
};

#[derive(Debug)]
pub struct Session {
    http: reqwest::Client,
    /// `scheme://host:port` of the server.
    origin: String,
    /// Where the stream's requests go: the origin, or the server the last response named.
    base: String,
    token: Option<String>,
    baton: Option<String>,
    pub in_transaction: Arc<AtomicBool>,
    pub closed: Arc<AtomicBool>,
}

/// A request that failed before reaching a statement: network, TLS, HTTP status.
#[derive(Debug)]
struct Transport(String);

impl Session {
    /// Opens a session, checking that the server answers; returns it with whether it uses
    /// TLS and the SQLite version. `prefer` falls back to plain HTTP when HTTPS fails.
    pub async fn open(config: &ConnectionConfig) -> Result<(Self, bool, String)> {
        let schemes: &[&str] = match config.ssl_mode {
            SslMode::Disable => &["http"],
            SslMode::Prefer => &["https", "http"],
            _ => &["https"],
        };
        let mut failure = None;
        for scheme in schemes {
            let mut session = Self::new(config, scheme)?;
            match session.request("SELECT sqlite_version()").await {
                Ok(result) => return Ok((session, *scheme == "https", version(result)?)),
                Err(Ok(Transport(message))) => {
                    failure = Some(format!("{}: {message}", session.origin));
                }
                Err(Err(e)) => return Err(e),
            }
        }
        Err(Error::Connect(failure.unwrap_or_default()))
    }

    fn new(config: &ConnectionConfig, scheme: &str) -> Result<Self> {
        if config.host.is_empty() {
            return Err(Error::Config("no libSQL server host given".to_owned()));
        }
        let mut builder = reqwest::Client::builder();
        if let Some(timeout) = config.connect_timeout {
            builder = builder.connect_timeout(timeout);
        }
        if scheme == "https"
            && let Some(tls) = platypus_driver_pg::tls::client_config(config)?
        {
            builder = builder.tls_backend_preconfigured(tls);
        }
        // Through a tunnel: the host still names the server, for TLS and routing.
        let port = match config.connect_addr {
            Some(addr) => {
                builder = builder.resolve(&config.host, addr);
                Some(addr.port())
            }
            None => config.port,
        };
        let http = builder
            .build()
            .map_err(|e| Error::Config(format!("cannot set up HTTP: {e}")))?;
        let origin = origin(scheme, &config.host, port);
        Ok(Self {
            http,
            base: origin.clone(),
            origin,
            token: config
                .password
                .as_ref()
                .map(|p| p.expose().to_owned())
                .filter(|t| !t.is_empty()),
            baton: None,
            in_transaction: Arc::new(AtomicBool::new(false)),
            closed: Arc::new(AtomicBool::new(false)),
        })
    }

    /// Runs `sql` on the stream. A stream that expired outside a transaction is replaced.
    pub async fn execute(&mut self, sql: &str) -> Result<StmtResult> {
        let resumed = self.baton.is_some();
        match self.request(sql).await {
            Ok(result) => Ok(result),
            Err(Err(e)) => Err(e),
            Err(Ok(Transport(message))) if !resumed => Err(Error::Connect(message)),
            Err(Ok(Transport(message))) => {
                self.baton = None;
                self.base.clone_from(&self.origin);
                if self.in_transaction.swap(false, Ordering::AcqRel) {
                    self.closed.store(true, Ordering::Release);
                    return Err(Error::Connect(format!(
                        "the transaction was lost with the server stream: {message}"
                    )));
                }
                self.request(sql).await.map_err(|e| match e {
                    Ok(Transport(message)) => Error::Connect(message),
                    Err(e) => e,
                })
            }
        }
    }

    /// One pipeline: the statement, then whether a transaction is open after it.
    async fn request(&mut self, sql: &str) -> std::result::Result<StmtResult, Result<Transport>> {
        let body = PipelineRequest {
            baton: self.baton.as_deref(),
            requests: vec![
                StreamRequest::Execute {
                    stmt: Stmt {
                        sql,
                        want_rows: true,
                    },
                },
                StreamRequest::GetAutocommit,
            ],
        };
        let mut request = self
            .http
            .post(format!("{}/v3/pipeline", self.base.trim_end_matches('/')))
            .json(&body);
        if let Some(token) = &self.token {
            request = request.bearer_auth(token);
        }
        let transport = |e: reqwest::Error| Ok(Transport(error_chain(&e)));
        let response = request.send().await.map_err(transport)?;
        let status = response.status();
        if !status.is_success() {
            let text = response.text().await.unwrap_or_default();
            return Err(Ok(Transport(format!("HTTP {status}: {text}"))));
        }
        let response: PipelineResponse = response.json().await.map_err(transport)?;
        self.baton = response.baton;
        if let Some(base) = response.base_url {
            self.base = base;
        }
        let mut results = response.results.into_iter();
        let (statement, autocommit) = (results.next(), results.next());
        if let Some(StreamResult::Ok {
            response: StreamResponse::GetAutocommit { is_autocommit },
        }) = autocommit
        {
            self.in_transaction.store(!is_autocommit, Ordering::Release);
        }
        match statement {
            Some(StreamResult::Ok {
                response: StreamResponse::Execute { result },
            }) => Ok(result),
            Some(StreamResult::Error { error }) => Err(Err(error.into_error(sql))),
            other => Err(Err(Error::Other(format!(
                "unexpected libSQL response: {other:?}"
            )))),
        }
    }
}

/// The URL of the server; without a port, the scheme's default.
fn origin(scheme: &str, host: &str, port: Option<u16>) -> String {
    let host = if host.contains(':') {
        format!("[{host}]")
    } else {
        host.to_owned()
    };
    match port {
        Some(port) => format!("{scheme}://{host}:{port}"),
        None => format!("{scheme}://{host}"),
    }
}

/// The text of the single row and column of `result`.
fn version(result: StmtResult) -> Result<String> {
    let value = result
        .rows
        .into_iter()
        .next()
        .and_then(|r| r.into_iter().next());
    Ok(
        match value.map(crate::hrana::Value::into_owned).transpose()? {
            Some(Owned::Text(text)) => String::from_utf8_lossy(&text).into_owned(),
            _ => String::new(),
        },
    )
}

/// An error with its causes: reqwest's own message rarely says what went wrong.
fn error_chain(e: &dyn std::error::Error) -> String {
    let mut message = e.to_string();
    let mut source = e.source();
    while let Some(cause) = source {
        message = format!("{message}: {cause}");
        source = cause.source();
    }
    message
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_server_urls() {
        assert_eq!(
            origin("https", "db-acme.turso.io", None),
            "https://db-acme.turso.io"
        );
        assert_eq!(origin("http", "::1", Some(8080)), "http://[::1]:8080");
    }

    #[test]
    fn uses_the_tunnel_port() {
        let mut config = ConnectionConfig::new(platypus_core::DriverKind::Libsql);
        config.host = "db.example".to_owned();
        config.port = Some(443);
        config.connect_addr = Some("127.0.0.1:40000".parse().unwrap());
        config.password = Some(platypus_core::Secret::new(""));
        let session = Session::new(&config, "https").unwrap();
        assert_eq!(session.origin, "https://db.example:40000");
        assert_eq!(session.token, None);
    }

    #[test]
    fn needs_a_host() {
        let mut config = ConnectionConfig::new(platypus_core::DriverKind::Libsql);
        config.host.clear();
        assert!(matches!(
            Session::new(&config, "http"),
            Err(Error::Config(_))
        ));
    }
}
