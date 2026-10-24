"""Shared helpers for the Agent acceptance harness (Python 3.9 stdlib)."""

import json
import os
import stat
import subprocess
import sys
import tempfile
from datetime import datetime, timezone
from pathlib import Path
from typing import Any, Dict, List, Optional
from zoneinfo import ZoneInfo

AGENT_VERSION = 1
MAX_TOKEN_BYTES = 4096


class HarnessError(Exception):
    def __init__(self, code: str, message: str, exit_code: int = 2) -> None:
        super().__init__(message)
        self.code = code
        self.message = message
        self.exit_code = exit_code


def emit(value: Dict[str, Any]) -> None:
    sys.stdout.write(json.dumps(value, ensure_ascii=False, sort_keys=True) + "\n")


def load_json(path: Path) -> Any:
    try:
        with path.open("r", encoding="utf-8") as handle:
            return json.load(handle)
    except OSError as error:
        raise HarnessError("invalid_input", "cannot read %s: %s" % (path, error))
    except json.JSONDecodeError as error:
        raise HarnessError("invalid_input", "%s is not valid JSON: %s" % (path, error))


def write_json(path: Path, value: Any, mode: int = 0o600) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    descriptor, temporary = tempfile.mkstemp(prefix=".tmp-", dir=str(path.parent))
    try:
        os.fchmod(descriptor, mode)
        with os.fdopen(descriptor, "w", encoding="utf-8") as handle:
            json.dump(value, handle, ensure_ascii=False, indent=2, sort_keys=True)
            handle.write("\n")
            handle.flush()
            os.fsync(handle.fileno())
        os.replace(temporary, path)
    except OSError as error:
        try:
            os.unlink(temporary)
        except OSError:
            pass
        raise HarnessError("state_unwritable", "cannot write %s: %s" % (path, error))


def ensure_private_dir(path: Path) -> None:
    path.mkdir(mode=0o700, parents=True, exist_ok=True)
    info = os.lstat(path)
    if stat.S_ISLNK(info.st_mode) or not stat.S_ISDIR(info.st_mode):
        raise HarnessError("state_unwritable", "%s must be a real directory" % path)
    if stat.S_IMODE(info.st_mode) & 0o077:
        raise HarnessError("state_unwritable", "%s must be private (0700)" % path)


def normalize_token(raw: str) -> str:
    if len(raw.encode("utf-8")) > MAX_TOKEN_BYTES:
        raise HarnessError("invalid_credential", "token exceeds 4096 bytes")
    value = raw
    if value.endswith("\r\n"):
        value = value[:-2]
    elif value.endswith("\n"):
        value = value[:-1]
    if not value:
        raise HarnessError("invalid_credential", "token is empty")
    if any(character.isspace() for character in value):
        raise HarnessError("invalid_credential", "token must be one line with no whitespace")
    return value


def read_token(path: Path) -> str:
    try:
        link_info = os.lstat(path)
    except OSError as error:
        raise HarnessError("invalid_credential", "cannot inspect token file: %s" % error)
    if stat.S_ISLNK(link_info.st_mode):
        raise HarnessError("invalid_credential", "token file must not be a symlink")
    descriptor = os.open(path, os.O_RDONLY | getattr(os, "O_NOFOLLOW", 0)
                         | getattr(os, "O_NONBLOCK", 0))
    try:
        info = os.fstat(descriptor)
        if not stat.S_ISREG(info.st_mode):
            raise HarnessError("invalid_credential", "token file must be a regular file")
        if info.st_uid != os.geteuid():
            raise HarnessError("invalid_credential", "token file must be owned by the current user")
        if stat.S_IMODE(info.st_mode) & 0o777 not in (0o400, 0o600):
            raise HarnessError("invalid_credential", "token file mode must be 0400 or 0600")
        chunks: List[bytes] = []
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
        raise HarnessError("invalid_credential", "token exceeds 4096 bytes")
    try:
        return normalize_token(raw.decode("utf-8"))
    except UnicodeDecodeError:
        raise HarnessError("invalid_credential", "token is not valid UTF-8")


