// SPDX-License-Identifier: AGPL-3.0-or-later
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

/// `cue login` with no `--token` runs the whole native phone login interactively,
/// so one process invocation takes the user from a phone number to a stored token.
fn login_interactive(base_flag: Option<&String>) -> Result<String, String> {
    let origin = client::resolve_base(base_flag);

    eprintln!("Cue login (native phone path). Ctrl-C to abort.");
    let number = match read_line("phone number (E.164, e.g. +819012345678): ")? {
        Some(n) => n,
        None => return Err("phone number is required".into()),
    };

    let req = crate::phone::send_code(&origin, &number, crate::phone::Purpose::Login)
        .map_err(|e| format!("could not send the SMS code: {e}"))?;
    eprintln!(
        "SMS sent (verificationId {}).{}",
        req.verification_id,
        match req.expires_in_seconds {
            Some(s) => format!(" Valid for {s}s."),
            None => String::new(),
        }
    );

    let code = match read_line("SMS code: ")? {
        Some(c) => c,
        None => return Err("SMS code is required".into()),
    };

    let resp = crate::phone::login_with_code(&origin, &req.verification_id, &code)
        .map_err(|e| format!("login rejected: {e}"))?;
    let token = crate::phone::extract_token(&resp)?;
    let user = store_verified_token(&origin, &token)?;

    Ok(pretty(&json!({
        "stored": true,
        "base": origin,
        "user": user.get("user").cloned().unwrap_or(Value::Null),
    })))
}

