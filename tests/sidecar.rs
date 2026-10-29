//! TRANSPORT-16 · the sidecar against a gated fixture backend, over its
//! real WebSocket, driven by an rmcp 3.x client over streamable HTTP.

mod common;

use std::net::SocketAddr;
use std::time::Duration;

use common::backend;
use plexus_transport_mcp::{router, McpBridge, McpConfig, OAuthConfig, SocketUpstream};
use rmcp::model::CallToolRequestParams;
use rmcp::transport::streamable_http_client::StreamableHttpClientTransportConfig;
use rmcp::transport::StreamableHttpClientTransport;
use rmcp::ServiceExt;
use serde_json::{json, Value};

/// The sidecar in front of `backend`, configured once its port is known.
async fn sidecar(backend: SocketAddr, configure: impl FnOnce(SocketAddr) -> McpConfig) -> SocketAddr {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let config = configure(addr);
    let bridge = McpBridge::new(SocketUpstream::new(format!("ws://{backend}"), "menger"), config);
    tokio::spawn(async move {
        axum::serve(listener, router(bridge)).await.unwrap();
    });
    addr
}

type Client = rmcp::service::RunningService<rmcp::RoleClient, ()>;

async fn client(addr: SocketAddr, bearer: Option<&str>) -> Client {
    let mut config = StreamableHttpClientTransportConfig::with_uri(format!("http://{addr}/mcp"));
    if let Some(b) = bearer {
        config = config.auth_header(b);
    }
    let transport = StreamableHttpClientTransport::with_client(reqwest::Client::default(), config);
    ().serve(transport).await.expect("initialize")
}

async fn names(c: &Client) -> Vec<String> {
    let mut names: Vec<String> = c.peer().list_all_tools().await.unwrap().into_iter().map(|t| t.name.to_string()).collect();
    names.sort();
    names
}

async fn call(c: &Client, name: &str, args: Value) -> rmcp::model::CallToolResult {
    let params = CallToolRequestParams::new(name.to_string()).with_arguments(args.as_object().cloned().unwrap_or_default());
    c.peer().call_tool(params).await.expect("the call is answered")
}

fn text(r: &rmcp::model::CallToolResult) -> String {
    r.content.iter().filter_map(|c| c.as_text().map(|t| t.text.clone())).collect::<Vec<_>>().join("")
}

