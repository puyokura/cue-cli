#!/usr/bin/env python3
"""Minimal raw WebSocket client for Engine.IO v4 probing.

Implements just enough RFC6455 (client masking, text frames, ping/pong) plus
Engine.IO/Socket.IO packet prefixes to observe what the server sends before
authentication is supplied.

Usage: socketio_probe.py <host> <path-with-query> [--send 40] [--wait 6]
"""
import base64
import os
import socket
import ssl
import struct
import sys
import time


def ws_connect(host, path, extra_headers=None):
    raw = socket.create_connection((host, 443), timeout=15)
    ctx = ssl.create_default_context()
    sock = ctx.wrap_socket(raw, server_hostname=host)
    key = base64.b64encode(os.urandom(16)).decode()
    headers = [
        f"GET {path} HTTP/1.1",
        f"Host: {host}",
        "Upgrade: websocket",
        "Connection: Upgrade",
        f"Sec-WebSocket-Key: {key}",
        "Sec-WebSocket-Version: 13",
        "Origin: https://manus.im",
        "User-Agent: cue-re/0.1",
    ]
    for k, v in (extra_headers or {}).items():
        headers.append(f"{k}: {v}")
    sock.sendall(("\r\n".join(headers) + "\r\n\r\n").encode())
    buf = b""
    while b"\r\n\r\n" not in buf:
        chunk = sock.recv(4096)
        if not chunk:
            break
        buf += chunk
    head, rest = buf.split(b"\r\n\r\n", 1)
    return sock, head.decode("latin-1"), rest


def send_text(sock, text):
    payload = text.encode()
    mask = os.urandom(4)
    masked = bytes(b ^ mask[i % 4] for i, b in enumerate(payload))
    header = bytearray([0x81])
    ln = len(payload)
    if ln < 126:
        header.append(0x80 | ln)
    elif ln < 65536:
        header.append(0x80 | 126)
        header += struct.pack(">H", ln)
    else:
        header.append(0x80 | 127)
        header += struct.pack(">Q", ln)
    header += mask
    sock.sendall(bytes(header) + masked)


def recv_frames(sock, deadline):
    buf = b""
    out = []
    sock.settimeout(0.5)
    while time.time() < deadline:
        try:
            chunk = sock.recv(8192)
            if not chunk:
                out.append("<closed>")
                break
            buf += chunk
        except socket.timeout:
            if buf:
                break
            continue
        while len(buf) >= 2:
            b0, b1 = buf[0], buf[1]
            opcode = b0 & 0x0F
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
            out.append(f"op={opcode} {payload[:400]!r}")
    return out


def main():
    host = sys.argv[1]
    path = sys.argv[2]
    sends = []
    wait = 6.0
    if "--send" in sys.argv:
        sends.append(sys.argv[sys.argv.index("--send") + 1])
    if "--wait" in sys.argv:
        wait = float(sys.argv[sys.argv.index("--wait") + 1])
    sock, head, rest = ws_connect(host, path)
    print("HANDSHAKE:", head.replace("\r\n", " | "))
    deadline = time.time() + wait
    frames = []
    if rest:
        frames.append(f"pre {rest[:200]!r}")
    for s in sends:
        send_text(sock, s)
        print(f"SENT: {s!r}")
    frames += recv_frames(sock, deadline)
    for f in frames:
        print("RECV:", f)
    sock.close()
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