pub fn login(token: Option<&String>, base: Option<&String>) -> Result<String, String> {
    if token.is_none() {
        return login_interactive(base);
    }
    let token = token.ok_or("login requires --token <value> (a bearer token issued by the service)")?;
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

/// Persist a token only after the service proves it works.
fn store_verified_token(origin: &str, token: &str) -> Result<Value, String> {
    let user = client::call_json(origin, USER_INFO, Some(token), &json!({}))
        .map_err(|e| format!("token rejected, nothing stored: {e}"))?;
    let dir = crate::state_dir()?;
    let path = dir.join("token");
    fs::write(&path, token).map_err(|e| format!("write {}: {e}", path.display()))?;
    let base_path = dir.join("base");
    fs::write(&base_path, origin).map_err(|e| format!("write {}: {e}", base_path.display()))?;
    Ok(user)
}

/// `cue phone <start|verify|password|reset>` — the native SMS login path.
pub fn phone(sub: Option<&String>, flags: &HashMap<String, String>) -> Result<String, String> {
    let sub = sub.map(String::as_str).unwrap_or("help");
    let base = base();

    match sub {
        "start" => {
            let number = flags
                .get("number")
                .ok_or("phone start requires --number <e164>")?;
            let purpose = match flags.get("purpose") {
                Some(p) => crate::phone::parse_purpose(p)?,
                None => crate::phone::Purpose::Login,
            };
            let req = crate::phone::send_code(&base, number, purpose)?;
            Ok(pretty(&json!({
                "verificationId": req.verification_id,
                "expiresInSeconds": req.expires_in_seconds,
                "retryAfterSeconds": req.retry_after_seconds,
                "purpose": match purpose {
                    crate::phone::Purpose::Login => "login",
                    crate::phone::Purpose::ResetPassword => "reset_password",
                },
            })))
        }
        "verify" => {
            let id = flags
                .get("id")
                .ok_or("phone verify requires --id <verificationId>")?;
            let code = flags.get("code").ok_or("phone verify requires --code <sms code>")?;
            let resp = crate::phone::login_with_code(&base, id, code)?;
            let token = crate::phone::extract_token(&resp)?;
            let user = store_verified_token(&base, &token)?;
            Ok(pretty(&json!({
                "stored": true,
                "user": user.get("user").cloned().unwrap_or(Value::Null),
            })))
        }
        "password" => {
            let number = flags
                .get("number")
                .ok_or("phone password requires --number <e164>")?;
            let password = match flags.get("password") {
                Some(p) => p.clone(),
                None => read_secret("password")?,
            };
            let resp = crate::phone::login_with_password(&base, number, &password)?;
            let token = crate::phone::extract_token(&resp)?;
            let user = store_verified_token(&base, &token)?;
            Ok(pretty(&json!({
                "stored": true,
                "user": user.get("user").cloned().unwrap_or(Value::Null),
            })))
        }
        "reset" => {
            let id = flags
                .get("id")
                .ok_or("phone reset requires --id <verificationId>")?;
            let code = flags.get("code").ok_or("phone reset requires --code <sms code>")?;
            let password = read_secret("new password")?;
            let resp = crate::phone::reset_password(&base, id, code, &password)?;
            Ok(pretty(&resp))
        }
        "help" => Ok(json!({
            "usage": [
                "cue phone start --number +8190... [--purpose login|reset_password]",
                "cue phone verify --id <verificationId> --code <sms code>",
                "cue phone password --number +8190...",
                "cue phone reset --id <verificationId> --code <sms code>",
            ],
        })
        .to_string()),
        other => Err(format!("unknown phone subcommand: {other}")),
    }
}

/// Read one line from stdin without echoing it to the terminal.
/// Read one line from stdin and return it trimmed. EOF returns `None` so the
/// caller can decide whether an empty answer is fatal.
fn read_line(prompt: &str) -> Result<Option<String>, String> {
    eprint!("{prompt}");
    let mut line = String::new();
    let read = std::io::stdin()
        .read_line(&mut line)
        .map_err(|e| format!("read input: {e}"))?;
    if read == 0 {
        return Ok(None);
    }
    let trimmed = line.trim();
    if trimmed.is_empty() {
        Ok(None)
    } else {
        Ok(Some(trimmed.to_string()))
    }
}

fn read_secret(label: &str) -> Result<String, String> {
    read_line(&format!("{label}: "))?.ok_or_else(|| format!("{label} is empty"))
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
        ("x-product-name".to_string(), crate::client::PRODUCT_NAME.to_string()),
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


/// `cue discover <service> <method>` — test whether a Connect route exists.
///
/// The service distinguishes a missing route (plain HTTP 404 without a Connect
/// body) from an existing one (401/400 with a Connect body). That difference is
/// the only oracle available before authentication, so this command exists to
/// make the inventory check reproducible instead of ad hoc.
pub fn discover(service: Option<&String>, methods: &[String]) -> Result<String, String> {
    let service = service.ok_or("discover requires <service> (e.g. user.v1.UserAuthService)")?;
    if methods.is_empty() {
        return Err("discover requires at least one <method> name".into());
    }
    let origin = base();
    let mut rows = Vec::new();
    for method in methods {
        let path = format!("/{service}/{method}");
        match client::call_json(&origin, &path, None, &json!({})) {
            Ok(_) => rows.push(json!({ "method": method, "exists": true, "status": "ok" })),
            Err(err) => {
                let exists = !(err.contains("HTTP 404") && !err.contains('['));
                rows.push(json!({ "method": method, "exists": exists, "detail": err }));
            }
        }
    }
    Ok(pretty(&json!({ "service": service, "probes": rows })))
}

/// `cue mfa <status|totp-start|totp-finish|enable|disable|codes>`
///
/// Every call needs a session token. The TOTP enrollment sequence is the one
/// the web client uses: start -> authenticator app shows a 6-digit code ->
/// finish -> enable (which returns recovery codes).
pub fn mfa(sub: Option<&String>, flags: &HashMap<String, String>) -> Result<String, String> {
    let sub = sub.map(String::as_str).unwrap_or("help");

    // Help must work before any token exists.
    if sub == "help" {
        return Ok(json!({
            "usage": [
                "cue mfa status",
                "cue mfa totp-start",
                "cue mfa totp-finish --flow <flowId> --code <6-digit>",
                "cue mfa enable | cue mfa disable",
                "cue mfa codes",
                "cue mfa login-methods",
                "cue mfa passkeys",
                "cue mfa passkey-start",
                "cue mfa passkey-finish --flow <id> --credential <json> [--name <label>]",
                "cue mfa passkey-delete --id <passkey id>",
            ],
        })
        .to_string());
    }

    let token = authed()?;
    let origin = base();

    match sub {
        "status" => {
            let value = client::call_json(
                &origin,
                "/user.v1.UserAuthService/GetMfaStatus",
                Some(&token),
                &json!({}),
            )?;
            Ok(pretty(&value))
        }
        "totp-start" => {
            let value = client::call_json(
                &origin,
                "/user.v1.UserAuthService/StartTotpEnrollment",
                Some(&token),
                &json!({}),
            )?;
            Ok(pretty(&value))
        }
        "totp-finish" => {
            let flow = flags
                .get("flow")
                .ok_or("mfa totp-finish requires --flow <flowId>")?;
            let code = flags
                .get("code")
                .ok_or("mfa totp-finish requires --code <6-digit code>")?;
            let value = client::call_json(
                &origin,
                "/user.v1.UserAuthService/FinishTotpEnrollment",
                Some(&token),
                &json!({ "flow_id": flow, "code": code }),
            )?;
            Ok(pretty(&value))
        }
        "enable" | "disable" => {
            let enabled = sub == "enable";
            let value = client::call_json(
                &origin,
                "/user.v1.UserAuthService/SetLoginMfa",
                Some(&token),
                &json!({ "enabled": enabled }),
            )?;
            Ok(pretty(&value))
        }
        "codes" => {
            let value = client::call_json(
                &origin,
                "/user.v1.UserAuthService/RegenerateRecoveryCodes",
                Some(&token),
                &json!({}),
            )?;
            Ok(pretty(&value))
        }
        "login-methods" => {
            let value = client::call_json(
                &origin,
                "/user.v1.UserAuthService/ListLoginMethods",
                Some(&token),
                &json!({}),
            )?;
            Ok(pretty(&value))
        }
        "passkeys" => {
            let value = client::call_json(
                &origin,
                "/user.v1.UserAuthService/ListPasskeys",
                Some(&token),
                &json!({}),
            )?;
            Ok(pretty(&value))
        }
        "passkey-start" => {
            // Mints a WebAuthn challenge. Nothing is registered until `passkey-finish`.
            let value = client::call_json(
                &origin,
                "/user.v1.UserAuthService/StartPasskeyRegistration",
                Some(&token),
                &json!({}),
            )?;
            Ok(pretty(&value))
        }
        "passkey-finish" => {
            let flow = flags
                .get("flow")
                .ok_or("mfa passkey-finish requires --flow <flowId>")?;
            let credential = flags
                .get("credential")
                .ok_or("mfa passkey-finish requires --credential <credential JSON from the authenticator>")?;
            let parsed: Value = serde_json::from_str(credential)
                .map_err(|e| format!("--credential is not valid JSON: {e}"))?;
            let mut body = json!({
                "flow_id": flow,
                "credential_json": parsed.to_string(),
            });
            if let Some(name) = flags.get("name") {
                body["name"] = Value::from(name.as_str());
            }
            let value = client::call_json(
                &origin,
                "/user.v1.UserAuthService/FinishPasskeyRegistration",
                Some(&token),
                &body,
            )?;
            Ok(pretty(&value))
        }
        "passkey-delete" => {
            let id = flags
                .get("id")
                .ok_or("mfa passkey-delete requires --id <passkey id>")?;
            let value = client::call_json(
                &origin,
                "/user.v1.UserAuthService/DeletePasskey",
                Some(&token),
                &json!({ "id": id }),
            )?;
            Ok(pretty(&value))
        }

        other => Err(format!("unknown mfa subcommand: {other}")),
    }
}


/// `cue join <session_id>` — subscribe to a session and observe what the server
/// sends, without posting a prompt. Used to verify session subscription and the
/// event replay (`eventsNotifyEventsAfter`) separately from message sending.
pub fn join(session_id: Option<&String>, flags: &HashMap<String, String>) -> Result<String, String> {
    let session_id = session_id.ok_or("join requires <session_id>")?;
    let token = client::stored_token()
        .ok_or_else(|| "no token; run `cue login` first".to_string())?;
    let seconds: u64 = match flags.get("seconds") {
        Some(s) => s.parse().map_err(|_| "--seconds must be a number")?,
        None => 20,
    };
    let max_frames: usize = match flags.get("frames") {
        Some(s) => s.parse().map_err(|_| "--frames must be a number")?,
        None => 80,
    };
    let locale = flags.get("locale").cloned().unwrap_or_else(|| "ja-JP".into());
    let ws_base = match std::env::var("CUE_WS_URL") {
        Ok(v) if !v.trim().is_empty() => v.trim_end_matches('/').to_string(),
        _ => "wss://api.manus.im".to_string(),
    };

    let plan = crate::session::Plan {
        base_ws: &ws_base,
        token: Some(token.as_str()),
        locale: &locale,
        join_session: Some(session_id.as_str()),
        prompt: None,
        task_mode: "standard",
        delivery_mode: "interrupt",
        country_iso: None,
        seconds,
        max_frames,
        verbose: flags.contains_key("verbose"),
    };

    let mut out: Vec<String> = Vec::new();
    let outcome = crate::session::run(plan, &mut |line| out.push(line.to_string()))?;
    out.push(String::new());
    out.push(format!(
        "summary: engine_open={} namespace_ok={} joined={} events={} chat_frames={}",
        outcome.engine_open, outcome.namespace_ok, outcome.joined,
        outcome.events, outcome.chat_frames
    ));
    if let Some(err) = outcome.namespace_error {
        out.push(format!("namespace_error: {err}"));
    }
    Ok(out.join("\n"))
}

/// `cue coverage` — read-only survey of the authenticated surface.
///
/// Only GET-like reads are issued. Every entry records the service's own answer,
/// so an empty object is reported as an empty state rather than as a failure.
pub fn coverage() -> Result<String, String> {
    let token = authed()?;
    let origin = base();

    let probes: Vec<(&str, &str, serde_json::Value)> = vec![
        ("user info", "/user.v1.UserService/UserInfo", json!({})),
        ("client config", "/user.v1.UserService/GetUserClientConfig", json!({})),
        ("credits", "/user.v1.UserService/GetAvailableCredits", json!({})),
        ("region codes", "/user.v1.UserService/ListRegionCode", json!({})),
        ("phone auth info", "/user.v1.UserAuthService/GetCuePhoneAuthInfo", json!({})),
        ("login methods", "/user.v1.UserAuthService/ListLoginMethods", json!({})),
        ("mfa status", "/user.v1.UserAuthService/GetMfaStatus", json!({})),
        ("passkeys", "/user.v1.UserAuthService/ListPasskeys", json!({})),
        ("trusted phones", "/user.v1.UserAuthService/ListCueTrustedPhoneNumbers", json!({})),
        ("sessions", "/session.v1.SessionService/SearchAgentSessions", json!({"query":"","limit":20})),
        ("agents", "/agents.v1.AgentsService/ListAgentsV2", json!({"agentsLimit":20,"channelsLimit":20})),
        ("skills", "/skill.v1.SkillService/ListSkills", json!({})),
        ("connectors", "/connectors.v1.ConnectorsService/ListConnectors", json!({})),
        ("notifications", "/notifier.v1.NotificationService/GetNotificationListV2", json!({"limit":20})),
        ("team", "/team.v1.TeamService/ListTeam", json!({"page":1,"pageSize":10})),
        ("cloud pcs", "/cloud_pc.v1.CloudPCService/List", json!({"limit":20})),
        ("phone numbers", "/telecom.v1.TelecomService/ListPhoneInfo", json!({})),
        ("remote control", "/device_authorization.v1.DeviceAuthorizationService/ListRemoteControlGrants", json!({})),
        ("agent emails", "/telecom.v1.TelecomService/ListAgentEmails", json!({})),
        ("email namespace", "/telecom.v1.TelecomService/GetAgentEmailNamespace", json!({})),
        ("cloud pc regions", "/cloud_pc.v1.CloudPCService/ListAvailableRegions", json!({})),
        ("webdev projects", "/webdev.v1.WebDevService/ListWebDevProjects", json!({"limit":10})),
        ("projects", "/session.v1.ProjectService/ListProjects", json!({"limit":10})),
    ];

    let mut rows = Vec::new();
    for (label, path, body) in probes {
        match client::call_json(&origin, path, Some(&token), &body) {
            Ok(value) => {
                let shape = describe(&value);
                rows.push(json!({ "check": label, "rpc": path, "ok": true, "shape": shape }));
            }
            Err(err) => {
                rows.push(json!({ "check": label, "rpc": path, "ok": false, "error": err }));
            }
        }
    }

    let passed = rows.iter().filter(|r| r["ok"] == json!(true)).count();
    Ok(pretty(&json!({
        "passed": passed,
        "total": rows.len(),
        "checks": rows,
    })))
}

/// Summarise a response without inventing content: counts for arrays, key names for objects.
fn describe(value: &Value) -> Value {
    match value {
        Value::Object(map) if map.is_empty() => json!("{} (empty state)"),
        Value::Object(map) => {
            let mut summary = Vec::new();
            for (k, v) in map {
                match v {
                    Value::Array(items) => summary.push(format!("{k}:{} items", items.len())),
                    Value::Object(_) => summary.push(format!("{k}:object")),
                    _ => summary.push(format!("{k}:scalar")),
                }
            }
            json!(summary.join(", "))
        }
        Value::Array(items) => json!(format!("{} items", items.len())),
        other => json!(other.to_string()),
    }
}



