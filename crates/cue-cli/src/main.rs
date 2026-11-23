//! Cue CLI — independent client for the Cue (ai.manus.agents) service.
//!
//! Wire protocol facts are taken from the shipped Android client and confirmed
//! by live probes of api.manus.im (see docs/protocol.md):
//!   * Connect RPC over POST /<package>.<Service>/<Method>
//!   * `connect-protocol-version: 1`
//!   * unary JSON (`application/json`) and unary proto (`application/proto`)
//!   * `authorization: Bearer <token>` where token is the value the web client
//!     stores as `sessionId`

use std::collections::HashMap;
use std::env;
use std::fs;
use std::path::PathBuf;

use serde_json::Value;

mod client;
mod commands;

fn usage() -> ! {
    eprintln!(
        "cue {version}

USAGE:
  cue <command> [args]

Commands:
  login --token <t> [--base <url>]   store bearer token (from the web client's sessionId)
  logout                              remove stored token
  whoami                              user.v1.UserService/UserInfo
  settings                            user.v1.UserPublicService/GetGlobalSettings (no auth)
  region                              user.v1.UserPublicService/CheckRegion (no auth)
  sessions [--limit n]                session.v1.SessionService/ListSessions
  session <session_uid>               session.v1.SessionService/GetSession
  agents [--limit n]                  agents.v1.AgentsService/ListAgentsV2
  rpc <service.Method> [--json <b>]   call any Connect RPC by path
  raw <service.Method> [--bytes <hex>] call any Connect RPC over proto
  methods                             print the static RPC inventory path
",
        version = env!("CARGO_PKG_VERSION")
    );
    std::process::exit(2)
}

fn main() {
    let args: Vec<String> = env::args().skip(1).collect();
    if args.is_empty() {
        usage();
    }
    let cmd = args[0].as_str();
    let rest = &args[1..];

    let mut flags: HashMap<String, String> = HashMap::new();
    let mut positional: Vec<String> = Vec::new();
    let mut i = 0;
    while i < rest.len() {
        let a = &rest[i];
        if let Some(name) = a.strip_prefix("--") {
            if i + 1 < rest.len() && !rest[i + 1].starts_with("--") {
                flags.insert(name.to_string(), rest[i + 1].clone());
                i += 2;
            } else {
                flags.insert(name.to_string(), "true".to_string());
                i += 1;
            }
        } else {
            positional.push(a.clone());
            i += 1;
        }
    }

    let result = match cmd {
        "login" => commands::login(flags.get("token"), flags.get("base")),
        "logout" => commands::logout(),
        "whoami" => commands::whoami(),
        "settings" => commands::settings(),
        "region" => commands::region(),
        "sessions" => commands::sessions(flags.get("limit")),
        "session" => commands::session(positional.first()),
        "agents" => commands::agents(flags.get("limit")),
        "rpc" => commands::rpc(positional.first(), flags.get("json")),
        "raw" => commands::raw(positional.first(), flags.get("bytes")),
        "methods" => commands::methods(),
        "--help" | "-h" | "help" => {
            usage();
        }
        other => {
            eprintln!("unknown command: {other}");
            usage();
        }
    };

    match result {
        Ok(out) => {
            if !out.is_empty() {
                println!("{out}");
            }
        }
        Err(err) => {
            eprintln!("error: {err}");
            std::process::exit(1);
        }
    }
}

/// Directory that holds CLI state. Created on demand by `login`.
pub fn state_dir() -> Result<PathBuf, String> {
    let base = if cfg!(windows) {
        env::var_os("APPDATA").map(PathBuf::from)
    } else {
        env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .or_else(|| env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
    };
    let base = base.ok_or("cannot locate a config directory (set APPDATA or XDG_CONFIG_HOME)")?;
    let dir = base.join("cue-cli");
    fs::create_dir_all(&dir).map_err(|e| format!("create {}: {e}", dir.display()))?;
    Ok(dir)
}

/// Pretty-print a JSON value with stable ordering.
pub fn pretty(value: &Value) -> String {
    serde_json::to_string_pretty(value).unwrap_or_else(|_| value.to_string())
}
