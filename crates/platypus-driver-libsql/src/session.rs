//! The HTTP client and the server-side stream statements run on.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use platypus_core::{ConnectionConfig, Error, Result, SslMode};
use platypus_driver_sqlite::columns::Owned;

use crate::hrana::{
    Batch, BatchStep, CursorRequest, PipelineRequest, PipelineResponse, Stmt, StmtResult,
    StreamRequest, StreamResponse, StreamResult,
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
struct Transport {
    message: String,
    /// The server refused the request with an HTTP status, so it ran nothing. Otherwise
    /// the request may have reached it.
    answered: bool,
}

impl Session {
    /// Opens a session, checking that the server answers; returns it with whether it uses
    /// TLS and the SQLite version. `prefer` falls back to plain HTTP when HTTPS cannot be
    /// reached, but not when the server answers it: that would send the token in clear.
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
                Err(Ok(Transport { message, answered })) => {
                    failure = Some(format!("{}: {message}", session.origin));
                    if answered {
                        break;
                    }
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
        // reqwest has no crypto provider of its own: it panics without a configuration, even
        // for `http`.
        let tls = match platypus_tls::client_config(config)? {
            Some(tls) if scheme == "https" => tls,
            _ => platypus_tls::plaintext_config()?,
        };
        builder = builder.tls_backend_preconfigured(tls);
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

    /// Runs `sql` on the stream. A stream the server no longer knows, outside a transaction,
    /// is replaced and the statement sent again.
    pub async fn execute(&mut self, sql: &str) -> Result<StmtResult> {
        let resumed = self.baton.is_some();
        match self.request(sql).await {
            Ok(result) => Ok(result),
            Err(Err(e)) => Err(e),
            Err(Ok(Transport { message, .. })) if !resumed => Err(Error::Connect(message)),
            Err(Ok(Transport { message, answered })) => {
                self.baton = None;
                self.base.clone_from(&self.origin);
                if self.in_transaction.swap(false, Ordering::AcqRel) {
                    self.closed.store(true, Ordering::Release);
                    return Err(Error::Connect(format!(
                        "the transaction was lost with the server stream: {message}"
                    )));
                }
                // After a network failure the statement may have run: sending it again could
                // apply it twice.
                if !answered {
                    return Err(Error::Connect(message));
                }
                self.request(sql).await.map_err(|e| match e {
                    Ok(Transport { message, .. }) => Error::Connect(message),
                    Err(e) => e,
                })
            }
        }
    }

    /// Runs `sql`: a query through a cursor, whose result has no size limit (a table of
    /// 200 000 rows is too large for a pipeline's response: `RESPONSE_TOO_LARGE`), and
    /// which leaves the transaction as it was; any other statement through a pipeline,
    /// which also tells whether a transaction is open after it.
    async fn request(&mut self, sql: &str) -> std::result::Result<StmtResult, Result<Transport>> {
        if platypus_sql::is_read_only(sql, platypus_sql::Dialect::Sqlite) {
            self.cursor(sql).await
        } else {
            self.pipeline(sql).await
        }
    }

    /// Sends `body` to `endpoint` of the stream's server; the text of the answer.
    async fn post(
        &self,
        endpoint: &str,
        body: &impl serde::Serialize,
    ) -> std::result::Result<String, Result<Transport>> {
        let mut request = self
            .http
            .post(format!("{}/v3/{endpoint}", self.base.trim_end_matches('/')))
            .json(body);
        if let Some(token) = &self.token {
            request = request.bearer_auth(token);
        }
        let transport = |e: reqwest::Error| {
            Ok(Transport {
                message: error_chain(&e),
                answered: false,
            })
        };
        let response = request.send().await.map_err(transport)?;
        let status = response.status();
        if !status.is_success() {
            let text = response.text().await.unwrap_or_default();
            return Err(Ok(Transport {
                message: format!("HTTP {status}: {text}"),
                answered: true,
            }));
        }
        response.text().await.map_err(transport)
    }

    /// The stream continues with `baton`, on `base` when the server names one.
    fn follow(&mut self, baton: Option<String>, base: Option<String>) {
        self.baton = baton;
        if let Some(base) = base {
            self.base = base;
        }
    }

    /// One cursor of the statement.
    async fn cursor(&mut self, sql: &str) -> std::result::Result<StmtResult, Result<Transport>> {
        let body = CursorRequest {
            baton: self.baton.as_deref(),
            batch: Batch {
                steps: vec![BatchStep {
                    stmt: Stmt {
                        sql,
                        want_rows: true,
                    },
                }],
            },
        };
        let text = self.post("cursor", &body).await?;
        let (head, result) = crate::hrana::cursor_result(&text).map_err(Err)?;
        self.follow(head.baton, head.base_url);
        result.map_err(|error| Err(error.into_error(sql)))
    }

    /// One pipeline: the statement, then whether a transaction is open after it.
    async fn pipeline(&mut self, sql: &str) -> std::result::Result<StmtResult, Result<Transport>> {
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
        let text = self.post("pipeline", &body).await?;
        let response: PipelineResponse = serde_json::from_str(&text)
            .map_err(|e| Err(Error::Other(format!("unexpected libSQL response: {e}"))))?;
        self.follow(response.baton, response.base_url);
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
    fn builds_plaintext_clients() {
        let mut config = ConnectionConfig::new(platypus_core::DriverKind::Libsql);
        config.host = "db.example".to_owned();
        config.ssl_mode = SslMode::Disable;
        assert!(Session::new(&config, "http").is_ok());
        config.ssl_mode = SslMode::Prefer;
        assert!(Session::new(&config, "http").is_ok());
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
