#!/usr/bin/env python3
"""Probe web-client method names against one Connect service.

The web bundle calls methods in camelCase (`getUserClientConfig`); the wire
path uses PascalCase (`GetUserClientConfig`). This converts, probes, and
reports which routes exist using the 404/401 oracle.

Usage: probe_web_methods.py <service> <method-list> [--out file] [--limit N]
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
    "user-agent": "cue-re/0.1 (web-method probe)",
}


def pascal(name: str) -> str:
    return name[:1].upper() + name[1:]


def probe(service: str, method: str):
    url = f"{BASE}/{service}/{method}"
    req = urllib.request.Request(url, data=b"{}", method="POST", headers=HEADERS)
    try:
        with urllib.request.urlopen(req, timeout=20, context=ssl.create_default_context()) as r:
            return True, r.status, r.read(240).decode("utf-8", "replace")
    except urllib.error.HTTPError as err:
        body = err.read(240).decode("utf-8", "replace")
        if err.code == 404 and '"code"' not in body:
            return False, err.code, body
        return True, err.code, body
    except Exception as exc:  # noqa: BLE001
        return None, -1, str(exc)


def main():
    if len(sys.argv) < 3:
        print("usage: probe_web_methods.py <service> <methods> [--out f] [--limit n]", file=sys.stderr)
        return 2
    service = sys.argv[1]
    names = [ln.strip() for ln in open(sys.argv[2], encoding="utf-8") if ln.strip()]
    out_path = None
    limit = len(names)
    if "--out" in sys.argv:
        out_path = sys.argv[sys.argv.index("--out") + 1]
    if "--limit" in sys.argv:
        limit = int(sys.argv[sys.argv.index("--limit") + 1])

    found, unknown = [], []
    for name in names[:limit]:
        method = pascal(name)
        ok, status, body = probe(service, method)
        if ok:
            found.append({"web_name": name, "method": method, "status": status,
                          "body_head": body.replace("\n", " ")[:140]})
        elif ok is None:
            unknown.append({"web_name": name, "error": body[:120]})
        time.sleep(0.12)

    result = {"service": service, "probed": min(limit, len(names)),
              "exists": found, "unknown": unknown}
    text = json.dumps(result, ensure_ascii=False, indent=1)
    if out_path:
        with open(out_path, "w", encoding="utf-8", newline="\n") as fh:
            fh.write(text + "\n")
    print(f"probed={result['probed']} exists={len(found)} unknown={len(unknown)}", file=sys.stderr)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
