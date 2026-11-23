#!/usr/bin/env python3
"""Probe candidate WebSocket endpoints for an HTTP 101 upgrade.

Usage: probe_ws.py <host> <path>...
Prints status per path. A 101 proves the route exists; 404 proves it does not.
"""
import socket
import ssl
import sys


def probe(host, path):
    sock = socket.create_connection((host, 443), timeout=12)
    ctx = ssl.create_default_context()
    sock = ctx.wrap_socket(sock, server_hostname=host)
    key = "AAAAAAAAAAAAAAAAAAAAAA=="
    req = (
        f"GET {path} HTTP/1.1\r\n"
        f"Host: {host}\r\n"
        "Upgrade: websocket\r\n"
        "Connection: Upgrade\r\n"
        f"Sec-WebSocket-Key: {key}\r\n"
        "Sec-WebSocket-Version: 13\r\n"
        "User-Agent: cue-re/0.1\r\n\r\n"
    )
    sock.sendall(req.encode())
    data = sock.recv(2048)
    sock.close()
    first = data.split(b"\r\n", 1)[0].decode("ascii", "replace")
    body = data.split(b"\r\n\r\n", 1)[1][:120] if b"\r\n\r\n" in data else b""
    return first, body.decode("utf-8", "replace")


def main():
    host = sys.argv[1]
    for path in sys.argv[2:]:
        try:
            status, body = probe(host, path)
        except Exception as exc:  # noqa: BLE001 - transport detail is the finding
            status, body = f"ERROR {exc}", ""
        print(f"{path}\t{status}\t{body}")


if __name__ == "__main__":
    raise SystemExit(main())
