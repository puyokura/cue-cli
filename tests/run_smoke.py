#!/usr/bin/env python3
"""Live smoke test for the cue CLI.

Runs the built binary as a separate process against the real service and
asserts on observed output. No mocks: every expectation is a fact the service
actually returns.

Usage: run_smoke.py [path-to-cue-binary]
"""
import json
import os
import subprocess
import sys
import tempfile

_EXE = "cue.exe" if os.name == "nt" else "cue"


def _default_bin():
    """Prefer the debug build for fast iteration, then the release build."""
    for profile in ("debug", "release"):
        candidate = os.path.join("target", profile, _EXE)
        if os.path.exists(candidate):
            return candidate
    raise SystemExit("no binary found; run `cargo build` or pass the path explicitly")


BIN = sys.argv[1] if len(sys.argv) > 1 else _default_bin()
if not os.path.exists(BIN):
    raise SystemExit(f"binary not found: {BIN}")

results = []


def run(args, expect_ok=True, env_extra=None, stdin_data=None):
    env = dict(os.environ)
    if env_extra:
        env.update(env_extra)
    proc = subprocess.run(
        [BIN] + args,
        capture_output=True,
        text=True,
        timeout=120,
        env=env,
        input=stdin_data,
    )
    out = proc.stdout.strip()
    err = proc.stderr.strip()
    return proc.returncode, out, err


def check(name, cond, detail=""):
    results.append((name, bool(cond), detail))
    print(("PASS " if cond else "FAIL ") + name + (("  " + detail) if detail and not cond else ""))


