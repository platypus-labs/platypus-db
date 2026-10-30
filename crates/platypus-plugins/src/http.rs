//! The `http` host function: requests to the hosts a plugin was allowed, through the
//! system's proxy (`HTTP_PROXY`, `HTTPS_PROXY`, `NO_PROXY`, and the system settings on
//! Windows and macOS) or the one of the connection, with verified TLS. Redirects are
//! followed only to allowed hosts.

use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use platypus_core::{ConnectionConfig, DriverKind, SslMode};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tokio::runtime::Handle;

use crate::PluginError;
use crate::manifest::host_allowed;
use crate::runtime::{Grants, Limits};

/// Redirects followed for one request.
const MAX_REDIRECTS: usize = 5;

/// A request, as the guest writes it.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Request {
    #[serde(default = "get")]
    method: String,
    url: String,
    #[serde(default)]
    headers: BTreeMap<String, String>,
    /// Text, sent as is.
    #[serde(default)]
    body: Option<String>,
}

fn get() -> String {
    "GET".to_owned()
}

/// The answer the guest reads: the body as text (invalid UTF-8 replaced).
#[derive(Debug, Serialize)]
struct Response {
    status: u16,
    headers: BTreeMap<String, String>,
    body: String,
}

pub(crate) struct Http {
    client: reqwest::Client,
    allowed: Arc<Vec<String>>,
    runtime: Handle,
    timeout: Duration,
}

impl Http {
    /// A client for `grants`; it needs the Tokio runtime of the calling thread.
    pub(crate) fn new(grants: &Grants, limits: &Limits) -> Result<Self, PluginError> {
        let runtime = Handle::try_current().map_err(|_| {
            PluginError::Invalid("HTTP needs the app's runtime, which is not running".to_owned())
        })?;
        let allowed = Arc::new(grants.http.clone());
        let follow = Arc::clone(&allowed);
        let mut config = ConnectionConfig::new(DriverKind::Plugin);
        config.ssl_mode = SslMode::VerifyFull;
        let tls = platypus_tls::client_config(&config)
            .map_err(|e| PluginError::Invalid(e.to_string()))?
            .ok_or_else(|| PluginError::Invalid("no TLS configuration".to_owned()))?;
        let mut builder = reqwest::Client::builder()
            .user_agent(concat!(
                "platypus-db/",
                env!("CARGO_PKG_VERSION"),
                " plugin"
            ))
            .connect_timeout(limits.http_timeout)
            .tls_backend_preconfigured(tls)
            .redirect(reqwest::redirect::Policy::custom(move |attempt| {
                if attempt.previous().len() >= MAX_REDIRECTS {
                    attempt.error("too many redirects")
                } else if attempt
                    .url()
                    .host_str()
                    .is_some_and(|host| host_allowed(&follow, host))
                {
                    attempt.follow()
                } else {
                    // The guest gets the redirect itself, and may not follow it either.
                    attempt.stop()
                }
            }));
        if let Some(proxy) = grants.proxy.as_deref().filter(|p| !p.trim().is_empty()) {
            if !proxy.trim().to_ascii_lowercase().starts_with("http://") {
                return Err(PluginError::Invalid(
                    "plugins reach the network through HTTP proxies only (http://host:port)"
                        .to_owned(),
                ));
            }
            builder = builder.proxy(
                reqwest::Proxy::all(proxy.trim())
                    .map_err(|e| PluginError::Invalid(format!("proxy {proxy:?}: {e}")))?,
            );
        }
        let client = builder
            .build()
            .map_err(|e| PluginError::Invalid(format!("cannot set up HTTP: {e}")))?;
        Ok(Self {
            client,
            allowed,
            runtime,
            timeout: limits.http_timeout,
        })
    }

    /// Runs the request in `raw` and answers `{"ok": response}` or `{"error": message}`.
    pub(crate) fn request(&self, raw: &[u8], deadline: Instant, max: usize) -> Value {
        match self.send(raw, deadline, max) {
            Ok(response) => json!({ "ok": response }),
            Err(message) => json!({ "error": message }),
        }
    }

    fn send(&self, raw: &[u8], deadline: Instant, max: usize) -> Result<Response, String> {
        let request: Request =
            serde_json::from_slice(raw).map_err(|e| format!("invalid HTTP request: {e}"))?;
        let url = url::Url::parse(&request.url).map_err(|e| format!("{:?}: {e}", request.url))?;
        if !matches!(url.scheme(), "http" | "https") {
            return Err(format!("{:?}: only http and https URLs", request.url));
        }
        let host = url.host_str().unwrap_or_default();
        if !host_allowed(&self.allowed, host) {
            return Err(format!("not allowed: the plugin may not reach {host}"));
        }
        let method = reqwest::Method::from_bytes(request.method.to_ascii_uppercase().as_bytes())
            .map_err(|_| format!("invalid HTTP method {:?}", request.method))?;
        let left = deadline.saturating_duration_since(Instant::now());
        if left.is_zero() {
            return Err("the call ran past its deadline".to_owned());
        }
        let mut builder = self
            .client
            .request(method, url)
            .timeout(self.timeout.min(left));
        for (name, value) in &request.headers {
            builder = builder.header(name, value);
        }
        if let Some(body) = request.body {
            builder = builder.body(body);
        }
        self.runtime.block_on(async move {
            let mut response = builder.send().await.map_err(|e| describe(&e))?;
            let status = response.status().as_u16();
            let headers = response
                .headers()
                .iter()
                .filter_map(|(k, v)| Some((k.as_str().to_owned(), v.to_str().ok()?.to_owned())))
                .collect();
            let mut body = Vec::new();
            while let Some(chunk) = response.chunk().await.map_err(|e| describe(&e))? {
                if body.len() + chunk.len() > max {
                    return Err(format!("the answer is larger than {} MB", max / (1 << 20)));
                }
                body.extend_from_slice(&chunk);
            }
            Ok(Response {
                status,
                headers,
                body: String::from_utf8_lossy(&body).into_owned(),
            })
        })
    }
}

/// The error with its causes: reqwest's own message is only "error sending request".
fn describe(e: &reqwest::Error) -> String {
    let mut message = e.to_string();
    let mut source = std::error::Error::source(e);
    while let Some(cause) = source {
        message.push_str(": ");
        message.push_str(&cause.to_string());
        source = cause.source();
    }
    message
}
