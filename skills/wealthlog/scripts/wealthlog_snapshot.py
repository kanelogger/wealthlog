#!/usr/bin/env python3
"""Create, verify and record Wealthlog write-request snapshots.

The script never sends a business request and never retries one. It writes a
private request file, a context with content/credential digests, and a status
file. Standard library only; compatible with Python 3.9.
"""

import argparse
import hashlib
import json
import os
import stat
import sys
import uuid
from datetime import datetime, timezone
from pathlib import Path
from typing import Any, Dict, List, Optional

STATUSES = ("pending", "unknown", "resolved", "abandoned")
MAX_TOKEN_BYTES = 4096


class SnapshotError(Exception):
    def __init__(self, code: str, message: str) -> None:
        super().__init__(message)
        self.code = code
        self.message = message


def emit(value: Dict[str, Any]) -> None:
    sys.stdout.write(json.dumps(value, ensure_ascii=False, sort_keys=True) + "\n")


def sha256_bytes(value: bytes) -> str:
    return hashlib.sha256(value).hexdigest()


def normalize_token(raw: str) -> str:
    if len(raw.encode("utf-8")) > MAX_TOKEN_BYTES:
        raise SnapshotError("invalid_credential", "token exceeds 4096 bytes")
    value = raw
    if value.endswith("\r\n"):
        value = value[:-2]
    elif value.endswith("\n"):
        value = value[:-1]
    if not value:
        raise SnapshotError("invalid_credential", "token is empty")
    if any(character.isspace() for character in value):
        raise SnapshotError(
            "invalid_credential", "token must be one line with no whitespace"
        )
    return value


def read_token(path: Path) -> str:
    """Mirror the CLI's checks: regular file, current owner, 0400/0600, no link."""
    try:
        link_info = os.lstat(path)
    except OSError as error:
        raise SnapshotError("invalid_credential", "cannot inspect token file: %s" % error)
    if stat.S_ISLNK(link_info.st_mode):
        raise SnapshotError("invalid_credential", "token file must not be a symlink")
    try:
        descriptor = os.open(path, os.O_RDONLY | getattr(os, "O_NOFOLLOW", 0)
                             | getattr(os, "O_NONBLOCK", 0))
    except OSError as error:
        raise SnapshotError("invalid_credential", "cannot open token file: %s" % error)
    try:
        info = os.fstat(descriptor)
        if not stat.S_ISREG(info.st_mode):
            raise SnapshotError("invalid_credential", "token file must be a regular file")
        if info.st_uid != os.geteuid():
            raise SnapshotError(
                "invalid_credential", "token file must be owned by the current user"
            )
        mode = stat.S_IMODE(info.st_mode) & 0o777
        if mode not in (0o400, 0o600):
            raise SnapshotError(
                "invalid_credential", "token file mode is %o; expected 0400 or 0600" % mode
            )
        chunks = []
        remaining = MAX_TOKEN_BYTES + 1
        while remaining > 0:
            chunk = os.read(descriptor, remaining)
            if not chunk:
                break
            chunks.append(chunk)
            remaining -= len(chunk)
    finally:
        os.close(descriptor)
    raw = b"".join(chunks)
    if len(raw) > MAX_TOKEN_BYTES:
        raise SnapshotError("invalid_credential", "token exceeds 4096 bytes")
    try:
        text = raw.decode("utf-8")
    except UnicodeDecodeError:
        raise SnapshotError("invalid_credential", "token is not valid UTF-8")
    return normalize_token(text)


def read_json(path: Path) -> Any:
    try:
        with path.open("r", encoding="utf-8") as handle:
            return json.load(handle)
    except OSError as error:
        raise SnapshotError("invalid_input", "cannot read %s: %s" % (path, error))
    except json.JSONDecodeError as error:
        raise SnapshotError("invalid_input", "%s is not valid JSON: %s" % (path, error))


def write_new(path: Path, payload: bytes) -> None:
    try:
        descriptor = os.open(
            path, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600
        )
    except FileExistsError:
        raise SnapshotError("already_exists", "%s already exists" % path)
    except OSError as error:
        raise SnapshotError("state_unwritable", "cannot create %s: %s" % (path, error))
    try:
        with os.fdopen(descriptor, "wb") as handle:
            handle.write(payload)
            handle.flush()
            os.fsync(handle.fileno())
    except OSError as error:
        raise SnapshotError("state_unwritable", "cannot write %s: %s" % (path, error))


