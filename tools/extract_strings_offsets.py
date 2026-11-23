#!/usr/bin/env python3
"""Extract ASCII string runs with their file offsets.

Usage: extract_strings_offsets.py <input> <output.tsv> [--min-len N]
Output: one "offset<TAB>string" row per run (offsets are not de-duplicated).
"""
import sys


def main() -> int:
    if len(sys.argv) < 3:
        print("usage: extract_strings_offsets.py <input> <output> [--min-len N]", file=sys.stderr)
        return 2
    path_in, path_out = sys.argv[1], sys.argv[2]
    min_len = 6
    if "--min-len" in sys.argv:
        min_len = int(sys.argv[sys.argv.index("--min-len") + 1])

    data = open(path_in, "rb").read()
    rows = []
    run = bytearray()
    start = 0
    for i, b in enumerate(data):
        if 0x20 <= b <= 0x7E or b == 0x09:
            if not run:
                start = i
            run.append(b)
        else:
            if len(run) >= min_len:
                rows.append((start, run.decode("ascii", "replace")))
            run.clear()
    if len(run) >= min_len:
        rows.append((start, run.decode("ascii", "replace")))

    with open(path_out, "w", encoding="utf-8", newline="\n") as fh:
        for off, s in rows:
            fh.write(f"{off}\t{s}\n")
    print(f"input_bytes={len(data)} runs={len(rows)} min_len={min_len}", file=sys.stderr)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
