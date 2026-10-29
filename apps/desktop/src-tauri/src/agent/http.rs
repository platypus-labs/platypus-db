//! The bridge's HTTP server: `/v1/sessions`, `/v1/confirm`, `/v1/context` and `/v1/hello` for the `--mcp`
//! server and the CLI, `/mcp` for MCP clients over HTTP. Every request gives the token.

use std::convert::Infallible;
use std::sync::Arc;

use bytes::Bytes;
use http_body_util::combinators::BoxBody;
use http_body_util::{BodyExt, Full, Limited};
use hyper::body::Incoming;
use hyper::{Method, Request, Response, StatusCode};
use hyper_util::rt::TokioIo;
use platypus_mcp::{AppLink, ContextRequest, PlatypusServer, WriteRequest};
use rmcp::transport::streamable_http_server::StreamableHttpService;
use rmcp::transport::streamable_http_server::session::local::LocalSessionManager;
use serde::Deserialize;
use tokio::net::TcpListener;
use tokio_util::sync::CancellationToken;
use tower_service::Service;

use super::Link;

type Body = BoxBody<Bytes, Infallible>;
type Mcp = StreamableHttpService<PlatypusServer, LocalSessionManager>;

/// The largest body of a `/v1` request: a statement and its plan.
const MAX_BODY: usize = 4 * 1024 * 1024;

/// Serves `listener` until `stop` is cancelled.
pub async fn serve(
    listener: TcpListener,
    link: Arc<Link>,
    token: String,
    mcp: Mcp,
    stop: CancellationToken,
) {
    let token: Arc<str> = token.into();
    loop {
        let stream = tokio::select! {
            () = stop.cancelled() => break,
            accepted = listener.accept() => match accepted {
                Ok((stream, _)) => stream,
                Err(_) => continue,
            },
        };
        let (link, token, mcp, stop) = (
            Arc::clone(&link),
            Arc::clone(&token),
            mcp.clone(),
            stop.clone(),
        );
        tokio::spawn(async move {
            let service = hyper::service::service_fn(move |request| {
                handle(request, Arc::clone(&link), Arc::clone(&token), mcp.clone())
            });
            let connection = hyper::server::conn::http1::Builder::new()
                .serve_connection(TokioIo::new(stream), service);
            tokio::select! {
                _ = connection => {}
                () = stop.cancelled() => {}
            }
        });
    }
}

fn text(status: StatusCode, body: &str) -> Response<Body> {
    let mut response = Response::new(Full::new(Bytes::from(body.to_owned())).boxed());
    *response.status_mut() = status;
    response
}

fn json(value: &impl serde::Serialize) -> Response<Body> {
    let mut response = text(
        StatusCode::OK,
        &serde_json::to_string(value).unwrap_or_default(),
    );
    response.headers_mut().insert(
        hyper::header::CONTENT_TYPE,
        hyper::header::HeaderValue::from_static("application/json"),
    );
    response
}

/// Whether `request` gives `token`, compared in constant time.
fn authorized(request: &Request<Incoming>, token: &str) -> bool {
    let given = request
        .headers()
        .get(hyper::header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .unwrap_or_default();
    given.len() == token.len()
        && given
            .bytes()
            .zip(token.bytes())
            .fold(0, |diff, (a, b)| diff | (a ^ b))
            == 0
}

/// The JSON body of `request`, within [`MAX_BODY`].
async fn body<T: for<'de> Deserialize<'de>>(request: Request<Incoming>) -> Result<T, String> {
    let bytes = Limited::new(request.into_body(), MAX_BODY)
        .collect()
        .await
        .map_err(|e| e.to_string())?
        .to_bytes();
    serde_json::from_slice(&bytes).map_err(|e| e.to_string())
}

#[derive(Deserialize)]
struct Hello {
    session: String,
    agent: String,
}

async fn handle(
    request: Request<Incoming>,
    link: Arc<Link>,
    token: Arc<str>,
    mut mcp: Mcp,
) -> Result<Response<Body>, Infallible> {
    if !authorized(&request, &token) {
        return Ok(text(
            StatusCode::UNAUTHORIZED,
            "a wrong or missing token: see Settings › Agents in PlatypusDB",
        ));
    }
    let path = request.uri().path();
    if path == "/mcp" || path.starts_with("/mcp/") {
        return mcp.call(request).await;
    }
    Ok(match (request.method(), path) {
        (&Method::GET, "/v1/sessions") => json(&link.sessions().await.unwrap_or_default()),
        (&Method::POST, "/v1/confirm") => match body::<WriteRequest>(request).await {
            Ok(change) => match link.confirm(change).await {
                Ok(verdict) => json(&verdict),
                Err(e) => text(StatusCode::INTERNAL_SERVER_ERROR, &e),
            },
            Err(e) => text(StatusCode::BAD_REQUEST, &e),
        },
        (&Method::POST, "/v1/context") => match body::<ContextRequest>(request).await {
            Ok(asked) => match link.context(asked).await {
                Ok(verdict) => json(&verdict),
                Err(e) => text(StatusCode::INTERNAL_SERVER_ERROR, &e),
            },
            Err(e) => text(StatusCode::BAD_REQUEST, &e),
        },
        (&Method::POST, "/v1/hello") => match body::<Hello>(request).await {
            Ok(hello) => {
                let _ = link.hello(&hello.session, &hello.agent).await;
                json(&serde_json::json!({}))
            }
            Err(e) => text(StatusCode::BAD_REQUEST, &e),
        },
        _ => text(StatusCode::NOT_FOUND, "no such endpoint"),
    })
}
