#!/usr/bin/env python3
"""Find which package owns a method name, using the 404/401 oracle.

For each name, try it against the known packages in order and stop at the first
hit. This is cheaper than probing every name against every package and gives an
exact (package, method) pair for each discovery.

Usage: locate_methods.py <names-file> <out.json> [--start N] [--count M]
"""
import json
import ssl
import sys
import time
import urllib.error
import urllib.request

BASE = "https://api.manus.im"
HEADERS = {
    "content-type": "application/json",
    "accept": "application/json",
    "connect-protocol-version": "1",
    "x-product-name": "manus-agents",
    "user-agent": "cue-re/0.1 (locate)",
}

PACKAGES = [
    "session.v1.SessionService",
    "session.v1.ProjectService",
    "session.v1.SessionPublicService",
    "session.v1.SessionCollaborateService",
    "user.v1.UserService",
    "user.v1.UserAuthService",
    "user.v1.UserAuthPublicService",
    "user.v1.UserPublicService",
    "space.v1.SpaceService",
    "team.v1.TeamService",
    "agents.v1.AgentsService",
    "agents.v1.AgentGroupsService",
    "agents.v1.AgentComputerService",
    "skill.v1.SkillService",
    "telecom.v1.TelecomService",
    "cloud_pc.v1.CloudPCService",
    "webdev.v1.WebDevService",
    "connectors.v1.ConnectorTeamPolicyService",
    "connectors.v1.ConnectorsService",
    "notifier.v1.NotificationService",
    "notifier.v1.NotifierService",
    "file.v1.FileService",
    "device_authorization.v1.DeviceAuthorizationService",
    "orchestrator.v1.OrchestratorService",
    "place.v1.PlaceService",
]


def pascal(name: str) -> str:
    return name[:1].upper() + name[1:]


def probe(service: str, method: str):
    url = f"{BASE}/{service}/{method}"
    req = urllib.request.Request(url, data=b"{}", method="POST", headers=HEADERS)
    try:
        with urllib.request.urlopen(req, timeout=15, context=ssl.create_default_context()) as r:
            return True, r.status, r.read(200).decode("utf-8", "replace")
    except urllib.error.HTTPError as err:
        body = err.read(200).decode("utf-8", "replace")
        if err.code == 404 and '"code"' not in body:
            return False, err.code, body
        return True, err.code, body
    except Exception:  # noqa: BLE001
        return None, -1, ""


def main():
    if len(sys.argv) < 3:
        print("usage: locate_methods.py <names> <out.json> [--start N] [--count M]", file=sys.stderr)
        return 2
    names = [ln.strip() for ln in open(sys.argv[1], encoding="utf-8") if ln.strip()]
    start = 0
    count = len(names)
    if "--start" in sys.argv:
        start = int(sys.argv[sys.argv.index("--start") + 1])
    if "--count" in sys.argv:
        count = int(sys.argv[sys.argv.index("--count") + 1])
    chunk = names[start:start + count]

    found = []
    for name in chunk:
        method = pascal(name)
        hit = None
        for pkg in PACKAGES:
            ok, status, body = probe(pkg, method)
            if ok:
                hit = {"web_name": name, "package": pkg, "method": method,
                       "status": status, "body_head": body.replace("\n", " ")[:140]}
                break
        if hit:
            found.append(hit)
            print(f"HIT  {hit['package']}/{method} ({hit['status']})", file=sys.stderr)
        time.sleep(0.05)

    result = {"probed": len(chunk), "found": found, "packages": PACKAGES}
    with open(sys.argv[2], "w", encoding="utf-8", newline="\n") as fh:
        json.dump(result, fh, ensure_ascii=False, indent=1)
    print(f"probed={len(chunk)} found={len(found)}", file=sys.stderr)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
