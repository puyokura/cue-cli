// SPDX-License-Identifier: AGPL-3.0-or-later
//! One chat-socket session: connect, authenticate, optionally join a session,
//! optionally post a prompt, then stream the server's event frames.
//!
//! Frame shapes come from the shipped Android client and the web bundle; see
//! `docs/protocol.md` §6 and §8. Wire keys are snake_case in both directions.

use std::time::{Duration, Instant};

use serde_json::{json, Value};
use tungstenite::client::IntoClientRequest;
use tungstenite::Message;

use crate::chat;
use crate::socket::{self, Frame};

/// What the caller wants this session to do.
pub struct Plan<'a> {
    pub base_ws: &'a str,
    pub token: Option<&'a str>,
    pub locale: &'a str,
    pub join_session: Option<&'a str>,
    /// `(session_id, content)` — post a prompt once the socket is ready.
    pub prompt: Option<(&'a str, &'a str)>,
    pub task_mode: &'a str,
    pub delivery_mode: &'a str,
    pub country_iso: Option<&'a str>,
    pub seconds: u64,
    pub max_frames: usize,
    /// Print the raw frame JSON as well as the one-line summary.
    pub verbose: bool,
}

/// Result of a session run, kept small so callers can assert on it.
#[derive(Debug, Default)]
pub struct Outcome {
    pub engine_open: bool,
    pub namespace_ok: bool,
    pub namespace_error: Option<String>,
    pub joined: bool,
    pub prompt_sent: bool,
    pub events: usize,
    pub chat_frames: usize,
    pub transcript: Vec<String>,
}

pub fn run(plan: Plan<'_>, out: &mut dyn FnMut(&str)) -> Result<Outcome, String> {
    let query = [
        ("locale".to_string(), plan.locale.to_string()),
        ("tz".to_string(), "Asia/Tokyo".to_string()),
        ("clientType".to_string(), "CUE".to_string()),
        ("clientVersion".to_string(), "1.2.0".to_string()),
        (
            "x-product-name".to_string(),
            crate::client::PRODUCT_NAME.to_string(),
        ),
    ];

    let mut request = format!(
        "{}/socket.io/?EIO=4&transport=websocket{}",
        plan.base_ws,
        query
            .iter()
            .map(|(k, v)| format!("&{k}={}", urlencode(v)))
            .collect::<String>()
    )
    .as_str()
    .into_client_request()
    .map_err(|e| format!("build request: {e}"))?;
    {
        let headers = request.headers_mut();
        headers.insert("Origin", "https://manus.im".parse().unwrap());
        headers.insert(
            "User-Agent",
            format!("cue-cli/{}", env!("CARGO_PKG_VERSION"))
                .parse()
                .unwrap(),
        );
    }

    let (mut ws, response) =
        tungstenite::connect(request).map_err(|e| format!("connect {}: {e}", plan.base_ws))?;
    out(&format!(
        "handshake {} server={}",
        response.status(),
        header(&response, "server")
    ));

    let deadline = Instant::now() + Duration::from_secs(plan.seconds);
    let mut outcome = Outcome::default();
    let mut frames = 0usize;
    let mut auth_sent = false;
    let mut join_sent = false;
    let mut prompt_sent = false;
    let message_id = chat::message_id();

    loop {
        if frames >= plan.max_frames || Instant::now() >= deadline {
            break;
        }
        let msg = match ws.read() {
            Ok(m) => m,
            Err(tungstenite::Error::ConnectionClosed | tungstenite::Error::AlreadyClosed) => {
                out("connection closed by peer");
                break;
            }
            Err(tungstenite::Error::Io(ref e))
                if e.kind() == std::io::ErrorKind::WouldBlock
                    || e.kind() == std::io::ErrorKind::TimedOut =>
            {
                continue
            }
            Err(e) => return Err(format!("read: {e}")),
        };

        let text = match msg {
            Message::Text(t) => t.to_string(),
            Message::Binary(b) => {
                out(&format!("binary frame ({} bytes)", b.len()));
                continue;
            }
            Message::Ping(_) | Message::Pong(_) => continue,
            Message::Close(c) => {
                out(&format!("close: {c:?}"));
                break;
            }
            other => {
                out(&format!("unhandled frame: {other:?}"));
                continue;
            }
        };

        let frame = socket::decode(&text);
        frames += 1;
        out(&format!("<- {}", frame.summary()));
        if plan.verbose {
            out(&format!("   raw: {}", truncate(&text, 1200)));
        }

        match frame {
            Frame::Open(_) if !auth_sent => {
                let packet = match plan.token {
                    Some(t) => format!("40{}", json!({ "token": t })),
                    None => "40{}".to_string(),
                };
                ws.send(Message::Text(packet.into()))
                    .map_err(|e| format!("send auth: {e}"))?;
                out("-> 40 auth");
                auth_sent = true;
                outcome.engine_open = true;
            }
            Frame::Ping => {
                ws.send(Message::Text("3".into()))
                    .map_err(|e| format!("send pong: {e}"))?;
            }
            Frame::Connected(_) => {
                outcome.namespace_ok = true;
                if let Some(session) = plan.join_session {
                    if !join_sent {
                        let payload = chat::join_session(session, "new_session");
                        send_event(&mut ws, "message", &payload)?;
                        out(&format!("-> join_session {session}"));
                        join_sent = true;
                    }
                }
                if let Some((session, content)) = plan.prompt {
                    if !prompt_sent {
                        let payload = chat::user_message(
                            &message_id,
                            session,
                            content,
                            plan.task_mode,
                            "text",
                            plan.delivery_mode,
                            plan.country_iso,
                            None,
                        );
                        send_event(&mut ws, "message", &payload)?;
                        out(&format!(
                            "-> user_message session={session} id={message_id} task_mode={}",
                            plan.task_mode
                        ));
                        prompt_sent = true;
                        outcome.prompt_sent = true;
                        outcome.joined = join_sent;
                    }
                }
            }
            Frame::ConnectError(v) => {
                outcome.namespace_error = Some(v.to_string());
                out("namespace rejected; stopping");
                break;
            }
            Frame::Event { name, args } => {
                record_event(&name, &args, &mut outcome, out);
            }
            _ => {}
        }
    }

    ws.close(None).ok();
    if !auth_sent {
        out("server never sent the Engine.IO open packet");
    }
    outcome.joined = join_sent;
    Ok(outcome)
}

