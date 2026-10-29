//! `McpConfig`: everything the bridge is told, as plain data.
//!
//! A TOML table, command-line flags and a builder call all fill the same
//! struct, so a backend can stand MCP up from configuration alone:
//!
//! ```toml
//! path = "/mcp"
//! listen = "127.0.0.1:4459"
//! [upstream]
//! url = "ws://127.0.0.1:4455"
//! service = "menger"
//! [oauth]
//! resource = "https://app.menger.sh/mcp"
//! authorization_servers = ["https://login.menger.sh"]
//! ```

use std::net::SocketAddr;

use serde::{Deserialize, Serialize};

/// The bridge's configuration. Every field has a default; `serde(default)`
/// lets a table name only what it changes.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct McpConfig {
    /// The path the MCP endpoint is served at. Default `/mcp`.
    pub path: String,
    /// Where a sidecar listens. `None` for a bridge mounted on another
    /// router (plexus-transport's `with_mcp`).
    pub listen: Option<SocketAddr>,
    /// The running backend a sidecar fronts. `None` for an in-process mount.
    pub upstream: Option<UpstreamConfig>,
    /// `/mcp` as an OAuth 2.1 resource server: its RFC 9728 metadata, and the
    /// 401 challenge that names it. `None`: a request without a credential
    /// is the anonymous caller, and a refused one gets a bare challenge.
    pub oauth: Option<OAuthConfig>,
    /// What `initialize` says the server is.
    pub server: ServerIdentity,
    /// ACCESS-25: the `Host` values the endpoint answers to. Empty: the
    /// loopback names, plus the literal `listen` address when it is not
    /// loopback. A sidecar behind a proxy names its public host here.
    pub allowed_hosts: Vec<String>,
    /// Caps on one `tools/call`: never a hang, never unbounded memory.
    pub limits: Limits,
    /// Admit a request that carries no credential, as the anonymous caller
    /// (AUTH-6's anonymous view). Default `true`; forced `false` whenever
    /// `oauth` is configured — an MCP client learns where to sign in from
    /// the 401, not from an empty tool list.
    pub anonymous: bool,
}

impl Default for McpConfig {
    fn default() -> Self {
        Self {
            path: "/mcp".to_string(),
            listen: None,
            upstream: None,
            oauth: None,
            server: ServerIdentity::default(),
            allowed_hosts: Vec::new(),
            limits: Limits::default(),
            anonymous: true,
        }
    }
}

impl McpConfig {
    /// A bridge mounted at `path` (`/mcp`), everything else default.
    pub fn at(path: impl Into<String>) -> Self {
        Self { path: path.into(), ..Self::default() }
    }

    /// A sidecar listening on `listen`, everything else default.
    pub fn listen(addr: SocketAddr) -> Self {
        Self { listen: Some(addr), ..Self::default() }
    }

    /// The backend a sidecar fronts.
    pub fn with_upstream(mut self, url: impl Into<String>, service: impl Into<String>) -> Self {
        self.upstream = Some(UpstreamConfig { url: url.into(), service: service.into() });
        self
    }

    /// The OAuth 2.1 resource this endpoint is, and who mints tokens for it.
    pub fn with_oauth(mut self, oauth: OAuthConfig) -> Self {
        self.oauth = Some(oauth);
        self
    }

    /// Whether a request with no credential is admitted as the anonymous
    /// caller: never under `oauth`, else as configured.
    pub fn admits_anonymous(&self) -> bool {
        self.oauth.is_none() && self.anonymous
    }

    /// The `Host` values to accept: the configured ones, or loopback plus
    /// the listen address's literal.
    pub fn effective_allowed_hosts(&self) -> Vec<String> {
        if !self.allowed_hosts.is_empty() {
            return self.allowed_hosts.clone();
        }
        let mut hosts = vec!["localhost".to_string(), "127.0.0.1".to_string(), "::1".to_string()];
        if let Some(addr) = self.listen {
            if !addr.ip().is_loopback() && !addr.ip().is_unspecified() {
                hosts.push(addr.to_string());
            }
        }
        hosts
    }
}

/// The running backend a sidecar fronts.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UpstreamConfig {
    /// Its WebSocket, e.g. `ws://127.0.0.1:4455`.
    pub url: String,
    /// Its service namespace — what its `call` method is prefixed with
    /// (`menger` for menger).
    pub service: String,
}

/// RFC 9728 protected-resource metadata for `/mcp`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct OAuthConfig {
    /// The resource identifier: the URL clients reach the endpoint at,
    /// e.g. `https://app.menger.sh/mcp`. The audience a token must carry.
    pub resource: String,
    /// Issuer identifiers of the authorization servers that mint tokens
    /// for this resource.
    pub authorization_servers: Vec<String>,
    /// The scopes a token may carry here; named in the 401 challenge too.
    pub scopes: Option<Vec<String>>,
}

impl Default for OAuthConfig {
    fn default() -> Self {
        Self { resource: String::new(), authorization_servers: Vec::new(), scopes: None }
    }
}

/// What `initialize` reports. Defaults: the upstream service's name, this
/// crate's version.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ServerIdentity {
    pub name: Option<String>,
    pub version: Option<String>,
    /// Instructions an MCP client shows its model.
    pub instructions: Option<String>,
}

/// Caps on one `tools/call`. Past any of them the call ends with a typed
/// error carrying the items so far — never a hang, never unbounded memory.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Limits {
    /// Data items collected into one result.
    pub items: usize,
    /// Bytes of item JSON collected into one result.
    pub bytes: usize,
    /// Seconds one call may run.
    pub call_secs: u64,
}

impl Default for Limits {
    fn default() -> Self {
        Self { items: 10_000, bytes: 100 * 1024 * 1024, call_secs: 300 }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_table_names_only_what_it_changes() {
        let c: McpConfig = toml::from_str(
            r#"
            listen = "127.0.0.1:4459"
            [upstream]
            url = "ws://127.0.0.1:4455"
            service = "menger"
            [oauth]
            resource = "https://app.menger.sh/mcp"
            authorization_servers = ["https://login.menger.sh"]
            [limits]
            items = 5
            "#,
        )
        .unwrap();
        assert_eq!(c.path, "/mcp");
        assert_eq!(c.upstream.as_ref().unwrap().service, "menger");
        assert_eq!(c.limits.items, 5);
        assert_eq!(c.limits.call_secs, 300);
        assert!(!c.admits_anonymous(), "oauth forces the 401");
    }

    #[test]
    fn an_unknown_key_is_refused() {
        assert!(toml::from_str::<McpConfig>("port = 4459").is_err());
    }

    #[test]
    fn hosts_default_to_loopback_and_the_listen_literal() {
        let c = McpConfig::listen("10.0.0.5:4459".parse().unwrap());
        assert_eq!(c.effective_allowed_hosts(), ["localhost", "127.0.0.1", "::1", "10.0.0.5:4459"]);
        let c = McpConfig::listen("127.0.0.1:4459".parse().unwrap());
        assert_eq!(c.effective_allowed_hosts().len(), 3);
    }
}
