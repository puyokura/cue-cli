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

/// Convert camelCase keys to snake_case exactly like the web client does.
/// Already-snake keys pass through unchanged.
pub fn camel_to_snake(key: &str) -> String {
    let mut out = String::with_capacity(key.len() + 4);
    for (i, ch) in key.chars().enumerate() {
        if ch.is_ascii_uppercase() {
            if i > 0 {
                out.push('_');
            }
            out.push(ch.to_ascii_lowercase());
        } else {
            out.push(ch);
        }
    }
    out
}

/// Recursively normalise every object key to snake_case.
pub fn to_wire(value: &Value) -> Value {
    match value {
        Value::Object(map) => {
            let mut out = Map::with_capacity(map.len());
            for (k, v) in map {
                out.insert(camel_to_snake(k), to_wire(v));
            }
            Value::Object(out)
        }
        Value::Array(items) => Value::Array(items.iter().map(to_wire).collect()),
        other => other.clone(),
    }
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
        "session_id": session_id,
        "timestamp": timestamp_ms(),
        "message_type": message_type,
        "task_mode": task_mode,
        "content": content,
        "contents": [{ "type": "text", "value": content }],
        "attachments": [],
        "delivery_mode": delivery_mode,
    });
    if let Some(iso) = country_iso {
        payload["country_iso_code"] = Value::from(iso);
    }
    if let Some(Value::Object(extra_map)) = extra {
        if let Value::Object(base) = &mut payload {
            for (k, v) in extra_map {
                base.insert(camel_to_snake(&k), v);
            }
        }
    }
    payload
}

/// Build the `join_session` frame.
pub fn join_session(session_id: &str, last_message_id: &str) -> Value {
    json!({
        "type": "join_session",
        "session_id": session_id,
        "last_message_id": last_message_id,
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
    use std::time::{SystemTime, UNIX_EPOCH};
    let ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    let head = base36(ms as u64);
    let head = if head.len() > 3 { &head[3..] } else { &head[..] };
    let tail = base36((ms as u64).wrapping_mul(2654435761) % 36u64.pow(5));
    format!("{head}{tail}")
}

fn base36(mut n: u64) -> String {
    const DIGITS: &[u8] = b"0123456789abcdefghijklmnopqrstuvwxyz";
    if n == 0 {
        return "0".to_string();
    }
    let mut s = Vec::new();
    while n > 0 {
        s.push(DIGITS[(n % 36) as usize]);
        n /= 36;
    }
    s.reverse();
    String::from_utf8(s).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snake_conversion_matches_client() {
        assert_eq!(camel_to_snake("sessionId"), "session_id");
        assert_eq!(camel_to_snake("countryIsoCode"), "country_iso_code");
        assert_eq!(camel_to_snake("x-product-name"), "x-product-name");
        assert_eq!(camel_to_snake("already_snake"), "already_snake");
    }

    #[test]
    fn payload_nested_keys_are_converted() {
        let v = json!({ "sessionId": "s", "extData": { "maxEffort": "max" } });
        let w = to_wire(&v);
        assert_eq!(w["session_id"], "s");
        assert_eq!(w["ext_data"]["max_effort"], "max");
    }

    #[test]
    fn user_message_matches_observed_shape() {
        let m = user_message("id1", "sess", "hi", "standard", "text", "interrupt", Some("JP"), None);
        assert_eq!(m["type"], "user_message");
        assert_eq!(m["session_id"], "sess");
        assert_eq!(m["message_type"], "text");
        assert_eq!(m["country_iso_code"], "JP");
        assert_eq!(m["contents"][0]["type"], "text");
        assert!(m["timestamp"].as_u64().unwrap() > 1_600_000_000_000);
    }

    #[test]
    fn join_session_matches_observed_shape() {
        let j = join_session("sess", "new_session");
        assert_eq!(j["type"], "join_session");
        assert_eq!(j["version"], 2);
    }

    #[test]
    fn message_id_has_expected_length() {
        let id = message_id();
        assert!(id.len() >= 10 && id.len() <= 24, "id was {id}");
    }
}