def write_status(request_dir: Path, status: str) -> None:
    if status not in STATUSES:
        raise SnapshotError("invalid_input", "unknown status: %s" % status)
    target = request_dir / "status"
    temporary = request_dir / (".status.%s.tmp" % uuid.uuid4().hex)
    write_new(temporary, (status + "\n").encode("utf-8"))
    try:
        os.replace(temporary, target)
    except OSError as error:
        try:
            os.unlink(temporary)
        except OSError:
            pass
        raise SnapshotError("state_unwritable", "cannot update status: %s" % error)


def ensure_state_dir(path: Path) -> None:
    try:
        path.mkdir(mode=0o700, parents=True, exist_ok=True)
        info = os.lstat(path)
    except OSError as error:
        raise SnapshotError("state_unwritable", "cannot create state dir: %s" % error)
    if stat.S_ISLNK(info.st_mode) or not stat.S_ISDIR(info.st_mode):
        raise SnapshotError("state_unwritable", "state dir must be a real directory")
    mode = stat.S_IMODE(info.st_mode) & 0o077
    if mode:
        raise SnapshotError(
            "state_unwritable",
            "state dir is accessible to others; use a private directory (0700)",
        )


def context_for(
    request_dir: Path,
    app_id: str,
    ledger_id: str,
    operation: str,
    received_at: str,
    resolved_date: Optional[str],
    summary: str,
    credential_ref: str,
    credential_sha: str,
) -> Dict[str, Any]:
    request_bytes = (request_dir / "request.json").read_bytes()
    return {
        "snapshotVersion": 1,
        "requestKey": request_dir.name,
        "appId": app_id,
        "ledgerId": ledger_id,
        "operation": operation,
        "receivedAt": received_at,
        "resolvedDate": resolved_date,
        "summary": summary,
        "credentialRef": credential_ref,
        "credentialSha256": credential_sha,
        "requestSha256": sha256_bytes(request_bytes),
        "createdAt": datetime.now(timezone.utc).isoformat(),
    }


def create(args: argparse.Namespace) -> Dict[str, Any]:
    state_dir = Path(args.state_dir).resolve()
    try:
        state_dir.relative_to(Path.cwd().resolve())
    except ValueError:
        raise SnapshotError("state_unwritable", "state directory must be inside the current workspace")
    configured = os.environ.get("WEALTHLOG_AGENT_STATE_DIR")
    if configured and Path(configured).resolve() != state_dir:
        raise SnapshotError("state_unwritable", "state directory differs from WEALTHLOG_AGENT_STATE_DIR")
    ensure_state_dir(state_dir)
    token = read_token(Path(args.credential_file))
    credential_sha = sha256_bytes(token.encode("utf-8"))

    payload = read_json(Path(args.input))
    if not isinstance(payload, dict):
        raise SnapshotError("invalid_input", "--input must contain a JSON object")
    if "requestKey" in payload:
        raise SnapshotError(
            "invalid_input",
            "input already has requestKey; create a fresh input for a new request",
        )
    request_key = str(uuid.uuid4())
    payload["requestKey"] = request_key

    request_dir = state_dir / request_key
    try:
        os.mkdir(request_dir, 0o700)
    except FileExistsError:
        raise SnapshotError("already_exists", "request directory already exists")
    except OSError as error:
        raise SnapshotError("state_unwritable", "cannot create request dir: %s" % error)

    encoded = (json.dumps(payload, ensure_ascii=False, indent=2, sort_keys=True) + "\n").encode(
        "utf-8"
    )
    write_new(request_dir / "request.json", encoded)
    context = context_for(
        request_dir,
        args.app_id,
        args.ledger,
        args.operation,
        args.received_at,
        args.resolved_date,
        args.summary,
        args.credential_file,
        credential_sha,
    )
    write_new(
        request_dir / "context.json",
        (json.dumps(context, ensure_ascii=False, indent=2, sort_keys=True) + "\n").encode(
            "utf-8"
        ),
    )
    write_status(request_dir, "pending")
    return {
        "ok": True,
        "requestKey": request_key,
        "requestDir": str(request_dir),
        "requestPath": str(request_dir / "request.json"),
        "contextPath": str(request_dir / "context.json"),
        "requestSha256": context["requestSha256"],
        "credentialSha256": credential_sha,
    }


