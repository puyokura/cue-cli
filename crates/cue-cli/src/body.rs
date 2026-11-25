// SPDX-License-Identifier: AGPL-3.0-or-later
//! Build the request body for `rpc` / `raw` from the caller's arguments.
//!
//! Three routes, in the order a caller should prefer them:
//!
//! 1. `--set key=value` — no quote characters, so no shell can mangle it.
//! 2. `--json-file PATH` — exact UTF-8 bytes from a file.
//! 3. `--json JSON` / `--json -` — inline, or read UTF-8 JSON from stdin.
//!
//! `--set` and `--json-file` exist because Windows PowerShell 5.1 strips
//! double quotes even inside single quotes before a native process sees the
//! argument:
//!
//! ```text
//! '{"sessionUid":"abc"}'  ->  {sessionUid:abc}   (quotes gone, JSON invalid)
//! ```
//!
//! and it defaults `$OutputEncoding` to us-ascii, which rewrites non-ASCII to
//! `?` on the way into a pipe.

use crate::cli::parse_scalar;
use serde_json::Value;

/// Read a JSON body from `--set`, `--json`, or `--json-file`.
///
/// Returns `Ok(None)` when the caller supplied none, which the callers treat as
/// an empty object.
pub fn json_argument(args: &crate::cli::Args) -> Result<Option<Value>, String> {
    let sets = args.flag_all("set");
    if !sets.is_empty() {
        if args.flag("json").is_some() || args.flag("json-file").is_some() {
            return Err("use either --set or --json/--json-file, not both".into());
        }
        let mut map = serde_json::Map::new();
        for entry in sets {
            let (key, raw) = entry
                .split_once('=')
                .ok_or_else(|| format!("--set expects key=value, got `{entry}`"))?;
            let key = key.trim();
            if key.is_empty() {
                return Err(format!("--set needs a key before `=`: `{entry}`"));
            }
            map.insert(key.to_string(), parse_scalar(raw.trim()));
        }
        return Ok(Some(Value::Object(map)));
    }

    if let Some(inline) = args.flag("json") {
        if inline == "-" {
            use std::io::Read;
            let mut buffer = String::new();
            std::io::stdin()
                .read_to_string(&mut buffer)
                .map_err(|e| format!("read stdin: {e}"))?;
            if buffer.trim().is_empty() {
                return Ok(None);
            }
            return serde_json::from_str(&buffer)
                .map(Some)
                .map_err(|e| format!("stdin is not valid JSON: {e}"));
        }
        return serde_json::from_str(inline).map(Some).map_err(|e| {
            format!(
                "--json is not valid JSON: {e}\n\
                 Windows PowerShell strips quotes; use --set key=value or --json-file instead"
            )
        });
    }
    if let Some(path) = args.flag("json-file") {
        let raw = std::fs::read_to_string(path)
            .map_err(|e| format!("cannot read {path}: {e}"))?;
        return serde_json::from_str(&raw)
            .map(Some)
            .map_err(|e| format!("{path} does not contain valid JSON: {e}"));
    }
    Ok(None)
}

/// Refuse a flag the chosen command does not read, and refuse a flag that needs
/// a value but was given none.
///
/// Without the first check, `cue sessions --limit n` and `cue sessions --limt n`
/// behave identically, so a typo silently returns data the caller did not ask
/// for. Without the second, `cue sessions --limit` looks accepted and quietly
/// falls back to the default page size — the same failure the first check exists
/// to prevent, wearing a different hat.
pub fn reject_unknown_flags(
    command: &str,
    args: &crate::cli::Args,
    global: &[&str],
    known: &[&str],
    booleans: &[&str],
) -> Result<(), String> {
    for name in args.supplied() {
        if global.contains(&name.as_str()) || known.contains(&name.as_str()) {
            continue;
        }
        let mut all: Vec<&str> = global.to_vec();
        all.extend_from_slice(known);
        all.sort();
        let rendered: Vec<String> = all.iter().map(|f| format!("--{f}")).collect();
        return Err(format!(
            "unknown flag `--{name}` for `cue {command}`; known flags: {}",
            rendered.join(", ")
        ));
    }
    // `--json` is a switch for `doctor`/`status` (output mode) but a valued flag
    // for `rpc` (request body), so whether a missing value is an error depends
    // on the command, not on the name alone.
    for name in args.without_value() {
        let known_here = global.contains(&name.as_str()) || known.contains(&name.as_str());
        if known_here && !booleans.contains(&name.as_str()) && !crate::cli::is_boolean_switch(&name) {
            return Err(format!(
                "flag `--{name}` requires a value for `cue {command}`"
            ));
        }
    }
    Ok(())
}

