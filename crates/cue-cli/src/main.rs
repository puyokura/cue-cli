// SPDX-License-Identifier: AGPL-3.0-or-later
//! Cue CLI — independent client for the Cue (ai.manus.agents) service.
//!
//! Wire protocol facts are taken from the shipped Android client and confirmed
//! by live probes of api.manus.im (see docs/protocol.md):
//!   * Connect RPC over POST /<package>.<Service>/<Method>
//!   * `connect-protocol-version: 1`
//!   * unary JSON (`application/json`) and unary proto (`application/proto`)
//!   * `authorization: Bearer <token>` where token is a service-issued bearer
//!     token (the native login flows issue it; see docs/auth-status.md)

use std::collections::HashMap;
use std::env;
use std::fs;
use std::path::PathBuf;

use serde_json::Value;

mod chat;
mod mcp;
mod client;
mod commands;
mod phone;
mod session;
mod socket;

/// Print usage and exit 0 (explicit help request).
fn usage_ok() -> ! {
    usage_body();
    std::process::exit(0)
}

/// Print usage and exit 2 (misuse: no command, unknown command).
fn usage() -> ! {
    usage_body();
    std::process::exit(2)
}

fn usage_body() {
    eprintln!(
        "cue {version}

USAGE:
  cue <command> [args]

Commands:
  login [--token <t>] [--base <url>]  no --token: interactive phone+SMS login
  phone start --number <n> [--purpose login|reset_password]
                                     send an SMS code; prints the verification id
  phone verify --id <v> --code <c>   exchange the SMS code for a session token
  phone password --number <n>       log in with a phone number and password
                                     (password is read from stdin when omitted)
  phone reset --id <v> --code <c>    set a new password (read from stdin)
  mfa <status|totp-start|totp-finish|enable|disable|codes|login-methods|passkeys|passkey-start|passkey-finish|passkey-delete>
                                     two-step verification management (needs a token)
  logout                              remove stored token
  whoami                              user.v1.UserService/UserInfo
  settings                            user.v1.UserPublicService/GetGlobalSettings (no auth)
  region                              user.v1.UserPublicService/CheckRegion (no auth)
  sessions [--limit n]                session.v1.SessionService/ListSessions
  session <session_uid>               session.v1.SessionService/GetSession
  agents [--limit n]                  agents.v1.AgentsService/ListAgentsV2
  rpc <service.Method> [--json <b>]   call any Connect RPC by path
  raw <service.Method> [--bytes <hex>] call any Connect RPC over proto
  watch [--seconds n] [--frames n]    Socket.IO session: observe frames
  send <session_id> --text <prompt>   join an existing session and post a prompt
  new --text <prompt>                 start a new session (id is generated locally)
  sandbox <session_id>                report the sandbox state for a session (read-only)
  webdev [--limit n] [--detail <id>]  list WebDev projects or read one (read-only)
  webdev-deploy --project <id> --version <id>
                                      publish a WebDev project to its public domain
  join <session_id> [--seconds n]     subscribe to a session without posting
  methods                             print the static RPC inventory path
  discover <service> <method>...      probe whether Connect routes exist (404/401 oracle)
  coverage                            read-only survey of the authenticated surface
  version                              print the version
  mcp                                  serve MCP over stdio (initialize/tools/list/tools/call)
",
        version = env!("CARGO_PKG_VERSION")
    );
}

