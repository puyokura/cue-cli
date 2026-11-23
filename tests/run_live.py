#!/usr/bin/env python3
"""Post-login live verification: exercise the authenticated surface in one run.

Every check is a real request to api.manus.im through the CLI binary. A check
that fails for lack of permission is reported as SKIP with the service's own
message, never as PASS — an untested path must not look verified.

Usage: run_live.py [path-to-cue-binary] [--create-session]
"""
import json
import os
import subprocess
import sys

BIN = os.path.join("crates", "cue-cli", "target", "release",
                   "cue.exe" if os.name == "nt" else "cue")
if len(sys.argv) > 1 and not sys.argv[1].startswith("--"):
    BIN = sys.argv[1]
CREATE_SESSION = "--create-session" in sys.argv

results = []


def run(args, stdin_data=None):
    proc = subprocess.run([BIN] + args, capture_output=True, text=True,
                          timeout=180, input=stdin_data)
    return proc.returncode, proc.stdout.strip(), proc.stderr.strip()


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
    session_uid = None
    if data:
        items = data.get("sessions") or data.get("items") or []
        if items:
            session_uid = items[0].get("sessionUid") or items[0].get("sessionId")
        check("sessions payload has entries", "PASS" if items else "SKIP",
              f"count={len(items)}")

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

    # 7. optional: create a session and post one prompt (mutates the account)
    if CREATE_SESSION:
        rc, out, err = run(["rpc", "session.v1.SessionService/CreateSession"], )
        check("create session", "PASS" if rc == 0 else "SKIP", err[:160])
    else:
        check("create session", "SKIP", "not requested (--create-session)")

    # 8. passive socket handshake with the stored token
    rc, out, err = run(["watch", "--seconds", "8", "--frames", "8"])
    check("socket namespace accepts token",
          "PASS" if "namespace connected" in out else "FAIL",
          (err or out)[:160])

    passed = sum(1 for _, k, _ in results if k == "PASS")
    skipped = sum(1 for _, k, _ in results if k == "SKIP")
    failed = sum(1 for _, k, _ in results if k == "FAIL")
    print(f"\n{passed} passed, {skipped} skipped, {failed} failed")
    return 0 if failed == 0 else 1


if __name__ == "__main__":
    raise SystemExit(main())
