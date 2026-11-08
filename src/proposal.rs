//! ASK-5: a method's proposal (`needs_input`, plexus-wire spec/08) as a
//! tool result.
//!
//! A method marked `asks` that would change something finishes its call
//! with a `needs_input` item — what it would do, the values it needs and a
//! token — having done nothing. MCP has no second call of its own for it,
//! and the bridge does not use elicitation: the tool result says plainly
//! what would happen and how to carry it out — the same tool, the same
//! arguments, plus `confirm` and any field's value under its own name.
//! It is a successful result (the call finished), never an error.
//!
//! ```text
//! → tools/call tickets.archive { filter: "label stale" }
//! ← "Nothing was done yet: tickets.archive proposes … call tickets.archive again with … \"confirm\": \"v1.…\""
//! → tools/call tickets.archive { filter: "label stale", confirm: "v1.…" }
//! ← the method's items
//! ```

use rmcp::model::{CallToolResult, ContentBlock};
use serde_json::{json, Map, Value};

/// The proposal a `data` item carries, if it is one: an object whose
/// `type` is `needs_input` and which has a `token`.
pub fn needs_input(content: &Value) -> Option<&Value> {
    let obj = content.as_object()?;
    (obj.get("type").and_then(Value::as_str) == Some("needs_input") && obj.get("token").is_some_and(Value::is_string)).then_some(content)
}

/// The arguments of the confirming call: `args` as the caller sent them
/// (any older `confirm` replaced), plus `confirm`.
pub fn confirming_args(args: &Value, proposal: &Value) -> Value {
    let mut again: Map<String, Value> = args.as_object().cloned().unwrap_or_default();
    again.insert("confirm".into(), proposal.get("token").cloned().unwrap_or(Value::Null));
    Value::Object(again)
}

/// The proposal as plain text: what would happen, each effect, each field,
/// and the call that carries it out.
pub fn describe(tool: &str, args: &Value, proposal: &Value) -> String {
    let summary = proposal.get("summary").and_then(Value::as_str).unwrap_or("(no summary)");
    let effects = proposal.get("effects").and_then(Value::as_array).cloned().unwrap_or_default();
    let fields = proposal.get("fields").and_then(Value::as_array).cloned().unwrap_or_default();
    let expires = proposal.get("expires").and_then(Value::as_str);

    let mut out = format!("Nothing was done yet: {tool} proposes this and waits for a confirmation.\n\nWould: {summary}\n");
    if !effects.is_empty() {
        out.push_str("\nEffects:\n");
        for e in &effects {
            out.push_str(&format!("- {}\n", inline(e)));
        }
    }
    if !fields.is_empty() {
        out.push_str("\nFields to fill, each a parameter of this tool, sent under its own name:\n");
        for f in &fields {
            out.push_str(&format!("- {}\n", field_line(f)));
        }
    }
    let again = confirming_args(args, proposal);
    let fill = if fields.is_empty() { "" } else { " and a value for each field above" };
    out.push_str(&format!(
        "\nTo carry it out, call {tool} again with the same arguments plus \"confirm\"{fill}:\n{}\n",
        serde_json::to_string(&again).unwrap_or_default()
    ));
    match expires {
        Some(at) => out.push_str(&format!(
            "\nThe token stands until {at}. Past then, or if anything it covers changes first, the call proposes again and does nothing.\n"
        )),
        None => out.push_str("\nIf anything the token covers changes first, the call proposes again and does nothing.\n"),
    }
    out
}

/// The tool result of a call that answered a proposal: the text of
/// [`describe`], and as structured content the proposal itself, the
/// confirming call's arguments, and the call's items.
pub fn result(tool: &str, args: &Value, proposal: &Value, items: Vec<Value>) -> CallToolResult {
    let mut r = CallToolResult::success(vec![ContentBlock::text(describe(tool, args, proposal))]);
    r.structured_content = Some(json!({
        "needs_input": proposal,
        "confirm_with": confirming_args(args, proposal),
        "items": items,
    }));
    r
}

fn inline(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        other => serde_json::to_string(other).unwrap_or_default(),
    }
}

fn field_line(f: &Value) -> String {
    let name = f.get("name").and_then(Value::as_str).unwrap_or("?");
    let mut parts: Vec<String> = Vec::new();
    if let Some(options) = f.get("options").and_then(Value::as_array) {
        let opts: Vec<String> = options.iter().map(|o| serde_json::to_string(o).unwrap_or_default()).collect();
        parts.push(format!("one of {}", opts.join(", ")));
    }
    if let Some(d) = f.get("default") {
        parts.push(format!("default {}", serde_json::to_string(d).unwrap_or_default()));
    }
    let mut line = name.to_string();
    if !parts.is_empty() {
        line.push_str(&format!(" ({})", parts.join("; ")));
    }
    if let Some(help) = f.get("help").and_then(Value::as_str) {
        line.push_str(&format!(": {help}"));
    }
    line
}

#[cfg(test)]
mod tests {
    use super::*;

    fn proposal() -> Value {
        json!({
            "type": "needs_input",
            "summary": "delete KILN-2 (4 children)",
            "effects": ["KILN-2", {"children": 4}],
            "fields": [{"name": "children", "options": ["reparent", "delete"], "default": "reparent", "help": "what moves"}],
            "token": "v1.1800000000.abc",
            "expires": "2027-01-15T08:00:00Z"
        })
    }

    #[test]
    fn a_proposal_is_recognised_and_nothing_else_is() {
        assert!(needs_input(&proposal()).is_some());
        assert!(needs_input(&json!({"type": "needs_input"})).is_none(), "no token");
        assert!(needs_input(&json!({"type": "archived", "token": "x"})).is_none());
        assert!(needs_input(&json!("needs_input")).is_none());
    }

    #[test]
    fn the_text_says_what_would_happen_and_the_call_that_does_it() {
        let args = json!({"ids": ["KILN-2"], "confirm": "old"});
        let text = describe("tickets.delete", &args, &proposal());
        // The confirming call's JSON, in whatever key order serde_json keeps.
        let again = serde_json::to_string(&json!({"ids": ["KILN-2"], "confirm": "v1.1800000000.abc"})).unwrap();
        assert_eq!(
            text.replace(&again, "<AGAIN>"),
            "Nothing was done yet: tickets.delete proposes this and waits for a confirmation.\n\
             \n\
             Would: delete KILN-2 (4 children)\n\
             \n\
             Effects:\n\
             - KILN-2\n\
             - {\"children\":4}\n\
             \n\
             Fields to fill, each a parameter of this tool, sent under its own name:\n\
             - children (one of \"reparent\", \"delete\"; default \"reparent\"): what moves\n\
             \n\
             To carry it out, call tickets.delete again with the same arguments plus \"confirm\" and a value for each field above:\n\
             <AGAIN>\n\
             \n\
             The token stands until 2027-01-15T08:00:00Z. Past then, or if anything it covers changes first, the call proposes again and does nothing.\n"
        );
    }

    #[test]
    fn the_result_is_a_success_carrying_the_proposal() {
        let r = result("tickets.delete", &json!({"ids": ["KILN-2"]}), &proposal(), vec![proposal()]);
        assert_ne!(r.is_error, Some(true));
        let s = r.structured_content.unwrap();
        assert_eq!(s["needs_input"]["token"], "v1.1800000000.abc");
        assert_eq!(s["confirm_with"], json!({"ids": ["KILN-2"], "confirm": "v1.1800000000.abc"}));
    }
}
