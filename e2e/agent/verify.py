#!/usr/bin/env python3
"""Read-only business baselines, diffs and audit inspection.

Uses the CLI for ordinary activities and pending entries, and opens the
profile database read-only for request/audit rows when `--db` is supplied.
Never writes business data and never prints a token.
"""

import argparse
import json
import sqlite3
import sys
from datetime import datetime, timezone
from pathlib import Path
from typing import Any, Dict, Iterable, List, Optional

from common import HarnessError, emit, load_config, run_cli, service_state

PAGE_SIZE = 200
FIELDS = ("accountId", "activityType", "activityDate", "currency", "amount", "fee", "tax",
          "fxRate", "taxonomyId", "categoryId", "eventId", "notes", "status")


def all_ordinary(config: Dict[str, Any]) -> List[Dict[str, Any]]:
    rows: List[Dict[str, Any]] = []
    page = 0
    while True:
        payload = run_cli(
            config,
            ["ordinary", "list", "--page", str(page), "--page-size", str(PAGE_SIZE)],
        )
        page_data = (payload.get("data") or {}).get("page") or {}
        batch = page_data.get("rows") or []
        rows.extend(batch)
        page += 1
        total = int(page_data.get("totalCount") or 0)
        if not batch or total <= page * PAGE_SIZE:
            break
        if page > 10000:
            raise HarnessError("invalid_response", "pagination did not terminate", 5)
    return rows


def all_pending(config: Dict[str, Any]) -> List[Dict[str, Any]]:
    payload = run_cli(config, ["pending", "list", "--include-resolved"])
    data = payload.get("data") or {}
    pending = data.get("pending")
    return pending if isinstance(pending, list) else []


def snapshot(config: Dict[str, Any]) -> Dict[str, Any]:
    service = service_state(config)
    status = service.get("data") or {}
    active_ledger = service.get("ledgerId") or status.get("activeLedgerId")
    if status.get("state") != "ready" or active_ledger != config["ledgerId"]:
        raise HarnessError("profile_mismatch", "snapshot requires the ready, explicitly bound ledger")
    activities = all_ordinary(config)
    activities = [run_cli(config, ["ordinary", "get", row["id"]])["data"]["activity"]
                  for row in activities]
    pending = all_pending(config)
    return {
        "ok": True,
        "capturedAt": datetime.now(timezone.utc).isoformat(),
        "activeLedgerId": active_ledger,
        "serviceState": status.get("state"),
        "activities": [
            {
                "id": row.get("id"),
                "version": row.get("version"),
                "status": row.get("status"),
                **{key: row.get(key) for key in FIELDS},
            }
            for row in activities
        ],
        "pending": [
            {
                "id": row.get("id"),
                "status": row.get("status"),
                "version": row.get("version"),
                "activityType": row.get("activityType"),
                "activityDate": row.get("activityDate"),
                "currency": row.get("currency"),
                "amount": row.get("amount"),
                "accountId": row.get("accountId"),
                "createdBy": row.get("createdBy"),
                "confirmedBy": row.get("confirmedBy"),
                "committedActivityId": row.get("committedActivityId"),
            }
            for row in pending
        ],
    }


def indexed(rows: Iterable[Dict[str, Any]]) -> Dict[str, Dict[str, Any]]:
    result = {}
    for row in rows:
        if row.get("id"):
            result[str(row["id"])] = row
    return result


