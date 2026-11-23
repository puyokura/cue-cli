#!/usr/bin/env python3
"""Send one arbitrary chat-socket event and report what comes back.

Used to separate "the gateway rejects this event" from "the gateway rejects
this event for this account". Results are printed verbatim.

Usage: socket_event.py <event_name> <json_payload> [--wait 6]
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


def token():
    path = os.path.join(os.environ.get("APPDATA", ""), "cue-cli", "token")
    with open(path, "r", encoding="utf-8") as fh:
        return fh.read().strip()


def ws_connect(query):
    qs = urllib.parse.urlencode(query)
    raw = socket.create_connection((HOST, 443), timeout=20)
    sock = ssl.create_default_context().wrap_socket(raw, server_hostname=HOST)
    key = base64.b64encode(os.urandom(16)).decode()
    req = (f"GET /socket.io/?EIO=4&transport=websocket&{qs} HTTP/1.1\r\n"
           f"Host: {HOST}\r\nUpgrade: websocket\r\nConnection: Upgrade\r\n"
           f"Sec-WebSocket-Key: {key}\r\nSec-WebSocket-Version: 13\r\n"
           "Origin: https://manus.im\r\n\r\n")
    sock.sendall(req.encode())
    buf = b""
    while b"\r\n\r\n" not in buf:
        buf += sock.recv(4096)
    head, rest = buf.split(b"\r\n\r\n", 1)
    if b"101" not in head.split(b"\r\n")[0]:
        raise RuntimeError(head.decode("latin-1"))
    return sock, rest


def send_text(sock, text):
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
    sock.settimeout(0.4)
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
            ln = buf[1] & 0x7F
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
            out.append(buf[off:off + ln].decode("utf-8", "replace"))
            buf = buf[off + ln:]
    return out


# --- usage guard ---
if len(sys.argv) < 3:
    print("usage: socket_event.py <event_name> <json_payload> [--wait SECONDS]", file=sys.stderr)
    raise SystemExit(2)
def main():
    event = sys.argv[1]
    payload = json.loads(sys.argv[2])
    wait = float(sys.argv[sys.argv.index("--wait") + 1]) if "--wait" in sys.argv else 6.0

    sock, rest = ws_connect({"locale": "ja-JP", "tz": "Asia/Tokyo",
                             "clientType": "CUE", "clientVersion": "1.2.0",
                             "x-product-name": "manus-agents"})
    send_text(sock, "40" + json.dumps({"token": token()}))
    for f in read_frames(sock, 1.5):
        print("<-", f[:200])
    send_text(sock, "42" + json.dumps([event, payload]))
    print(f"-> {event}")
    for f in read_frames(sock, wait):
        print("<-", f[:300])
    sock.close()
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
