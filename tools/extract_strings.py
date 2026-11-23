#!/usr/bin/env python3
"""Extract candidate ASCII string runs from a binary artifact.

Usage: extract_strings.py <input> <output.txt> [--min-len N]

Writes one unique run per line, preserving first-seen file offset as a
comment-free prefix column is avoided so the file stays diffable.
Report counts to stderr.
"""
import sys


def main() -> int:
    if len(sys.argv) < 3:
        print("usage: extract_strings.py <input> <output> [--min-len N]", file=sys.stderr)
        return 2
    path_in, path_out = sys.argv[1], sys.argv[2]
    min_len = 6
    if "--min-len" in sys.argv:
        min_len = int(sys.argv[sys.argv.index("--min-len") + 1])

    data = open(path_in, "rb").read()
    out = []
    seen = set()
    run = bytearray()
    for b in data:
        if 0x20 <= b <= 0x7E or b in (0x09,):
            run.append(b)
        else:
            if len(run) >= min_len:
                s = run.decode("ascii", "replace")
                if s not in seen:
                    seen.add(s)
                    out.append(s)
            run.clear()
    if len(run) >= min_len:
        s = run.decode("ascii", "replace")
        if s not in seen:
            seen.add(s)
            out.append(s)

    with open(path_out, "w", encoding="utf-8", newline="\n") as fh:
        fh.write("\n".join(out))
        fh.write("\n")
    print(f"input_bytes={len(data)} unique_runs={len(out)} min_len={min_len}", file=sys.stderr)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
