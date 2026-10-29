//! What the statements of one connection share: the endpoint, the product and its version,
//! and the cancel signal.

use std::future::Future;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use platypus_core::{Error, Result, ServerVersion};
use tokio::sync::watch;

use crate::http::{Http, Payload, Reply};

/// Which of the two products the server is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Flavor {
    Elasticsearch,
    OpenSearch,
}

impl Flavor {
    #[must_use]
    pub const fn product(self) -> &'static str {
        match self {
            Self::Elasticsearch => "Elasticsearch",
            Self::OpenSearch => "OpenSearch",
        }
    }
}

#[derive(Debug)]
pub struct Shared {
    pub http: Http,
    pub flavor: Flavor,
    pub version: ServerVersion,
    /// Bumped by the cancel handle; running requests watch it.
    pub cancel: watch::Sender<u64>,
    /// The last request failed because the server could not be reached.
    pub closed: AtomicBool,
}

/// Waits until the cancel handle is used after the subscription.
async fn cancelled(rx: &mut watch::Receiver<u64>) {
    if rx.changed().await.is_err() {
        std::future::pending::<()>().await;
    }
}

impl Shared {
    /// Runs `request` until the cancel handle is used. Dropping a request closes its
    /// connection, which stops a search on the server.
    async fn guarded(&self, request: impl Future<Output = Result<Reply>>) -> Result<Reply> {
        let mut rx = self.cancel.subscribe();
        let result = tokio::select! {
            reply = request => reply,
            () = cancelled(&mut rx) => Err(Error::Cancelled),
        };
        match &result {
            Ok(_) => self.closed.store(false, Ordering::Relaxed),
            Err(Error::Connect(_)) => self.closed.store(true, Ordering::Relaxed),
            Err(_) => {}
        }
        result
    }

    /// Sends a request; an unsuccessful status is the server's error. `sql` is the
    /// statement it carries, to place a syntax error in it.
    pub async fn call(
        &self,
        method: &str,
        target: &str,
        payload: Payload<'_>,
        sql: Option<&str>,
    ) -> Result<Reply> {
        self.guarded(self.http.call(method, target, payload, sql))
            .await
    }

    /// Sends a request; any status is a reply.
    pub async fn send(&self, method: &str, target: &str, payload: Payload<'_>) -> Result<Reply> {
        self.guarded(self.http.send(method, target, payload)).await
    }

    /// Sends a request nobody waits for: closing a cursor, a point in time or a scroll.
    pub fn later(self: &Arc<Self>, method: &'static str, target: &'static str, body: String) {
        if let Ok(runtime) = tokio::runtime::Handle::try_current() {
            let shared = Arc::clone(self);
            runtime.spawn(async move {
                let _ = shared.http.send(method, target, Payload::Json(&body)).await;
            });
        }
    }
}
