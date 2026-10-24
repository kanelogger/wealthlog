#!/usr/bin/env python3
"""Phase 0 environment and contract preflight.

Checks the interpreter/timezone, CLI binary, config, token file, discovery,
service state, and the MCP initialize + tools/list path. It never writes
business data and never prints a token.
"""

import argparse
import json
import os
import platform
import subprocess
import sys
import tempfile
import urllib.error
import urllib.request
from pathlib import Path
from typing import Any, Dict, List

from common import (
    HarnessError,
    discovery_path,
    emit,
    load_config,
    now_shanghai,
    read_token,
    service_state,
    ensure_private_dir,
)

REQUIRED_TOOLS = [
    "query_ordinary_activities",
    "get_ordinary_activity",
    "create_ordinary_activity",
    "update_ordinary_activity",
    "list_pending_activities",
    "get_accounts",
]
DRAFT_TOOL = "create_pending_activity"


def check(name: str, ok: bool, detail: str, required: bool = True) -> Dict[str, Any]:
    return {"name": name, "ok": bool(ok), "required": required, "detail": detail}


def cli_version(cli: str) -> str:
    completed = subprocess.run(
        [cli, "--version"], check=False, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True
    )
    if completed.returncode != 0:
        raise HarnessError("cli_missing", "CLI --version failed")
    return (completed.stdout or completed.stderr).strip()


def mcp_probe(config: Dict[str, Any], discovery: Dict[str, Any], token: str) -> Dict[str, Any]:
    port = discovery.get("port")
    if not isinstance(port, int) or port <= 0:
        raise HarnessError("mcp_unavailable", "discovery file has no port")
    url = "http://127.0.0.1:%d/mcp" % port
    headers = {
        "Content-Type": "application/json",
        "Accept": "application/json, text/event-stream",
        "Authorization": "Bearer " + token,
        "X-WF-Profile-Id": str(config["ledgerId"]),
    }

    def rpc(body: Dict[str, Any], session: str = "") -> Any:
        data = json.dumps(body).encode("utf-8")
        request = urllib.request.Request(url, data=data, headers=dict(headers), method="POST")
        if session:
            request.add_header("mcp-session-id", session)
        try:
            with urllib.request.urlopen(request, timeout=20) as response:
                session_id = response.headers.get("mcp-session-id", "")
                raw = response.read().decode("utf-8", "replace")
                return session_id, raw
        except urllib.error.HTTPError as error:
            raise HarnessError(
                "mcp_rejected",
                "MCP HTTP %s" % error.code,
                3 if error.code in (401, 403) else 5,
            )
        except urllib.error.URLError as error:
            raise HarnessError("mcp_unavailable", "MCP connection failed: %s" % error.reason, 5)

    session, raw = rpc(
        {
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": {
                "protocolVersion": "2025-03-26",
                "capabilities": {},
                "clientInfo": {"name": "wealthlog-acceptance", "version": "1"},
            },
        }
    )
    if not session:
        raise HarnessError("mcp_unavailable", "MCP initialize returned no session id", 5)
    rpc({"jsonrpc": "2.0", "method": "notifications/initialized"}, session)
    _, raw = rpc(
        {"jsonrpc": "2.0", "id": 2, "method": "tools/list", "params": {}}, session
    )
    names: List[str] = []
    for line in raw.splitlines():
        text = line.strip()
        if text.startswith("data:"):
            text = text[5:].strip()
        if not text:
            continue
        try:
            payload = json.loads(text)
        except json.JSONDecodeError:
            continue
        result = payload.get("result") if isinstance(payload, dict) else None
        if isinstance(result, dict) and isinstance(result.get("tools"), list):
            names = [str(tool.get("name")) for tool in result["tools"] if isinstance(tool, dict)]
    missing = [tool for tool in REQUIRED_TOOLS if tool not in names]
    return {
        "url": url,
        "sessionIdPresent": bool(session),
        "tools": names,
        "missingTools": missing,
        "draftToolPresent": DRAFT_TOOL in names,
    }


