//! RFC 9728 protected-resource metadata, as `/mcp` serves it.
//!
//! The same document plexus-transport serves for its WebSocket (AUTH-6);
//! kept here so a sidecar needs no server-side crate. TRANSPORT-10 decides
//! where the shared copy lives.

use serde::Serialize;

/// The well-known path of RFC 9728 §3.
pub const WELL_KNOWN_PATH: &str = "/.well-known/oauth-protected-resource";

/// RFC 9728 protected-resource metadata, as served.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ProtectedResource {
    /// The resource identifier: the URL clients reach this endpoint at.
    pub resource: String,
    /// Issuer identifiers of the authorization servers that mint tokens for it.
    pub authorization_servers: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scopes_supported: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub bearer_methods_supported: Vec<String>,
}

impl ProtectedResource {
    pub fn new<I, S>(resource: impl Into<String>, authorization_servers: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        Self {
            resource: resource.into(),
            authorization_servers: authorization_servers.into_iter().map(Into::into).collect(),
            scopes_supported: None,
            bearer_methods_supported: vec!["header".to_string()],
        }
    }

    pub fn with_scopes<I, S>(mut self, scopes: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.scopes_supported = Some(scopes.into_iter().map(Into::into).collect());
        self
    }

    /// `/.well-known/oauth-protected-resource`, followed by the resource's
    /// own path when it has one (RFC 9728 §3.1): `https://h/mcp` →
    /// `/.well-known/oauth-protected-resource/mcp`.
    pub fn metadata_path(&self) -> String {
        let path = split_origin(&self.resource).1.trim_end_matches('/');
        format!("{WELL_KNOWN_PATH}{path}")
    }

    /// The absolute URL of the document, as `resource_metadata` names it.
    pub fn metadata_url(&self) -> String {
        let origin = split_origin(&self.resource).0;
        format!("{origin}{}", self.metadata_path())
    }

    pub fn to_json(&self) -> String {
        serde_json::to_string(self).expect("metadata serializes")
    }
}

impl From<&crate::config::OAuthConfig> for ProtectedResource {
    fn from(o: &crate::config::OAuthConfig) -> Self {
        let mut p = ProtectedResource::new(o.resource.clone(), o.authorization_servers.clone());
        if let Some(scopes) = &o.scopes {
            p = p.with_scopes(scopes.clone());
        }
        p
    }
}

/// `("scheme://authority", "/path…")`, query and fragment dropped.
fn split_origin(resource: &str) -> (&str, &str) {
    let resource = resource.split(['?', '#']).next().unwrap_or(resource);
    let after_scheme = resource.find("://").map(|i| i + 3).unwrap_or(0);
    match resource[after_scheme..].find('/') {
        Some(i) => resource.split_at(after_scheme + i),
        None => (resource, ""),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_resource_path_follows_the_well_known_path() {
        let p = ProtectedResource::new("https://app.menger.sh/mcp", ["https://login.menger.sh"]);
        assert_eq!(p.metadata_path(), "/.well-known/oauth-protected-resource/mcp");
        assert_eq!(p.metadata_url(), "https://app.menger.sh/.well-known/oauth-protected-resource/mcp");
        let p = ProtectedResource::new("http://127.0.0.1:4459", ["https://as.example"]);
        assert_eq!(p.metadata_path(), "/.well-known/oauth-protected-resource");
    }
}
