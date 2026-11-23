#!/usr/bin/env python3
"""Probe the chat socket with different identity query values.

The CLI's `join` produces `agent_error code=1 "You are not allowed to perform
this action."` even though `GetSession` reports the caller as owner with
READ_WRITE. This script isolates whether the denial comes from the identity
query the socket announces (clientType / product name) rather than the session.

Usage: join_socket.py <session_id> [--token-file PATH]
"""
import base64
import json
import os
import socket
import ssl
import struct
import sys
import time
import urllib.parse

HOST = "api.manus.im"
PATH = "/socket.io/"


def token():
    path = sys.argv[2] if len(sys.argv) > 2 else os.path.join(
        os.environ.get("APPDATA", ""), "cue-cli", "token")
    with open(path, "r", encoding="utf-8") as fh:
        return fh.read().strip()


def connect(query):
    qs = urllib.parse.urlencode(query)
    raw = socket.create_connection((HOST, 443), timeout=20)
    sock = ssl.create_default_context().wrap_socket(raw, server_hostname=HOST)
    key = base64.b64encode(os.urandom(16)).decode()
    req = (f"GET {PATH}?EIO=4&transport=websocket&{qs} HTTP/1.1\r\n"
           f"Host: {HOST}\r\nUpgrade: websocket\r\nConnection: Upgrade\r\n"
           f"Sec-WebSocket-Key: {key}\r\nSec-WebSocket-Version: 13\r\n"
           "Origin: https://manus.im\r\nUser-Agent: cue-re/probe\r\n" + os.environ.get("PROBE_COOKIE", "") + "\r\n")
    sock.sendall(req.encode())
    buf = b""
    while b"\r\n\r\n" not in buf:
        buf += sock.recv(4096)
    head, rest = buf.split(b"\r\n\r\n", 1)
    if b"101" not in head.split(b"\r\n")[0]:
        raise RuntimeError(head.decode("latin-1"))
    return sock, rest


def send(sock, text):
    payload = text.encode()
    mask = os.urandom(4)
    masked = bytes(b ^ mask[i % 4] for i, b in enumerate(payload))
    hdr = bytearray([0x81])
    n = len(payload)
    if n < 126:
        hdr.append(0x80 | n)
    elif n < 65536:
        hdr.append(0x80 | 126)
        hdr += struct.pack(">H", n)
    else:
        hdr.append(0x80 | 127)
        hdr += struct.pack(">Q", n)
    hdr += mask
    sock.sendall(bytes(hdr) + masked)


def read_frames(sock, seconds):
    out, buf = [], b""
    deadline = time.time() + seconds
    sock.settimeout(0.5)
    while time.time() < deadline:
        try:
            chunk = sock.recv(8192)
            if not chunk:
                break
            buf += chunk
        except socket.timeout:
            if buf:
                break
            continue
        while len(buf) >= 2:
            b0, b1 = buf[0], buf[1]
            ln = b1 & 0x7F
            off = 2
            if ln == 126:
                if len(buf) < 4:
                    break
                ln = struct.unpack(">H", buf[2:4])[0]
                off = 4
            elif ln == 127:
                if len(buf) < 10:
                    break
                ln = struct.unpack(">Q", buf[2:10])[0]
                off = 10
            if len(buf) < off + ln:
                break
            payload = buf[off:off + ln]
            buf = buf[off + ln:]
            out.append(payload.decode("utf-8", "replace"))
    return out


# --- usage guard ---
if len(sys.argv) < 2:
    print("usage: join_socket.py <session_id> [--token-file PATH]", file=sys.stderr)
    raise SystemExit(2)
def main():
    session_id = sys.argv[1]
    tok = token()

    variants = [
        ("clientType=CUE product=cue",
         {"locale": "ja-JP", "tz": "Asia/Tokyo", "clientType": "CUE",
          "clientVersion": "1.2.0", "x-product-name": "cue"}),
        ("clientType=web product=manus-agents",
         {"locale": "ja-JP", "tz": "Asia/Tokyo", "clientType": "web",
          "clientVersion": "26.19.1", "x-product-name": "manus-agents"}),
        ("clientType=Manus product=Manus",
         {"locale": "ja-JP", "tz": "Asia/Tokyo", "clientType": "Manus",
          "clientVersion": "26.19.1", "x-product-name": "Manus"}),
        ("no query",
         {}),
    ]

    for label, q in variants:
        print(f"\n=== {label} ===")
        try:
            sock, rest = connect(q)
        except Exception as exc:  # noqa: BLE001
            print(f"handshake failed: {exc}")
            continue
        send(sock, f'40{json.dumps({"token": tok})}')
        for frame in read_frames(sock, 2.0):
            print("  <-", frame[:200])
        send(sock, "42" + json.dumps(["message", {
            "type": "join_session", "session_id": session_id,
            "last_message_id": "new_session", "version": 2}]))
        print("  -> join_session")
        for frame in read_frames(sock, 6.0):
            print("  <-", frame[:260])
        sock.close()
        time.sleep(0.4)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

