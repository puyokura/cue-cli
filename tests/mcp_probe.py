#!/usr/bin/env python3
"""Drive the MCP server over stdio and check ordering under a slow tool.

Sends `join` (slow) first and `region` (fast) second, then reports the order and
wall-clock time each response arrived. If the fast call's answer arrives before
the slow one finishes, the server is not blocking its read loop.

Usage: mcp_probe.py [path-to-cue-binary] [--session <uid>] [--slow-seconds N]
"""
import json
import os
import queue
import subprocess
import sys
import threading
import time

_EXE = "cue.exe" if os.name == "nt" else "cue"
BIN = os.path.join("target", "release", _EXE)
if len(sys.argv) > 1 and not sys.argv[1].startswith("--"):
    BIN = sys.argv[1]

SESSION = "0000000000000000000000"
if "--session" in sys.argv:
    SESSION = sys.argv[sys.argv.index("--session") + 1]
SLOW = 12
if "--slow-seconds" in sys.argv:
    SLOW = int(sys.argv[sys.argv.index("--slow-seconds") + 1])


def main():
    proc = subprocess.Popen([BIN, "mcp"], stdin=subprocess.PIPE,
                            stdout=subprocess.PIPE, stderr=subprocess.DEVNULL,
                            text=True, encoding="utf-8", bufsize=1)
    responses = queue.Queue()

    def reader():
        for line in proc.stdout:
            line = line.strip()
            if not line:
                continue
            try:
                responses.put((time.time(), json.loads(line)))
            except json.JSONDecodeError:
                responses.put((time.time(), {"raw": line}))

    thread = threading.Thread(target=reader, daemon=True)
    thread.start()

    def send(msg):
        proc.stdin.write(json.dumps(msg) + "\n")
        proc.stdin.flush()

    start = time.time()
    send({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {}})
    send({"jsonrpc": "2.0", "id": 2, "method": "tools/list", "params": {}})
    send({"jsonrpc": "2.0", "id": 10, "method": "tools/call",
          "params": {"name": "cue_join",
                     "arguments": {"session_id": SESSION, "seconds": SLOW}}})
    send({"jsonrpc": "2.0", "id": 11, "method": "tools/call",
          "params": {"name": "cue_region", "arguments": {}}})

    seen = {}
    deadline = time.time() + SLOW + 25
    while time.time() < deadline and len(seen) < 4:
        try:
            at, msg = responses.get(timeout=1.0)
        except queue.Empty:
            continue
        rid = msg.get("id")
        if rid is None:
            continue
        seen[rid] = (at - start, msg)

    proc.stdin.close()
    proc.wait(timeout=10)

    for rid in sorted(seen):
        offset, msg = seen[rid]
        # Careful: `"error" in msg is False` would chain-compare. Parenthesise.
        has_error = "error" in msg
        is_error = msg.get("result", {}).get("isError") is True
        ok = not has_error and not is_error
        print(f"id={rid:<3} at {offset:6.2f}s  ok={ok}")

    if 10 in seen and 11 in seen:
        fast_first = seen[11][0] < seen[10][0]
        print(f"\nfast tool answered before the slow one: {fast_first}")
        if not fast_first:
            print("FAIL: the read loop was blocked by the slow tool")
            return 1
    else:
        print("\nFAIL: did not receive both responses")
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
