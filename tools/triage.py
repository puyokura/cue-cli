#!/usr/bin/env python3
"""Anchored triage over the offset-indexed unique string table.

Usage: triage.py <strings_off.tsv> <outdir>
Writes several inventory files under outdir plus a summary to stdout.
Exact, anchored regexes only; no substring guessing.
"""
import os
import re
import sys
from collections import Counter, defaultdict


def load(path):
    rows = []
    with open(path, "r", encoding="utf-8", errors="replace") as fh:
        for line in fh:
            line = line.rstrip("\n")
            if "\t" not in line:
                continue
            off, s = line.split("\t", 1)
            rows.append((int(off), s))
    return rows


def dump(outdir, name, items):
    path = os.path.join(outdir, name)
    with open(path, "w", encoding="utf-8", newline="\n") as fh:
        for it in items:
            fh.write(str(it) + "\n")
    print(f"{name}: {len(items)}", file=sys.stderr)
    return path


def main():
    if len(sys.argv) != 3:
        print("usage: triage.py <strings_off.tsv> <outdir>", file=sys.stderr)
        return 2
    rows = load(sys.argv[1])
    outdir = sys.argv[2]
    os.makedirs(outdir, exist_ok=True)
    seen = {}
    for off, s in rows:
        seen.setdefault(s, off)

    # 1. Connect RPC paths: /<pkg>.<Service>/<Method>
    rpc_re = re.compile(r"^/[a-z][a-z0-9_]*\.v[0-9][a-z0-9_]*\.[A-Za-z][A-Za-z0-9_]+/[A-Za-z][A-Za-z0-9_]+$")
    rpc = sorted(s for s in seen if rpc_re.match(s))

    # 2. REST endpoints under /api/
    rest_re = re.compile(r"^/api/[A-Za-z0-9_./\-]+$")
    rest = sorted(s for s in seen if rest_re.match(s))

    # 3. Full URLs and websocket URLs
    url_re = re.compile(r"(?:https|wss|ws):/?/?[A-Za-z0-9._\-]+(?:[^\s\"'<>]*)?")
    urls = sorted({m.group(0) for s in seen for m in url_re.finditer(s) if len(m.group(0)) > 8})
    hosts = Counter()
    for u in urls:
        m = re.match(r"(?:https|wss|ws):/?/?([A-Za-z0-9._\-]+)", u)
        if m:
            hosts[m.group(1)] += 1

    # 4. HTTP header names (exact shapes only)
    hdr_re = re.compile(r"^(?:x|X)-[A-Za-z0-9\-]+$|^(?:authorization|Authorization|AUTHORIZATION|cookie|Cookie|COOKIE|content-type|accept|origin|"
                        r"user-agent|connect-protocol-version|connect-accept-encoding|connect-content-encoding|"
                        r"grpc-status|grpc-message|grpc-encoding|grpc-timeout|te|host|referer|sec-websocket-[A-Za-z\-]+|"
                        r"sec-ch-ua[A-Za-z0-9\-]*|x-requested-with)$")
    headers = sorted(s for s in seen if hdr_re.match(s))

    # 5. Connect/grpc protocol literals
    proto_re = re.compile(r"^(application/(?:grpc|grpc\+proto|grpc\+json|proto|json|connect\+json|x-protobuf|octet-stream)|"
                          r"connect[-_.][A-Za-z0-9]+|grpc[-_.][A-Za-z0-9]+|ConnectException|ConnectRpc[A-Za-z]*)$",
                          re.IGNORECASE)
    proto = sorted(s for s in seen if proto_re.match(s))

    # 6. Auth/credential field names
    auth_re = re.compile(r"^(auth_token|access_token|refresh_token|id_token|token_type|expires_in|expires_at|accessToken|refreshToken|authToken|"
                        r"accessTokenExpiresAt|refreshTokenExpiresAt|auth_token_expires_at|cookie|set-cookie|Set-Cookie|x-xsrf-token|X-CSRF-TOKEN)$",
                        re.IGNORECASE)
    auth = sorted(s for s in seen if auth_re.match(s))

    dump(outdir, "rpc_paths.txt", rpc)
    dump(outdir, "rest_endpoints.txt", rest)
    dump(outdir, "urls.txt", urls)
    dump(outdir, "hosts.txt", [f"{h}\t{c}" for h, c in hosts.most_common()])
    dump(outdir, "headers.txt", headers)
    dump(outdir, "protocol_literals.txt", proto)
    dump(outdir, "auth_literals.txt", auth)

    # 7. Storage / preference keys (lower_snake or camel lists used as map keys)
    key_re = re.compile(r"^(?:[a-z]+[A-Z][A-Za-z0-9]{2,40}|[a-z][a-z0-9_]{3,40})$")

    summary = {
        "rpc": len(rpc),
        "rest": len(rest),
        "urls": len(urls),
        "hosts": len(hosts),
        "headers": len(headers),
        "proto": len(proto),
        "auth": len(auth),
    }
    print("summary:", summary)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
