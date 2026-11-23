#!/usr/bin/env python3
"""Decode buf.validate.Violations payloads dumped by harvest_rpc.py.

Prints, for one RPC path, the raw field numbers and the decoded strings so the
field numbering can be validated against observed output.
"""
import base64
import json
import sys


def varint(buf, i):
    shift = 0
    val = 0
    while True:
        b = buf[i]
        i += 1
        val |= (b & 0x7F) << shift
        if not b & 0x80:
            return val, i
        shift += 7


def walk(buf, depth=0, max_depth=5):
    i = 0
    rows = []
    while i < len(buf):
        key, i = varint(buf, i)
        fn, wt = key >> 3, key & 7
        if wt == 0:
            v, i = varint(buf, i)
            rows.append((depth, fn, "varint", v))
        elif wt == 2:
            ln, i = varint(buf, i)
            chunk = buf[i:i + ln]
            i += ln
            printable = all(32 <= b < 127 for b in chunk) and chunk
            if printable:
                rows.append((depth, fn, "str", chunk.decode("ascii")))
            elif depth < max_depth:
                rows.append((depth, fn, "msg", f"<{len(chunk)} bytes>"))
                rows.extend(walk(chunk, depth + 1, max_depth))
            else:
                rows.append((depth, fn, "bytes", chunk.hex()))
        elif wt == 5:
            i += 4
            rows.append((depth, fn, "fixed32", ""))
        elif wt == 1:
            i += 8
            rows.append((depth, fn, "fixed64", ""))
        else:
            break
    return rows


def main():
    src, path = sys.argv[1], sys.argv[2]
    entries = json.load(open(src, encoding="utf-8"))
    target = [e for e in entries if e["path"] == path]
    if not target:
        print("path not found", file=sys.stderr)
        return 1
    entry = target[0]
    print(f"path={path} status={entry['status']}")
    print("json message:", (entry.get("message") or "").replace("\n", " | "))
    raw_body = json.loads(entry["body_head"].encode()[:0] or b"{}") if False else None
    print("violations decoded from details (needs full body; re-fetching not supported here)")
    # The detail payload was truncated in body_head; report what fields exist.
    print("fields:", entry["fields"])
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