def verify(args: argparse.Namespace) -> Dict[str, Any]:
    request_dir = Path(args.request_dir)
    request_path = request_dir / "request.json"
    context_path = request_dir / "context.json"
    if not request_path.is_file() or not context_path.is_file():
        raise SnapshotError("invalid_input", "request or context file is missing")
    context = read_json(context_path)
    if not isinstance(context, dict):
        raise SnapshotError("invalid_input", "context.json must be an object")
    request_sha = sha256_bytes(request_path.read_bytes())
    token = read_token(Path(args.credential_file or context["credentialRef"]))
    credential_sha = sha256_bytes(token.encode("utf-8"))
    status_path = request_dir / "status"
    status = status_path.read_text(encoding="utf-8").strip() if status_path.exists() else None
    problems = []
    if request_sha != context.get("requestSha256"):
        problems.append("requestSha256")
    if credential_sha != context.get("credentialSha256"):
        problems.append("credentialSha256")
    if status not in STATUSES:
        problems.append("status")
    if context.get("requestKey") != request_dir.name:
        problems.append("requestKey")
    for argument, field in ((args.app_id, "appId"), (args.ledger, "ledgerId"),
                            (args.operation, "operation")):
        if argument is not None and argument != context.get(field):
            problems.append(field)
    return {
        "ok": not problems,
        "requestDir": str(request_dir),
        "requestKey": context.get("requestKey"),
        "status": status,
        "problems": problems,
        "requestSha256": request_sha,
        "credentialSha256": credential_sha,
    }


def mark(args: argparse.Namespace) -> Dict[str, Any]:
    request_dir = Path(args.request_dir)
    if not (request_dir / "context.json").is_file():
        raise SnapshotError("invalid_input", "request directory is not a snapshot")
    write_status(request_dir, args.status)
    return {"ok": True, "requestDir": str(request_dir), "status": args.status}


def show(args: argparse.Namespace) -> Dict[str, Any]:
    request_dir = Path(args.request_dir)
    context = read_json(request_dir / "context.json")
    request = read_json(request_dir / "request.json")
    status_path = request_dir / "status"
    status = status_path.read_text(encoding="utf-8").strip() if status_path.exists() else None
    return {
        "ok": True,
        "context": context,
        "request": request,
        "status": status,
    }


def list_snapshots(args: argparse.Namespace) -> Dict[str, Any]:
    state_dir = Path(args.state_dir)
    items: List[Dict[str, Any]] = []
    if state_dir.is_dir():
        for child in sorted(state_dir.iterdir()):
            if not child.is_dir() or child.name.startswith("."):
                continue
            context_path = child / "context.json"
            if not context_path.is_file():
                continue
            try:
                context = read_json(context_path)
                status_path = child / "status"
                status = (
                    status_path.read_text(encoding="utf-8").strip()
                    if status_path.exists()
                    else None
                )
            except SnapshotError:
                continue
            items.append(
                {
                    "requestKey": child.name,
                    "status": status,
                    "operation": context.get("operation"),
                    "ledgerId": context.get("ledgerId"),
                    "receivedAt": context.get("receivedAt"),
                    "summary": context.get("summary"),
                }
            )
    return {"ok": True, "stateDir": str(state_dir), "items": items}


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description=__doc__)
    sub = parser.add_subparsers(dest="command", required=True)

    create_parser = sub.add_parser("create")
    create_parser.add_argument("--state-dir", required=True)
    create_parser.add_argument("--app-id", required=True)
    create_parser.add_argument("--ledger", required=True)
    create_parser.add_argument(
        "--operation",
        required=True,
        choices=("ordinary_create", "ordinary_update", "pending_create"),
    )
    create_parser.add_argument("--credential-file", required=True)
    create_parser.add_argument("--received-at", required=True)
    create_parser.add_argument("--resolved-date")
    create_parser.add_argument("--summary", default="")
    create_parser.add_argument("--input", required=True)

    verify_parser = sub.add_parser("verify")
    verify_parser.add_argument("--request-dir", required=True)
    verify_parser.add_argument("--credential-file", help="credential actually selected for this run")
    verify_parser.add_argument("--app-id")
    verify_parser.add_argument("--ledger")
    verify_parser.add_argument("--operation", choices=("ordinary_create", "ordinary_update", "pending_create"))

    mark_parser = sub.add_parser("mark")
    mark_parser.add_argument("--request-dir", required=True)
    mark_parser.add_argument("--status", required=True, choices=STATUSES)

    show_parser = sub.add_parser("show")
    show_parser.add_argument("--request-dir", required=True)

    list_parser = sub.add_parser("list")
    list_parser.add_argument("--state-dir", required=True)
    return parser


def main(argv: Optional[List[str]] = None) -> int:
    args = build_parser().parse_args(argv)
    handlers = {
        "create": create,
        "verify": verify,
        "mark": mark,
        "show": show,
        "list": list_snapshots,
    }
    try:
        result = handlers[args.command](args)
        emit(result)
        return 0 if result.get("ok") else 2
    except SnapshotError as error:
        emit({"ok": False, "error": {"code": error.code, "message": error.message}})
        return 2
    except (KeyError, OSError, ValueError) as error:
        emit({"ok": False, "error": {"code": "invalid_input", "message": str(error)}})
        return 2


if __name__ == "__main__":
    sys.exit(main())
