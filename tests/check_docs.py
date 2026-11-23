#!/usr/bin/env python3
"""Verify that counts documented in the repo match the files themselves.

A stale number in a published document is a claim nobody re-measured. This
re-derives each documented count from the source of truth and fails loudly.

Usage: check_docs.py
"""
import os
import re
import sys

REPO = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))


def read(path, encoding="utf-8"):
    with open(os.path.join(REPO, path), encoding=encoding) as fh:
        return fh.read()


def count_inventory():
    lines = [ln.strip() for ln in read("docs/rpc-inventory.txt").splitlines()]
    return sum(1 for ln in lines if ln.startswith("/"))


def count_discovered():
    lines = [ln.strip() for ln in read("docs/rpc-discovered.txt").splitlines()]
    return sum(1 for ln in lines if ln and not ln.startswith("#"))


def count_field_rows():
    lines = [ln for ln in read("docs/rpc-fields.tsv").splitlines() if ln.strip()]
    return len(lines)


def main():
    checks = []

    static = count_inventory()
    discovered = count_discovered()
    total = static + discovered

    protocol = read("docs/protocol.md")
    checks.append(
        ("protocol.md states the static count",
         f"**{static} 本**" in protocol, f"static={static}")
    )
    checks.append(
        ("protocol.md states the discovered count",
         f"**{discovered} 本**" in protocol, f"discovered={discovered}")
    )
    checks.append(
        ("protocol.md states the total",
         f"{total} 本" in protocol, f"total={total}")
    )

    fields = count_field_rows()
    readme = read("README.md")
    checks.append(
        ("rpc-fields.tsv has rows", fields > 100, f"rows={fields}")
    )

    # Every file referenced from the README must exist.
    refs = re.findall(r"`([A-Za-z0-9_./-]+\.(?:md|txt|tsv|py|rs))`", readme)
    missing = [
        r for r in refs
        if not os.path.exists(os.path.join(REPO, r))
        and not os.path.exists(os.path.join(REPO, "tests", r))
        and not os.path.exists(os.path.join(REPO, "docs", r))
    ]
    checks.append(("README references resolve", not missing, str(missing)))

    failed = [(name, detail) for name, ok, detail in checks if not ok]
    for name, ok, detail in checks:
        print(f"{'PASS' if ok else 'FAIL'}  {name}  [{detail}]")
    if failed:
        print(f"\n{len(failed)} documented count(s) are stale")
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

