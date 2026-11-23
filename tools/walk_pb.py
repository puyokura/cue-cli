#!/usr/bin/env python3
"""Walk a protobuf byte message and print fields with decoded strings.

Usage: walk_pb.py <file.bin> [depth]
"""
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


def walk(buf, depth=0, max_depth=6):
    i = 0
    while i < len(buf):
        key, i = varint(buf, i)
        fn, wt = key >> 3, key & 7
        pad = "  " * depth
        if wt == 0:
            v, i = varint(buf, i)
            print(f"{pad}field={fn} wt=0 varint={v}")
        elif wt == 2:
            ln, i = varint(buf, i)
            chunk = buf[i:i + ln]
            i += ln
            if chunk and all(32 <= c < 127 for c in chunk):
                print(f"{pad}field={fn} wt=2 str={chunk.decode('ascii')!r}")
            else:
                print(f"{pad}field={fn} wt=2 msg({len(chunk)}B):")
                if depth < max_depth:
                    walk(chunk, depth + 1, max_depth)
        elif wt == 5:
            print(f"{pad}field={fn} wt=5 fixed32")
            i += 4
        elif wt == 1:
            print(f"{pad}field={fn} wt=1 fixed64")
            i += 8
        else:
            print(f"{pad}field={fn} UNKNOWN wt={wt} at {i}")
            return


# --- usage guard ---
if len(sys.argv) < 2:
    print("usage: walk_pb.py <protobuf-file>", file=sys.stderr)
    raise SystemExit(2)
def main():
    path = sys.argv[1]
    depth = int(sys.argv[2]) if len(sys.argv) > 2 else 6
    walk(open(path, "rb").read(), 0, depth)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