/// Flags that are switches for a particular command even though the same name
/// takes a value elsewhere.
pub fn command_booleans(command: &str) -> &'static [&'static str] {
    match command {
        "doctor" | "status" => &["json"],
        _ => &[],
    }
}

/// Whether this command can change state, and therefore needs `--confirm`.
///
/// The default is inverted on purpose: a method is treated as state-changing
/// unless its leading verb is on the read list. The other way round needs a
/// code change every time the service adds a verb, and a miss ships silently as
/// "the gate did not catch it". With this shape, a new `DeleteFoo` is gated
/// without anyone remembering to add it.
pub fn is_destructive_command(command: &str, args: &crate::cli::Args) -> bool {
    match command {
        // Publishing to the internet is not undoable from here.
        "webdev-deploy" => true,
        "mfa" => {
            // Both directions change the account's login surface, so both ask.
            // `codes` rotates recovery codes, which invalidates printed copies.
            let sub = args.positional(0).unwrap_or("");
            matches!(
                sub,
                "disable" | "enable" | "passkey-delete" | "passkey-start"
                    | "passkey-finish" | "totp-start" | "totp-finish" | "codes"
            )
        },
        "phone" => {
            // `reset` sets a new account password. `start`/`verify`/`password`
            // are in the login flow, where confirmation is redundant.
            args.positional(0).unwrap_or("") == "reset"
        }
        "rpc" => state_changing_method(args.positional(0).unwrap_or("")),
        "raw" => state_changing_method(args.positional(0).unwrap_or("")),
        _ => false,
    }
}

/// Leaders whose RPC only reads.
///
/// Anything absent from this list is treated as a state change. That is the
/// safe direction to be wrong in: a needless `--confirm` costs one keystroke, a
/// missing one costs the account.
const READ_LEADERS: &[&str] = &[
    "get",
    "list",
    "search",
    "page",
    "check",
    "query",
    "view",
    "read",
    // `UserInfo` splits into `User`, and it is the only method with that leader.
    "user",
    "autocompleteaddress",
    // Authorize starts a consent flow and returns a URL; it does not alter the
    // account until the user completes the consent.
    "oauth2authorize",
];

/// Whether this RPC method changes remote state.
pub fn state_changing_method(name: &str) -> bool {
    let leader = leading_segment(name).to_ascii_lowercase();
    !READ_LEADERS.iter().any(|read| leader == *read)
}

