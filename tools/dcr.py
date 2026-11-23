#!/usr/bin/env python3
"""Reproduce the web client's `x-client-dcr` value.

The web bundle builds it as:
  JSON {ua, locale, languages, timezone, fgRequestId, fgFailureReason,
        action, clientId, screen{width,height}, viewport{width,height},
        timestamp, timezoneOffset}
then base64, then a fixed alphabet rotation (+3 letters, +3 digits).

This reproduces the non-fingerprint path (fgRequestId empty), which the client
itself uses for `login_by_email` (getDcr(false, ...)).

Usage: dcr.py [--action login_by_email] [--ua ...] [--locale ja-JP]
"""
import argparse
import base64
import json
import time


def encode_dcr(raw: str) -> str:
    b = base64.b64encode(raw.encode()).decode()
    out = []
    for ch in b:
        n = ord(ch)
        if 65 <= n <= 90:          # A-Z
            out.append(chr((n - 65 + 3) % 26 + 65))
        elif 97 <= n <= 122:       # a-z
            out.append(chr((n - 97 + 3) % 26 + 97))
        elif 48 <= n <= 57:        # 0-9
            out.append(chr((n - 48 + 3) % 10 + 48))
        else:
            out.append(b64_char(ch))
    return "".join(out)


def b64_char(ch: str) -> str:
    return ch


def build(action: str, ua: str, locale: str, client_id: str,
          screen=(1920, 1080), viewport=(1280, 800), tz_offset=540) -> str:
    payload = {
        "ua": ua,
        "locale": locale,
        "languages": [locale, "en-US", "en"],
        "timezone": "Asia/Tokyo",
        "fgRequestId": "",
        "fgFailureReason": "",
        "action": action,
        "clientId": client_id,
        "screen": {"width": screen[0], "height": screen[1]},
        "viewport": {"width": viewport[0], "height": viewport[1]},
        "timestamp": int(time.time() * 1000),
        "timezoneOffset": tz_offset,
    }
    raw = json.dumps(payload, separators=(",", ":"))
    return encode_dcr(raw)


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--action", default="login_by_email")
    ap.add_argument("--ua", default=(
        "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 "
        "(KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36"
    ))
    ap.add_argument("--locale", default="ja-JP")
    ap.add_argument("--client-id", default="0" * 22)
    args = ap.parse_args()
    print(build(args.action, args.ua, args.locale, args.client_id))


if __name__ == "__main__":
    main()
