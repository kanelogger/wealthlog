#!/usr/bin/env python3
"""Minimal MCP client for protocol-level acceptance checks.

Reads the PAT from the config's token file, initializes a session, optionally
sends a fixed X-WF-Profile-Id header, and calls one tool. Prints only the
JSON-RPC result; the token never appears in output.
"""

import argparse
import json
import sys
import urllib.error
import urllib.request
from pathlib import Path
from typing import Any, Dict, Optional

from common import HarnessError, discovery_path, emit, load_config, read_token


def parse_sse(raw: str) -> Dict[str, Any]:
    for line in raw.splitlines():
        text = line.strip()
        if text.startswith("data:"):
            text = text[5:].strip()
        if not text:
            continue
        try:
            value = json.loads(text)
        except json.JSONDecodeError:
            continue
        if isinstance(value, dict):
            return value
    raise HarnessError("invalid_response", "no JSON-RPC payload in MCP response", 5)


def request(
    url: str,
    body: Dict[str, Any],
    token: str,
    ledger: Optional[str],
    session: Optional[str] = None,
) -> Any:
    headers = {
        "Content-Type": "application/json",
        "Accept": "application/json, text/event-stream",
        "Authorization": "Bearer " + token,
    }
    if ledger:
        headers["X-WF-Profile-Id"] = ledger
    if session:
        headers["mcp-session-id"] = session
    data = json.dumps(body).encode("utf-8")
    req = urllib.request.Request(url, data=data, headers=headers, method="POST")
    try:
        with urllib.request.urlopen(req, timeout=30) as response:
            raw = response.read().decode("utf-8", "replace")
            return response.headers.get("mcp-session-id"), raw
    except urllib.error.HTTPError as error:
        raw = error.read().decode("utf-8", "replace")
        return None, raw
    except urllib.error.URLError as error:
        raise HarnessError("mcp_unavailable", "MCP connection failed: %s" % error.reason, 5)


def main(argv: Optional[list] = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--config", required=True)
    parser.add_argument("--tool", required=True)
    parser.add_argument("--args", default="{}")
    parser.add_argument("--ledger", help="override X-WF-Profile-Id")
    parser.add_argument("--no-ledger-header", action="store_true")
    args = parser.parse_args(argv)
    try:
        config = load_config(args.config)
        token = read_token(Path(config["tokenFile"]))
        lock = json.loads(discovery_path(config).read_text(encoding="utf-8"))
        url = "http://127.0.0.1:%d/mcp" % int(lock["port"])
        ledger = None if args.no_ledger_header else (args.ledger or config["ledgerId"])
        session, raw = request(
            url,
            {
                "jsonrpc": "2.0",
                "id": 1,
                "method": "initialize",
                "params": {
                    "protocolVersion": "2025-03-26",
                    "capabilities": {},
                    "clientInfo": {"name": "wealthlog-protocol-check", "version": "1"},
                },
            },
            token,
            ledger,
        )
        if not session:
            emit({"ok": False, "phase": "initialize", "response": parse_sse(raw)})
            return 1
        request(
            url,
            {"jsonrpc": "2.0", "method": "notifications/initialized"},
            token,
            ledger,
            session,
        )
        _, raw = request(
            url,
            {
                "jsonrpc": "2.0",
                "id": 2,
                "method": "tools/call",
                "params": {"name": args.tool, "arguments": json.loads(args.args)},
            },
            token,
            ledger,
            session,
        )
        emit({"ok": True, "sessionId": session, "response": parse_sse(raw)})
        return 0
    except (HarnessError, OSError, ValueError, json.JSONDecodeError) as error:
        emit({"ok": False, "error": {"code": "mcp_client", "message": str(error)}})
        return 2


if __name__ == "__main__":
    sys.exit(main())
