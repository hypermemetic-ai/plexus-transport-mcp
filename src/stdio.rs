//! The same bridge on stdin and stdout, for an MCP client that runs a
//! command. There is no `Authorization` header here: the credential is
//! the bridge's fixed one (`McpBridge::with_fixed_credential`), and MCP's
//! own guidance applies — a stdio server takes its credentials from its
//! environment.

use rmcp::ServiceExt;

use crate::bridge::McpBridge;
use crate::upstream::Upstream;

/// Serve until stdin closes. KEEL-59: its close is every open call's
/// cancel, at once ([`crate::gone::Watched`]).
pub async fn serve_stdio<U: Upstream>(bridge: McpBridge<U>) -> Result<(), StdioError> {
    let (stdin, stdout) = rmcp::transport::stdio();
    let transport = crate::gone::Watched::new(rmcp::transport::async_rw::AsyncRwTransport::new_server(stdin, stdout));
    let running = bridge.serve(transport).await.map_err(|e| StdioError(e.to_string()))?;
    running.waiting().await.map_err(|e| StdioError(e.to_string()))?;
    Ok(())
}

#[derive(Debug, thiserror::Error)]
#[error("stdio: {0}")]
pub struct StdioError(String);