def diff(before: Dict[str, Any], after: Dict[str, Any]) -> Dict[str, Any]:
    old, new = indexed(before.get("activities") or []), indexed(after.get("activities") or [])
    old_pending, new_pending = indexed(before.get("pending") or []), indexed(after.get("pending") or [])
    changed = []
    for identifier in sorted(set(old) & set(new)):
        fields = {}
        for key in ("version", "status", *FIELDS):
            if old[identifier].get(key) != new[identifier].get(key):
                fields[key] = {"before": old[identifier].get(key), "after": new[identifier].get(key)}
        if fields:
            changed.append({"id": identifier, "fields": fields})
    pending_changed = []
    for identifier in sorted(set(old_pending) & set(new_pending)):
        fields = {}
        for key in ("status", "version", "activityType", "activityDate", "currency", "amount", "accountId"):
            if old_pending[identifier].get(key) != new_pending[identifier].get(key):
                fields[key] = {
                    "before": old_pending[identifier].get(key),
                    "after": new_pending[identifier].get(key),
                }
        if fields:
            pending_changed.append({"id": identifier, "fields": fields})
    return {
        "newActivities": [new[key] for key in sorted(set(new) - set(old))],
        "removedActivities": [old[key] for key in sorted(set(old) - set(new))],
        "changedActivities": changed,
        "newPending": [new_pending[key] for key in sorted(set(new_pending) - set(old_pending))],
        "removedPending": [old_pending[key] for key in sorted(set(old_pending) - set(new_pending))],
        "changedPending": pending_changed,
    }


def check(change: Dict[str, Any], expected_new: int, expected_pending: int,
          expected_changed: int = 0, expected_pending_changed: int = 0) -> bool:
    return (
        len(change["newActivities"]) == expected_new
        and len(change["newPending"]) == expected_pending
        and not change["removedActivities"]
        and not change["removedPending"]
        and len(change["changedActivities"]) == expected_changed
        and len(change["changedPending"]) == expected_pending_changed
    )


def audit(db_path: Path, since: str, request_keys: List[str]) -> Dict[str, Any]:
    uri = "file:%s?mode=ro" % db_path
    connection = sqlite3.connect(uri, uri=True)
    connection.row_factory = sqlite3.Row
    result: Dict[str, Any] = {"ok": True, "db": str(db_path), "since": since}
    try:
        rows = connection.execute(
            "SELECT id, ledger_id, subject, operation, request_key, state, "
            "result_json, result_json IS NOT NULL AS has_result, error_message, created_at "
            "FROM business_requests WHERE created_at >= ? ORDER BY created_at",
            (since,),
        ).fetchall()
        result["businessRequests"] = [dict(row) for row in rows]
        audit_rows = connection.execute(
            "SELECT id, ledger_id, channel, actor, authenticated, operation, entity_type, "
            "entity_id, request_id, outcome, created_at FROM business_audit "
            "WHERE created_at >= ? ORDER BY created_at",
            (since,),
        ).fetchall()
        result["businessAudit"] = [dict(row) for row in audit_rows]
        mcp_rows = connection.execute(
            "SELECT id, session_id, actor_kind, actor_fingerprint, tool, outcome, "
            "error_message, created_at, business_request_id FROM mcp_audit_log "
            "WHERE created_at >= ? ORDER BY created_at",
            (since,),
        ).fetchall()
        result["mcpAudit"] = [dict(row) for row in mcp_rows]
        if request_keys:
            placeholders = ",".join("?" for _ in request_keys)
            selected = connection.execute(
                "SELECT request_key, state, error_message, created_at FROM business_requests "
                "WHERE request_key IN (%s)" % placeholders,
                tuple(request_keys),
            ).fetchall()
            result["requestKeys"] = [dict(row) for row in selected]
    finally:
        connection.close()
    return result


def committed(db_path: Path, request_key: str) -> Dict[str, Any]:
    connection = sqlite3.connect("file:%s?mode=ro" % db_path, uri=True)
    try:
        rows = connection.execute(
            "SELECT id, state, result_json FROM business_requests WHERE request_key = ?",
            (request_key,),
        ).fetchall()
        if len(rows) != 1 or rows[0][1] != "completed":
            return {"ok": False, "requestKey": request_key}
        result = json.loads(rows[0][2])
        identifier = (result.get("activity") or {}).get("id")
        count = connection.execute("SELECT COUNT(*) FROM activities WHERE id = ?", (identifier,)).fetchone()[0]
        return {"ok": bool(identifier) and count == 1, "requestKey": request_key,
                "activityId": identifier, "rows": count}
    finally:
        connection.close()


