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

/// Constraints a caller needs before choosing tools.
///
/// A wrong first assumption costs a wasted turn: which surface carries what,
/// where credentials live, and which commands change remote state. This is
/// returned by `initialize`, so it is read before `tools/list`.
pub const CUE_INSTRUCTIONS: &str = "\
Cue (Cue+ by Manus) tools, an independent AGPL-3.0 client for the Cue service.

Auth: the bearer token is reused from a state file (%APPDATA%/cue-cli, else \
$XDG_CONFIG_HOME/cue-cli or ~/.config/cue-cli; override with CUE_STATE or \
CUE_MCP_STATE). Call cue_doctor first. If it reports auth=fail, the user must run \
`cue login` from a terminal: the login needs a phone number and an SMS code, so \
it cannot be completed from a tool call.

Surface: cue_rpc reaches every Connect RPC (JSON body), so an unimplemented \
command is not a blocker. cue_coverage reports which surfaces were actually \
verified against the live service; treat anything absent from it as unproven.

Limits: cue_send and cue_new post a real prompt to a real account and wait for \
the agent, so they are slow (tens of seconds) and leave a session behind. \
cue_join blocks for its whole window. Prefer cue_rpc for reads.

Destructive operations: publishing a site (cue webdev-deploy), disabling two-step \
verification, and deleting a passkey change remote state. The CLI requires \
--confirm for them; there is no MCP tool that skips that, so ask the user first.

Secrets: no tool prints a token, a password, or a recovery code. Values are \
replaced with {redacted: true, bytes: N}. An empty object from a read usually \
means an empty state, not an error.";

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
        tool("cue_doctor", "Check state, auth, and the service. Run this first.", json!({
            "live": { "type": "boolean", "description": "also round-trip the service" }
        }), vec![]),
        tool("cue_status", "What the CLI thinks its own state is (state path, signed_in).", json!({}), vec![]),
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
        tool("cue_rpc", "Call any Connect RPC by path with a JSON body. A state-changing method needs \"confirm\": true.", json!({
            "method": { "type": "string", "description": "service.Method or /service.Method" },
            "body": { "type": "object", "description": "request body (defaults to {})" },
            "confirm": { "type": "boolean", "description": "required for a method that changes remote state" }
        }), vec!["method"]),
        tool("cue_send", "Join a session and post a prompt; returns what came back.", json!({
            "session_id": { "type": "string" },
            "text": { "type": "string" },
            "seconds": { "type": "integer", "description": "observation window" }
        }), vec!["session_id", "text"]),
        tool("cue_telecom", "Read telecom countries, rates and free-phone settings.", json!({}), vec![]),
        tool("cue_sandbox", "Report a session's sandbox status, id and VNC endpoints.", json!({
            "session_id": { "type": "string" }
        }), vec!["session_id"]),
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
        "cue_doctor" => {
            let live = args.get("live").and_then(Value::as_bool).unwrap_or(false);
            let report = crate::doctor::run(live);
            Ok(serde_json::to_string_pretty(&report.to_json(live))
                .unwrap_or_else(|_| report.to_text(live)))
        }
        "cue_status" => Ok(serde_json::to_string_pretty(&crate::doctor::status())
            .unwrap_or_else(|_| "{}".to_string())),
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
            // The same gate as the CLI. A machine-facing caller is not trusted
            // more than a human one; it has to say `confirm: true` for a
            // state-changing method exactly as `--confirm` does.
            if crate::body::state_changing_method(method)
                && args.get("confirm").and_then(Value::as_bool) != Some(true)
            {
                return Err(format!(
                    "{method} can change remote state; pass \"confirm\": true to call it"
                ));
            }
            let body = args
                .get("body")
                .cloned()
                .unwrap_or_else(|| serde_json::json!({}));
            crate::commands::rpc_with_body(Some(&method.to_string()), &body)
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
        "cue_telecom" => crate::commands::telecom(),
        "cue_sandbox" => {
            let sid = arg_str(args, "session_id").ok_or("session_id is required")?;
            crate::commands::sandbox(Some(&sid.to_string()))
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
                            "instructions": CUE_INSTRUCTIONS,
                        },
                    }),
                );
            }
            // A liveness check the client may send at any time. It must answer
            // even while a slow tool is running, which the threaded design
            // already guarantees.
            "ping" => {
                write_message(
                    &stdout_lock,
                    &json!({ "jsonrpc": "2.0", "id": id, "result": {} }),
                );
            }
            // Notifications carry no id and are answered above; this arm keeps
            // an explicit acknowledgement from being sent twice.
            "notifications/initialized" | "initialized" => {}
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
    fn a_state_changing_rpc_needs_confirm() {
        let err = call_tool(
            "cue_rpc",
            &json!({"method": "user.v1.UserAuthService/DeletePasskey", "body": {"id": "x"}}),
        )
        .unwrap_err();
        assert!(err.contains("confirm"), "{err}");
    }

    #[test]
    fn a_read_rpc_does_not_need_confirm() {
        // Checked without a network call: the gate decision is the unit, not the
        // request. A read must never demand confirmation.
        for method in [
            "user.v1.UserService/UserInfo",
            "user.v1.UserService/GetAvailableCredits",
            "session.v1.SessionService/ListSessions",
        ] {
            assert!(!crate::body::state_changing_method(method), "{method}");
        }
        for method in [
            "user.v1.UserAuthService/DeletePasskey",
            "cloud_pc.v1.CloudPCService/ResetSSHPassword",
        ] {
            assert!(crate::body::state_changing_method(method), "{method}");
        }
    }

    #[test]
    fn missing_arguments_are_rejected() {
        assert!(call_tool("cue_session", &json!({})).is_err());
        assert!(call_tool("cue_rpc", &json!({})).is_err());
    }
}


