//! Requests to the HTTP interface of Elasticsearch and `OpenSearch`.

use std::sync::Arc;
use std::time::Duration;

use platypus_core::{ConnectionConfig, Error, Result, Secret};
use reqwest::{Client, Method, Url};

use crate::error::{from_reply, transport};

const DEFAULT_CONNECT_TIMEOUT: Duration = Duration::from_secs(30);

/// How the requests log in: a user and password (Basic), or a password alone, which is an
/// API key.
#[derive(Debug, Clone)]
enum Auth {
    None,
    Basic(String, Option<Arc<Secret>>),
    ApiKey(Arc<Secret>),
}

/// The body of a request.
#[derive(Debug, Clone, Copy)]
pub enum Payload<'a> {
    None,
    Json(&'a str),
    /// Lines of JSON, as `_bulk` and `_msearch` take them.
    Ndjson(&'a str),
}

/// A reply: its status and body text.
#[derive(Debug, Clone)]
pub struct Reply {
    pub status: u16,
    pub body: String,
}

/// The endpoint and credentials of a cluster.
#[derive(Debug, Clone)]
pub struct Http {
    client: Client,
    base: Url,
    auth: Auth,
}

impl Http {
    /// `https` when the TLS mode requires it: HTTP cannot negotiate TLS, so `prefer`
    /// connects in plaintext like `disable`.
    pub fn new(config: &ConnectionConfig) -> Result<Self> {
        let tls = config.es_tls();
        let host = if config.host.is_empty() {
            "localhost"
        } else {
            &config.host
        };
        let port = config.port.unwrap_or(9200);
        let scheme = if tls { "https" } else { "http" };
        let base = Url::parse(&format!("{scheme}://{host}:{port}/"))
            .map_err(|e| Error::Config(format!("invalid host `{host}`: {e}")))?;
        let mut builder = Client::builder()
            .connect_timeout(config.connect_timeout.unwrap_or(DEFAULT_CONNECT_TIMEOUT))
            .user_agent("PlatypusDB");
        let mut rustls = if tls {
            platypus_tls::client_config(config)?
                .ok_or_else(|| Error::Config("TLS is disabled".to_owned()))?
        } else {
            platypus_tls::plaintext_config()?
        };
        rustls.alpn_protocols = vec![b"http/1.1".to_vec()];
        builder = builder.use_preconfigured_tls(rustls);
        // Through a tunnel the URL (and the certificate) still name the server.
        if let Some(addr) = config.connect_addr {
            builder = builder.resolve(host, addr);
        }
        let password = config.password.clone().map(Arc::new);
        let auth = match (config.user.as_str(), password) {
            ("", Some(key)) => Auth::ApiKey(key),
            ("", None) => Auth::None,
            (user, password) => Auth::Basic(user.to_owned(), password),
        };
        Ok(Self {
            client: builder.build().map_err(transport)?,
            base,
            auth,
        })
    }

    pub fn encrypted(&self) -> bool {
        self.base.scheme() == "https"
    }

    /// Sends a request: `target` is the path with its query string. Any status is a reply.
    pub async fn send(&self, method: &str, target: &str, payload: Payload<'_>) -> Result<Reply> {
        let method = Method::from_bytes(method.as_bytes())
            .map_err(|_| Error::Other(format!("invalid HTTP method `{method}`")))?;
        let url = self
            .base
            .join(target.trim_start_matches('/'))
            .map_err(|e| Error::Other(format!("invalid request path `{target}`: {e}")))?;
        let mut request = self
            .client
            .request(method, url)
            .header("Accept", "application/json");
        request = match &self.auth {
            Auth::None => request,
            Auth::Basic(user, password) => {
                request.basic_auth(user, password.as_ref().map(|p| p.expose()))
            }
            Auth::ApiKey(key) => {
                let mut value =
                    reqwest::header::HeaderValue::from_str(&format!("ApiKey {}", key.expose()))
                        .map_err(|_| {
                            Error::Config("the API key is not a valid header".to_owned())
                        })?;
                value.set_sensitive(true);
                request.header(reqwest::header::AUTHORIZATION, value)
            }
        };
        request = match payload {
            Payload::None => request,
            Payload::Json(body) => request
                .header("Content-Type", "application/json")
                .body(body.to_owned()),
            Payload::Ndjson(body) => request
                .header("Content-Type", "application/x-ndjson")
                .body(body.to_owned()),
        };
        let response = request.send().await.map_err(transport)?;
        let status = response.status().as_u16();
        let body = response.text().await.map_err(transport)?;
        Ok(Reply { status, body })
    }

    /// [`send`](Self::send), where an unsuccessful status is the server's error. `sql` is
    /// the statement the request carries, to place a syntax error in it.
    pub async fn call(
        &self,
        method: &str,
        target: &str,
        payload: Payload<'_>,
        sql: Option<&str>,
    ) -> Result<Reply> {
        let reply = self.send(method, target, payload).await?;
        if (200..300).contains(&reply.status) {
            Ok(reply)
        } else {
            Err(from_reply(reply.status, &reply.body, sql))
        }
    }
}

#[cfg(test)]
mod tests {
    use platypus_core::{DriverKind, SslMode};

    use super::*;

    #[test]
    fn picks_the_scheme_and_login() {
        let mut config = ConnectionConfig::new(DriverKind::Elasticsearch);
        config.host = "es.example".to_owned();
        let http = Http::new(&config).unwrap();
        assert_eq!(http.base.as_str(), "http://es.example:9200/");
        assert!(matches!(http.auth, Auth::None));
        config.password = Some(Secret::new("key"));
        assert!(matches!(Http::new(&config).unwrap().auth, Auth::ApiKey(_)));
        config.user = "elastic".to_owned();
        assert!(matches!(Http::new(&config).unwrap().auth, Auth::Basic(..)));
        config.ssl_mode = SslMode::Require;
        config.port = Some(443);
        let http = Http::new(&config).unwrap();
        assert_eq!(http.base.as_str(), "https://es.example/");
        assert!(http.encrypted());
    }
}
