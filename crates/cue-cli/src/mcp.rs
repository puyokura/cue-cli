// SPDX-License-Identifier: AGPL-3.0-or-later
//! Minimal MCP (Model Context Protocol) server over stdio.
//!
//! Implements `initialize`, `tools/list` and `tools/call`. Each `tools/call` runs
//! on its own thread, so a slow tool (a session join, a prompt round trip) does not
//! delay a later quick request: the read loop keeps reading and responses are
//! written as they finish, serialised by a stdout mutex.

use serde_json::{json, Value};
use std::collections::HashMap;
use std::io::{BufRead, Write};
use std::sync::{Arc, Mutex};

const PROTOCOL_VERSION: &str = "2024-11-05";

fn write_message(lock: &Arc<Mutex<()>>, value: &Value) {
    let _guard = lock.lock().unwrap_or_else(|e| e.into_inner());
    let mut stdout = std::io::stdout().lock();
    let _ = writeln!(stdout, "{value}");
    let _ = stdout.flush();
}

fn tool(
    name: &str,
    description: &str,
    properties: Value,
    required: Vec<&str>,
) -> Value {
    json!({
        "name": name,
        "description": description,
        "inputSchema": {
            "type": "object",
            "properties": properties,
            "required": required,
            "additionalProperties": false,
        },
    })
}

/// The tool surface mirrors the CLI commands that are non-interactive.
fn tools() -> Value {
    json!([
        tool("cue_region", "Check region hints (public RPC, no token).", json!({}), vec![]),
        tool("cue_settings", "Read global settings (public RPC, no token).", json!({}), vec![]),
        tool("cue_whoami", "Read the signed-in user (needs a stored token).", json!({}), vec![]),
        tool("cue_coverage", "Read-only survey of the authenticated surface.", json!({}), vec![]),
        tool("cue_sessions", "List sessions on the account.", json!({
            "limit": { "type": "integer", "description": "maximum number of sessions" }
        }), vec![]),
        tool("cue_session", "Read one session by its 22-character uid.", json!({
            "session_uid": { "type": "string", "description": "session uid" }
        }), vec!["session_uid"]),
        tool("cue_agents", "List agents on the account.", json!({
            "limit": { "type": "integer" }
        }), vec![]),
        tool("cue_rpc", "Call any Connect RPC by path with a JSON body.", json!({
            "method": { "type": "string", "description": "service.Method or /service.Method" },
            "body": { "type": "object", "description": "request body (defaults to {})" }
        }), vec!["method"]),
        tool("cue_send", "Join a session and post a prompt; returns what came back.", json!({
            "session_id": { "type": "string" },
            "text": { "type": "string" },
            "seconds": { "type": "integer", "description": "observation window" }
        }), vec!["session_id", "text"]),
        tool("cue_new", "Start a new session (the client generates the id).", json!({
            "text": { "type": "string" },
            "seconds": { "type": "integer" }
        }), vec!["text"]),
        tool("cue_join", "Subscribe to a session without posting (slow tool).", json!({
            "session_id": { "type": "string" },
            "seconds": { "type": "integer" }
        }), vec!["session_id"]),
        tool("cue_mfa_status", "Read two-step verification state.", json!({}), vec![]),
        tool("cue_discover", "Probe whether Connect routes exist (404/401 oracle).", json!({
            "service": { "type": "string" },
            "methods": { "type": "array", "items": { "type": "string" } }
        }), vec!["service"]),
    ])
}

fn arg_str<'a>(args: &'a Value, key: &str) -> Option<&'a str> {
    args.get(key).and_then(Value::as_str)
}

fn arg_num(args: &Value, key: &str) -> Option<String> {
    args.get(key).and_then(|v| {
        v.as_i64()
            .map(|n| n.to_string())
            .or_else(|| v.as_str().map(|s| s.to_string()))
    })
}

