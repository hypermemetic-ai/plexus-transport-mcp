//! The rmcp `ServerHandler`: `initialize`, `tools/list`, `tools/call`, each
//! answered as the caller the request proved.

use std::collections::HashMap;
use std::sync::Arc;

use rmcp::model::{CallToolRequestParams, CallToolResponse, Implementation, ListToolsResult, PaginatedRequestParams, ServerCapabilities, ServerConfig};
use rmcp::service::{RequestContext, RoleServer};
use rmcp::{ErrorData, ServerHandler};
use tokio::sync::Mutex;

use crate::config::McpConfig;
use crate::tools::{self, ToolSet};
use crate::upstream::{CallerTransport, Credential, DynTransport, Upstream, UpstreamError};

/// The bridge: one per endpoint, cheap to clone (rmcp makes one handler
/// per session from a factory).
pub struct McpBridge<U: Upstream> {
    inner: Arc<Inner<U>>,
}

struct Inner<U> {
    upstream: U,
    config: McpConfig,
    /// The credential every call carries when the transport names none
    /// (stdio): what `--bearer-file` held, or anonymous.
    fixed: Credential,
    /// The caller's tools at the view hash they were read at.
    cache: Mutex<HashMap<Credential, Arc<ToolSet>>>,
}

impl<U: Upstream> Clone for McpBridge<U> {
    fn clone(&self) -> Self {
        Self { inner: self.inner.clone() }
    }
}

impl<U: Upstream> std::fmt::Debug for McpBridge<U> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("McpBridge").field("service", &self.inner.upstream.service()).field("path", &self.inner.config.path).finish()
    }
}

impl<U: Upstream> McpBridge<U> {
    pub fn new(upstream: U, config: McpConfig) -> Self {
        Self { inner: Arc::new(Inner { upstream, config, fixed: Credential::Anonymous, cache: Mutex::new(HashMap::new()) }) }
    }

    /// The credential presented upstream when a request carries none of
    /// its own — stdio, where there is no `Authorization` header.
    pub fn with_fixed_credential(self, credential: Credential) -> Self {
        let inner = Arc::try_unwrap(self.inner).unwrap_or_else(|arc| Inner {
            upstream: unreachable_clone(&arc),
            config: arc.config.clone(),
            fixed: arc.fixed.clone(),
            cache: Mutex::new(HashMap::new()),
        });
        Self { inner: Arc::new(Inner { fixed: credential, ..inner }) }
    }

    pub fn config(&self) -> &McpConfig {
        &self.inner.config
    }

    pub fn upstream(&self) -> &U {
        &self.inner.upstream
    }

    /// The caller of one request: what the HTTP layer resolved, else the
    /// fixed credential, reached through the upstream.
    async fn caller(&self, ctx: &RequestContext<RoleServer>) -> Result<(Credential, DynTransport), ErrorData> {
        if let Some(parts) = ctx.extensions.get::<http::request::Parts>() {
            if let Some(caller) = parts.extensions.get::<CallerTransport>() {
                return Ok((caller.credential.clone(), caller.transport.clone()));
            }
        }
        let credential = self.inner.fixed.clone();
        let transport = self.inner.upstream.transport_for(&credential).await.map_err(upstream_error)?;
        Ok((credential, transport))
    }

    /// The caller's tools: re-read when the view's hash moved.
    async fn tools_for(&self, credential: &Credential, transport: &DynTransport) -> Result<Arc<ToolSet>, ErrorData> {
        let service = self.inner.upstream.service();
        let hash = tools::view_hash(transport, service).await.map_err(read_error)?;
        if let Some(set) = self.inner.cache.lock().await.get(credential) {
            if set.hash == hash {
                return Ok(set.clone());
            }
        }
        let set = Arc::new(tools::read(transport, service).await.map_err(read_error)?);
        self.inner.cache.lock().await.insert(credential.clone(), set.clone());
        Ok(set)
    }
}

/// `with_fixed_credential` is called once, before the bridge is shared;
/// a shared bridge cannot be rebuilt around a new credential.
fn unreachable_clone<U>(_: &Arc<Inner<U>>) -> U {
    panic!("with_fixed_credential must be called before the bridge is shared")
}

fn upstream_error(e: UpstreamError) -> ErrorData {
    match e {
        UpstreamError::Unauthorized => ErrorData::invalid_request("the backend refused the credential", None),
        UpstreamError::Unreachable(m) => ErrorData::internal_error(format!("the backend is unreachable: {m}"), None),
    }
}

fn read_error(e: tools::ReadError) -> ErrorData {
    ErrorData::internal_error(e.to_string(), None)
}

impl<U: Upstream> ServerHandler for McpBridge<U> {
    fn get_info(&self) -> ServerConfig {
        let server = &self.inner.config.server;
        let name = server.name.clone().unwrap_or_else(|| self.inner.upstream.service().to_string());
        let version = server.version.clone().unwrap_or_else(|| crate::VERSION.to_string());
        let mut info = ServerConfig::new(ServerCapabilities::builder().enable_tools().enable_tool_list_changed().build());
        info.server_info = Implementation::new(name, version);
        info.instructions = server.instructions.clone();
        info
    }

    async fn list_tools(&self, _request: Option<PaginatedRequestParams>, ctx: RequestContext<RoleServer>) -> Result<ListToolsResult, ErrorData> {
        let (credential, transport) = self.caller(&ctx).await?;
        let set = self.tools_for(&credential, &transport).await?;
        tracing::debug!(tools = set.tools.len(), "tools/list");
        Ok(ListToolsResult::with_all_items(set.tools.clone()))
    }

    async fn call_tool(&self, request: CallToolRequestParams, ctx: RequestContext<RoleServer>) -> Result<CallToolResponse, ErrorData> {
        let (credential, transport) = self.caller(&ctx).await?;
        let name = request.name.to_string();
        let set = self.tools_for(&credential, &transport).await?;
        let Some(tool) = set.refs.get(&name) else {
            return Err(ErrorData::invalid_params(format!("unknown tool: {name}"), None));
        };
        let args = serde_json::Value::Object(request.arguments.unwrap_or_default());
        let result = crate::call::run(&transport, tool, args, &ctx, &self.inner.config.limits).await;
        Ok(result.into())
    }
}
