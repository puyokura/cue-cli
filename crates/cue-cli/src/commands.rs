//! Subcommands. Every command prints JSON so output stays machine-readable
//! and nothing here invents values the service did not return.

use serde_json::{json, Value};
use std::collections::HashMap;
use std::fs;

use crate::client;
use crate::pretty;

const USER_INFO: &str = "/user.v1.UserService/UserInfo";
const GLOBAL_SETTINGS: &str = "/user.v1.UserPublicService/GetGlobalSettings";
const CHECK_REGION: &str = "/user.v1.UserPublicService/CheckRegion";
const LIST_SESSIONS: &str = "/session.v1.SessionService/ListSessions";
const GET_SESSION: &str = "/session.v1.SessionService/GetSession";
const LIST_AGENTS: &str = "/agents.v1.AgentsService/ListAgentsV2";

pub fn login(token: Option<&String>, base: Option<&String>) -> Result<String, String> {
    let token = token.ok_or("login requires --token <value> (the web client's sessionId)")?;
    let token = token.trim();
    if token.is_empty() {
        return Err("--token is empty".into());
    }
    let origin = client::resolve_base(base);

    // Verify before persisting: a rejected token must not be left on disk as
    // if it were a working session.
    let user = client::call_json(&origin, USER_INFO, Some(token), &json!({}))
        .map_err(|e| format!("token rejected, nothing stored: {e}"))?;

    let dir = crate::state_dir()?;
    let path = dir.join("token");
    fs::write(&path, token).map_err(|e| format!("write {}: {e}", path.display()))?;
    let base_path = dir.join("base");
    fs::write(&base_path, &origin).map_err(|e| format!("write {}: {e}", base_path.display()))?;

    Ok(pretty(&json!({
        "stored": path.display().to_string(),
        "base": origin,
        "user": user.get("user").cloned().unwrap_or(Value::Null),
    })))
}

pub fn logout() -> Result<String, String> {
    let dir = crate::state_dir()?;
    let path = dir.join("token");
    let existed = path.exists();
    if existed {
        fs::remove_file(&path).map_err(|e| format!("remove {}: {e}", path.display()))?;
    }
    Ok(json!({ "removed": existed, "path": path.display().to_string() }).to_string())
}

fn base() -> String {
    let dir = crate::state_dir().ok();
    let stored = dir
        .and_then(|d| fs::read_to_string(d.join("base")).ok())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    match stored {
        Some(b) => b,
        None => client::DEFAULT_BASE.to_string(),
    }
}

fn authed() -> Result<String, String> {
    client::stored_token().ok_or_else(|| "no token; run `cue login --token <value>`".to_string())
}

pub fn whoami() -> Result<String, String> {
    let token = authed()?;
    let value = client::call_json(&base(), USER_INFO, Some(&token), &json!({}))?;
    Ok(pretty(&value))
}

pub fn settings() -> Result<String, String> {
    let value = client::call_json(&base(), GLOBAL_SETTINGS, None, &json!({}))?;
    Ok(pretty(&value))
}

pub fn region() -> Result<String, String> {
    let value = client::call_json(&base(), CHECK_REGION, None, &json!({}))?;
    Ok(pretty(&value))
}

pub fn sessions(limit: Option<&String>) -> Result<String, String> {
    let token = authed()?;
    let mut body = json!({});
    if let Some(n) = limit {
        let parsed: u32 = n.parse().map_err(|_| "--limit must be a number")?;
        body["limit"] = Value::from(parsed);
    }
    let value = client::call_json(&base(), LIST_SESSIONS, Some(&token), &body)?;
    Ok(pretty(&value))
}

pub fn session(uid: Option<&String>) -> Result<String, String> {
    let token = authed()?;
    let uid = uid.ok_or("session requires <session_uid>")?;
    let body = json!({ "sessionUid": uid });
    let value = client::call_json(&base(), GET_SESSION, Some(&token), &body)?;
    Ok(pretty(&value))
}

pub fn agents(limit: Option<&String>) -> Result<String, String> {
    let token = authed()?;
    let mut body = json!({ "agentsLimit": 20, "channelsLimit": 20 });
    if let Some(n) = limit {
        let parsed: u32 = n.parse().map_err(|_| "--limit must be a number")?;
        body["agentsLimit"] = Value::from(parsed);
    }
    let value = client::call_json(&base(), LIST_AGENTS, Some(&token), &body)?;
    Ok(pretty(&value))
}

pub fn rpc(method: Option<&String>, body: Option<&String>) -> Result<String, String> {
    let method = method.ok_or("rpc requires <Service.Method> (e.g. user.v1.UserService/UserInfo)")?;
    let path = if method.starts_with('/') {
        method.clone()
    } else {
        format!("/{method}")
    };
    let payload: Value = match body {
        Some(b) => serde_json::from_str(b).map_err(|e| format!("--json is not valid JSON: {e}"))?,
        None => json!({}),
    };
    let token = client::stored_token();
    let value = client::call_json(&base(), &path, token.as_deref(), &payload)?;
    Ok(pretty(&value))
}

pub fn raw(method: Option<&String>, hex: Option<&String>) -> Result<String, String> {
    let method = method.ok_or("raw requires <Service.Method>")?;
    let path = if method.starts_with('/') {
        method.clone()
    } else {
        format!("/{method}")
    };
    let payload = match hex {
        Some(h) => hex_decode(h)?,
        None => Vec::new(),
    };
    let token = client::stored_token();
    let bytes = client::call_proto(&base(), &path, token.as_deref(), &payload)?;
    Ok(format!(
        "{} bytes as hex:\n{}",
        bytes.len(),
        hex_encode(&bytes)
    ))
}

