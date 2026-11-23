#!/usr/bin/env python3
"""Decode buf.validate.Violations into (field_number, field_name, rule, message).

buf.validate.Violations:   repeated Violation element = 1;
Violation:  2 = rule id/text, 3 = human message,
            5 = FieldPath (element: 1 = repeated FieldPathElement),
            6 = FieldRules
FieldPathElement: 1 = field number, 2 = field name, 3 = kind oneof discriminator.

Only field 5 is expanded, so protobuf rule trees cannot break the walk.

Usage: harvest_fields.py <rpc_schema.json> <out.tsv>
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


def fields(buf):
    """Yield (field_no, wire_type, payload) until the buffer is exhausted."""
    i = 0
    while i < len(buf):
        key, i = varint(buf, i)
        fn, wt = key >> 3, key & 7
        if wt == 0:
            v, i = varint(buf, i)
            yield fn, wt, v
        elif wt == 2:
            ln, i = varint(buf, i)
            yield fn, wt, buf[i:i + ln]
            i += ln
        elif wt == 5:
            yield fn, wt, buf[i:i + 4]
            i += 4
        elif wt == 1:
            yield fn, wt, buf[i:i + 8]
            i += 8
        else:
            return


def decode_violations(b64):
    pad = "=" * (-len(b64) % 4)
    raw = base64.b64decode(b64 + pad)
    out = []
    for fn, wt, val in fields(raw):
        if fn != 1 or wt != 2:
            continue
        rule = ""
        message = ""
        path = []
        for ifn, iwt, ival in fields(val):
            if ifn == 2 and iwt == 2:
                rule = ival.decode("utf-8", "replace")
            elif ifn == 3 and iwt == 2:
                message = ival.decode("utf-8", "replace")
            elif ifn == 5 and iwt == 2:
                for pfn, pwt, pval in fields(ival):
                    if pfn != 1 or pwt != 2:
                        continue
                    num = None
                    name = None
                    for efn, ewt, eval_ in fields(pval):
                        if efn == 1 and ewt == 0:
                            num = eval_
                        elif efn == 2 and ewt == 2:
                            name = eval_.decode("utf-8", "replace")
                    if num is not None or name:
                        path.append((num, name))
        out.append({"rule": rule, "message": message, "path": path})
    return out


# --- usage guard ---
if len(sys.argv) < 3:
    print("usage: harvest_fields.py <responses.json> <out.tsv>", file=sys.stderr)
    raise SystemExit(2)
def main():
    src, dst = sys.argv[1], sys.argv[2]
    entries = json.load(open(src, encoding="utf-8"))
    rows = []
    for e in entries:
        for v in e.get("violations") or []:
            if not isinstance(v, dict):
                continue
            parts = []
            for item in v.get("path") or []:
                if isinstance(item, (list, tuple)) and len(item) >= 2:
                    num, name = item[0], item[1]
                    parts.append(f"{name}#{num}" if num is not None else str(name))
                else:
                    parts.append(str(item))
            rows.append(
                (
                    e["path"],
                    str(e["status"]),
                    "/".join(parts),
                    v.get("rule", ""),
                    v.get("message", ""),
                )
            )
    with open(dst, "w", encoding="utf-8", newline="\n") as fh:
        fh.write("rpc\tstatus\tfield_path\trule\tmessage\n")
        for r in rows:
            fh.write("\t".join(str(x).replace("\t", " ") for x in r) + "\n")
    print(f"rows={len(rows)} -> {dst}", file=sys.stderr)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
