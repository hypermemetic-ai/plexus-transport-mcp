//! `tools/list`, read off the wire: the backend's `hashes` names the
//! activations the caller may see and the view's hash; each `<ns>.schema`
//! (and each static child's, recursively) names the methods. The service
//! answers both from the caller's view (AUTH-4), so the list is the
//! caller's reach by construction.

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex, OnceLock};

use futures::StreamExt;
use plexus_client::{MethodRef, StreamItem};
use rmcp::model::{JsonObject, Tool, ToolAnnotations};
use serde_json::{json, Value};

use crate::upstream::DynTransport;

/// Static children nested deeper than this are not walked.
const MAX_DEPTH: usize = 4;

/// One tool, and the method it is.
#[derive(Debug, Clone)]
pub struct ToolRef {
    /// The tool's name: `tickets.get`, or `tickets.inputs.list` for a child's.
    pub name: String,
    pub namespace: &'static str,
    pub method: &'static str,
    /// The dotted path the activation is reached at (`tickets.inputs`).
    pub path: &'static str,
}

impl ToolRef {
    /// The wire reference for the service's `call`.
    pub fn method_ref(&self) -> MethodRef {
        MethodRef::new(self.namespace, self.method).at(self.path)
    }
}

/// The caller's tools, as read at one view hash.
#[derive(Debug, Clone, Default)]
pub struct ToolSet {
    /// The view's hash: the list is re-read when it changes.
    pub hash: String,
    pub tools: Vec<Tool>,
    pub refs: HashMap<String, ToolRef>,
}

/// Why the tools could not be read.
#[derive(Debug, thiserror::Error)]
pub enum ReadError {
    #[error("the backend answered {method} with an error: {message}")]
    Refused { method: String, message: String },
    #[error("the backend's answer to {method} was not understood: {detail}")]
    Shape { method: String, detail: String },
    #[error("the transport failed on {method}: {message}")]
    Transport { method: String, message: String },
}

/// A method name read off the wire, kept for the life of the process so a
/// `MethodRef` can name it. The set is bounded by the backends' schemas,
/// as plexus-client's `join_path` is by the generated clients'.
pub fn intern(s: &str) -> &'static str {
    static NAMES: OnceLock<Mutex<HashSet<&'static str>>> = OnceLock::new();
    let mut names = NAMES.get_or_init(Default::default).lock().unwrap_or_else(|e| e.into_inner());
    if let Some(existing) = names.get(s) {
        return existing;
    }
    let leaked: &'static str = Box::leak(s.to_string().into_boxed_str());
    names.insert(leaked);
    leaked
}

/// The first `data` item of one call, as JSON.
pub async fn one(transport: &DynTransport, method: MethodRef, params: Value) -> Result<Value, ReadError> {
    let name = method.qualified();
    let mut stream = transport.call(method, params);
    while let Some(item) = stream.next().await {
        match item {
            Ok(StreamItem::Data { content, .. }) => return Ok(content),
            Ok(StreamItem::Error { message, error, .. }) => {
                return Err(ReadError::Refused { method: name, message: message.unwrap_or_else(|| error.to_string()) })
            }
            Ok(StreamItem::Done { .. }) => break,
            Ok(_) => continue,
            Err(e) => return Err(ReadError::Transport { method: name, message: e.message }),
        }
    }
    Err(ReadError::Shape { method: name, detail: "no data item".into() })
}

/// The view's hash alone: one round trip, to know whether to re-read.
pub async fn view_hash(transport: &DynTransport, service: &str) -> Result<String, ReadError> {
    let hashes = one(transport, MethodRef::new(intern(service), "hashes"), json!({})).await?;
    hashes
        .get("hash")
        .and_then(Value::as_str)
        .map(str::to_string)
        .ok_or_else(|| ReadError::Shape { method: format!("{service}.hashes"), detail: "no hash".into() })
}

/// Everything the caller may call, as tools.
pub async fn read(transport: &DynTransport, service: &str) -> Result<ToolSet, ReadError> {
    let hashes = one(transport, MethodRef::new(intern(service), "hashes"), json!({})).await?;
    let hash = hashes.get("hash").and_then(Value::as_str).unwrap_or_default().to_string();
    let namespaces: Vec<String> = hashes
        .get("children")
        .and_then(Value::as_array)
        .map(|kids| kids.iter().filter_map(|c| c.get("namespace").and_then(Value::as_str)).map(str::to_string).collect())
        .unwrap_or_default();

    let mut set = ToolSet { hash, ..Default::default() };
    for ns in namespaces {
        walk(transport, &ns, &ns, 0, &mut set).await?;
    }
    Ok(set)
}