def main(argv: Optional[List[str]] = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    sub = parser.add_subparsers(dest="command", required=True)

    snap = sub.add_parser("snapshot")
    snap.add_argument("--config", required=True)
    snap.add_argument("--out", required=True)

    dif = sub.add_parser("diff")
    dif.add_argument("--before", required=True)
    dif.add_argument("--after", required=True)
    dif.add_argument("--out")

    chk = sub.add_parser("check")
    chk.add_argument("--before", required=True)
    chk.add_argument("--after", required=True)
    chk.add_argument("--expect-new-activities", type=int, default=0)
    chk.add_argument("--expect-new-pending", type=int, default=0)
    chk.add_argument("--expect-changed-activities", type=int, default=0)
    chk.add_argument("--expect-changed-pending", type=int, default=0)

    aud = sub.add_parser("audit")
    aud.add_argument("--db", required=True)
    aud.add_argument("--since", required=True)
    aud.add_argument("--request-key", action="append")
    aud.add_argument("--out")
    commit = sub.add_parser("committed")
    commit.add_argument("--db", required=True)
    commit.add_argument("--request-key", required=True)
    args = parser.parse_args(argv)

    try:
        if args.command == "snapshot":
            value = snapshot(load_config(args.config))
            Path(args.out).parent.mkdir(parents=True, exist_ok=True)
            Path(args.out).write_text(
                json.dumps(value, ensure_ascii=False, indent=2) + "\n", encoding="utf-8"
            )
            emit({"ok": True, "out": args.out, "activities": len(value["activities"]), "pending": len(value["pending"])})
        elif args.command in ("diff", "check"):
            before = json.loads(Path(args.before).read_text(encoding="utf-8"))
            after = json.loads(Path(args.after).read_text(encoding="utf-8"))
            change = diff(before, after)
            if args.command == "diff":
                value: Dict[str, Any] = {"ok": True, **change}
                if args.out:
                    Path(args.out).write_text(
                        json.dumps(value, ensure_ascii=False, indent=2) + "\n", encoding="utf-8"
                    )
                emit(
                    {
                        "ok": True,
                        "newActivities": len(change["newActivities"]),
                        "changedActivities": len(change["changedActivities"]),
                        "newPending": len(change["newPending"]),
                        "changedPending": len(change["changedPending"]),
                    }
                )
                return 0
            ok = check(change, args.expect_new_activities, args.expect_new_pending,
                       args.expect_changed_activities, args.expect_changed_pending)
            emit(
                {
                    "ok": ok,
                    "newActivities": len(change["newActivities"]),
                    "changedActivities": len(change["changedActivities"]),
                    "newPending": len(change["newPending"]),
                    "changedPending": len(change["changedPending"]),
                    "change": change,
                }
            )
            return 0 if ok else 1
        elif args.command == "committed":
            value = committed(Path(args.db), args.request_key)
            emit(value)
            return 0 if value["ok"] else 1
        else:
            value = audit(Path(args.db), args.since, args.request_key or [])
            if args.out:
                Path(args.out).write_text(
                    json.dumps(value, ensure_ascii=False, indent=2) + "\n", encoding="utf-8"
                )
            emit(
                {
                    "ok": True,
                    "businessRequests": len(value["businessRequests"]),
                    "businessAudit": len(value["businessAudit"]),
                    "mcpAudit": len(value["mcpAudit"]),
                }
            )
        return 0
    except HarnessError as error:
        emit({"ok": False, "error": {"code": error.code, "message": error.message}})
        return error.exit_code
    except (OSError, ValueError, json.JSONDecodeError, sqlite3.Error) as error:
        emit({"ok": False, "error": {"code": "verify", "message": str(error)}})
        return 2


if __name__ == "__main__":
    sys.exit(main())
