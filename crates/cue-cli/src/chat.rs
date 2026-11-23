//! Chat gateway frames over the Socket.IO `message` event.
//!
//! Facts taken from the shipped Android client and the web client bundle
//! (`docs/protocol.md` §6, §8):
//!
//! * Outgoing payloads are **snake_case** (the web client runs
//!   `convertObjectNames("server", payload)` = camelCase → snake_case).
//! * Incoming payloads are snake_case too and are converted to camelCase on
//!   receipt, so the wire side is snake_case in both directions.
//! * A prompt is `{"type":"user_message", "session_id": ..., "content": ...,
//!   "message_type":"text", "task_mode":"standard", "contents":[...],
//!   "attachments":[], "delivery_mode":"interrupt", "id":..., "timestamp":...}`.
//! * Session subscription is `{"type":"join_session","session_id":...,
//!   "last_message_id":"new_session","version":2}`.
//! * Replies arrive as `{"type":"event","session_id":...,"event":{...}}` while
//!   streamed progress is a `chat` event with a matching `id`.

use serde_json::{json, Map, Value};


/// Recursively normalise every object key to snake_case.
/// Normalise a frame for the wire.
///
/// Measured correction: the chat gateway accepts **camelCase** keys on the wire,
/// not snake_case. A snake_case `user_message` is answered with
/// `agent_error code=1 "You are not allowed to perform this action."`, while the
/// same payload in camelCase is accepted and the agent replies. (The web client's
/// internal `convertObjectNames` call is a store-level transform, not a wire one.)
pub fn to_wire(value: &Value) -> Value {
    match value {
        Value::Object(map) => {
            let mut out = Map::with_capacity(map.len());
            for (k, v) in map {
                out.insert(camelize(k), to_wire(v));
            }
            Value::Object(out)
        }
        Value::Array(items) => Value::Array(items.iter().map(to_wire).collect()),
        other => other.clone(),
    }
}

/// Convert a snake_case key to camelCase, leaving already-camel keys untouched.
pub fn camelize(key: &str) -> String {
    if !key.contains('_') {
        return key.to_string();
    }
    let mut out = String::with_capacity(key.len());
    let mut upper_next = false;
    for ch in key.chars() {
        if ch == '_' {
            upper_next = true;
        } else if upper_next {
            out.push(ch.to_ascii_uppercase());
            upper_next = false;
        } else {
            out.push(ch);
        }
    }
    out
}

/// Build the `user_message` frame for a text prompt.
#[allow(clippy::too_many_arguments)]
pub fn user_message(
    id: &str,
    session_id: &str,
    content: &str,
    task_mode: &str,
    message_type: &str,
    delivery_mode: &str,
    country_iso: Option<&str>,
    extra: Option<Value>,
) -> Value {
    let mut payload = json!({
        "type": "user_message",
        "id": id,
        "sessionId": session_id,
        "timestamp": timestamp_ms(),
        "messageType": message_type,
        "taskMode": task_mode,
        "content": content,
        "contents": [{ "type": "text", "value": content }],
        "attachments": [],
        "deliveryMode": delivery_mode,
    });
    if let Some(iso) = country_iso {
        payload["countryIsoCode"] = Value::from(iso);
    }
    // The web client always sends `extData`. Measured: a payload without it is
    // accepted once the keys are camelCase, but the fields below are what the
    // client normally sends and they pin the deployment target to a sandbox.
    payload["extData"] = json!({
        "mode": "standard",
        "executionTarget": { "kind": "sandbox" },
        "psBindings": [],
        "capabilities": { "enabledSkills": [] },
    });
    if let Some(Value::Object(extra_map)) = extra {
        if let Value::Object(base) = &mut payload {
            for (k, v) in extra_map {
                base.insert(k.clone(), v);
            }
        }
    }
    payload
}

/// Build the `join_session` frame.
pub fn join_session(session_id: &str, last_message_id: &str) -> Value {
    json!({
        "type": "join_session",
        "sessionId": session_id,
        "lastMessageId": last_message_id,
        "version": 2,
    })
}

/// Milliseconds since the Unix epoch.
pub fn timestamp_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// A 22-character identifier in the same shape the client generates
/// (`Date.now().toString(36).substring(3)` + random base36). It identifies the
/// message within the session; uniqueness, not secrecy, is what matters.
pub fn message_id() -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    // Measured: the server rejects ids that are not exactly 22 base62 characters
    // ("data/user_message/id must match pattern ^[A-Za-z0-9]{22}$"). The web client
    // builds one from the millisecond timestamp in base36 plus random padding; a
    // 64-bit nanosecond value plus a counter is unique and always 22 digits.
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);
    let seq = COUNTER.fetch_add(1, Ordering::Relaxed);

    const ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789";
    let mut state = millis
        .wrapping_mul(6364136223846793005)
        .wrapping_add(1442695040888963407)
        .wrapping_add(seq.wrapping_mul(0x9E3779B97F4A7C15));
    let mut out = String::with_capacity(22);
    for _ in 0..22 {
        state = state
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        out.push(ALPHABET[(state >> 60) as usize & 0x3F] as char);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn camelize_handles_the_key_shapes_we_send() {
        assert_eq!(camelize("country_iso_code"), "countryIsoCode");
        assert_eq!(camelize("id"), "id");
        assert_eq!(camelize("x-product-name"), "x-product-name");
        assert_eq!(camelize("sessionId"), "sessionId");
    }

    #[test]
    fn payload_nested_keys_stay_camel_on_the_wire() {
        // Measured: snake_case keys are rejected by the gateway, camelCase accepted.
        let v = json!({ "sessionId": "s", "extData": { "maxEffort": "max" } });
        let w = to_wire(&v);
        assert_eq!(w["sessionId"], "s");
        assert_eq!(w["extData"]["maxEffort"], "max");
        assert_eq!(camelize("execution_target"), "executionTarget");
    }

    #[test]
    fn user_message_matches_observed_shape() {
        let m = user_message("id1", "sess", "hi", "standard", "text", "interrupt", Some("JP"), None);
        assert_eq!(m["type"], "user_message");
        assert_eq!(m["sessionId"], "sess");
        assert_eq!(m["messageType"], "text");
        assert_eq!(m["taskMode"], "standard");
        assert_eq!(m["deliveryMode"], "interrupt");
        assert_eq!(m["countryIsoCode"], "JP");
        assert_eq!(m["contents"][0]["type"], "text");
        assert_eq!(m["extData"]["executionTarget"]["kind"], "sandbox");
        assert!(m["timestamp"].as_u64().unwrap() > 1_600_000_000_000);
    }

    #[test]
    fn join_session_matches_observed_shape() {
        let j = join_session("sess", "new_session");
        assert_eq!(j["type"], "join_session");
        assert_eq!(j["version"], 2);
    }

    #[test]
    fn message_id_matches_server_pattern() {
        // Measured constraint: ^[A-Za-z0-9]{22}$
        for _ in 0..1000 {
            let id = message_id();
            assert_eq!(id.len(), 22, "id was {id}");
            assert!(
                id.chars().all(|c| c.is_ascii_alphanumeric()),
                "id was {id}"
            );
        }
    }

    #[test]
    fn message_ids_are_unique() {
        let mut seen = std::collections::HashSet::new();
        for _ in 0..1000 {
            assert!(seen.insert(message_id()), "duplicate id generated");
        }
    }
}




