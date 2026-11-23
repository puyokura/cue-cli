#!/usr/bin/env python3
"""Harvest Connect/gRPC-JSON schema evidence from api.manus.im.

Sends an intentionally minimal body to each RPC path discovered statically and
records the HTTP status plus any validation-error details. The server's own
buf.validate violations name exact proto fields, so this recovers field
inventory without authentication or guessing.

Usage: harvest_rpc.py <rpc_paths.txt> <out.json>
"""
import base64
import json
import ssl
import sys
import time
import urllib.error
import urllib.request

BASE = "https://api.manus.im"
UA = "cue-re/0.1 (schema harvest)"


def post(path, body: bytes, content_type="application/json"):
    req = urllib.request.Request(
        BASE + path,
        data=body,
        method="POST",
        headers={
            "content-type": content_type,
            "connect-protocol-version": "1",
            "x-product-name": "manus-agents",
            "user-agent": UA,
            "accept": "*/*",
        },
    )
    ctx = ssl.create_default_context()
    try:
        with urllib.request.urlopen(req, timeout=25, context=ctx) as resp:
            return resp.status, resp.read(), dict(resp.headers)
    except urllib.error.HTTPError as err:
        return err.code, err.read(), dict(err.headers)
    except Exception as exc:  # noqa: BLE001 - transport detail is the finding
        return -1, str(exc).encode(), {}


# ---- minimal protobuf wire decoder (stdlib only) ----
def _varint(buf, i):
    shift = 0
    val = 0
    while True:
        b = buf[i]
        i += 1
        val |= (b & 0x7F) << shift
        if not b & 0x80:
            return val, i
        shift += 7


def decode(buf):
    """Return [(field_no, wire_type, value)] for a protobuf message."""
    out = []
    i = 0
    while i < len(buf):
        key, i = _varint(buf, i)
        fn, wt = key >> 3, key & 7
        if wt == 0:
            v, i = _varint(buf, i)
            out.append((fn, wt, v))
        elif wt == 2:
            ln, i = _varint(buf, i)
            out.append((fn, wt, buf[i:i + ln]))
            i += ln
        elif wt == 5:
            out.append((fn, wt, buf[i:i + 4]))
            i += 4
        elif wt == 1:
            out.append((fn, wt, buf[i:i + 8]))
            i += 8
        else:
            break
    return out


def violations_from_b64(b64):
    """Decode buf.validate.Violations: field 1 repeated Violation.

    Violation uses 2=rule, 3=message, 5=FieldPath (1=repeated element,
    element: 1=field number, 2=field name, 3=kind).
    """
    pad = "=" * (-len(b64) % 4)
    raw = base64.b64decode(b64 + pad)
    rows = []
    for fn, wt, val in decode(raw):
        if fn != 1 or wt != 2:
            continue
        rule = ""
        msg = ""
        path = []
        for ifn, iwt, ival in decode(val):
            if ifn == 2 and iwt == 2:
                rule = ival.decode("utf-8", "replace")
            elif ifn == 3 and iwt == 2:
                msg = ival.decode("utf-8", "replace")
            elif ifn == 5 and iwt == 2:
                for pfn, pwt, pval in decode(ival):
                    if pfn != 1 or pwt != 2:
                        continue
                    num = None
                    name = None
                    for efn, ewt, eval_ in decode(pval):
                        if efn == 1 and ewt == 0:
                            num = eval_
                        elif efn == 2 and ewt == 2:
                            name = eval_.decode("utf-8", "replace")
                    if num is not None or name:
                        path.append((num, name))
        rows.append({"rule": rule, "message": msg, "path": path})
    return rows


def main():
    src, dst = sys.argv[1], sys.argv[2]
    paths = [ln.strip() for ln in open(src, encoding="utf-8") if ln.strip().startswith("/")]

    results = []
    for idx, path in enumerate(paths):
        status, body, headers = post(path, b"{}")
        entry = {
            "path": path,
            "status": status,
            "content_type": headers.get("Content-Type") or headers.get("content-type"),
            "body_head": body[:600].decode("utf-8", "replace"),
            "violations": [],
            "violations_raw": [],
            "fields": [],
        }
        text = entry["body_head"]
        if status >= 0 and text.startswith("{"):
            try:
                obj = json.loads(body.decode("utf-8", "replace"))
            except json.JSONDecodeError:
                obj = None
            if isinstance(obj, dict):
                entry["code"] = obj.get("code")
                entry["message"] = obj.get("message")
                for d in obj.get("details") or []:
                    if isinstance(d, dict) and d.get("type") == "buf.validate.Violations":
                        rows = violations_from_b64(d.get("value", ""))
                        entry["violations"].extend(rows)
                        entry["violations_raw"].append(d.get("value", ""))
        # Derive field names from both the message text and decoded rules.
        seeds = []
        msg = entry.get("message") or ""
        for chunk in msg.split("\n"):
            chunk = chunk.strip()
            if chunk.startswith("- "):
                chunk = chunk[2:]
            if ":" in chunk:
                seeds.append(chunk.split(":", 1)[0].strip())
        for v in entry["violations"]:
            for _num, name in v.get("path") or []:
                if name:
                    seeds.append(name)
        seen = []
        for s in seeds:
            if s and s not in seen:
                seen.append(s)
        entry["fields"] = seen
        results.append(entry)
        print(f"{idx+1}/{len(paths)} {path} -> {status} fields={len(seen)}", file=sys.stderr)
        time.sleep(0.12)

    with open(dst, "w", encoding="utf-8", newline="\n") as fh:
        json.dump(results, fh, ensure_ascii=False, indent=1)
    print(f"wrote {dst} entries={len(results)}", file=sys.stderr)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())


