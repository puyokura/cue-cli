// SPDX-License-Identifier: AGPL-3.0-or-later
//! Cue CLI — independent client for the Cue service.
//!
//! Wire protocol facts are taken from the shipped Android client and confirmed
//! by live probes (see docs/protocol.md):
//!   * Connect RPC over POST /<package>.<Service>/<Method>
//!   * `connect-protocol-version: 1`
//!   * unary JSON (`application/json`) and unary proto (`application/proto`)
//!   * `authorization: Bearer <token>` where the token is service-issued

use std::collections::HashMap;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use serde_json::Value;

mod body;
mod chat;
mod cli;
mod client;
mod commands;
mod doctor;
mod mcp;
mod phone;
mod session;
mod socket;
mod state;

/// Flags every command accepts.
const GLOBAL_FLAGS: &[&str] = &["state", "help", "h"];

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

USAGE
  cue <command> [subcommand] [args] [--flags]

ACCOUNT AND SESSION
  login [--token <t>] [--base <url>]   no --token: interactive phone+SMS login
  logout                               remove the stored token
  whoami                               user.v1.UserService/UserInfo
  status [--json]                      what this CLI thinks its state is
  doctor [--live] [--json]             check state, auth, and the service
  settings                             public settings (no auth)
  region                               region hints (no auth)

PHONE AND TWO-STEP VERIFICATION
  phone start --number <n> [--purpose login|reset_password]
  phone verify --id <v> --code <c>     exchange the SMS code for a token
  phone password --number <n>          log in with a password (stdin when omitted)
  phone reset --id <v> --code <c>      set a new password (stdin)
  mfa <status|totp-start|totp-finish|enable|disable|codes|login-methods>
  mfa <passkeys|passkey-start|passkey-finish|passkey-delete>

CHAT AND SESSIONS
  sessions [--limit n]                 list sessions
  session <session_uid>                read one session
  new --text <prompt>                  start a session (the id is generated here)
  send <session_id> --text <prompt>    post a prompt to an existing session
  join <session_id> [--seconds n]      subscribe without posting
  watch [--seconds n] [--frames n]     observe the Socket.IO stream
  agents [--limit n]                   list agents
  sandbox <session_id>                 sandbox state and VNC endpoints (read-only)

WORKSPACE AND TELEPHONY
  webdev [--limit n] [--detail <id>]   list or read WebDev projects (read-only)
  webdev-deploy --project <id> --version <id> [--confirm]
                                       publish a project to its public domain
  telecom                              countries, rates, free-phone settings

LOW LEVEL
  rpc <Service.Method> [--set k=v ...] [--json FILE|-] [--confirm]
                                       call any Connect RPC with a JSON body
  raw <Service.Method> [--bytes <hex>] [--confirm]
                                       call any Connect RPC over the proto codec
  discover <service> <method>... [--confirm]
                                       probe whether Connect routes exist
  coverage                             read-only survey of the authenticated surface
  methods                              print the static RPC inventory path
  version                              print the version
  mcp                                  serve MCP over stdio

JSON INPUT
  --set key=value        First choice: no quote characters, so no shell can mangle
                         it. Repeatable. true/false/null and integers are typed;
                         a,b,c becomes an array.
  --json-file PATH       Second choice: exact bytes from a file (UTF-8).
  --json JSON            Only when the shell preserves quotes (bash, cmd).
  --json -               Read UTF-8 JSON from stdin.

  Windows PowerShell 5.1 strips double quotes even inside single quotes before a
  native process sees the argument, and defaults $OutputEncoding to us-ascii, so
  non-ASCII becomes '?' on the way into a pipe. Use --set or --json-file there.

DESTRUCTIVE OPERATIONS
  Every command that can change remote state requires --confirm: webdev-deploy,
  new, send, discover, `mfa` (enable/disable/totp/passkey/codes),
  `phone reset`, and any `rpc`/`raw` whose method is not one of the read prefixes
  (Get/List/Search/Page/Check/Query/View/Read/User) or is a credential minter.

  --confirm takes no value and must be the last argument. `--confirm false`,
  an empty value, or an unset-variable expansion are refused: only the bare
  switch counts as consent.

  The decision is made on the method name, and the default is `state-changing`:
  a method the CLI has never seen is gated until its name says it reads. A
  malformed method path (with `#` or `?`, or more than one `/`) is never treated
  as a read.

