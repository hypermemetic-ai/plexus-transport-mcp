//! `plexus-transport-mcp`: any running Plexus RPC backend as an MCP server.
//!
//! ```sh
//! plexus-transport-mcp --url ws://127.0.0.1:4455 --service menger --listen 127.0.0.1:4459
//! plexus-transport-mcp --url ws://127.0.0.1:4455 --service menger --stdio --bearer-file ~/.plexus/mcp.token
//! plexus-transport-mcp --config mcp.toml
//! ```
//!
//! Flags win over the file; the file holds the table `McpConfig` reads.

use std::net::SocketAddr;
use std::path::PathBuf;

use clap::Parser;
use plexus_transport_mcp::{router, serve_stdio, Credential, McpBridge, McpConfig, OAuthConfig, SocketUpstream, UpstreamConfig};

#[derive(Parser, Debug)]
#[command(name = "plexus-transport-mcp", version, about = "Any Plexus RPC backend as an MCP server")]
struct Args {
    /// The backend's WebSocket, e.g. ws://127.0.0.1:4455.
    #[arg(long)]
    url: Option<String>,
    /// The backend's service namespace (what its `call` is prefixed with).
    #[arg(long)]
    service: Option<String>,
    /// Where to listen for MCP over HTTP.
    #[arg(long)]
    listen: Option<SocketAddr>,
    /// The path the endpoint is served at.
    #[arg(long)]
    path: Option<String>,
    /// Serve MCP on stdin and stdout instead of HTTP.
    #[arg(long)]
    stdio: bool,
    /// A file holding the bearer token presented upstream on stdio (never
    /// the token itself on the command line).
    #[arg(long)]
    bearer_file: Option<PathBuf>,
    /// A TOML file with the `McpConfig` table.
    #[arg(long)]
    config: Option<PathBuf>,
    /// A `Host` value to answer to (repeatable).
    #[arg(long = "allowed-host")]
    allowed_hosts: Vec<String>,
    /// The OAuth 2.1 resource identifier of this endpoint (its public URL).
    #[arg(long)]
    resource: Option<String>,
    /// An authorization server that mints tokens for it (repeatable).
    #[arg(long = "authorization-server")]
    authorization_servers: Vec<String>,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt().with_env_filter(tracing_subscriber::EnvFilter::from_default_env()).with_writer(std::io::stderr).init();
    let args = Args::parse();

    let mut config: McpConfig = match &args.config {
        Some(path) => toml::from_str(&std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?)?,
        None => McpConfig::default(),
    };
    if let (Some(url), Some(service)) = (&args.url, &args.service) {
        config.upstream = Some(UpstreamConfig { url: url.clone(), service: service.clone() });
    } else if args.url.is_some() || args.service.is_some() {
        return Err("--url and --service go together".into());
    }
    if let Some(listen) = args.listen {
        config.listen = Some(listen);
    }
    if let Some(path) = &args.path {
        config.path = path.clone();
    }
    if !args.allowed_hosts.is_empty() {
        config.allowed_hosts = args.allowed_hosts.clone();
    }
    if let Some(resource) = &args.resource {
        let mut oauth = config.oauth.take().unwrap_or_default();
        oauth.resource = resource.clone();
        if !args.authorization_servers.is_empty() {
            oauth.authorization_servers = args.authorization_servers.clone();
        }
        config.oauth = Some(oauth);
    } else if !args.authorization_servers.is_empty() {
        let mut oauth: OAuthConfig = config.oauth.take().unwrap_or_default();
        oauth.authorization_servers = args.authorization_servers.clone();
        config.oauth = Some(oauth);
    }

    let Some(upstream) = config.upstream.clone() else {
        return Err("no backend: give --url and --service, or [upstream] in the config".into());
    };
    let bridge = McpBridge::new(SocketUpstream::new(&upstream.url, &upstream.service), config.clone());

    if args.stdio {
        let credential = match &args.bearer_file {
            Some(path) => {
                let token = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
                Credential::Bearer(token.trim().to_string())
            }
            None => Credential::Anonymous,
        };
        tracing::info!(backend = %upstream.url, service = %upstream.service, "MCP on stdio");
        serve_stdio(bridge.with_fixed_credential(credential)).await?;
        return Ok(());
    }

    let listen = config.listen.unwrap_or_else(|| "127.0.0.1:4459".parse().expect("a socket address"));
    let listener = tokio::net::TcpListener::bind(listen).await.map_err(|e| format!("cannot listen on {listen}: {e}"))?;
    let bound = listener.local_addr()?;
    tracing::info!(backend = %upstream.url, service = %upstream.service, "MCP at http://{bound}{}", config.path);
    if let Some(o) = &config.oauth {
        tracing::info!(resource = %o.resource, "an OAuth 2.1 resource server; metadata at {}", plexus_transport_mcp::ProtectedResource::from(o).metadata_url());
    }
    axum::serve(listener, router(bridge)).with_graceful_shutdown(shutdown()).await?;
    Ok(())
}

async fn shutdown() {
    #[cfg(unix)]
    {
        use tokio::signal::unix::{signal, SignalKind};
        if let Ok(mut term) = signal(SignalKind::terminate()) {
            tokio::select! {
                _ = tokio::signal::ctrl_c() => {}
                _ = term.recv() => {}
            }
            return;
        }
    }
    let _ = tokio::signal::ctrl_c().await;
}
