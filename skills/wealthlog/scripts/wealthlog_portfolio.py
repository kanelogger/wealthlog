#!/usr/bin/env python3
"""Local portfolio CLI adapter with frozen requests. Python 3.9+, stdlib only.

Reads the existing connection config, passes only a token FILE to the CLI,
never accesses SQLite and never retries a write automatically.
"""
import argparse
import json
import os
import subprocess
import sys
from datetime import datetime, timezone
from pathlib import Path

sys.dont_write_bytecode = True
import wealthlog_snapshot as snapshots

OPERATIONS = {
    "transfer_create": ("transfers", "create"),
    "manual_trade_record": ("holdings", "trade"),
    "holdings_snapshot_patch": ("holdings", "patch"),
}


def config(path):
    path = Path(path or os.environ.get("WEALTHLOG_AGENT_CONFIG", "~/.config/wealthlog-agent/config.json")).expanduser()
    value = snapshots.read_json(path)
    for key in ("cli", "appId", "ledgerId", "tokenFile", "timezone", "stateDir"):
        if not isinstance(value.get(key), str) or not value[key].strip():
            raise snapshots.SnapshotError("invalid_config", "missing config field: " + key)
    for key in ("cli", "tokenFile", "stateDir"):
        if not Path(value[key]).is_absolute():
            raise snapshots.SnapshotError("invalid_config", key + " must be absolute")
    return value


def cli_call(cfg, args):
    command = [cfg["cli"], "--app-id", cfg["appId"], "--ledger", cfg["ledgerId"], "--token-file", cfg["tokenFile"], *args]
    try:
        process = subprocess.run(command, capture_output=True, text=True, timeout=60, check=False)
    except subprocess.TimeoutExpired:
        return 5, {"ok": False, "error": {"code": "result_unknown", "message": "CLI timed out; retain the same frozen request", "retryable": True}}
    try:
        result = json.loads(process.stdout)
    except (ValueError, TypeError):
        return 5, {"ok": False, "error": {"code": "result_unknown", "message": "CLI did not return structured JSON; retain the frozen request", "retryable": True}}
    return process.returncode, result


def ready(cfg):
    code, result = cli_call(cfg, ["service", "status"])
    state = result.get("data", {}).get("state")
    if code or state != "ready":
        raise snapshots.SnapshotError("service_" + str(state or "unavailable"), "target service is not ready; open or unlock the configured app")


def prepare(cfg, args):
    payload = snapshots.read_json(Path(args.input))
    if not isinstance(payload, dict):
        raise snapshots.SnapshotError("invalid_input", "input must be an object")
    date = payload.get("activityDate") or payload.get("snapshotDate")
    if not isinstance(date, str):
        raise snapshots.SnapshotError("invalid_input", "supply the user's explicit activityDate/snapshotDate")
    return snapshots.create(argparse.Namespace(
        state_dir=cfg["stateDir"], app_id=cfg["appId"], ledger=cfg["ledgerId"],
        operation=args.operation, credential_file=cfg["tokenFile"],
        received_at=datetime.now(timezone.utc).isoformat(), resolved_date=date,
        summary=args.summary, input=args.input,
    ))


def submit(cfg, args):
    request_dir = Path(args.request_dir).resolve()
    state_dir = Path(cfg["stateDir"]).resolve()
    if request_dir.parent != state_dir:
        raise snapshots.SnapshotError("invalid_input", "request must belong to the configured stateDir")
    context = snapshots.read_json(request_dir / "context.json")
    operation = context.get("operation")
    if operation not in OPERATIONS:
        raise snapshots.SnapshotError("invalid_input", "not a portfolio request")
    verified = snapshots.verify(argparse.Namespace(request_dir=str(request_dir),
        credential_file=cfg["tokenFile"],app_id=cfg["appId"],ledger=cfg["ledgerId"],operation=operation))
    if not verified.get("ok"):
        return 2, verified
    ready(cfg)
    code, result = cli_call(cfg, [*OPERATIONS[operation], "--input", str(request_dir / "request.json")])
    # Persist the observation before reporting it. The response contains no PAT.
    response_path = request_dir / "result.json"
    descriptor = os.open(response_path, os.O_WRONLY | os.O_CREAT | os.O_TRUNC, 0o600)
    with os.fdopen(descriptor, "w") as stream:
        json.dump(result, stream, ensure_ascii=False, sort_keys=True)
        stream.write("\n")
    if code == 0:
        snapshots.write_status(request_dir, "resolved")
    elif code == 5:
        snapshots.write_status(request_dir, "unknown")
    return code, result


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--config")
    sub = parser.add_subparsers(dest="command", required=True)
    sub.add_parser("accounts")
    state = sub.add_parser("state")
    state.add_argument("--account-id", required=True)
    frozen = sub.add_parser("prepare")
    frozen.add_argument("operation", choices=tuple(OPERATIONS))
    frozen.add_argument("--input", required=True)
    frozen.add_argument("--summary", default="")
    send = sub.add_parser("submit")
    send.add_argument("--request-dir", required=True)
    args = parser.parse_args(argv)
    try:
        cfg = config(args.config)
        if args.command == "prepare":
            result = prepare(cfg, args)
            snapshots.emit(result)
            return 0
        if args.command == "submit":
            code, result = submit(cfg, args)
        else:
            ready(cfg)
            cli_args = ["accounts", "list"] if args.command == "accounts" else ["holdings", "get", "--account-id", args.account_id]
            code, result = cli_call(cfg, cli_args)
        snapshots.emit(result)
        return code
    except (snapshots.SnapshotError, OSError) as error:
        snapshots.emit({"ok": False, "error": {"code": getattr(error, "code", "local_error"), "message": str(error), "retryable": False}})
        return 2


if __name__ == "__main__":
    sys.exit(main())