/// The first camel-case segment of a method or path name.
///
/// `session.v1.SessionService/DeletePasskey` → `Delete`;
/// `PageGetUserSpaces` → `Page`; `delete` → `delete`.
fn leading_segment(name: &str) -> String {
    // Use the part after the last `/`: the service name carries no verb.
    let tail = name.rsplit('/').next().unwrap_or(name);
    let mut out = String::new();
    for (index, ch) in tail.chars().enumerate() {
        if index > 0 && ch.is_ascii_uppercase() {
            break;
        }
        if ch == '_' || ch == '-' {
            break;
        }
        out.push(ch);
        let _ = index;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::Args;

    fn args_of(raw: &[&str]) -> Args {
        Args::parse(&raw.iter().map(|s| s.to_string()).collect::<Vec<_>>())
    }

    #[test]
    fn set_builds_an_object_without_quoting() {
        let a = args_of(&["rpc", "m", "--set", "sessionUid=abc", "--set", "limit=5"]);
        let body = json_argument(&a).unwrap().unwrap();
        assert_eq!(body["sessionUid"], "abc");
        assert_eq!(body["limit"], 5);
    }

    #[test]
    fn set_and_json_together_are_rejected() {
        let a = args_of(&["rpc", "m", "--set", "a=1", "--json", "{}"]);
        assert!(json_argument(&a).is_err());
    }

    #[test]
    fn json_file_is_read() {
        let dir = std::env::temp_dir().join("cue-body-test");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("body.json");
        std::fs::write(&path, r#"{"a":1}"#).unwrap();
        let a = args_of(&["rpc", "m", "--json-file", path.to_str().unwrap()]);
        let body = json_argument(&a).unwrap().unwrap();
        assert_eq!(body["a"], 1);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn missing_json_file_is_an_error_with_the_path() {
        let a = args_of(&["rpc", "m", "--json-file", "/definitely/not/here.json"]);
        let err = json_argument(&a).unwrap_err();
        assert!(err.contains("/definitely/not/here.json"), "{err}");
    }

    #[test]
    fn unknown_flags_list_the_known_set() {
        let a = args_of(&["sessions", "--limt", "5"]);
        let err = reject_unknown_flags("sessions", &a, &["state"], &["limit"], &[]).unwrap_err();
        assert!(err.contains("--limt"));
        assert!(err.contains("--limit"));
    }

    #[test]
    fn global_and_known_flags_pass() {
        let a = args_of(&["doctor", "--live", "--json", "--state", "d"]);
        assert!(reject_unknown_flags("doctor", &a, &["state"], &["live", "json"], &["json"]).is_ok());
    }

    #[test]
    fn deploy_and_destructive_mfa_need_confirmation() {
        // The command name is split off before parsing, so positionals start at
        // the subcommand — the tests must mirror that or they test a shape the
        // binary never sees.
        assert!(is_destructive_command("webdev-deploy", &args_of(&[])));
        assert!(is_destructive_command("mfa", &args_of(&["disable"])));
        assert!(is_destructive_command("mfa", &args_of(&["passkey-delete"])));
        assert!(is_destructive_command("mfa", &args_of(&["totp-finish"])));
        assert!(is_destructive_command("mfa", &args_of(&["enable"])));
        // Rotating recovery codes invalidates every printed copy.
        assert!(is_destructive_command("mfa", &args_of(&["codes"])));
        // Resetting the account password is a state change.
        assert!(is_destructive_command("phone", &args_of(&["reset"])));
        assert!(!is_destructive_command("phone", &args_of(&["start"])));
        assert!(!is_destructive_command("phone", &args_of(&["verify"])));
        assert!(!is_destructive_command("mfa", &args_of(&["status"])));
    }

    #[test]
    fn reads_are_not_gated_whoever_named_them() {
        for name in [
            "user.v1.UserService/UserInfo",
            "user.v1.UserService/ListRegionCode",
            "user.v1.UserService/GetGlobalSettings",
            "session.v1.SessionService/SearchAgentSessions",
            "space.v1.SpaceService/PageGetUserSpaces",
            "user.v1.UserPublicService/CheckRegion",
        ] {
            let a = args_of(&[name]);
            assert!(!is_destructive_command("rpc", &a), "{name} should read");
        }
    }

    #[test]
    fn writes_are_gated_even_when_the_verb_is_new() {
        // The list is a *read* allowlist, so any newcomer defaults to gated.
        for name in [
            "cloud_pc.v1.CloudPCService/ResetSSHPassword",
            "cloud_pc.v1.CloudPCService/Restart",
            "session.v1.SessionCollaborateService/AcceptInvite",
            "user.v1.UserAuthService/VerifyEmail",
            "user.v1.UserAuthPublicService/SendCuePhoneCode",
            "user.v1.UserAuthService/RegenerateRecoveryCodes",
            "some.v1.Service/DeleteThing",
            "some.v1.Service/UnheardOfMutation",
        ] {
            let a = args_of(&[name]);
            assert!(is_destructive_command("rpc", &a), "{name} should be gated");
        }
    }

    #[test]
    fn the_body_transport_does_not_change_the_verdict() {
        // A body key that happens to look like a verb used to flip the verdict
        // and, worse, only for `--json`. The name decides now, so all three
        // transports agree.
        let read = "user.v1.UserService/GetGlobalSettings";
        for form in [
            vec![read, "--json", r#"{"updatedAt":1}"#],
            vec![read, "--set", "updatedAt=1"],
            vec![read, "--json", r#"{"cancel":true}"#],
        ] {
            let a = args_of(&form);
            assert!(!is_destructive_command("rpc", &a), "{form:?} should read");
        }
        let write = "user.v1.UserAuthService/DeletePasskey";
        for form in [
            vec![write],
            vec![write, "--set", "id=x"],
            vec![write, "--json", "{}"],
        ] {
            let a = args_of(&form);
            assert!(is_destructive_command("rpc", &a), "{form:?} should be gated");
        }
    }
}