GLOBAL
  --state PATH   State file or directory. A directory holds `token` and `base`;
                 a file is the token itself and its base lives beside it.

ENVIRONMENT
  CUE_STATE / CUE_MCP_STATE   Override the state path (either spelling wins
                              over the OS default; `--state` wins over both)
  CUE_BASE_URL                Override the service origin
  CUE_WS_URL                  Override the Socket.IO origin
",
        version = env!("CARGO_PKG_VERSION")
    );
}

/// The command names this binary implements.
const COMMANDS: &[&str] = &[
    "login",
    "logout",
    "whoami",
    "status",
    "doctor",
    "settings",
    "region",
    "phone",
    "mfa",
    "sessions",
    "session",
    "agents",
    "rpc",
    "raw",
    "watch",
    "send",
    "new",
    "sandbox",
    "telecom",
    "webdev",
    "webdev-deploy",
    "join",
    "discover",
    "coverage",
    "methods",
    "version",
    "mcp",
    "help",
];

fn is_known_command(name: &str) -> bool {
    COMMANDS.contains(&name)
}

/// Flags each command reads, beyond `GLOBAL_FLAGS`.
fn known_flags(command: &str) -> &'static [&'static str] {
    match command {
        "login" => &["token", "base"],
        "phone" => &["number", "purpose", "id", "code", "password", "confirm"],
        "mfa" => &["flow", "code", "credential", "name", "id", "confirm"],
        "status" => &["json"],
        "doctor" => &["live", "json"],
        "sessions" | "agents" => &["limit"],
        "webdev" => &["limit", "detail"],
        "webdev-deploy" => &["project", "version", "confirm"],
        "join" => &["seconds", "frames", "locale", "verbose"],
        "new" => &[
            "text",
            "seconds",
            "frames",
            "task-mode",
            "delivery-mode",
            "locale",
            "country-iso",
            "verbose",
            "confirm",
        ],
        "send" => &[
            "text",
            "seconds",
            "frames",
            "task-mode",
            "delivery-mode",
            "locale",
            "country-iso",
            "verbose",
            "confirm",
        ],
        "watch" => &["seconds", "frames", "locale"],
        "rpc" => &["set", "json", "json-file", "confirm"],
        "methods" => &["list"],
        "discover" => &["confirm"],
        "raw" => &["bytes", "confirm"],
        _ => &[],
    }
}

