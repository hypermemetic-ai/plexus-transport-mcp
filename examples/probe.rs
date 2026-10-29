//! A probe for an MCP endpoint: lists the tools a credential may see and,
//! given `--call`, runs one.
//!
//! ```sh
//! cargo run --example probe -- http://127.0.0.1:4459/mcp --bearer-file ~/.plexus/mcp.token --call tickets.health '{}'
//! ```

use std::path::PathBuf;

use rmcp::model::CallToolRequestParams;
use rmcp::transport::streamable_http_client::StreamableHttpClientTransportConfig;
use rmcp::transport::StreamableHttpClientTransport;
use rmcp::ServiceExt;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let url = args.next().ok_or("usage: probe <url> [--bearer-file FILE] [--call NAME JSON]")?;
    let (mut bearer_file, mut call): (Option<PathBuf>, Option<(String, String)>) = (None, None);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--bearer-file" => bearer_file = Some(args.next().ok_or("--bearer-file FILE")?.into()),
            "--call" => call = Some((args.next().ok_or("--call NAME JSON")?, args.next().unwrap_or_else(|| "{}".into()))),
            other => return Err(format!("unknown argument {other}").into()),
        }
    }
    let mut config = StreamableHttpClientTransportConfig::with_uri(url);
    if let Some(path) = bearer_file {
        config = config.auth_header(std::fs::read_to_string(path)?.trim().to_string());
    }
    let client = ().serve(StreamableHttpClientTransport::with_client(reqwest::Client::default(), config)).await?;
    let info = client.peer().peer_info();
    let server = info.and_then(|i| i.server_info.as_ref().map(|s| format!("{} {}", s.name, s.version))).unwrap_or_default();
    println!("server: {server}");
    let tools = client.peer().list_all_tools().await?;
    println!("tools: {}", tools.len());
    for t in &tools {
        let ro = t.annotations.as_ref().and_then(|a| a.read_only_hint).unwrap_or(false);
        println!("  {}{}  {}", t.name, if ro { " (read-only)" } else { "" }, t.description.as_deref().unwrap_or(""));
    }
    if let Some((name, json)) = call {
        let args: serde_json::Value = serde_json::from_str(&json)?;
        let params = CallToolRequestParams::new(name).with_arguments(args.as_object().cloned().unwrap_or_default());
        let r = client.peer().call_tool(params).await?;
        println!("is_error: {}", r.is_error.unwrap_or(false));
        for c in &r.content {
            if let Some(t) = c.as_text() {
                println!("{}", t.text);
            }
        }
    }
    client.cancel().await?;
    Ok(())
}
