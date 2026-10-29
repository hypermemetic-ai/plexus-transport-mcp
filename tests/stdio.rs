//! TRANSPORT-16 · the same bridge on stdin and stdout: the built binary,
//! run as an MCP client runs a command, its credential from a file.

mod common;

use std::time::Duration;

use common::backend;
use rmcp::model::CallToolRequestParams;
use rmcp::transport::TokioChildProcess;
use rmcp::ServiceExt;
use serde_json::json;

#[tokio::test(flavor = "multi_thread")]
async fn stdio_lists_and_calls_as_the_bearer_file_says() {
    let backend = backend().await;
    let token = std::env::temp_dir().join(format!("plexus-transport-mcp-{}.token", std::process::id()));
    std::fs::write(&token, "ada\n").unwrap();

    let mut cmd = tokio::process::Command::new(env!("CARGO_BIN_EXE_plexus-transport-mcp"));
    cmd.args(["--url", &format!("ws://{backend}"), "--service", "menger", "--stdio", "--bearer-file"]).arg(&token);
    let client = tokio::time::timeout(Duration::from_secs(20), ().serve(TokioChildProcess::new(cmd).unwrap()))
        .await
        .expect("the child answers initialize")
        .expect("initialize");

    let mut names: Vec<String> = client.peer().list_all_tools().await.unwrap().into_iter().map(|t| t.name.to_string()).collect();
    names.sort();
    assert_eq!(names, ["tickets.door", "tickets.get", "tickets.list", "tickets.set_status", "tickets.tick"]);

    let r = client
        .peer()
        .call_tool(CallToolRequestParams::new("tickets.get").with_arguments(json!({"id": "AMP-4"}).as_object().cloned().unwrap()))
        .await
        .unwrap();
    let text: String = r.content.iter().filter_map(|c| c.as_text().map(|t| t.text.clone())).collect();
    assert_eq!(text, "ticket AMP-4");

    client.cancel().await.unwrap();
    let _ = std::fs::remove_file(&token);
}
