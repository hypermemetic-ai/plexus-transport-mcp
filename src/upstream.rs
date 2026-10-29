//! Where a bridge reaches its backend, as one caller.
//!
//! [`Upstream`] turns a [`Credential`] into a [`plexus_client::Transport`]
//! that speaks as that caller. [`SocketUpstream`] opens the backend's
//! WebSocket with the credential on the upgrade — the backend's validator
//! and gate then decide, as they do for synapse. An in-process upstream
//! (plexus-transport's `with_mcp`) names the caller from the same
//! credential and calls the service in memory.

use std::collections::HashMap;
use std::fmt;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use plexus_client::{Transport, TransportErrorKind, WsTransport};
use tokio::sync::Mutex;

/// What a request proved: nothing, or the bearer token it carried — the
/// token itself, never logged, presented upstream as it was shown.
#[derive(Clone, PartialEq, Eq, Hash)]
pub enum Credential {
    Anonymous,
    Bearer(String),
}

impl fmt::Debug for Credential {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Credential::Anonymous => f.write_str("Anonymous"),
            Credential::Bearer(_) => f.write_str("Bearer(…)"),
        }
    }
}

impl Credential {
    /// The `Authorization` header's value, when the credential is one.
    pub fn authorization(&self) -> Option<String> {
        match self {
            Credential::Anonymous => None,
            Credential::Bearer(token) => Some(format!("Bearer {token}")),
        }
    }
}

/// A transport shared across the requests of one caller.
pub type DynTransport = Arc<dyn Transport + Send + Sync>;

/// A boxed, sendable future.
pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// Why a backend could not be reached as the caller.
#[derive(Debug, thiserror::Error)]
pub enum UpstreamError {
    /// The backend refused the credential (401 or 403 at the upgrade).
    #[error("the backend refused the credential")]
    Unauthorized,
    /// No socket could be opened.
    #[error("the backend is unreachable: {0}")]
    Unreachable(String),
}

/// How a bridge reaches its backend.
pub trait Upstream: Send + Sync + 'static {
    /// The backend's service namespace: what its `call`, `hashes` and
    /// `schema` methods are prefixed with (`menger` for menger).
    fn service(&self) -> &str;

    /// A transport that calls the backend as `credential`.
    fn transport_for(&self, credential: &Credential) -> BoxFuture<'_, Result<DynTransport, UpstreamError>>;
}

/// A running backend, reached over its WebSocket. One socket per
/// credential, kept while it lives; a closed one is reopened on the next
/// request.
pub struct SocketUpstream {
    url: String,
    service: String,
    open: Mutex<HashMap<Credential, WsTransport>>,
}

impl fmt::Debug for SocketUpstream {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SocketUpstream").field("url", &self.url).field("service", &self.service).finish()
    }
}

impl SocketUpstream {
    /// The backend at `url` (`ws://host:port`), whose service namespace is
    /// `service`.
    pub fn new(url: impl Into<String>, service: impl Into<String>) -> Self {
        Self { url: url.into(), service: service.into(), open: Mutex::new(HashMap::new()) }
    }

    /// The backend's URL.
    pub fn url(&self) -> &str {
        &self.url
    }

    async fn connect(&self, credential: &Credential) -> Result<WsTransport, UpstreamError> {
        let headers: Vec<(String, String)> =
            credential.authorization().into_iter().map(|v| ("authorization".to_string(), v)).collect();
        WsTransport::connect_with_headers(&self.url, &self.service, &headers).await.map_err(|e| match e.kind {
            TransportErrorKind::Http { status: 401 | 403 } => UpstreamError::Unauthorized,
            _ => UpstreamError::Unreachable(e.message),
        })
    }
}

impl Upstream for SocketUpstream {
    fn service(&self) -> &str {
        &self.service
    }

    fn transport_for(&self, credential: &Credential) -> BoxFuture<'_, Result<DynTransport, UpstreamError>> {
        let credential = credential.clone();
        Box::pin(async move {
            {
                let mut open = self.open.lock().await;
                match open.get(&credential) {
                    Some(t) if !t.is_closed() => return Ok(Arc::new(t.clone()) as DynTransport),
                    Some(_) => {
                        open.remove(&credential);
                    }
                    None => {}
                }
            }
            // Connected outside the lock: a slow backend must not hold every
            // other caller. Two first requests may race and open two
            // sockets; the loser's closes when its last handle drops.
            let t = self.connect(&credential).await?;
            let mut open = self.open.lock().await;
            let t = open.entry(credential).or_insert(t).clone();
            Ok(Arc::new(t) as DynTransport)
        })
    }
}

/// The caller's transport, as the HTTP layer resolved it for one request
/// ([`crate::http::router`]'s identify step). Read by the bridge from the
/// request's extensions; absent on stdio.
#[derive(Clone)]
pub struct CallerTransport {
    pub credential: Credential,
    pub transport: DynTransport,
}

impl fmt::Debug for CallerTransport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CallerTransport").field("credential", &self.credential).finish()
    }
}