fn main() {
    let raw: Vec<String> = env::args().skip(1).collect();
    if raw.is_empty() {
        usage();
    }

    // Global flags may precede the command (`--state DIR status`,
    // `--state=DIR status`). The `=` form has to be handled here as well: after
    // the command the parser splits it, before the command this loop is the
    // parser, and the two positions must not disagree.
    let mut prefix: Vec<String> = Vec::new();
    let mut index = 0usize;
    while index < raw.len() {
        let token = raw[index].clone();
        if let Some(value) = token.strip_prefix("--state=") {
            prefix.push("--state".to_string());
            prefix.push(value.to_string());
            index += 1;
            continue;
        }
        match token.as_str() {
            "--version" | "-V" => {
                println!("cue {}", env!("CARGO_PKG_VERSION"));
                return;
            }
            "--help" | "-h" => {
                usage_ok();
            }
            "--state" if index + 1 < raw.len() => {
                prefix.push("--state".to_string());
                prefix.push(raw[index + 1].clone());
                index += 2;
            }
            _ => break,
        }
    }

    let remaining = &raw[index..];
    if remaining.is_empty() {
        usage();
    }
    let command = remaining[0].clone();
    let rest: Vec<String> = prefix.into_iter().chain(remaining[1..].to_vec()).collect();
    let args = cli::Args::parse(&rest);

    // `--state` redirects the state path before anything reads it, and it wins
    // over both environment spellings. An empty value would fall through to the
    // OS default, which looks like it worked while reading a directory the
    // caller did not name — so it is rejected here.
    if let Some(path) = args.flag("state") {
        if path.trim().is_empty() {
            eprintln!("error: --state needs a path");
            std::process::exit(2);
        }
        // Safety: this runs before any other thread exists.
        env::set_var("CUE_STATE", path);
        // The flag wins, so a lower-priority variable that happens to be empty
        // must not block it.
        env::remove_var("CUE_MCP_STATE");
    }

    if let Err(err) = body::reject_unknown_flags(
        &command,
        &args,
        GLOBAL_FLAGS,
        known_flags(&command),
        body::command_booleans(&command),
    ) {
        eprintln!("error: {err}");
        std::process::exit(2);
    }

    // An explicit help request is a read of the usage text, not a request to run
    // the command. It has to win over every gate, otherwise `mfa disable --help`
    // tries to disable MFA while printing how to use it.
    // An unknown command is a usage error, and --help does not change that: a
    // typo plus `--help` must not read as a successful request for help.
    if !is_known_command(&command) {
        eprintln!("unknown command: {command}");
        usage();
    }

    // `-h` is help wherever it appears; treating only `--help` as help meant
    // `webdev-deploy ... -h` performed the deploy instead of printing usage.
    if args.has("help") || args.has("h") {
        usage_ok();
    }

    // A command that can change remote state has to say so. Argument validation
    // runs first, so `webdev-deploy` with no --project reports the missing
    // argument instead of asking for --confirm on a call that cannot start.
    if let Err(err) = require_arguments(&command, &args) {
        eprintln!("error: {err}");
        std::process::exit(2);
    }
    // Consent has to be unmistakable. Three shapes keep going wrong in scripts:
    //   --confirm=false     a flag with a value
    //   --confirm false     a switch plus a trailing word
    //   --confirm ""        a switch plus an empty token (the unset-variable case)
    // Only a bare `--confirm` that ends the command line counts, so nothing can
    // follow it and be read as an answer. A trailing token is refused whatever
    // it says, which makes the rule a shape rule rather than a word list:
    // `cue rpc <method> --confirm`, not `cue rpc --confirm <method>`.
    let confirmed = args.has("confirm") && args.flag("confirm").is_none();
    if args.has("confirm") && !args.confirm_is_last() {
        eprintln!(
            "error: --confirm must be the last argument; \
             `--confirm false` and `--confirm \"\"` are not confirmation."
        );
        std::process::exit(2);
    }
    if body::is_destructive_command(&command, &args) && !confirmed {
        eprintln!("error: `cue {command}` can change remote state; add --confirm");
        std::process::exit(2);
    }

    let flags = legacy_flags(&args);

    let result = match command.as_str() {
        "login" => commands::login(
            args.flag("token").map(str::to_string).as_ref(),
            args.flag("base").map(str::to_string).as_ref(),
        ),
        "phone" => commands::phone(args.positional(0).map(str::to_string).as_ref(), &flags),
        "mfa" => commands::mfa(args.positional(0).map(str::to_string).as_ref(), &flags),
        "logout" => commands::logout(),
        "whoami" => commands::whoami(),
        "status" => Ok(status_output(&args)),
        "doctor" => Ok(doctor_output(&args)),
        "settings" => commands::settings(),
        "region" => commands::region(),
        "sessions" => commands::sessions(args.flag("limit").map(str::to_string).as_ref()),
        "session" => commands::session(args.positional(0).map(str::to_string).as_ref()),
        "agents" => commands::agents(args.flag("limit").map(str::to_string).as_ref()),
        "rpc" => commands::rpc(args.positional(0).map(str::to_string).as_ref(), &args),
        "raw" => commands::raw(
            args.positional(0).map(str::to_string).as_ref(),
            args.flag("bytes").map(str::to_string).as_ref(),
        ),
        "watch" => commands::watch(
            args.flag("seconds").map(str::to_string).as_ref(),
            args.flag("frames").map(str::to_string).as_ref(),
            args.flag("locale").map(str::to_string).as_ref(),
        ),
        "send" => commands::send(
            args.positional(0).map(str::to_string).as_ref(),
            args.flag("text").map(str::to_string).as_ref(),
            &flags,
        ),
        "new" => commands::new_session(args.flag("text").map(str::to_string).as_ref(), &flags),
        "sandbox" => commands::sandbox(args.positional(0).map(str::to_string).as_ref()),
        "telecom" => commands::telecom(),
        "webdev" => {
            if let Some(pid) = args.flag("detail") {
                commands::webdev_detail(Some(&pid.to_string()))
            } else {
                commands::webdev(args.flag("limit").map(str::to_string).as_ref())
            }
        }
        "webdev-deploy" => commands::webdev_deploy(
            args.flag("project").map(str::to_string).as_ref(),
            args.flag("version").map(str::to_string).as_ref(),
        ),
        "join" => commands::join(args.positional(0).map(str::to_string).as_ref(), &flags),
        "discover" => {
            let methods: Vec<String> = args.positionals.iter().skip(1).cloned().collect();
            commands::discover(args.positional(0).map(str::to_string).as_ref(), &methods)
        }
        "coverage" => commands::coverage(),
        "methods" => {
            if args.has("list") {
                commands::methods_list()
            } else {
                commands::methods()
            }
        }
        "version" => Ok(format!("cue {}", env!("CARGO_PKG_VERSION"))),
        "mcp" => mcp::serve(),
        "--help" | "-h" | "help" => usage_ok(),
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

/// Arguments that must be present before any work starts.
///
/// A missing required argument is a usage error, not a runtime failure, so it
/// exits 2 and never reaches the network or the confirm gate.
fn require_arguments(command: &str, args: &cli::Args) -> Result<(), String> {
    let required: &[(&str, &str)] = match command {
        "webdev-deploy" => &[
            ("project", "--project <projectId>"),
            ("version", "--version <versionId>"),
        ],
        "send" | "new" => &[("text", "--text <prompt>")],
        _ => &[],
    };
    for (flag, usage) in required {
        if args.flag(flag).is_none() {
            return Err(format!("`cue {command}` requires {usage}"));
        }
    }
    Ok(())
}

fn legacy_flags(args: &cli::Args) -> HashMap<String, String> {
    let mut map = HashMap::new();
    for name in args.supplied() {
        if name == "set" || name == "confirm" {
            continue;
        }
        if let Some(value) = args.flag(&name) {
            map.insert(name.clone(), value.to_string());
        } else {
            map.insert(name, "true".to_string());
        }
    }
    map
}

fn status_output(args: &cli::Args) -> String {
    let value = doctor::status();
    if args.has("json") {
        pretty(&value)
    } else {
        let signed_in = value
            .get("signed_in")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let state_path = value
            .get("state_path")
            .and_then(Value::as_str)
            .unwrap_or("-");
        let base = value.get("base").and_then(Value::as_str).unwrap_or("-");
        let identity = value.get("identity_check").cloned().unwrap_or(Value::Null);
        let ok = identity.get("ok").and_then(Value::as_bool).unwrap_or(false);
        let mut out = String::new();
        out.push_str(&format!("state_path: {state_path}\n"));
        out.push_str(&format!("base:       {base}\n"));
        out.push_str(&format!(
            "signed_in:  {signed_in} ({})\n",
            if signed_in {
                "a token is stored; this does not prove it is accepted"
            } else {
                "no token; run `cue login`"
            }
        ));
        out.push_str(&format!(
            "identity:   {}\n",
            if ok {
                "token accepted by the service"
            } else {
                "token not verified"
            }
        ));
        out
    }
}

fn doctor_output(args: &cli::Args) -> String {
    let live = args.has("live");
    let report = doctor::run(live);
    // A doctor a script cannot read is half a doctor: exit non-zero when any
    // check failed, the same way the Muse baseline does.
    if report.failed() {
        if args.has("json") {
            println!("{}", pretty(&report.to_json(live)));
        } else {
            print!("{}", report.to_text(live));
        }
        std::process::exit(1);
    }
    if args.has("json") {
        pretty(&report.to_json(live))
    } else {
        report.to_text(live)
    }
}

/// Create and tighten the state directory.
pub fn state_dir() -> Result<PathBuf, String> {
    let paths = state::StatePaths::resolve()?;
    paths.ensure()?;
    Ok(paths.dir)
}

/// Resolved state paths without creating anything.
pub fn state_paths() -> Result<state::StatePaths, String> {
    state::StatePaths::resolve()
}

/// Tighten permissions on Unix: directory 0700, files 0600. A no-op elsewhere.
pub(crate) fn restrict_path(path: &Path) -> Result<(), String> {
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
pub fn write_state(path: &Path, contents: &str) -> Result<(), String> {
    let tmp = path.with_extension("tmp");
    fs::write(&tmp, contents).map_err(|e| format!("write {}: {e}", tmp.display()))?;
    restrict_path(&tmp)?;
    fs::rename(&tmp, path).map_err(|e| format!("rename {}: {e}", path.display()))?;
    restrict_path(path)?;
    Ok(())
}

/// Pretty-print a JSON value with stable ordering.
pub fn pretty(value: &Value) -> String {
    serde_json::to_string_pretty(value).unwrap_or_else(|_| value.to_string())
}
