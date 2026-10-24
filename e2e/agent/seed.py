#!/usr/bin/env python3
"""Create the synthetic A01/A11 data through the real ordinary-create tool.

Uses the preseed credential from the supplied config. Each synthetic row gets
a fresh requestKey; the manifest records IDs and keys for verification and
cleanup. Never touches the GUI database directly.
"""

import argparse
import json
import sys
import uuid
from datetime import date, timedelta
from pathlib import Path
from typing import Any, Dict, List

from common import HarnessError, emit, load_config, run_cli


def create(config: Dict[str, Any], account_id: str, payload: Dict[str, Any]) -> Dict[str, Any]:
    body = dict(payload)
    body["requestKey"] = str(uuid.uuid4())
    body["accountId"] = account_id
    response = run_cli(config, ["ordinary", "create", "--input", "-"], stdin_json=body)
    result = (response.get("data") or {}).get("result") or {}
    activity = result.get("activity") or {}
    return {
        "requestKey": body["requestKey"],
        "activityId": activity.get("id"),
        "version": result.get("version"),
        "replayed": result.get("replayed"),
        "activityType": body["activityType"],
        "amount": body["amount"],
        "activityDate": body["activityDate"],
    }


def main(argv: List[str]) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--config", required=True)
    parser.add_argument("--account-id", required=True)
    parser.add_argument("--day", required=True, help="D-1 as YYYY-MM-DD")
    parser.add_argument("--out", required=True)
    parser.add_argument("--skip-a11", action="store_true")
    args = parser.parse_args(argv)

    try:
        config = load_config(args.config)
        day = date.fromisoformat(args.day)
        previous = day - timedelta(days=1)
        created: List[Dict[str, Any]] = []
        rows = [
            {"activityType": "WITHDRAWAL", "activityDate": "%sT00:10:00+08:00" % day, "currency": "CNY", "amount": "12.30", "notes": "A01 清晨"},
            {"activityType": "WITHDRAWAL", "activityDate": day.isoformat(), "currency": "CNY", "amount": "35.00", "notes": "A01 午餐"},
            {"activityType": "WITHDRAWAL", "activityDate": "%sT23:50:00+08:00" % day, "currency": "CNY", "amount": "7.70", "notes": "A01 深夜"},
            {"activityType": "DEPOSIT", "activityDate": day.isoformat(), "currency": "CNY", "amount": "100.00", "notes": "A01 存入"},
            {"activityType": "WITHDRAWAL", "activityDate": previous.isoformat(), "currency": "CNY", "amount": "99.00", "notes": "A01 边界"},
        ]
        for row in rows:
            created.append(create(config, args.account_id, row))
        if not args.skip_a11:
            created.append(
                create(
                    config,
                    args.account_id,
                    {
                        "activityType": "WITHDRAWAL",
                        "activityDate": previous.isoformat(),
                        "currency": "CNY",
                        "amount": "11.11",
                        "notes": "A11 preseed duplicate target",
                    },
                )
            )
        value = {
            "ok": True,
            "accountId": args.account_id,
            "day": day.isoformat(),
            "rows": created,
        }
        Path(args.out).parent.mkdir(parents=True, exist_ok=True)
        Path(args.out).write_text(
            json.dumps(value, ensure_ascii=False, indent=2) + "\n", encoding="utf-8"
        )
        emit({"ok": True, "out": args.out, "created": len(created)})
        return 0
    except HarnessError as error:
        emit({"ok": False, "error": {"code": error.code, "message": error.message}})
        return error.exit_code
    except (OSError, ValueError) as error:
        emit({"ok": False, "error": {"code": "seed", "message": str(error)}})
        return 2


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
