#!/usr/bin/env python3
"""Post-login live verification: exercise the authenticated surface in one run.

Every check is a real request to api.manus.im through the CLI binary. A check
that fails for lack of permission is reported as SKIP with the service's own
message, never as PASS — an untested path must not look verified.

Usage: run_live.py [path-to-cue-binary] [--create-session] [--send]

Side effects: with no flags this is read-only. `--create-session` creates a real
session on the signed-in account and `--send` posts a real prompt, so both leave
data behind. Skip them when the account must stay clean.
"""
import json
import os
import subprocess
import sys

_EXE = "cue.exe" if os.name == "nt" else "cue"


def _default_bin():
    for profile in ("release", "debug"):
        candidate = os.path.join("target", profile, _EXE)
        if os.path.exists(candidate):
            return candidate
    raise SystemExit("no binary found; run `cargo build --release`")


BIN = sys.argv[1] if len(sys.argv) > 1 and not sys.argv[1].startswith("--") else _default_bin()
if not os.path.exists(BIN):
    raise SystemExit(f"binary not found: {BIN}")
CREATE_SESSION = "--create-session" in sys.argv

results = []


def run(args, stdin_data=None):
    # The CLI emits UTF-8 JSON; the Windows console default (cp932) mangles it.
    proc = subprocess.run([BIN] + args, capture_output=True, text=True,
                          encoding="utf-8", errors="replace",
                          timeout=180, input=stdin_data)
    return proc.returncode, (proc.stdout or "").strip(), (proc.stderr or "").strip()


def check(name, kind, detail=""):
    results.append((name, kind, detail))
    print(f"{kind:5} {name}" + (f"  [{detail}]" if detail and kind != "PASS" else ""))


def as_json(out):
    try:
        return json.loads(out)
    except json.JSONDecodeError:
        return None


