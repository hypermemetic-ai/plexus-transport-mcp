//! `tools/call`: one `{hub}.call` as the caller, its stream mapped onto one
//! `CallToolResult` — progress relayed when the client gave a token, and
//! the caps of [`Limits`] ending the call with a typed error rather than
//! a hang.

use std::time::Duration;

use futures::StreamExt;
use plexus_client::StreamItem;
use rmcp::model::{CallToolResult, ContentBlock, ProgressNotificationParam};
use rmcp::service::{RequestContext, RoleServer};
use serde_json::{json, Value};

use crate::config::Limits;
use crate::tools::ToolRef;
use crate::upstream::DynTransport;

/// Run `tool` as the caller with `args`.
pub async fn run(transport: &DynTransport, tool: &ToolRef, args: Value, ctx: &RequestContext<RoleServer>, limits: &Limits) -> CallToolResult {
    let mut stream = transport.call(tool.method_ref(), args);
    let progress_token = ctx.meta.get_progress_token();
    let deadline = tokio::time::sleep(Duration::from_secs(limits.call_secs));
    tokio::pin!(deadline);

    let mut items: Vec<Value> = Vec::new();
    let mut bytes = 0usize;
    let mut progress_count = 0.0f64;

    loop {
        let next = tokio::select! {
            _ = &mut deadline => {
                return failed(items, "timeout", format!("the call ran past {} s", limits.call_secs));
            }
            _ = ctx.ct.cancelled() => {
                return failed(items, "cancelled", "the client cancelled the call");
            }
            next = stream.next() => next,
        };
        match next {
            None => break,
            Some(Err(e)) => {
                let code = format!("{:?}", e.kind).to_ascii_lowercase();
                return failed(items, code, e.message);
            }
            Some(Ok(StreamItem::Data { content, .. })) => {
                bytes += content.to_string().len();
                if items.len() >= limits.items {
                    return failed(items, "limit", format!("more than {} items; narrow the call", limits.items));
                }
                if bytes > limits.bytes {
                    return failed(items, "limit", format!("more than {} bytes; narrow the call", limits.bytes));
                }
                // A streaming method's items are not relayed one by one:
                // MCP's logging notifications, the only channel for it,
                // are deprecated by SEP-2577 (revision 2026-07-28). The
                // collected items are the result; progress still flows.
                items.push(content);
            }
            Some(Ok(StreamItem::Progress { message, percentage, .. })) => {
                if let Some(token) = &progress_token {
                    progress_count += 1.0;
                    let mut p = ProgressNotificationParam::new(token.clone(), percentage.map(f64::from).unwrap_or(progress_count));
                    p.total = percentage.map(|_| 100.0);
                    p.message = Some(message);
                    let _ = ctx.peer.notify_progress(p).await;
                }
            }
            Some(Ok(StreamItem::Error { message, code, .. })) => {
                return failed(items, code.unwrap_or_else(|| "error".to_string()), message);
            }
            Some(Ok(StreamItem::Request { request_id, .. })) => {
                tracing::debug!(tool = %tool.name, request_id, "a bidirectional request has no answer channel over MCP");
            }
            Some(Ok(StreamItem::Done { .. })) => break,
        }
    }
    succeeded(items)
}

/// The items as one text content: a bare string as itself, one value as
/// its JSON, many as a JSON array.
fn succeeded(items: Vec<Value>) -> CallToolResult {
    let text = match items.as_slice() {
        [] => "(no output)".to_string(),
        [Value::String(s)] => s.clone(),
        [one] => serde_json::to_string_pretty(one).unwrap_or_default(),
        many => serde_json::to_string_pretty(many).unwrap_or_default(),
    };
    let mut result = CallToolResult::success(vec![ContentBlock::text(text)]);
    result.structured_content = Some(json!({ "items": items }));
    result
}

/// A typed failure, with the items collected before it.
fn failed(items: Vec<Value>, code: impl Into<String>, message: impl Into<String>) -> CallToolResult {
    let (code, message) = (code.into(), message.into());
    let body = json!({ "error": { "code": code, "message": message }, "items": items });
    let mut result = CallToolResult::error(vec![ContentBlock::text(format!("{code}: {message}"))]);
    result.structured_content = Some(body);
    result
}
