#!/usr/bin/env python3
"""A transparent CLI wrapper for A10/A12 fault injection.

Modes:
  unknown  run the real CLI, capture one successful target response, then
           return the documented `result_unknown` shape and exit 5. The next
           invocation for the same request key passes through to the real CLI.
  pause    before sending a matching update, create a marker and wait for a
           `release` file, then run the real CLI. Used to let a GUI edit land
           between read and write.

Only the request key is inspected; the token is never read or printed.
Control files are 0600 under the state directory.
"""

import argparse
import json
import os
import subprocess
import sys
import time
from pathlib import Path
from typing import Any, Dict, List, Optional


class WrapperError(Exception):
    pass


def write_control(path: Path, value: Dict[str, Any]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    os.chmod(path.parent, 0o700)
    temporary = path.with_name(path.name + ".tmp")
    descriptor = os.open(temporary, os.O_WRONLY | os.O_CREAT | os.O_TRUNC, 0o600)
    with os.fdopen(descriptor, "w", encoding="utf-8") as handle:
        json.dump(value, handle, ensure_ascii=False, indent=2)
        handle.flush()
        os.fsync(handle.fileno())
    os.replace(temporary, path)


def read_json(path: Path) -> Any:
    try:
        return json.loads(path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError):
        return None


def request_key_from(args: List[str], stdin_data: Optional[bytes]) -> Optional[str]:
    for index, value in enumerate(args):
        if value == "--input" and index + 1 < len(args):
            source = args[index + 1]
            try:
                if source == "-":
                    payload = json.loads((stdin_data or b"").decode("utf-8"))
                else:
                    payload = json.loads(Path(source).read_text(encoding="utf-8"))
            except (OSError, json.JSONDecodeError, UnicodeDecodeError):
                return None
            if isinstance(payload, dict):
                key = payload.get("requestKey")
                return str(key) if key else None
    return None


def run_real(real: str, args: List[str], stdin_data: Optional[bytes]) -> int:
    completed = subprocess.run(
        [real, *args],
        input=stdin_data,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        check=False,
    )
    sys.stdout.buffer.write(completed.stdout)
    sys.stdout.buffer.flush()
    sys.stderr.buffer.write(completed.stderr)
    sys.stderr.buffer.flush()
    return completed.returncode


def unknown_result(args: List[str], captured: Dict[str, Any]) -> int:
    ledger = captured.get("ledgerId")
    key = captured.get("requestKey")
    payload = {
        "ok": False,
        "ledgerId": ledger,
        "requestKey": key,
        "requestId": None,
        "data": None,
        "error": {
            "code": "result_unknown",
            "message": "the request was sent but its result is unknown; retry with the same request key",
            "retryable": True,
            "requestId": None,
            "data": None,
        },
    }
    sys.stdout.write(json.dumps(payload, ensure_ascii=False) + "\n")
    return 5


def main(argv: Optional[List[str]] = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--real-cli", required=True)
    parser.add_argument("--state", required=True)
    parser.add_argument("--mode", choices=("unknown", "pause"), required=True)
    target = parser.add_mutually_exclusive_group(required=True)
    target.add_argument("--match-request-key")
    target.add_argument("--match-next-create", action="store_true",
                        help="dedicated session only; hide the next successful ordinary create")
    target.add_argument("--match-next-update", action="store_true",
                        help="dedicated session only; pause the next ordinary update")
    parser.add_argument("--verify-db", help="read-only independent commit check before hiding success")
    parser.add_argument("--timeout", type=float, default=10.0)
    parser.add_argument("rest", nargs=argparse.REMAINDER)
    args = parser.parse_args(argv)
    if args.mode == "pause" and not 0 < args.timeout <= 10:
        parser.error("pause timeout must be greater than zero and at most 10 seconds")
    forwarded = args.rest
    if forwarded and forwarded[0] == "--":
        forwarded = forwarded[1:]

    stdin_data: Optional[bytes] = None
    if "-" in forwarded:
        stdin_data = sys.stdin.buffer.read()

    state = Path(args.state)
    state.mkdir(mode=0o700, parents=True, exist_ok=True)
    os.chmod(state, 0o700)
    key = request_key_from(forwarded, stdin_data)

    matches = key is not None and key == args.match_request_key
    if (args.match_next_create or args.match_next_update) and "ordinary" in forwarded:
        position = forwarded.index("ordinary")
        expected = "create" if args.match_next_create else "update"
        matches = key is not None and forwarded[position + 1:position + 2] == [expected]
    if not matches:
        return run_real(args.real_cli, forwarded, stdin_data)

    if args.mode == "unknown":
        fired = state / "unknown.fired"
        if fired.exists():
            return run_real(args.real_cli, forwarded, stdin_data)
        completed = subprocess.run(
            [args.real_cli, *forwarded],
            input=stdin_data,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            check=False,
        )
        if completed.returncode != 0:
            sys.stdout.buffer.write(completed.stdout)
            sys.stderr.buffer.write(completed.stderr)
            return completed.returncode
        try:
            captured = json.loads(completed.stdout.decode("utf-8"))
        except (UnicodeDecodeError, json.JSONDecodeError):
            sys.stdout.buffer.write(completed.stdout)
            return completed.returncode
        write_control(state / "hidden-response.json", captured)
        if args.verify_db:
            verification = subprocess.run(
                [sys.executable, str(Path(__file__).with_name("verify.py")), "committed",
                 "--db", args.verify_db, "--request-key", key],
                stdout=subprocess.PIPE, stderr=subprocess.PIPE, check=False,
            )
            if verification.returncode != 0:
                write_control(state / "unknown.not-injected", {"requestKey": key,
                              "reason": "independent persistence check failed"})
                sys.stdout.buffer.write(completed.stdout)
                sys.stderr.buffer.write(completed.stderr)
                return completed.returncode
        write_control(state / "unknown.fired", {"requestKey": key,
                      "persistenceVerified": bool(args.verify_db)})
        return unknown_result(forwarded, captured)

    waiting = state / "pause.waiting"
    release = state / "release"
    if release.exists():
        release.unlink()
    write_control(waiting, {"requestKey": key, "argv": forwarded})
    deadline = time.time() + args.timeout
    while time.time() < deadline:
        if release.exists():
            write_control(state / "pause.sent", {"requestKey": key})
            try:
                waiting.unlink()
            except OSError:
                pass
            return run_real(args.real_cli, forwarded, stdin_data)
        time.sleep(0.2)
    try:
        waiting.unlink()
    except OSError:
        pass
    sys.stdout.write(
        json.dumps(
            {
                "ok": False,
                "ledgerId": None,
                "requestKey": key,
                "requestId": None,
                "data": None,
                "environmentFailure": True,
                "requestSent": False,
                "error": {
                    "code": "acceptance_pause_timeout",
                    "message": "the paused request was not released before the timeout; nothing was sent",
                    "retryable": False,
                    "requestId": None,
                    "data": None,
                },
            },
            ensure_ascii=False,
        )
        + "\n"
    )
    return 5


if __name__ == "__main__":
    sys.exit(main())