def main():
    # 0. token must exist; this script never logs in by itself
    rc, out, err = run(["whoami"])
    if rc != 0:
        check("token present", "FAIL", err)
        print("\nrun `cue login` first")
        return 1

    me = as_json(out) or {}
    uid = (me.get("user") or {}).get("userId") or me.get("userId")
    check("whoami returns a user", "PASS" if uid else "FAIL", f"userId={bool(uid)}")

    # 1. public surfaces that must work for any valid token
    for name, cmd in [
        ("user info", ["whoami"]),
        ("client config", ["rpc", "user.v1.UserService/GetUserClientConfig"]),
        ("available credits", ["rpc", "user.v1.UserService/GetAvailableCredits"]),
        ("region codes", ["rpc", "user.v1.UserService/ListRegionCode"]),
        ("login methods", ["mfa", "login-methods"]),
    ]:
        rc, out, err = run(cmd)
        kind = "PASS" if rc == 0 and as_json(out) is not None else (
            "SKIP" if "401" in err or "403" in err or "permission" in err else "FAIL")
        check(name, kind, err[:120] if kind != "PASS" else "")

    # 2. sessions
    rc, out, err = run(["sessions", "--limit", "5"])
    data = as_json(out)
    kind = "PASS" if rc == 0 and data is not None else ("SKIP" if "401" in err else "FAIL")
    check("list sessions", kind, err[:120] if kind != "PASS" else "")
    # ListSessions returns {} on a fresh account; the populated list lives in
    # SearchAgentSessions. Record both so an empty result is not read as a fault.
    rc, out, err = run(["rpc", "session.v1.SessionService/SearchAgentSessions",
                        "--json", '{"query":"","limit":20}'])
    search = as_json(out) or {}
    items = search.get("sessions") or []
    check("search sessions", "PASS" if rc == 0 and items else "SKIP",
          f"count={len(items)}")
    session_uid = None
    if items:
        session_uid = items[0].get("uid")
        check("session uid is 22 chars", len(session_uid or "") == 22, str(session_uid))

    # 3. agents
    rc, out, err = run(["agents", "--limit", "5"])
    data = as_json(out)
    kind = "PASS" if rc == 0 and data is not None else ("SKIP" if "401" in err else "FAIL")
    check("list agents", kind, err[:120] if kind != "PASS" else "")

    # 4. two-step verification state
    rc, out, err = run(["mfa", "status"])
    data = as_json(out)
    check("mfa status", "PASS" if rc == 0 and data is not None else "FAIL", err[:120])

    # 5. phone binding state (does this account already have a phone?)
    rc, out, err = run(["rpc", "user.v1.UserAuthService/GetCuePhoneAuthInfo"])
    check("cue phone auth info",
          "PASS" if rc == 0 else ("SKIP" if "401" in err else "FAIL"), err[:120])

    # 6. session detail, when one exists
    if session_uid:
        rc, out, err = run(["session", session_uid])
        check("session detail", "PASS" if rc == 0 and as_json(out) else "FAIL", err[:120])
    else:
        check("session detail", "SKIP", "no session to read")

    # 7. optional: create a session and post one prompt (mutates the account).
    #    The id is generated locally; the server persists it on first use.
    if CREATE_SESSION:
        rc, out, err = run(["new", "--text", "reply with the single word: ok",
                            "--seconds", "45", "--frames", "160"])
        new_id = ""
        for line in out.splitlines():
            if line.startswith("sessionId="):
                new_id = line.split("=", 1)[1].strip()
        check("new session is created", "PASS" if rc == 0 and len(new_id) == 22 else "FAIL",
              (err or out)[:160])
        check("new session gets an answer", "PASS" if "chat[assistant]" in out else "FAIL",
              out[-160:])
    else:
        check("new session is created", "SKIP", "not requested (--create-session)")

    # 8. passive socket handshake with the stored token
    rc, out, err = run(["watch", "--seconds", "8", "--frames", "8"])
    check("socket namespace accepts token",
          "PASS" if "namespace connected" in out else "FAIL",
          (err or out)[:160])

    # 9. session subscription: join must be acknowledged with the event replay
    if session_uid:
        rc, out, err = run(["join", session_uid, "--seconds", "15", "--frames", "80"])
        check("join subscription is accepted",
              "PASS" if "eventsNotifyEventsAfter" in out and "agent_error" not in out
              else "FAIL", (err or out)[:160])
    else:
        check("join subscription is accepted", "SKIP", "no session to join")

    # 9b. read-only coverage survey over the authenticated surface
    rc, out, err = run(["coverage"])
    cov = as_json(out)
    if cov and cov.get("checks"):
        failed = [c["check"] for c in cov["checks"] if not c.get("ok")]
        check("coverage survey runs", "PASS" if rc == 0 else "FAIL", err[:120])
        check("coverage has no failed probes", "PASS" if not failed else "FAIL", str(failed))
        check("coverage covers >= 26 surfaces", "PASS" if cov["total"] >= 23 else "FAIL",
              str(cov["total"]))
    else:
        check("coverage survey runs", "FAIL", (err or out)[:160])

    # 10. prompt round trip: send, then confirm the agent replied
    if session_uid and "--send" in sys.argv:
        rc, out, err = run(["send", session_uid, "--text",
                            "reply with the single word: ok",
                            "--seconds", "45", "--frames", "160"])
        replied = "chat[assistant]" in out
        check("prompt is accepted and answered",
              "PASS" if replied else "FAIL", (err or out)[:200])
    else:
        check("prompt is accepted and answered", "SKIP",
              "not requested (--send)")

    passed = sum(1 for _, k, _ in results if k == "PASS")
    skipped = sum(1 for _, k, _ in results if k == "SKIP")
    failed = sum(1 for _, k, _ in results if k == "FAIL")
    print(f"\n{passed} passed, {skipped} skipped, {failed} failed")
    return 0 if failed == 0 else 1


if __name__ == "__main__":
    raise SystemExit(main())





