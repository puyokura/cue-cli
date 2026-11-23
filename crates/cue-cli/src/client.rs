// SPDX-License-Identifier: AGPL-3.0-or-later
//! Connect RPC transport for api.manus.im.
//!
//! Observed behaviour (see docs/protocol.md): a POST with
//! `connect-protocol-version: 1` and `content-type: application/json` returns
//! a JSON body on success and a Connect error object on failure. Unary proto
//! works with `content-type: application/proto` and returns a binary body.
//!
//! HTTP status is not treated as a transport error here: Connect encodes
//! failures in the body (`{"code":"...","message":"..."}`), so the status must
//! be read alongside the payload.

use serde_json::{json, Value};
use std::fs;
use std::time::Duration;

/// Default service origin. `--base` and `CUE_BASE_URL` override it.
pub const DEFAULT_BASE: &str = "https://api.manus.im";

/// Product identity the service uses to enable its native client surface.
///
/// Measured: without this header `SendCuePhoneCode` answers
/// `403 / 400232 "Phone authentication is not available for this account"`;
/// with `x-product-name: manus-agents` the same call proceeds to phone-number
/// validation. The value is the one the Android client sends.
pub const PRODUCT_NAME: &str = "manus-agents";

fn agent() -> ureq::Agent {
    ureq::Agent::config_builder()
        .http_status_as_error(false)
        .timeout_global(Some(Duration::from_secs(60)))
        .build()
        .into()
}

/// Client identity headers the service expects on every call.
fn default_headers(token: Option<&str>) -> Vec<(String, String)> {
    let mut h = vec![
        ("connect-protocol-version".into(), "1".into()),
        ("x-product-name".into(), PRODUCT_NAME.into()),
        ("x-client-type".into(), "CUE".into()),
        ("x-client-version".into(), "1.2.0".into()),
        (
            "user-agent".into(),
            format!("cue-cli/{}", env!("CARGO_PKG_VERSION")),
        ),
    ];
    if let Some(t) = token {
        h.push(("authorization".into(), format!("Bearer {t}")));
    }
    h
}

/// Read the stored bearer token, if any.
pub fn stored_token() -> Option<String> {
    let path = super::state_dir().ok()?.join("token");
    let raw = fs::read_to_string(path).ok()?;
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

/// Resolve the origin from a flag override, then `CUE_BASE_URL`, then default.
pub fn resolve_base(override_base: Option<&String>) -> String {
    if let Some(b) = override_base {
        return b.trim_end_matches('/').to_string();
    }
    if let Ok(b) = std::env::var("CUE_BASE_URL") {
        if !b.trim().is_empty() {
            return b.trim_end_matches('/').to_string();
        }
    }
    DEFAULT_BASE.to_string()
}

fn connect_error(status: u16, body: &str) -> String {
    match serde_json::from_str::<Value>(body) {
        Ok(v) => {
            let code = v.get("code").and_then(Value::as_str).unwrap_or("-");
            let message = v.get("message").and_then(Value::as_str).unwrap_or("-");
            format!("HTTP {status} [{code}] {message}")
        }
        Err(_) => format!("HTTP {status}: {}", truncate(body, 300)),
    }
}

/// Call one unary Connect RPC with a JSON body and return parsed JSON.
pub fn call_json(
    base: &str,
    method: &str,
    token: Option<&str>,
    body: &Value,
) -> Result<Value, String> {
    let url = format!("{}{}", base, method);
    let payload = serde_json::to_vec(body).map_err(|e| format!("encode body: {e}"))?;

    let mut req = agent()
        .post(&url)
        .header("content-type", "application/json")
        .header("accept", "application/json");
    for (k, v) in default_headers(token) {
        req = req.header(&k, &v);
    }

    let resp = req
        .send(&payload[..])
        .map_err(|e| format!("transport: {e} at POST {url}"))?;

    let status = resp.status().as_u16();
    let text = resp
        .into_body()
        .read_to_string()
        .map_err(|e| format!("read body: {e}"))?;

    if status >= 400 {
        return Err(connect_error(status, &text));
    }
    serde_json::from_str(&text).map_err(|_| {
        format!(
            "non-JSON response (HTTP {status}): {}",
            truncate(&text, 300)
        )
    })
}

/// Call one unary Connect RPC over the binary proto codec.
pub fn call_proto(
    base: &str,
    method: &str,
    token: Option<&str>,
    payload: &[u8],
) -> Result<Vec<u8>, String> {
    let url = format!("{}{}", base, method);
    let mut req = agent()
        .post(&url)
        .header("content-type", "application/proto")
        .header("accept", "application/proto");
    for (k, v) in default_headers(token) {
        req = req.header(&k, &v);
    }

    let resp = req
        .send(payload)
        .map_err(|e| format!("transport: {e} at POST {url}"))?;
    let status = resp.status().as_u16();
    let bytes = resp
        .into_body()
        .read_to_vec()
        .map_err(|e| format!("read body: {e}"))?;
    if status >= 400 {
        let text = String::from_utf8_lossy(&bytes).to_string();
        return Err(connect_error(status, &text));
    }
    Ok(bytes)
}

fn truncate(s: &str, max: usize) -> String {
    match s.char_indices().nth(max) {
        Some((idx, _)) => format!("{}…", &s[..idx]),
        None => s.to_string(),
    }
}

/// Convenience: an empty JSON object.
#[allow(dead_code)]
pub fn empty() -> Value {
    json!({})
}

