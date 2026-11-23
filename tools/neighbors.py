#!/usr/bin/env python3
"""Dump strings near an anchor from the offset-indexed table.

Usage: neighbors.py <strings_off.tsv> <anchor-string> <window-bytes> [--limit N]
Prints offset<TAB>string rows sorted by offset inside [off-window, off+window].
"""
import sys


def main() -> int:
    if len(sys.argv) < 4:
        print("usage: neighbors.py <tsv> <anchor> <window> [--limit N]", file=sys.stderr)
        return 2
    path, anchor, window = sys.argv[1], sys.argv[2], int(sys.argv[3])
    limit = 400
    if "--limit" in sys.argv:
        limit = int(sys.argv[sys.argv.index("--limit") + 1])

    rows = []
    with open(path, "r", encoding="utf-8", errors="replace") as fh:
        for line in fh:
            line = line.rstrip("\n")
            if "\t" not in line:
                continue
            off, s = line.split("\t", 1)
            rows.append((int(off), s))

    anchors = [(o, s) for o, s in rows if s == anchor]
    if not anchors:
        print(f"anchor not found: {anchor}", file=sys.stderr)
        return 1
    first = anchors[0][0]
    print(f"anchor_offset={first} occurrences={len(anchors)}", file=sys.stderr)

    hits = [(o, s) for o, s in rows if first - window <= o <= first + window]
    for o, s in hits[:limit]:
        print(f"{o}\t{s}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