def main():
    tmp = tempfile.mkdtemp(prefix="cue-smoke-")
    env = {}
    if os.name == "nt":
        env["APPDATA"] = tmp
    else:
        env["XDG_CONFIG_HOME"] = tmp

    # 1. region: public RPC, no auth
    rc, out, err = run(["region"], env_extra=env)
    check("region exits 0", rc == 0, err)
    try:
        region = json.loads(out)
        check("region has iso", "iso" in region, out[:200])
        check("region iso is 2 letters", len(region.get("iso", "")) == 2, str(region.get("iso")))
        check("region has dial code", region.get("code", "").startswith("+"), str(region.get("code")))
    except json.JSONDecodeError:
        check("region output is JSON", False, out[:200])

    # 2. settings: public RPC, no auth
    rc, out, err = run(["settings"], env_extra=env)
    check("settings exits 0", rc == 0, err)
    try:
        settings = json.loads(out)
        check("settings non-empty", len(settings) > 0, out[:200])
    except json.JSONDecodeError:
        check("settings output is JSON", False, out[:200])

    # 3. generic rpc passthrough on a public method
    rc, out, err = run(["rpc", "user.v1.UserPublicService/GetExchangeInfo"], env_extra=env)
    check("rpc passthrough exits 0", rc == 0, err)
    try:
        exch = json.loads(out)
        check("exchange has subscriptions", isinstance(exch.get("subscriptions"), list), out[:200])
    except json.JSONDecodeError:
        check("rpc output is JSON", False, out[:200])

    # 4. proto codec: empty body must decode to non-empty bytes
    rc, out, err = run(["raw", "user.v1.UserPublicService/CheckRegion"], env_extra=env)
    check("raw proto exits 0", rc == 0, err)
    check("raw proto returns bytes", "bytes as hex:" in out and len(out.splitlines()) > 1, out[:200])

    # 5. auth-required call without a token fails cleanly and names the reason
    rc, out, err = run(["whoami"], env_extra=env)
    check("whoami without token exits non-zero", rc != 0, out)
    check("whoami reports missing token", "no token" in err, err)

    # 6. bogus token must not be persisted
    rc, out, err = run(["login", "--token", "bogus-smoke"], env_extra=env)
    check("login with bogus token exits non-zero", rc != 0, out)
    check("login reports rejection", "rejected" in err or "401" in err, err)
    tok = os.path.join(tmp, "cue-cli", "token")
    check("bogus token not stored", not os.path.exists(tok), tok)

    # 7. unknown command exits 2 with usage
    rc, out, err = run(["nonsense"], env_extra=env)
    check("unknown command exits 2", rc == 2, f"rc={rc}")

    # 8. methods reports the static inventory count
    rc, out, err = run(["methods"], env_extra=env)
    check("methods exits 0", rc == 0, err)
    try:
        meta = json.loads(out)
        check("inventory has 252 methods", meta.get("methods") == 252, str(meta))
    except json.JSONDecodeError:
        check("methods output is JSON", False, out[:200])

    # 9. watch performs a real Socket.IO handshake and reports the namespace
    #    rejection, which is what an unauthenticated socket is supposed to see.
    rc, out, err = run(["watch", "--seconds", "6", "--frames", "6"], env_extra=env)
    check("watch exits 0", rc == 0, err)
    check("watch got a 101 handshake", "handshake 101" in out, out[:200])
    check("watch saw the engine.io open packet", "engine.io open" in out, out[:200])
    check("watch saw the namespace rejection", "namespace rejected" in out, out[:200])
    check("watch never echoed a token", '{"token":}' in out or "token" not in out, out[:200])

    # 10. send refuses to post without a token instead of silently failing
    rc, out, err = run(["send", "sess-x", "--text", "hello"], env_extra=env)
    check("send without token exits non-zero", rc != 0, out)
    check("send explains the missing token", "no token" in err, err)

    # 11. the Cue product header must open the phone surface.
    #     With it, a malformed number reaches business validation (400230).
    #     Without it, the same call is refused as 400232. The CLI always sends
    #     the header, so this asserts the CLI's behaviour, not the server's.
    rc, out, err = run(
        ["rpc", "user.v1.UserAuthPublicService/SendCuePhoneCode",
         "--json", '{"phoneNumber":"+","purpose":1}'],
        env_extra=env,
    )
    check("phone code call reaches validation", rc != 0, out)
    check("phone gate is open for the CLI", "400230" in err, err)
    check("phone gate is not closed (no 400232)", "400232" not in err, err)

    # 12. phone subcommands refuse to run without their required flags
    rc, out, err = run(["phone", "start"], env_extra=env)
    check("phone start without --number fails", rc != 0, out)
    check("phone start names the missing flag", "--number" in err, err)

    rc, out, err = run(["phone", "verify"], env_extra=env)
    check("phone verify without --id fails", rc != 0, out)

    rc, out, err = run(["phone", "help"], env_extra=env)
    check("phone help exits 0", rc == 0, err)
    check("phone help lists the four subcommands", out.count("cue phone") == 4, out[:300])

    # 13. the discovery oracle must separate existing from missing routes
    rc, out, err = run(
        ["discover", "user.v1.UserAuthService", "GetMfaStatus", "CreateTotp"],
        env_extra=env,
    )
    check("discover exits 0", rc == 0, err)
    try:
        probes = {p["method"]: p for p in json.loads(out)["probes"]}
        check("discover finds GetMfaStatus", probes["GetMfaStatus"]["exists"] is True, out[:200])
        check("discover rejects CreateTotp", probes["CreateTotp"]["exists"] is False, out[:200])
    except (json.JSONDecodeError, KeyError):
        check("discover output is JSON with probes", False, out[:200])

    # 14. mfa help is token-free; the rest of mfa requires a token
    rc, out, err = run(["mfa", "help"], env_extra=env)
    check("mfa help exits 0 without a token", rc == 0, err)
    subs = ["status", "totp-start", "totp-finish", "enable", "disable", "codes",
            "login-methods", "passkeys"]
    missing = [s for s in subs if f"cue mfa {s}" not in out]
    check("mfa help lists every subcommand", not missing, f"missing={missing}")

    rc, out, err = run(["mfa", "status"], env_extra=env)
    check("mfa status without token fails", rc != 0, out)
    check("mfa status explains the missing token", "no token" in err, err)

    # 15a. server errors must carry the service's own reason, not only Error<NNNN>.
    #      Uses a token-free call so the check is independent of a stored login.
    rc, out, err = run(
        ["rpc", "user.v1.UserAuthPublicService/Oauth2Authorize", "--json", '{"type":5}'],
        env_extra=env,
    )
    check("server error surfaces its message", "Facebook login has been discontinued" in err,
          err[:160])
    check("server error keeps the service code", "400228" in err, err[:160])

    # 15e. MCP over stdio: initialize, tools/list, tools/call, unknown tool
    mcp_input = "\n".join([
        '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}',
        '{"jsonrpc":"2.0","id":2,"method":"tools/list","params":{}}',
        '{"jsonrpc":"2.0","id":3,"method":"tools/call",'
        '"params":{"name":"cue_nope","arguments":{}}}',
    ]) + "\n"
    rc, out, err = run(["mcp"], env_extra=env, stdin_data=mcp_input)
    replies = {}
    for line in out.splitlines():
        line = line.strip()
        if not line:
            continue
        try:
            msg = json.loads(line)
        except json.JSONDecodeError:
            continue
        replies[msg.get("id")] = msg
    check("mcp answers initialize", 1 in replies and
          replies[1].get("result", {}).get("serverInfo", {}).get("name") == "cue",
          str(replies.get(1))[:160])
    check("mcp declares a protocol version", 1 in replies and
          "protocolVersion" in replies[1].get("result", {}), "")
    tool_list = replies.get(2, {}).get("result", {}).get("tools", [])
    check("mcp tools/list returns tools", len(tool_list) >= 10, f"count={len(tool_list)}")
    check("mcp tools/call rejects an unknown tool",
          3 in replies and replies[3].get("result", {}).get("isError") is True,
          str(replies.get(3))[:160])

    # 15i. `webdev-deploy` requires both identifiers before touching the network
    rc, out, err = run(["webdev-deploy"], env_extra=env)
    check("webdev-deploy needs --project", rc != 0 and "--project" in err, err[:120])
    rc, out, err = run(["webdev-deploy", "--project", "x"], env_extra=env)
    check("webdev-deploy needs --version", rc != 0 and "--version" in err, err[:120])

    # 15h. `webdev` rejects a non-numeric --limit locally
    rc, out, err = run(["webdev", "--limit", "abc"], env_extra=env)
    check("webdev with a bad limit fails", rc != 0, out)

    # 15g. `sandbox` requires a session id and fails cleanly without one
    rc, out, err = run(["sandbox"], env_extra=env, )
    check("sandbox without id fails", rc != 0, out)
    check("sandbox names the missing id", "<session_id>" in err, err[:120])

    # 15f. `new` validates its arguments before touching the network
    rc, out, err = run(["new"], env_extra=env)
    check("new without --text fails", rc != 0, out)
    check("new names the missing prompt", "--text" in err, err[:120])

    # 16. a stored token must never appear in any command output.
    #     Write a dummy token, then exercise a command's error path and inspect output.
    state = tempfile.mkdtemp(prefix="cue-leak-")
    token_path = os.path.join(state, "token")
    sentinel = "LEAKCANARY0123456789"
    with open(token_path, "w", encoding="utf-8") as fh:
        fh.write(sentinel)
    for cmd in (["whoami"], ["rpc", "user.v1.UserService/ListRegionCode"], ["methods"]):
        rc, out, err = run(["--state", state] + cmd, env_extra=env)
        check(f"no token leak: {cmd[0]}", sentinel not in out and sentinel not in err,
              (out + err)[:120])

    # 15c. unknown flags must be rejected, and version must work
    rc, out, err = run(["whoami", "--bogus"], env_extra=env)
    check("unknown flag is rejected", rc == 2, f"rc={rc}")
    check("unknown flag names the flag", "--bogus" in err, err[:120])
    rc, out, err = run(["version"], env_extra=env)
    check("version prints the crate version", rc == 0 and out.startswith("cue "), out[:80])
    rc, out, err = run(["--version"], env_extra=env)
    check("--version works before the command", rc == 0 and out.startswith("cue "), out[:80])

    # 15d. --state must redirect state without touching the default directory
    tmp_state = os.path.join(tempfile.mkdtemp(prefix="cue-state-"), "s")
    rc, out, err = run(["--state", tmp_state, "whoami"], env_extra=env)
    check("--state redirects state", rc != 0 and "no token" in err, err[:120])

    # 15b. explicit help is a success, misuse is an error
    for flag in (["help"], ["--help"], ["-h"]):
        rc, out, err = run(flag, env_extra=env)
        check(f"`{flag[0]}` exits 0", rc == 0, f"rc={rc}")

    # 15. interactive `login` must not hang or send an SMS when given no input.
    #     It reads the phone number from stdin; EOF has to fail fast instead.
    rc, out, err = run(["login"], env_extra=env, stdin_data="")
    check("interactive login fails without input", rc != 0, out)
    check("interactive login names the missing number", "phone number" in err, err)
    check("interactive login never claims an SMS was sent", "SMS sent" not in err, err)

    passed = sum(1 for _, ok, _ in results if ok)
    total = len(results)
    print(f"\n{passed}/{total} checks passed")
    return 0 if passed == total else 1


if __name__ == "__main__":
    raise SystemExit(main())











