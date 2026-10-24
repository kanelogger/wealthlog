#!/usr/bin/env python3
"""Track only the resources this acceptance run creates.

The manifest holds labels and paths, never credentials. It is written 0600 so
closeout can revoke, delete or report every item without scanning the host.
"""

import argparse
import json
import os
import sys
from datetime import datetime, timezone
from pathlib import Path
from typing import Any, Dict, List, Optional


class InventoryError(Exception):
    pass


def load(path: Path) -> Dict[str, Any]:
    if not path.exists():
        raise InventoryError("inventory does not exist; run init first")
    value = json.loads(path.read_text(encoding="utf-8"))
    if not isinstance(value, dict) or not isinstance(value.get("items"), list):
        raise InventoryError("inventory is not valid")
    return value


def save(path: Path, value: Dict[str, Any]) -> None:
    temporary = path.with_name(path.name + ".tmp")
    descriptor = os.open(temporary, os.O_WRONLY | os.O_CREAT | os.O_TRUNC, 0o600)
    with os.fdopen(descriptor, "w", encoding="utf-8") as handle:
        json.dump(value, handle, ensure_ascii=False, indent=2, sort_keys=True)
        handle.write("\n")
        handle.flush()
        os.fsync(handle.fileno())
    os.replace(temporary, path)
    os.chmod(path, 0o600)


def main(argv: Optional[List[str]] = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    sub = parser.add_subparsers(dest="command", required=True)
    init = sub.add_parser("init")
    init.add_argument("--path", required=True)
    init.add_argument("--run-id", required=True)
    init.add_argument("--app-id", required=True)
    init.add_argument("--ledger-a", required=True)
    init.add_argument("--ledger-b")
    add = sub.add_parser("add")
    add.add_argument("--path", required=True)
    add.add_argument("--kind", required=True)
    add.add_argument("--value", required=True)
    add.add_argument("--note", default="")
    listing = sub.add_parser("list")
    listing.add_argument("--path", required=True)
    close = sub.add_parser("close")
    close.add_argument("--path", required=True)
    args = parser.parse_args(argv)
    try:
        if args.command == "init":
            path = Path(args.path)
            path.parent.mkdir(parents=True, exist_ok=True)
            value = {
                "runId": args.run_id,
                "createdAt": datetime.now(timezone.utc).isoformat(),
                "appId": args.app_id,
                "ledgerA": args.ledger_a,
                "ledgerB": args.ledger_b,
                "status": "open",
                "items": [],
            }
            save(path, value)
            print(json.dumps({"ok": True, "path": str(path), "runId": args.run_id}))
        elif args.command == "add":
            path = Path(args.path)
            value = load(path)
            value["items"].append(
                {
                    "kind": args.kind,
                    "value": args.value,
                    "note": args.note,
                    "addedAt": datetime.now(timezone.utc).isoformat(),
                    "closed": False,
                }
            )
            save(path, value)
            print(json.dumps({"ok": True, "items": len(value["items"])}))
        elif args.command == "list":
            value = load(Path(args.path))
            print(json.dumps({"ok": True, **value}, ensure_ascii=False))
        else:
            path = Path(args.path)
            value = load(path)
            value["status"] = "closed"
            value["closedAt"] = datetime.now(timezone.utc).isoformat()
            save(path, value)
            print(json.dumps({"ok": True, "items": len(value["items"])}))
        return 0
    except (InventoryError, OSError, json.JSONDecodeError) as error:
        print(json.dumps({"ok": False, "error": {"code": "inventory", "message": str(error)}}))
        return 2


if __name__ == "__main__":
    sys.exit(main())
