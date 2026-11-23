#!/usr/bin/env python3
"""Enumerate RPC methods inside a known service using the 404/401 oracle.

A path that exists but requires auth answers 401 (Connect `unauthenticated`).
A path that does not exist answers a plain-HTTP 404 with no Connect body.
That difference lets the static inventory be extended without authentication.

Usage: enumerate_methods.py <service> <candidate-list-file> [--out file]
"""
import json
import ssl
import sys
import time
import urllib.error
import urllib.request

BASE = "https://api.manus.im"
HEADERS = {
    "content-type": "application/json",
    "accept": "application/json",
    "connect-protocol-version": "1",
    "x-product-name": "manus-agents",
    "user-agent": "cue-re/0.1 (enumeration)",
}


def exists(service: str, method: str):
    url = f"{BASE}/{service}/{method}"
    req = urllib.request.Request(url, data=b"{}", method="POST", headers=HEADERS)
    try:
        with urllib.request.urlopen(req, timeout=20, context=ssl.create_default_context()) as resp:
            return True, resp.status, resp.read(300).decode("utf-8", "replace")
    except urllib.error.HTTPError as err:
        body = err.read(300).decode("utf-8", "replace")
        # A missing route returns plain-text 404 without a Connect body.
        if err.code == 404 and '"code"' not in body:
            return False, err.code, body
        return True, err.code, body
    except Exception as exc:  # noqa: BLE001 - transport detail is the finding
        return None, -1, str(exc)


def main():
    if len(sys.argv) < 3:
        print("usage: enumerate_methods.py <service> <candidates-file> [--out file]", file=sys.stderr)
        return 2
    service = sys.argv[1]
    cand_file = sys.argv[2]
    out_path = None
    if "--out" in sys.argv:
        out_path = sys.argv[sys.argv.index("--out") + 1]

    candidates = [
        ln.strip()
        for ln in open(cand_file, encoding="utf-8")
        if ln.strip() and not ln.startswith("#")
    ]

    found = []
    unknown = []
    for method in candidates:
        ok, status, body = exists(service, method)
        if ok:
            found.append((method, status, body[:120].replace("\n", " ")))
        elif ok is None:
            unknown.append((method, body[:120]))
        print(f"{'EXISTS' if ok else 'absent'}  {service}/{method} -> {status}", file=sys.stderr)
        time.sleep(0.15)

    result = {
        "service": service,
        "candidates": len(candidates),
        "exists": [{"method": m, "status": s, "body_head": b} for m, s, b in found],
        "unknown": [{"method": m, "error": e} for m, e in unknown],
    }
    text = json.dumps(result, ensure_ascii=False, indent=1)
    if out_path:
        with open(out_path, "w", encoding="utf-8", newline="\n") as fh:
            fh.write(text + "\n")
    print(text)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