#[tokio::test(flavor = "multi_thread")]
async fn an_editor_lists_and_calls_what_it_holds() {
    let backend = backend().await;
    let mcp = sidecar(backend, |_| McpConfig::default()).await;
    let ada = client(mcp, Some("ada")).await;

    assert_eq!(names(&ada).await, ["tickets.door", "tickets.get", "tickets.list", "tickets.set_status", "tickets.tick"]);
    let tools = ada.peer().list_all_tools().await.unwrap();
    let get = tools.iter().find(|t| t.name == "tickets.get").unwrap();
    assert_eq!(get.annotations.as_ref().unwrap().read_only_hint, Some(true), "GET is read-only");
    assert_eq!(get.input_schema["type"], "object");
    assert!(get.input_schema["properties"]["id"].is_object(), "the params schema: {:?}", get.input_schema);
    let set = tools.iter().find(|t| t.name == "tickets.set_status").unwrap();
    assert_eq!(set.annotations.as_ref().unwrap().read_only_hint, Some(false));

    let r = call(&ada, "tickets.get", json!({"id": "AMP-4"})).await;
    assert_ne!(r.is_error, Some(true), "{r:?}");
    assert_eq!(text(&r), "ticket AMP-4");
    let r = call(&ada, "tickets.list", json!({})).await;
    assert_eq!(r.structured_content.as_ref().unwrap()["items"], json!(["AMP-1", "AMP-2"]));
    let r = call(&ada, "tickets.set_status", json!({"id": "AMP-4", "status": "Ready"})).await;
    assert_eq!(text(&r), "AMP-4 -> Ready");

    // Listed by no role: not a tool, and not callable by name either.
    let err = ada.peer().call_tool(CallToolRequestParams::new("tickets.delete").with_arguments(json!({"id": "AMP-4"}).as_object().cloned().unwrap())).await;
    assert!(err.is_err(), "delete is outside the view");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_viewer_sees_no_write_and_cannot_call_one() {
    let backend = backend().await;
    let mcp = sidecar(backend, |_| McpConfig::default()).await;
    let cat = client(mcp, Some("cat")).await;
    assert_eq!(names(&cat).await, ["tickets.door", "tickets.get", "tickets.list", "tickets.tick"]);
    let err = cat.peer().call_tool(CallToolRequestParams::new("tickets.set_status").with_arguments(json!({"id": "AMP-4", "status": "Ready"}).as_object().cloned().unwrap())).await;
    assert!(err.is_err(), "a write the viewer does not hold");
}

#[tokio::test(flavor = "multi_thread")]
async fn no_credential_is_the_anonymous_view() {
    let backend = backend().await;
    let mcp = sidecar(backend, |_| McpConfig::default()).await;
    let nobody = client(mcp, None).await;
    assert_eq!(names(&nobody).await, ["tickets.door"]);
    assert_eq!(text(&call(&nobody, "tickets.door", json!({})).await), "in");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_stream_past_the_cap_ends_with_a_typed_error() {
    let backend = backend().await;
    let mcp = sidecar(backend, |_| {
        let mut c = McpConfig::default();
        c.limits.items = 5;
        c
    })
    .await;
    let ada = client(mcp, Some("ada")).await;
    let r = tokio::time::timeout(Duration::from_secs(10), call(&ada, "tickets.tick", json!({}))).await.expect("no hang");
    assert_eq!(r.is_error, Some(true));
    let body = r.structured_content.unwrap();
    assert_eq!(body["error"]["code"], "limit");
    assert_eq!(body["items"].as_array().unwrap().len(), 5);
}

async fn raw_initialize(mcp: SocketAddr, bearer: Option<&str>) -> reqwest::Response {
    let mut req = reqwest::Client::default()
        .post(format!("http://{mcp}/mcp"))
        .header("accept", "application/json, text/event-stream")
        .json(&json!({"jsonrpc": "2.0", "id": 1, "method": "initialize",
            "params": {"protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": {"name": "t", "version": "0"}}}));
    if let Some(b) = bearer {
        req = req.bearer_auth(b);
    }
    req.send().await.unwrap()
}

#[tokio::test(flavor = "multi_thread")]
async fn under_oauth_a_missing_or_refused_token_is_a_401_naming_the_metadata() {
    let backend = backend().await;
    let mcp = sidecar(backend, |addr| {
        McpConfig::default().with_oauth(OAuthConfig {
            resource: format!("http://{addr}/mcp"),
            authorization_servers: vec!["https://login.example".into()],
            scopes: Some(vec!["tickets.get".into()]),
        })
    })
    .await;
    let metadata = format!("http://{mcp}/.well-known/oauth-protected-resource/mcp");

    let r = raw_initialize(mcp, None).await;
    assert_eq!(r.status(), 401);
    let challenge = r.headers()["www-authenticate"].to_str().unwrap().to_string();
    assert_eq!(challenge, format!("Bearer resource_metadata=\"{metadata}\", scope=\"tickets.get\""));

    let r = raw_initialize(mcp, Some("mallory")).await;
    assert_eq!(r.status(), 401);
    assert!(r.headers()["www-authenticate"].to_str().unwrap().contains("error=\"invalid_token\""));

    let doc: Value = reqwest::get(&metadata).await.unwrap().json().await.unwrap();
    assert_eq!(doc["resource"], format!("http://{mcp}/mcp"));
    assert_eq!(doc["authorization_servers"], json!(["https://login.example"]));
    assert_eq!(doc["scopes_supported"], json!(["tickets.get"]));

    // A good token signs in as before.
    let ada = client(mcp, Some("ada")).await;
    assert!(names(&ada).await.contains(&"tickets.set_status".to_string()));
}

#[tokio::test(flavor = "multi_thread")]
async fn without_oauth_a_refused_token_is_a_401_with_a_bare_realm() {
    let backend = backend().await;
    let mcp = sidecar(backend, |_| McpConfig::default()).await;
    let r = raw_initialize(mcp, Some("mallory")).await;
    assert_eq!(r.status(), 401);
    assert_eq!(r.headers()["www-authenticate"].to_str().unwrap(), "Bearer realm=\"plexus\", error=\"invalid_token\"");
}