pub fn methods() -> Result<String, String> {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../docs/rpc-inventory.txt");
    let text = fs::read_to_string(&path)
        .map_err(|e| format!("read {}: {e}", path.display()))?;
    let count = text.lines().filter(|l| !l.trim().is_empty()).count();
    Ok(json!({ "inventory": path.display().to_string(), "methods": count }).to_string())
}

/// Namespace-level Socket.IO observation. Prints one line per frame; the
/// bearer token is never echoed.
pub fn watch(
    seconds: Option<&String>,
    frames: Option<&String>,
    locale: Option<&String>,
) -> Result<String, String> {
    let secs: u64 = match seconds {
        Some(s) => s.parse().map_err(|_| "--seconds must be a number")?,
        None => 20,
    };
    let max_frames: usize = match frames {
        Some(s) => s.parse().map_err(|_| "--frames must be a number")?,
        None => 40,
    };
    let token = client::stored_token();
    let locale_value = locale.cloned().unwrap_or_else(|| "ja-JP".to_string());

    let ws_base = match std::env::var("CUE_WS_URL") {
        Ok(v) if !v.trim().is_empty() => v.trim_end_matches('/').to_string(),
        _ => "wss://api.manus.im".to_string(),
    };

    let query = vec![
        ("locale".to_string(), locale_value),
        ("tz".to_string(), "Asia/Tokyo".to_string()),
        ("clientType".to_string(), "CUE".to_string()),
        (
            "clientVersion".to_string(),
            "1.2.0".to_string(),
        ),
        ("x-product-name".to_string(), "cue".to_string()),
    ];

    let mut out: Vec<String> = Vec::new();
    crate::socket::run(&ws_base, token.as_deref(), &query, secs, max_frames, &mut |line| {
        out.push(line.to_string());
    })?;
    Ok(out.join("\n"))
}

/// Join a session and post one prompt, streaming the server's replies.
///
/// Refuses to send without a stored token: the socket accepts an unauthenticated
/// handshake but rejects the namespace, so a send would produce no result.
pub fn send(session_id: Option<&String>, text: Option<&String>, flags: &HashMap<String, String>) -> Result<String, String> {
    let session_id = session_id.ok_or("send requires <session_id>")?;
    let text = text.ok_or("send requires --text <prompt>")?;
    if text.trim().is_empty() {
        return Err("--text is empty".into());
    }
    let token = client::stored_token()
        .ok_or_else(|| "no token; run `cue login --token <value>` first".to_string())?;

    let seconds: u64 = match flags.get("seconds") {
        Some(s) => s.parse().map_err(|_| "--seconds must be a number")?,
        None => 60,
    };
    let max_frames: usize = match flags.get("frames") {
        Some(s) => s.parse().map_err(|_| "--frames must be a number")?,
        None => 400,
    };
    let task_mode = flags.get("task-mode").cloned().unwrap_or_else(|| "standard".into());
    let delivery_mode = flags.get("delivery-mode").cloned().unwrap_or_else(|| "interrupt".into());
    let locale = flags.get("locale").cloned().unwrap_or_else(|| "ja-JP".into());
    let verbose = flags.contains_key("verbose");

    let ws_base = match std::env::var("CUE_WS_URL") {
        Ok(v) if !v.trim().is_empty() => v.trim_end_matches('/').to_string(),
        _ => "wss://api.manus.im".to_string(),
    };

    // The CLI does not know the account's region; the caller may supply it.
    let iso = flags.get("country-iso").map(|s| s.as_str());

    let plan = crate::session::Plan {
        base_ws: &ws_base,
        token: Some(token.as_str()),
        locale: &locale,
        join_session: Some(session_id.as_str()),
        prompt: Some((session_id.as_str(), text.as_str())),
        task_mode: &task_mode,
        delivery_mode: &delivery_mode,
        country_iso: iso,
        seconds,
        max_frames,
        verbose,
    };

    let mut out: Vec<String> = Vec::new();
    let outcome = crate::session::run(plan, &mut |line| out.push(line.to_string()))?;

    out.push(String::new());
    out.push(format!(
        "summary: engine_open={} namespace_ok={} joined={} prompt_sent={} events={} chat_frames={}",
        outcome.engine_open,
        outcome.namespace_ok,
        outcome.joined,
        outcome.prompt_sent,
        outcome.events,
        outcome.chat_frames
    ));
    if let Some(err) = outcome.namespace_error {
        out.push(format!("namespace_error: {err}"));
    }
    Ok(out.join("\n"))
}

fn hex_decode(s: &str) -> Result<Vec<u8>, String> {
    let cleaned: String = s.chars().filter(|c| !c.is_whitespace()).collect();
    if cleaned.len() % 2 != 0 {
        return Err("--bytes must have an even number of hex digits".into());
    }
    let mut out = Vec::with_capacity(cleaned.len() / 2);
    let bytes = cleaned.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        let pair = u8::from_str_radix(&cleaned[i..i + 2], 16)
            .map_err(|_| format!("invalid hex at offset {i}"))?;
        out.push(pair);
        i += 2;
    }
    Ok(out)
}

fn hex_encode(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 3);
    for (i, b) in bytes.iter().enumerate() {
        if i > 0 && i % 32 == 0 {
            s.push('\n');
        }
        s.push_str(&format!("{:02x}", b));
    }
    s
}