fn main() {
    let args: Vec<String> = env::args().skip(1).collect();
    if args.is_empty() {
        usage();
    }

    // Global flags may precede the command (`cue --state DIR --version`).
    let mut flags: HashMap<String, String> = HashMap::new();
    let mut i = 0;
    while i < args.len() {
        let a = &args[i];
        match a.strip_prefix("--") {
            Some(name) if name == "state" && i + 1 < args.len() && !args[i + 1].starts_with("--") => {
                flags.insert("state".to_string(), args[i + 1].clone());
                i += 2;
            }
            Some(name) if name == "version" => {
                flags.insert("version".to_string(), "true".to_string());
                i += 1;
            }
            _ => break,
        }
    }
    let remaining = &args[i..];
    if remaining.is_empty() {
        if flags.contains_key("version") {
            println!("cue {}", env!("CARGO_PKG_VERSION"));
            return;
        }
        usage();
    }
    let cmd = remaining[0].as_str();
    let rest = &remaining[1..];

    let mut positional: Vec<String> = Vec::new();
    let mut j = 0;
    while j < rest.len() {
        let a = &rest[j];
        if let Some(name) = a.strip_prefix("--") {
            if j + 1 < rest.len() && !rest[j + 1].starts_with("--") {
                flags.insert(name.to_string(), rest[j + 1].clone());
                j += 2;
            } else {
                flags.insert(name.to_string(), "true".to_string());
                j += 1;
            }
        } else {
            positional.push(a.clone());
            j += 1;
        }
    }

    // `--state` redirects the state directory before anything reads it.
    if let Some(dir) = flags.get("state") {
        // Safety: this runs before any other thread exists.
        std::env::set_var("CUE_STATE", dir);
    }

    // Reject flags a command does not implement: silently ignoring them makes a
    // mistyped option look accepted.
    let allowed: &[&str] = match cmd {
        "login" => &["token", "base", "state"],
        "phone" => &["number", "purpose", "id", "code", "password", "state"],
        "mfa" => &["flow", "code", "credential", "name", "id", "state"],
        "join" => &["seconds", "frames", "locale", "verbose", "state"],
        "webdev" => &["limit", "detail", "state"],
        "webdev-deploy" => &["project", "version", "state"],
        "sandbox" => &["state"],
        "new" => &["text", "seconds", "frames", "task-mode", "delivery-mode", "locale",
                  "country-iso", "verbose", "state"],
        "send" => &["text", "seconds", "frames", "task-mode", "delivery-mode", "locale",
                    "country-iso", "verbose", "state"],
        "watch" => &["seconds", "frames", "locale", "state"],
        "sessions" | "agents" => &["limit", "state"],
        "rpc" => &["json", "state"],
        "raw" => &["bytes", "state"],
        _ => &["state"],
    };
    if !allowed.is_empty() {
        for name in flags.keys() {
            if !allowed.contains(&name.as_str()) {
                eprintln!("error: unknown flag --{name} for `cue {cmd}`");
                eprintln!("allowed: {}", allowed.iter().map(|f| format!("--{f}")).collect::<Vec<_>>().join(", "));
                std::process::exit(2);
            }
        }
    }

    let result = match cmd {
        "login" => commands::login(flags.get("token"), flags.get("base")),
        "phone" => commands::phone(positional.first(), &flags),
        "mfa" => commands::mfa(positional.first(), &flags),
        "logout" => commands::logout(),
        "whoami" => commands::whoami(),
        "settings" => commands::settings(),
        "region" => commands::region(),
        "sessions" => commands::sessions(flags.get("limit")),
        "session" => commands::session(positional.first()),
        "agents" => commands::agents(flags.get("limit")),
        "rpc" => commands::rpc(positional.first(), flags.get("json")),
        "raw" => commands::raw(positional.first(), flags.get("bytes")),
        "watch" => commands::watch(flags.get("seconds"), flags.get("frames"), flags.get("locale")),
        "send" => commands::send(positional.first(), flags.get("text"), &flags),
        "new" => commands::new_session(flags.get("text"), &flags),
        "sandbox" => commands::sandbox(positional.first()),
        "webdev" => {
            if let Some(pid) = flags.get("detail") {
                commands::webdev_detail(Some(pid))
            } else {
                commands::webdev(flags.get("limit"))
            }
        },
        "webdev-deploy" => commands::webdev_deploy(flags.get("project"), flags.get("version")),
        "join" => commands::join(positional.first(), &flags),
        "discover" => commands::discover(positional.first(), &positional[1..].to_vec()),
        "coverage" => commands::coverage(),
        "methods" => commands::methods(),
        "version" | "--version" | "-V" => Ok(format!("cue {}", env!("CARGO_PKG_VERSION"))),
        "mcp" => mcp::serve(),
        "--help" | "-h" | "help" => {
            usage_ok();
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
///
/// Overrides, in order: `CUE_STATE` (directory), then `--state` handled by the
/// caller; otherwise the OS config directory.
pub fn state_dir() -> Result<PathBuf, String> {
    if let Some(dir) = env::var_os("CUE_STATE") {
        let dir = PathBuf::from(dir);
        fs::create_dir_all(&dir).map_err(|e| format!("create {}: {e}", dir.display()))?;
        restrict(&dir)?;
        return Ok(dir);
    }
    let base = if cfg!(windows) {
        env::var_os("APPDATA").map(PathBuf::from)
    } else {
        env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .or_else(|| env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
    };
    let base = base.ok_or("cannot locate a config directory (set APPDATA, XDG_CONFIG_HOME or CUE_STATE)")?;
    let dir = base.join("cue-cli");
    fs::create_dir_all(&dir).map_err(|e| format!("create {}: {e}", dir.display()))?;
    restrict(&dir)?;
    Ok(dir)
}

/// Tighten permissions on Unix: directory 0700, files 0600. A no-op elsewhere.
fn restrict(path: &std::path::Path) -> Result<(), String> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = if path.is_dir() { 0o700 } else { 0o600 };
        fs::set_permissions(path, fs::Permissions::from_mode(mode))
            .map_err(|e| format!("chmod {}: {e}", path.display()))?;
    }
    let _ = path;
    Ok(())
}

/// Write a state file atomically with restrictive permissions.
/// A partially written token file would be worse than none.
pub fn write_state(path: &std::path::Path, contents: &str) -> Result<(), String> {
    let tmp = path.with_extension("tmp");
    fs::write(&tmp, contents).map_err(|e| format!("write {}: {e}", tmp.display()))?;
    restrict(&tmp)?;
    fs::rename(&tmp, path).map_err(|e| format!("rename {}: {e}", path.display()))?;
    restrict(path)?;
    Ok(())
}

/// Pretty-print a JSON value with stable ordering.
pub fn pretty(value: &Value) -> String {
    serde_json::to_string_pretty(value).unwrap_or_else(|_| value.to_string())
}



















