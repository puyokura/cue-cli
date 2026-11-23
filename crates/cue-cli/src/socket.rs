// SPDX-License-Identifier: AGPL-3.0-or-later
//! Socket.IO (Engine.IO v4) client for `wss://api.manus.im`.
//!
//! Observed behaviour (see docs/protocol.md §6):
//!   * `GET /socket.io/?EIO=4&transport=websocket` upgrades to 101
//!   * server sends `0{"sid":...,"pingInterval":15000,"pingTimeout":20000,...}`
//!   * client sends `40{"token":<bearer>}` to join the default namespace
//!   * server answers `44{"message":"Invalid Token"}` when the token is bad
//!
//! Engine.IO packet prefixes: `0` open, `1` close, `2` ping, `3` pong, `4`
//! message. Inside a message packet, Socket.IO prefixes are `0` connect, `1`
//! disconnect, `2` event, `3` ack, `4` error. Events carry a JSON array.

use std::time::{Duration, Instant};

use serde_json::Value;
use tungstenite::client::IntoClientRequest;
use tungstenite::Message;

/// One decoded Socket.IO frame.
#[derive(Debug, Clone)]
pub enum Frame {
    /// Engine.IO open packet: `0{...}`.
    Open(Value),
    /// Namespace connect acknowledgement: `40{...}`.
    Connected(Value),
    /// Namespace connect failure: `44{...}`.
    ConnectError(Value),
    /// Event packet: `42["name", ...]`.
    Event { name: String, args: Value },
    /// Ack packet from the server.
    Ack(Value),
    /// Engine.IO ping / pong.
    Ping,
    Pong,
    /// Engine.IO close.
    Close,
    /// Anything else, verbatim.
    Other(String),
}

impl Frame {
    pub fn summary(&self) -> String {
        match self {
            Frame::Open(v) => format!("engine.io open latencies/params: {v}"),
            Frame::Connected(v) => format!("namespace connected: {v}"),
            Frame::ConnectError(v) => format!("namespace rejected: {v}"),
            Frame::Event { name, args } => format!("event {name}: {args}"),
            Frame::Ack(v) => format!("ack: {v}"),
            Frame::Ping => "engine.io ping".into(),
            Frame::Pong => "engine.io pong".into(),
            Frame::Close => "engine.io close".into(),
            Frame::Other(s) => format!("other: {s}"),
        }
    }
}

/// Decode one text frame using the Engine.IO / Socket.IO prefix rules.
pub fn decode(text: &str) -> Frame {
    let (eio, rest) = match text.split_at_checked(1) {
        Some((p, r)) => (p, r),
        None => return Frame::Other(text.to_string()),
    };
    match eio {
        "0" => Frame::Open(serde_json::from_str(rest).unwrap_or(Value::Null)),
        "1" => Frame::Close,
        "2" => Frame::Ping,
        "3" => Frame::Pong,
        "4" => decode_sio(rest),
        _ => Frame::Other(text.to_string()),
    }
}

fn decode_sio(rest: &str) -> Frame {
    let (kind, body) = match rest.split_at_checked(1) {
        Some((k, b)) => (k, b),
        None => return Frame::Other(rest.to_string()),
    };
    match kind {
        "0" => Frame::Connected(serde_json::from_str(body).unwrap_or(Value::Null)),
        "1" => Frame::Other(format!("namespace disconnect: {body}")),
        "2" => match serde_json::from_str::<Value>(body) {
            Ok(Value::Array(items)) if !items.is_empty() => {
                let name = items[0].as_str().unwrap_or("").to_string();
                let args = Value::Array(items[1..].to_vec());
                Frame::Event { name, args }
            }
            _ => Frame::Other(body.to_string()),
        },
        "3" => Frame::Ack(serde_json::from_str(body).unwrap_or(Value::Null)),
        "4" => Frame::ConnectError(serde_json::from_str(body).unwrap_or(Value::Null)),
        _ => Frame::Other(rest.to_string()),
    }
}