fn call_tool(name: &str, args: &Value) -> Result<String, String> {
    match name {
        "cue_region" => crate::commands::region(),
        "cue_settings" => crate::commands::settings(),
        "cue_whoami" => crate::commands::whoami(),
        "cue_coverage" => crate::commands::coverage(),
        "cue_mfa_status" => {
            let mut flags = HashMap::new();
            flags.insert("sub".to_string(), "status".to_string());
            crate::commands::mfa(Some(&"status".to_string()), &flags)
        }
        "cue_sessions" => {
            let limit = arg_num(args, "limit");
            crate::commands::sessions(limit.as_ref())
        }
        "cue_agents" => {
            let limit = arg_num(args, "limit");
            crate::commands::agents(limit.as_ref())
        }
        "cue_session" => {
            let uid = arg_str(args, "session_uid").ok_or("session_uid is required")?;
            crate::commands::session(Some(&uid.to_string()))
        }
        "cue_rpc" => {
            let method = arg_str(args, "method").ok_or("method is required")?;
            let body = args
                .get("body")
                .map(|b| b.to_string())
                .unwrap_or_else(|| "{}".to_string());
            crate::commands::rpc(Some(&method.to_string()), Some(&body))
        }
        "cue_send" => {
            let sid = arg_str(args, "session_id").ok_or("session_id is required")?;
            let text = arg_str(args, "text").ok_or("text is required")?;
            let mut flags = HashMap::new();
            if let Some(secs) = arg_num(args, "seconds") {
                flags.insert("seconds".to_string(), secs);
            }
            flags.insert("frames".to_string(), "160".to_string());
            crate::commands::send(Some(&sid.to_string()), Some(&text.to_string()), &flags)
        }
        "cue_new" => {
            let text = arg_str(args, "text").ok_or("text is required")?;
            let mut flags = HashMap::new();
            if let Some(secs) = arg_num(args, "seconds") {
                flags.insert("seconds".to_string(), secs);
            }
            flags.insert("frames".to_string(), "160".to_string());
            crate::commands::new_session(Some(&text.to_string()), &flags)
        }
        "cue_join" => {
            let sid = arg_str(args, "session_id").ok_or("session_id is required")?;
            let mut flags = HashMap::new();
            if let Some(secs) = arg_num(args, "seconds") {
                flags.insert("seconds".to_string(), secs);
            }
            flags.insert("frames".to_string(), "80".to_string());
            crate::commands::join(Some(&sid.to_string()), &flags)
        }
        "cue_discover" => {
            let service = arg_str(args, "service").ok_or("service is required")?;
            let methods: Vec<String> = args
                .get("methods")
                .and_then(Value::as_array)
                .map(|a| {
                    a.iter()
                        .filter_map(|m| m.as_str().map(|s| s.to_string()))
                        .collect()
                })
                .unwrap_or_default();
            crate::commands::discover(Some(&service.to_string()), &methods)
        }
        other => Err(format!("unknown tool: {other}")),
    }
}

/// Serve MCP over stdio until stdin closes or EOF arrives.
pub fn serve() -> Result<String, String> {
    let stdout_lock: Arc<Mutex<()>> = Arc::new(Mutex::new(()));
    let stdin = std::io::stdin();
    let mut handles: Vec<std::thread::JoinHandle<()>> = Vec::new();

    for line in stdin.lock().lines() {
        let line = match line {
            Ok(l) => l,
            Err(_) => break,
        };
        if line.trim().is_empty() {
            continue;
        }
        let msg: Value = match serde_json::from_str(&line) {
            Ok(v) => v,
            Err(e) => {
                write_message(
                    &stdout_lock,
                    &json!({
                        "jsonrpc": "2.0",
                        "id": Value::Null,
                        "error": { "code": -32700, "message": format!("parse error: {e}") },
                    }),
                );
                continue;
            }
        };

        let id = msg.get("id").cloned();
        let method = msg.get("method").and_then(Value::as_str).unwrap_or("").to_string();

        // Notifications carry no id and must not be answered.
        if id.is_none() {
            continue;
        }
        let id = id.unwrap();

        match method.as_str() {
            "initialize" => {
                write_message(
                    &stdout_lock,
                    &json!({
                        "jsonrpc": "2.0",
                        "id": id,
                        "result": {
                            "protocolVersion": PROTOCOL_VERSION,
                            "capabilities": { "tools": { "listChanged": false } },
                            "serverInfo": {
                                "name": "cue",
                                "version": env!("CARGO_PKG_VERSION"),
                            },
                        },
                    }),
                );
            }
            "tools/list" => {
                write_message(
                    &stdout_lock,
                    &json!({ "jsonrpc": "2.0", "id": id, "result": { "tools": tools() } }),
                );
            }
            "tools/call" => {
                let params = msg.get("params").cloned().unwrap_or(Value::Null);
                let name = params
                    .get("name")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_string();
                let args = params
                    .get("arguments")
                    .cloned()
                    .unwrap_or_else(|| json!({}));
                let lock = Arc::clone(&stdout_lock);
                // Long tools run off the read loop so later requests stay responsive.
                handles.push(std::thread::spawn(move || {
                    let outcome = call_tool(&name, &args);
                    let result = match outcome {
                        Ok(text) => json!({
                            "content": [{ "type": "text", "text": text }],
                            "isError": false,
                        }),
                        Err(err) => json!({
                            "content": [{ "type": "text", "text": err }],
                            "isError": true,
                        }),
                    };
                    write_message(&lock, &json!({ "jsonrpc": "2.0", "id": id, "result": result }));
                }));
            }
            other => {
                write_message(
                    &stdout_lock,
                    &json!({
                        "jsonrpc": "2.0",
                        "id": id,
                        "error": {
                            "code": -32601,
                            "message": format!("method not found: {other}"),
                        },
                    }),
                );
            }
        }
    }

    for handle in handles {
        let _ = handle.join();
    }
    Ok(String::new())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tool_list_has_required_shape() {
        let list = tools();
        let arr = list.as_array().expect("tools is an array");
        assert!(arr.len() >= 10, "expected at least 10 tools, got {}", arr.len());
        for entry in arr {
            assert!(entry["name"].as_str().is_some());
            assert!(entry["description"].as_str().is_some());
            assert_eq!(entry["inputSchema"]["type"], "object");
        }
    }

    #[test]
    fn unknown_tool_is_an_error() {
        assert!(call_tool("nope", &json!({})).is_err());
    }

    #[test]
    fn missing_arguments_are_rejected() {
        assert!(call_tool("cue_session", &json!({})).is_err());
        assert!(call_tool("cue_rpc", &json!({})).is_err());
    }
}