def load_config(path: Optional[str]) -> Dict[str, Any]:
    if path:
        config_path = Path(path)
    elif os.environ.get("WEALTHLOG_AGENT_CONFIG"):
        config_path = Path(os.environ["WEALTHLOG_AGENT_CONFIG"])
    else:
        config_path = Path.home() / ".config" / "wealthlog-agent" / "config.json"
    if not config_path.is_file():
        raise HarnessError("config_missing", "config file not found: %s" % config_path)
    value = load_json(config_path)
    if not isinstance(value, dict):
        raise HarnessError("invalid_config", "config must be a JSON object")
    required = ["cli", "appId", "ledgerId", "tokenFile", "timezone", "stateDir"]
    missing = [key for key in required if not value.get(key)]
    if missing:
        raise HarnessError("invalid_config", "missing config fields: %s" % ", ".join(missing))
    value["_path"] = str(config_path)
    return value


def app_data_dir(app_id: str) -> Path:
    override = os.environ.get("WEALTHLOG_AGENT_APP_DATA")
    if override:
        return Path(override)
    if sys.platform == "darwin":
        return Path.home() / "Library" / "Application Support" / app_id
    if sys.platform.startswith("win"):
        return Path(os.environ.get("APPDATA", "")) / app_id
    return Path(os.environ.get("XDG_DATA_HOME", str(Path.home() / ".local" / "share"))) / app_id


def discovery_path(config: Dict[str, Any]) -> Path:
    return app_data_dir(str(config["appId"])) / "mcp.lock"


def cli_prefix(config: Dict[str, Any]) -> List[str]:
    return [
        str(config["cli"]),
        "--app-id",
        str(config["appId"]),
        "--token-file",
        str(config["tokenFile"]),
        "--ledger",
        str(config["ledgerId"]),
    ]


def run_cli(
    config: Dict[str, Any],
    args: List[str],
    timeout: int = 60,
    include_ledger: bool = True,
    stdin_json: Optional[Dict[str, Any]] = None,
) -> Dict[str, Any]:
    command = [str(config["cli"]), "--app-id", str(config["appId"])]
    if include_ledger:
        command.extend(["--token-file", str(config["tokenFile"]), "--ledger", str(config["ledgerId"])])
    command.extend(args)
    environment = os.environ.copy()
    environment.pop("WEALTHLOG_TOKEN", None)
    encoded = None
    if stdin_json is not None:
        encoded = json.dumps(stdin_json, ensure_ascii=False).encode("utf-8")
    try:
        completed = subprocess.run(
            command,
            check=False,
            input=encoded,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            env=environment,
            timeout=timeout,
        )
    except FileNotFoundError:
        raise HarnessError("cli_missing", "CLI executable not found", 5)
    except subprocess.TimeoutExpired:
        raise HarnessError("result_unknown", "CLI timed out; verify before retrying", 5)
    raw = completed.stdout.decode("utf-8", "replace").strip()
    try:
        payload = json.loads(raw) if raw else {}
    except json.JSONDecodeError:
        raise HarnessError("invalid_response", "CLI did not print one JSON object", 5)
    if not isinstance(payload, dict):
        raise HarnessError("invalid_response", "CLI output must be a JSON object", 5)
    if completed.returncode != 0 or not payload.get("ok"):
        error = payload.get("error") or {}
        raise HarnessError(
            str(error.get("code") or "cli_error"),
            str(error.get("message") or "CLI call failed"),
            completed.returncode if completed.returncode else 5,
        )
    return payload


def service_state(config: Dict[str, Any]) -> Dict[str, Any]:
    command = [str(config["cli"]), "--app-id", str(config["appId"]), "service", "status"]
    environment = os.environ.copy()
    environment.pop("WEALTHLOG_TOKEN", None)
    try:
        completed = subprocess.run(
            command,
            check=False,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            env=environment,
            timeout=20,
        )
    except FileNotFoundError:
        raise HarnessError("cli_missing", "CLI executable not found", 5)
    raw = completed.stdout.decode("utf-8", "replace").strip()
    try:
        payload = json.loads(raw) if raw else {}
    except json.JSONDecodeError:
        raise HarnessError("invalid_response", "service status did not print JSON", 5)
    return payload


def now_shanghai() -> Dict[str, str]:
    value = datetime.now(timezone.utc)
    local = value.astimezone(ZoneInfo("Asia/Shanghai"))
    return {
        "receivedAt": value.isoformat(),
        "localTime": local.isoformat(),
        "localDay": local.date().isoformat(),
    }
