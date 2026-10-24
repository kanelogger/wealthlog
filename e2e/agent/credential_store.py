#!/usr/bin/env python3
"""Save a one-time PAT without echo, arguments or screenshots.

Reads the secret from a no-echo prompt, stdin, or the macOS clipboard. Writes
a new 0600 file under a 0700 parent, refuses to overwrite, and never prints the
secret. Use --from-clipboard to clear the clipboard after saving.
"""

import argparse
import getpass
import os
import stat
import subprocess
import sys
from pathlib import Path
from typing import List, Optional

MAX_TOKEN_BYTES = 4096


class StoreError(Exception):
    pass


def normalize(raw: str) -> str:
    if len(raw.encode("utf-8")) > MAX_TOKEN_BYTES:
        raise StoreError("token exceeds 4096 bytes")
    value = raw
    if value.endswith("\r\n"):
        value = value[:-2]
    elif value.endswith("\n"):
        value = value[:-1]
    if not value:
        raise StoreError("token is empty")
    if any(character.isspace() for character in value):
        raise StoreError("token must be one line with no whitespace")
    return value


def read_clipboard() -> str:
    if sys.platform != "darwin":
        raise StoreError("--from-clipboard is only implemented on macOS")
    completed = subprocess.run(
        ["pbpaste"], check=False, stdout=subprocess.PIPE, stderr=subprocess.PIPE
    )
    if completed.returncode != 0:
        raise StoreError("cannot read the clipboard")
    return completed.stdout.decode("utf-8", "replace")


def clear_clipboard() -> None:
    if sys.platform != "darwin":
        return
    completed = subprocess.run(
        ["pbcopy"], input=b"", check=False, stdout=subprocess.PIPE, stderr=subprocess.PIPE
    )
    if completed.returncode != 0:
        raise StoreError("clipboard cleanup failed; do not treat credential cleanup as complete")
    if read_clipboard():
        raise StoreError("clipboard is not empty after cleanup; do not treat credential cleanup as complete")


def save(path: Path, secret: str) -> None:
    parent = path.parent
    parent.mkdir(mode=0o700, parents=True, exist_ok=True)
    info = os.lstat(parent)
    if stat.S_ISLNK(info.st_mode) or not stat.S_ISDIR(info.st_mode):
        raise StoreError("parent must be a real directory")
    if stat.S_IMODE(info.st_mode) & 0o077:
        os.chmod(parent, 0o700)
    try:
        descriptor = os.open(path, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
    except FileExistsError:
        raise StoreError("refusing to overwrite an existing credential file")
    except OSError as error:
        raise StoreError("cannot create credential file: %s" % error)
    try:
        with os.fdopen(descriptor, "w", encoding="utf-8") as handle:
            handle.write(secret)
            handle.flush()
            os.fsync(handle.fileno())
    except OSError as error:
        try:
            os.unlink(path)
        except OSError:
            pass
        raise StoreError("cannot write credential file: %s" % error)


def inspect(path: Path) -> dict:
    info = os.lstat(path)
    return {
        "ok": stat.S_ISREG(info.st_mode) and not stat.S_ISLNK(info.st_mode),
        "path": str(path),
        "mode": oct(stat.S_IMODE(info.st_mode)),
        "uid": info.st_uid,
        "size": info.st_size,
    }


def main(argv: Optional[List[str]] = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    sub = parser.add_subparsers(dest="command", required=True)
    save_parser = sub.add_parser("save")
    save_parser.add_argument("--path", required=True)
    source = save_parser.add_mutually_exclusive_group()
    source.add_argument("--stdin", action="store_true", help="read one line from stdin")
    source.add_argument("--from-clipboard", action="store_true")
    source.add_argument("--prompt", action="store_true", help="no-echo prompt (default)")
    inspect_parser = sub.add_parser("inspect")
    inspect_parser.add_argument("--path", required=True)
    delete_parser = sub.add_parser("delete")
    delete_parser.add_argument("--path", required=True)
    args = parser.parse_args(argv)

    try:
        if args.command == "save":
            if args.stdin:
                raw = sys.stdin.read()
            elif args.from_clipboard:
                raw = read_clipboard()
            else:
                raw = getpass.getpass("Wealthlog PAT (input hidden): ")
            secret = normalize(raw)
            save(Path(args.path), secret)
            if args.from_clipboard:
                clear_clipboard()
            print(
                json_line(
                    {
                        "ok": True,
                        "path": args.path,
                        "mode": "0600",
                        "clipboardCleared": bool(args.from_clipboard),
                    }
                )
            )
        elif args.command == "inspect":
            print(json_line(inspect(Path(args.path))))
        else:
            Path(args.path).unlink()
            clear_clipboard()
            print(json_line({"ok": True, "deleted": args.path}))
        return 0
    except (StoreError, OSError) as error:
        print(json_line({"ok": False, "error": {"code": "credential_store", "message": str(error)}}))
        return 2


def json_line(value: dict) -> str:
    import json

    return json.dumps(value, ensure_ascii=False, sort_keys=True)


if __name__ == "__main__":
    sys.exit(main())