/// Open the socket, join the default namespace, and relay frames until the
/// deadline or `max_frames` is reached.
#[allow(dead_code)]
pub fn run(
    base_ws: &str,
    token: Option<&str>,
    query_pairs: &[(String, String)],
    seconds: u64,
    max_frames: usize,
    printer: &mut dyn FnMut(&str),
) -> Result<(), String> {
    let mut qs = String::from("EIO=4&transport=websocket");
    for (k, v) in query_pairs {
        qs.push('&');
        qs.push_str(&urlencode(k));
        qs.push('=');
        qs.push_str(&urlencode(v));
    }
    let url = format!("{base_ws}/socket.io/?{qs}");

    let mut req = url
        .as_str()
        .into_client_request()
        .map_err(|e| format!("build request: {e}"))?;
    {
        let headers = req.headers_mut();
        headers.insert("Origin", "https://manus.im".parse().unwrap());
        headers.insert(
            "User-Agent",
            format!("cue-cli/{}", env!("CARGO_PKG_VERSION")).parse().unwrap(),
        );
    }

    let (mut socket, response) =
        tungstenite::connect(req).map_err(|e| format!("connect {url}: {e}"))?;
    printer(&format!(
        "handshake {} (server={})",
        response.status(),
        response
            .headers()
            .get("server")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("-")
    ));

    let deadline = Instant::now() + Duration::from_secs(seconds);
    let mut sent_auth = false;
    let mut seen_namespace_reply = false;
    let mut count = 0usize;

    loop {
        if count >= max_frames || Instant::now() >= deadline {
            break;
        }
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            break;
        }

        let msg = match socket.read() {
            Ok(m) => m,
            Err(tungstenite::Error::Io(ref e))
                if e.kind() == std::io::ErrorKind::WouldBlock
                    || e.kind() == std::io::ErrorKind::TimedOut =>
            {
                continue
            }
            Err(tungstenite::Error::ConnectionClosed | tungstenite::Error::AlreadyClosed) => {
                printer("connection closed by peer");
                break;
            }
            Err(e) => return Err(format!("read: {e}")),
        };

        let text = match msg {
            Message::Text(t) => t.to_string(),
            Message::Binary(b) => {
                printer(&format!("binary frame ({} bytes)", b.len()));
                continue;
            }
            Message::Ping(_) | Message::Pong(_) => continue,
            Message::Close(frame) => {
                printer(&format!("websocket close: {frame:?}"));
                break;
            }
            other => {
                printer(&format!("unhandled frame: {other:?}"));
                continue;
            }
        };

        let frame = decode(&text);
        printer(&format!("<- {}", frame.summary()));
        count += 1;

        match frame {
            Frame::Open(_) if !sent_auth => {
                let payload = match token {
                    Some(t) => serde_json::json!({ "token": t }),
                    None => serde_json::json!({}),
                };
                let packet = format!("40{}", payload);
                socket
                    .send(Message::Text(packet.clone().into()))
                    .map_err(|e| format!("send connect packet: {e}"))?;
                printer(&format!("-> 40{{\"token\":{}}}", if token.is_some() { "\"<redacted>\"" } else { "" }));
                sent_auth = true;
            }
            Frame::Ping => {
                socket
                    .send(Message::Text("3".into()))
                    .map_err(|e| format!("send pong: {e}"))?;
                printer("-> 3 (pong)");
            }
            Frame::Connected(_) | Frame::ConnectError(_) => {
                seen_namespace_reply = true;
                count += 1;
            }
            _ => {}
        }
    }

    socket.close(None).ok();
    if !sent_auth {
        printer("server never sent the Engine.IO open packet");
    } else if !seen_namespace_reply {
        printer("no namespace reply before the deadline");
    }
    Ok(())
}

fn urlencode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            'a'..='z' | 'A'..='Z' | '0'..='9' | '-' | '_' | '.' | '~' => out.push(c),
            ' ' => out.push_str("%20"),
            other => out.push_str(&format!("%{:02X}", other as u32)),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_open_packet() {
        let f = decode("0{\"sid\":\"abc\",\"pingInterval\":15000}");
        match f {
            Frame::Open(v) => assert_eq!(v["sid"], "abc"),
            other => panic!("unexpected: {other:?}"),
        }
    }

    #[test]
    fn decodes_connect_error() {
        match decode("44{\"message\":\"Invalid Token\"}") {
            Frame::ConnectError(v) => assert_eq!(v["message"], "Invalid Token"),
            other => panic!("unexpected: {other:?}"),
        }
    }

    #[test]
    fn decodes_event_with_args() {
        match decode("42[\"message\",{\"type\":\"ack\"}]") {
            Frame::Event { name, args } => {
                assert_eq!(name, "message");
                assert_eq!(args[0]["type"], "ack");
            }
            other => panic!("unexpected: {other:?}"),
        }
    }

    #[test]
    fn decodes_ping_pong() {
        assert!(matches!(decode("2"), Frame::Ping));
        assert!(matches!(decode("3"), Frame::Pong));
    }
}