fn send_event(
    ws: &mut tungstenite::WebSocket<tungstenite::stream::MaybeTlsStream<std::net::TcpStream>>,
    event: &str,
    payload: &Value,
) -> Result<(), String> {
    let wire = chat::to_wire(payload);
    let packet = format!("42{}", json!([event, wire]));
    ws.send(Message::Text(packet.into()))
        .map_err(|e| format!("send {event}: {e}"))
}

fn record_event(name: &str, args: &Value, outcome: &mut Outcome, out: &mut dyn FnMut(&str)) {
    outcome.events += 1;
    let payload = args.get(0).cloned().unwrap_or(Value::Null);
    let kind = payload.get("type").and_then(Value::as_str).unwrap_or("");

    // Server frames are `{"type":"event","sessionId":...,"event":{...}}`.
    if kind == "event" {
        let event = payload.get("event").cloned().unwrap_or(Value::Null);
        let event_type = event.get("type").and_then(Value::as_str).unwrap_or("");
        match event_type {
            "chat" => {
                outcome.chat_frames += 1;
                let sender = event.get("sender").and_then(Value::as_str).unwrap_or("?");
                let text = extract_text(&event);
                let line = format!("chat[{sender}] {}", truncate(&text, 400));
                outcome.transcript.push(line.clone());
                out(&line);
            }
            "chatDelta" => {
                let delta = event
                    .get("delta")
                    .and_then(|d| d.get("content"))
                    .and_then(Value::as_str)
                    .unwrap_or("");
                if !delta.is_empty() {
                    out(&format!("delta {}", truncate(delta, 200)));
                }
            }
            "agent_error" => {
                let code = event.get("code").cloned().unwrap_or(Value::Null);
                let message = event.get("message").and_then(Value::as_str).unwrap_or("");
                let line = format!("agent_error code={code} {message}");
                outcome.transcript.push(line.clone());
                out(&line);
            }
            "statusUpdate" => {
                let status = event
                    .get("agentStatus")
                    .and_then(Value::as_str)
                    .unwrap_or("");
                out(&format!("status {status}"));
            }
            "liveStatus" => {
                let text = event.get("text").and_then(Value::as_str).unwrap_or("");
                if !text.is_empty() {
                    out(&format!("liveStatus {text}"));
                }
            }
            other => out(other),
        }
        return;
    }

    match kind {
        "chat" => {
            outcome.chat_frames += 1;
            let event = payload.clone();
            let event_type = event.get("type").and_then(Value::as_str).unwrap_or("");
            let text = extract_text(&event);
            let line = format!("chat[{event_type}] {}", truncate(&text, 400));
            outcome.transcript.push(line.clone());
            out(&line);
        }
        "agent_error" => {
            let code = payload.get("code").cloned().unwrap_or(Value::Null);
            let message = payload.get("message").and_then(Value::as_str).unwrap_or("");
            let line = format!("agent_error code={code} {message}");
            outcome.transcript.push(line.clone());
            out(&line);
        }
        "" => out(&format!(
            "event::{name} {}",
            truncate(&payload.to_string(), 300)
        )),
        other => out(&format!("{other} {}", truncate(&payload.to_string(), 300))),
    }
}

/// Pull the human-readable text out of a chat event without inventing any.
fn extract_text(event: &Value) -> String {
    if let Some(text) = event.get("content").and_then(Value::as_str) {
        return text.to_string();
    }
    if let Some(text) = event.get("text").and_then(Value::as_str) {
        return text.to_string();
    }
    if let Some(delta) = event.get("delta").and_then(Value::as_str) {
        return delta.to_string();
    }
    String::new()
}

fn header(resp: &tungstenite::http::Response<Option<Vec<u8>>>, name: &str) -> String {
    resp.headers()
        .get(name)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("-")
        .to_string()
}

fn truncate(s: &str, max: usize) -> String {
    match s.char_indices().nth(max) {
        Some((idx, _)) => format!("{}…", &s[..idx]),
        None => s.to_string(),
    }
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
