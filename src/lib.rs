//! # plexus-transport-mcp
//!
//! Any Plexus RPC backend as an MCP server (TRANSPORT-16).
//!
//! The bridge is a **client** of Plexus RPC: it speaks
//! [`plexus_client::Transport`] on one side and MCP (rmcp 3.x) on the
//! other. Beside a running backend it opens the backend's WebSocket as the
//! caller ([`SocketUpstream`]); inside a backend's binary an in-process
//! [`Upstream`] calls the service in memory as the caller. Either way every
//! `tools/call` is `{service}.call` as that caller, so the backend's gate
//! decides it exactly as it decides a WebSocket call, and `tools/list` is
//! what the backend's `hashes` and `<ns>.schema` answer that caller — the
//! caller's view (AUTH-4), read off the wire.
//!
//! The bridge validates no credential of its own: the `Authorization:
//! Bearer` it is shown is the one it presents upstream. A refused upgrade
//! is a 401 to the MCP client, with RFC 9728's `resource_metadata` when an
//! `oauth` table is configured, so an MCP client knows where to sign in.
//!
//! ```text
//!   MCP client ──HTTP · stdio──► McpBridge ──Transport──► the backend's gate
//! ```
//!
//! - [`config::McpConfig`] — plain data: a TOML table, flags or a builder.
//! - [`upstream`] — [`Upstream`], [`SocketUpstream`], [`Credential`].
//! - [`bridge::McpBridge`] — the rmcp `ServerHandler`.
//! - [`http::router`] — the axum router: identify, `/mcp`, the metadata.
//! - [`stdio::serve_stdio`] — the same bridge on stdin and stdout.
//! - [`proposal`] — a method's proposal (`needs_input`) as a tool result
//!   that says what would happen and how to call again with `confirm`.

#![forbid(unsafe_code)]

pub mod bridge;
pub mod call;
pub mod config;
pub mod http;
pub mod proposal;
pub mod protected_resource;
pub mod stdio;
pub mod tools;
pub mod upstream;

pub use bridge::McpBridge;
pub use config::{Limits, McpConfig, OAuthConfig, ServerIdentity, UpstreamConfig};
pub use http::router;
pub use protected_resource::ProtectedResource;
pub use stdio::serve_stdio;
pub use upstream::{Credential, DynTransport, SocketUpstream, Upstream, UpstreamError};

/// Crate version, for `_info`-style manifests.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