def main(argv: List[str]) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--config", required=True)
    parser.add_argument("--out")
    parser.add_argument("--skip-mcp", action="store_true")
    parser.add_argument("--workspace", default=os.getcwd(), help="actual Agent workspace root")
    args = parser.parse_args(argv)

    checks: List[Dict[str, Any]] = []
    result: Dict[str, Any] = {"ok": False, "checks": checks}
    try:
        config = load_config(args.config)
        result["configPath"] = config["_path"]
        result["appId"] = config["appId"]
        result["ledgerId"] = config["ledgerId"]
        result["time"] = now_shanghai()
        try:
            workspace = Path(args.workspace).resolve()
            state_dir = Path(config["stateDir"]).resolve()
            state_dir.relative_to(workspace)
            ensure_private_dir(state_dir)
            with tempfile.TemporaryFile(dir=str(state_dir)) as probe:
                probe.write(b"wealthlog-state-probe")
                probe.flush()
                os.fsync(probe.fileno())
                probe.seek(0)
                if probe.read() != b"wealthlog-state-probe":
                    raise HarnessError("state_unwritable", "state readback differs")
            checks.append(check("snapshot_state", True, "private and readable/writable within %s" % workspace))
        except (OSError, ValueError, HarnessError) as error:
            checks.append(check("snapshot_state", False, str(error)))

        checks.append(
            check(
                "python",
                sys.version_info >= (3, 9),
                "%s (%s)" % (platform.python_version(), sys.executable),
            )
        )
        try:
            from zoneinfo import ZoneInfo

            ZoneInfo("Asia/Shanghai")
            checks.append(check("timezone", True, "Asia/Shanghai loadable"))
        except Exception as error:  # noqa: BLE001 - report the exact dependency
            checks.append(check("timezone", False, "Asia/Shanghai unavailable: %s" % error))

        cli = str(config["cli"])
        checks.append(check("cli_executable", Path(cli).is_file() and os.access(cli, os.X_OK), cli))
        try:
            version = cli_version(cli)
            checks.append(check("cli_version", True, version))
            result["cliVersion"] = version
        except Exception as error:  # noqa: BLE001
            checks.append(check("cli_version", False, str(error)))

        try:
            read_token(Path(config["tokenFile"]))
            checks.append(check("token_file", True, "private, regular, current user"))
        except HarnessError as error:
            checks.append(check("token_file", False, "%s: %s" % (error.code, error.message)))

        lock_path = discovery_path(config)
        checks.append(
            check(
                "discovery_file",
                lock_path.is_file(),
                str(lock_path),
            )
        )
        discovery: Dict[str, Any] = {}
        if lock_path.is_file():
            try:
                discovery = json.loads(lock_path.read_text(encoding="utf-8"))
                result["discovery"] = {
                    key: discovery.get(key)
                    for key in ("instanceId", "port", "pid", "status", "activeLedgerId")
                }
                checks.append(
                    check(
                        "active_ledger",
                        discovery.get("activeLedgerId") == config["ledgerId"],
                        "%s" % discovery.get("activeLedgerId"),
                    )
                )
            except (OSError, json.JSONDecodeError) as error:
                checks.append(check("discovery_file", False, str(error)))

        try:
            status = service_state(config)
            data = status.get("data") or {}
            state = data.get("state")
            checks.append(
                check(
                    "service_state",
                    state == "ready",
                    "state=%s reported=%s probeError=%s"
                    % (state, data.get("reportedState"), data.get("probeError")),
                )
            )
            result["service"] = data
        except HarnessError as error:
            checks.append(check("service_state", False, "%s: %s" % (error.code, error.message)))

        if not args.skip_mcp and discovery.get("status") == "ready":
            try:
                mcp = mcp_probe(config, discovery, read_token(Path(config["tokenFile"])))
                checks.append(
                    check(
                        "mcp_tools",
                        not mcp["missingTools"],
                        "missing=%s" % ",".join(mcp["missingTools"]),
                    )
                )
                result["mcp"] = {
                    "url": mcp["url"],
                    "toolCount": len(mcp["tools"]),
                    "missingTools": mcp["missingTools"],
                    "draftToolPresent": mcp["draftToolPresent"],
                }
            except HarnessError as error:
                checks.append(check("mcp_tools", False, "%s: %s" % (error.code, error.message)))

        required_ok = all(item["ok"] for item in checks if item["required"])
        result["ok"] = required_ok
    except HarnessError as error:
        result["error"] = {"code": error.code, "message": error.message}

    if args.out:
        Path(args.out).parent.mkdir(parents=True, exist_ok=True)
        Path(args.out).write_text(
            json.dumps(result, ensure_ascii=False, indent=2) + "\n", encoding="utf-8"
        )
    emit(result)
    return 0 if result.get("ok") else 1


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