/// One activation's schema, then its static children's.
async fn walk(transport: &DynTransport, ns: &str, path: &str, depth: usize, set: &mut ToolSet) -> Result<(), ReadError> {
    let (ns_s, path_s) = (intern(ns), intern(path));
    let schema = one(transport, MethodRef::new(ns_s, "schema").at(path_s), json!({})).await?;
    let methods = match schema.get("methods").and_then(Value::as_array) {
        Some(m) => m,
        None => return Ok(()),
    };
    let mut children = Vec::new();
    for m in methods {
        let Some(name) = m.get("name").and_then(Value::as_str) else { continue };
        if name == "schema" {
            continue;
        }
        match role_of(m) {
            Role::Rpc => {
                let tool = tool_of(path, name, m);
                set.refs.insert(
                    tool.name.to_string(),
                    ToolRef {
                        name: tool.name.to_string(),
                        namespace: ns_s,
                        method: intern(name),
                        path: path_s,
                    },
                );
                set.tools.push(tool);
            }
            Role::StaticChild => children.push(name.to_string()),
            Role::DynamicChild => {}
        }
    }
    if depth < MAX_DEPTH {
        for child in children {
            let child_path = format!("{path}.{child}");
            // A child's schema is answered through its parent's routing; a
            // child the view does not keep is simply absent.
            if let Err(e) = Box::pin(walk(transport, ns, &child_path, depth + 1, set)).await {
                tracing::debug!(child = %child_path, error = %e, "child schema not read");
            }
        }
    }
    Ok(())
}

enum Role {
    Rpc,
    StaticChild,
    DynamicChild,
}

/// `role` as the schema serializes it, read loosely: a string, or an object
/// with a `type` or a single variant key.
fn role_of(m: &Value) -> Role {
    let word = match m.get("role") {
        None | Some(Value::Null) => return Role::Rpc,
        Some(Value::String(s)) => s.clone(),
        Some(Value::Object(o)) => o
            .get("type")
            .and_then(Value::as_str)
            .map(str::to_string)
            .or_else(|| o.keys().next().cloned())
            .unwrap_or_default(),
        Some(_) => String::new(),
    };
    let word: String = word.to_ascii_lowercase().chars().filter(|c| c.is_ascii_alphabetic()).collect();
    match word.as_str() {
        "staticchild" => Role::StaticChild,
        "dynamicchild" => Role::DynamicChild,
        _ => Role::Rpc,
    }
}

/// One method as a tool: `path.method`, its description, its params with
/// `type: object` at the root (MCP requires it), read-only when the schema
/// says `GET`.
fn tool_of(path: &str, name: &str, m: &Value) -> Tool {
    let description = m.get("description").and_then(Value::as_str).unwrap_or_default().to_string();
    let mut input: JsonObject = match m.get("params") {
        Some(Value::Object(o)) => o.clone(),
        _ => JsonObject::new(),
    };
    input.entry("type".to_string()).or_insert_with(|| json!("object"));
    if !input.contains_key("properties") {
        input.insert("properties".to_string(), json!({}));
    }
    let read_only = m.get("http_method").and_then(Value::as_str).map(|v| v.eq_ignore_ascii_case("get")).unwrap_or(false);
    let mut tool = Tool::new(format!("{path}.{name}"), description, Arc::new(input));
    tool.annotations = Some(ToolAnnotations::new().read_only(read_only));
    tool
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roles_are_read_loosely() {
        assert!(matches!(role_of(&json!({})), Role::Rpc));
        assert!(matches!(role_of(&json!({"role": "rpc"})), Role::Rpc));
        assert!(matches!(role_of(&json!({"role": "static_child"})), Role::StaticChild));
        assert!(matches!(role_of(&json!({"role": {"type": "StaticChild"}})), Role::StaticChild));
        assert!(matches!(role_of(&json!({"role": {"dynamic_child": {"list_method": "list"}}})), Role::DynamicChild));
    }

    #[test]
    fn a_tool_has_an_object_schema_and_a_read_only_hint_for_get() {
        let t = tool_of("tickets", "get", &json!({"description": "One ticket.", "http_method": "GET", "params": {"properties": {"id": {"type": "string"}}}}));
        assert_eq!(t.name, "tickets.get");
        assert_eq!(t.input_schema["type"], "object");
        assert_eq!(t.annotations.as_ref().unwrap().read_only_hint, Some(true));
        let t = tool_of("tickets.inputs", "answer", &json!({"description": "x"}));
        assert_eq!(t.name, "tickets.inputs.answer");
        assert_eq!(t.input_schema["properties"], json!({}));
        assert_eq!(t.annotations.as_ref().unwrap().read_only_hint, Some(false));
    }

    #[test]
    fn interning_gives_one_static_per_name() {
        let a = intern("tickets");
        let b = intern("tickets");
        assert!(std::ptr::eq(a, b));
    }
}
