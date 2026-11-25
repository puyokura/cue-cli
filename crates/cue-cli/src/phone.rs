// SPDX-License-Identifier: AGPL-3.0-or-later
//! Phone (SMS) authentication, the native login path the Android client uses.
//!
//! Measured contract (see docs/auth-status.md):
//!
//! * `x-product-name: manus-agents` is required. Without it every phone call
//!   answers `403 / 400232 "Phone authentication is not available for this
//!   account"`; with it the same call proceeds to real validation.
//! * `SendCuePhoneCode{phone_number, purpose}` -> `{verificationId,
//!   expiresInSeconds, retryAfterSeconds}`. `purpose` 1 = login, 2 = reset.
//! * `LoginCueByPhoneCode{verification_id, code}` -> session token.
//! * `LoginCueByPhonePassword{phone_number, password}` -> session token.
//! * Rate limits surface as `429 / 400233 "Too many attempts"`.

use serde_json::{json, Value};

use crate::client;

const SEND_CODE: &str = "/user.v1.UserAuthPublicService/SendCuePhoneCode";
const LOGIN_CODE: &str = "/user.v1.UserAuthPublicService/LoginCueByPhoneCode";
const LOGIN_PASSWORD: &str = "/user.v1.UserAuthPublicService/LoginCueByPhonePassword";
const RESET_PASSWORD: &str = "/user.v1.UserAuthPublicService/ResetCuePhonePassword";

/// Purpose enum as the service defines it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Purpose {
    Login,
    ResetPassword,
}

impl Purpose {
    fn wire(self) -> u8 {
        match self {
            Purpose::Login => 1,
            Purpose::ResetPassword => 2,
        }
    }
}

pub fn parse_purpose(s: &str) -> Result<Purpose, String> {
    match s.to_ascii_lowercase().as_str() {
        "login" | "1" => Ok(Purpose::Login),
        "reset" | "reset_password" | "2" => Ok(Purpose::ResetPassword),
        other => Err(format!(
            "unknown purpose {other:?}; use login or reset_password"
        )),
    }
}

/// Normalise a phone number to the E.164 form the service expects.
///
/// The service accepts `+<country><number>`; `00` prefixes are converted and
/// separators are stripped. Anything else is passed through unchanged so the
/// server's own validation message reaches the caller.
pub fn normalise_number(raw: &str) -> String {
    let trimmed: String = raw
        .chars()
        .filter(|c| c.is_ascii_digit() || *c == '+')
        .collect();
    if let Some(rest) = trimmed.strip_prefix("00") {
        return format!("+{rest}");
    }
    if trimmed.starts_with('+') {
        return trimmed;
    }
    trimmed
}

/// Result of a code request.
#[derive(Debug, Clone)]
pub struct CodeRequest {
    pub verification_id: String,
    pub expires_in_seconds: Option<u64>,
    pub retry_after_seconds: Option<u64>,
}

pub fn send_code(base: &str, phone: &str, purpose: Purpose) -> Result<CodeRequest, String> {
    let body = json!({
        "phone_number": normalise_number(phone),
        "purpose": purpose.wire(),
    });
    let value = client::call_json(base, SEND_CODE, None, &body)?;
    let verification_id = value
        .get("verificationId")
        .and_then(Value::as_str)
        .ok_or_else(|| format!("response has no verificationId: {value}"))?
        .to_string();
    Ok(CodeRequest {
        verification_id,
        expires_in_seconds: value.get("expiresInSeconds").and_then(Value::as_u64),
        retry_after_seconds: value.get("retryAfterSeconds").and_then(Value::as_u64),
    })
}

/// Exchange a verification id + SMS code for a session token.
pub fn login_with_code(base: &str, verification_id: &str, code: &str) -> Result<Value, String> {
    let body = json!({
        "verification_id": verification_id,
        "code": code,
    });
    client::call_json(base, LOGIN_CODE, None, &body)
}

/// Password login on a phone number.
pub fn login_with_password(base: &str, phone: &str, password: &str) -> Result<Value, String> {
    let body = json!({
        "phone_number": normalise_number(phone),
        "password": password,
    });
    client::call_json(base, LOGIN_PASSWORD, None, &body)
}

/// Finish a password reset started by `Purpose::ResetPassword`.
pub fn reset_password(
    base: &str,
    verification_id: &str,
    code: &str,
    password: &str,
) -> Result<Value, String> {
    let body = json!({
        "verification_id": verification_id,
        "code": code,
        "password": password,
    });
    client::call_json(base, RESET_PASSWORD, None, &body)
}

/// Pull the session token out of a login response, whichever field carries it.
pub fn extract_token(value: &Value) -> Result<String, String> {
    for key in [
        "token",
        "sessionId",
        "session_id",
        "accessToken",
        "access_token",
    ] {
        if let Some(t) = value.get(key).and_then(Value::as_str) {
            if !t.is_empty() {
                return Ok(t.to_string());
            }
        }
    }
    if let Some(user) = value.get("user").and_then(Value::as_object) {
        for key in ["token", "sessionId"] {
            if let Some(t) = user.get(key).and_then(Value::as_str) {
                if !t.is_empty() {
                    return Ok(t.to_string());
                }
            }
        }
    }
    Err(format!(
        "login response carries no token; keys were: {}",
        value
            .as_object()
            .map(|m| m.keys().cloned().collect::<Vec<_>>().join(", "))
            .unwrap_or_else(|| "<not an object>".into())
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalises_common_forms() {
        assert_eq!(normalise_number("+81 90 1234 5678"), "+819012345678");
        assert_eq!(normalise_number("0090-1234-5678"), "+9012345678");
        assert_eq!(normalise_number("+"), "+");
    }

    #[test]
    fn parses_purpose_names() {
        assert_eq!(parse_purpose("login").unwrap(), Purpose::Login);
        assert_eq!(parse_purpose("1").unwrap(), Purpose::Login);
        assert_eq!(
            parse_purpose("reset_password").unwrap(),
            Purpose::ResetPassword
        );
        assert!(parse_purpose("nope").is_err());
    }

    #[test]
    fn extracts_tokens_from_known_shapes() {
        assert_eq!(extract_token(&json!({"token": "t"})).unwrap(), "t");
        assert_eq!(
            extract_token(&json!({"user": {"token": "u"}})).unwrap(),
            "u"
        );
        assert!(extract_token(&json!({"ok": true})).is_err());
    }
}
