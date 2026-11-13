//! The axum router: the identify step, `/mcp`, and the RFC 9728 document.
//!
//! The identify step runs before rmcp sees a request. It reads the
//! `Authorization` header, opens (or reuses) the backend transport for that
//! credential, and puts it in the request's extensions for the bridge —
//! so a credential the backend refuses is answered at the HTTP layer, as
//! MCP's authorization flow needs: `401` with `WWW-Authenticate: Bearer
//! resource_metadata="…"`, which is what sends an MCP client to sign in.

use std::sync::Arc;

use axum::body::Body;
use axum::extract::{Request, State};
use axum::http::{header, HeaderValue, StatusCode};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::Router;
use rmcp::transport::streamable_http_server::session::local::LocalSessionManager;
use rmcp::transport::streamable_http_server::{StreamableHttpServerConfig, StreamableHttpService};

use crate::bridge::McpBridge;
use crate::protected_resource::ProtectedResource;
use crate::upstream::{CallerTransport, Credential, Upstream, UpstreamError};

struct Shared<U: Upstream> {
    bridge: McpBridge<U>,
    metadata: Option<Prepared>,
    admits_anonymous: bool,
}

struct Prepared {
    path: String,
    url: String,
    body: String,
    scopes: Option<String>,
}

/// The router: `path` served by rmcp behind the identify step, and the
/// metadata document when `oauth` is configured.
pub fn router<U: Upstream>(bridge: McpBridge<U>) -> Router {
    let config = bridge.config().clone();
    let metadata = config.oauth.as_ref().map(|o| {
        let p = ProtectedResource::from(o);
        Prepared { path: p.metadata_path(), url: p.metadata_url(), body: p.to_json(), scopes: o.scopes.as_ref().map(|s| s.join(" ")) }
    });
    let shared = Arc::new(Shared { bridge: bridge.clone(), metadata, admits_anonymous: config.admits_anonymous() });

    let http_config = StreamableHttpServerConfig::default().with_allowed_hosts(config.effective_allowed_hosts());
    // KEEL-59: a session that ends cancels its calls at once.
    let sessions = crate::gone::EndsCalls::new(LocalSessionManager::default());
    let service = StreamableHttpService::new(move || Ok(bridge.clone()), Arc::new(sessions), http_config);

    let mut router = Router::new()
        .nest_service(&config.path, service)
        .layer(middleware::from_fn_with_state(shared.clone(), identify::<U>));
    if let Some(meta) = &shared.metadata {
        let body = meta.body.clone();
        router = router.route(
            &meta.path,
            get(move || {
                let body = body.clone();
                async move { ([(header::CONTENT_TYPE, "application/json")], body) }
            }),
        );
    }
    router
}

/// The credential a request carries.
fn credential_of(req: &Request) -> Result<Credential, ()> {
    let Some(value) = req.headers().get(header::AUTHORIZATION) else { return Ok(Credential::Anonymous) };
    let value = value.to_str().map_err(|_| ())?;
    let (scheme, token) = value.split_once(' ').ok_or(())?;
    if !scheme.eq_ignore_ascii_case("bearer") || token.trim().is_empty() {
        return Err(());
    }
    Ok(Credential::Bearer(token.trim().to_string()))
}

async fn identify<U: Upstream>(State(shared): State<Arc<Shared<U>>>, mut req: Request, next: Next) -> Response {
    let credential = match credential_of(&req) {
        Ok(c) => c,
        Err(()) => return shared.challenge(StatusCode::BAD_REQUEST, Some("invalid_request")),
    };
    if credential == Credential::Anonymous && !shared.admits_anonymous {
        return shared.challenge(StatusCode::UNAUTHORIZED, None);
    }
    match shared.bridge.upstream().transport_for(&credential).await {
        Ok(transport) => {
            req.extensions_mut().insert(CallerTransport { credential, transport });
            next.run(req).await
        }
        Err(UpstreamError::Unauthorized) => shared.challenge(StatusCode::UNAUTHORIZED, Some("invalid_token")),
        Err(UpstreamError::Unreachable(m)) => {
            tracing::warn!(error = %m, "the backend is unreachable");
            (StatusCode::BAD_GATEWAY, "the backend is unreachable").into_response()
        }
    }
}

impl<U: Upstream> Shared<U> {
    /// RFC 6750's challenge: the metadata URL and scopes when the endpoint
    /// is a protected resource, a bare realm otherwise.
    fn challenge(&self, status: StatusCode, error: Option<&str>) -> Response {
        let mut parts = Vec::new();
        match &self.metadata {
            Some(m) => {
                parts.push(format!("resource_metadata=\"{}\"", m.url));
                if let Some(scopes) = &m.scopes {
                    parts.push(format!("scope=\"{scopes}\""));
                }
            }
            None => parts.push("realm=\"plexus\"".to_string()),
        }
        if let Some(e) = error {
            parts.push(format!("error=\"{e}\""));
        }
        let value = format!("Bearer {}", parts.join(", "));
        let mut resp = Response::builder().status(status).header(header::CONTENT_TYPE, "text/plain").body(Body::from("Unauthorized")).expect("static response");
        if let Ok(v) = HeaderValue::from_str(&value) {
            resp.headers_mut().insert(header::WWW_AUTHENTICATE, v);
        }
        resp
    }
}
