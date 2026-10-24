#!/usr/bin/env python3
"""Resolve Wealthlog calendar dates and query ordinary spending.

Standard library only; compatible with Python 3.9. The script never prints a
token and never writes business data.
"""

import argparse
import json
import os
import subprocess
import sys
from datetime import date, datetime, time, timedelta, timezone
from decimal import Decimal, InvalidOperation
from typing import Any, Dict, Iterable, List, Optional, Tuple
from zoneinfo import ZoneInfo, ZoneInfoNotFoundError


class ScriptError(Exception):
    def __init__(self, code: str, message: str, exit_code: int = 2) -> None:
        super().__init__(message)
        self.code = code
        self.message = message
        self.exit_code = exit_code


MAX_PAGE_SIZE = 200


def emit(value: Dict[str, Any]) -> None:
    sys.stdout.write(json.dumps(value, ensure_ascii=False, sort_keys=True) + "\n")


def parse_instant(raw: str, tz: ZoneInfo) -> datetime:
    text = raw.strip()
    if not text:
        raise ScriptError("invalid_input", "--received-at must not be empty")
    normalized = text[:-1] + "+00:00" if text.endswith("Z") else text
    try:
        value = datetime.fromisoformat(normalized)
    except ValueError:
        raise ScriptError("invalid_input", "--received-at must be an ISO timestamp")
    if value.tzinfo is None:
        value = value.replace(tzinfo=tz)
    return value.astimezone(timezone.utc)


def resolve_day(expr: str, received_at: datetime, tz: ZoneInfo) -> date:
    local_day = received_at.astimezone(tz).date()
    normalized = expr.strip().lower()
    if normalized in ("today", "今天"):
        return local_day
    if normalized in ("yesterday", "昨天"):
        return local_day - timedelta(days=1)
    try:
        return date.fromisoformat(expr.strip())
    except ValueError:
        raise ScriptError(
            "invalid_input", "--expr must be today, yesterday or YYYY-MM-DD"
        )


def day_bounds(day: date, tz: ZoneInfo) -> Tuple[datetime, datetime]:
    start_local = datetime.combine(day, time.min, tzinfo=tz)
    end_local = datetime.combine(day + timedelta(days=1), time.min, tzinfo=tz)
    return start_local.astimezone(timezone.utc), end_local.astimezone(timezone.utc)


def row_instant(raw: str, tz: ZoneInfo) -> Optional[datetime]:
    text = (raw or "").strip()
    if not text:
        return None
    try:
        day = date.fromisoformat(text)
    except ValueError:
        pass
    else:
        # A bare date names a calendar day. Midnight in the configured zone is
        # close enough for filtering by local day; exact instants keep times.
        return datetime.combine(day, time.min, tzinfo=tz).astimezone(timezone.utc)
    normalized = text[:-1] + "+00:00" if text.endswith("Z") else text
    try:
        value = datetime.fromisoformat(normalized)
    except ValueError:
        # Python 3.9 rejects some valid RFC 3339 forms, such as 9-digit
        # fractional seconds. Failing loudly beats silently dropping a row.
        raise ScriptError(
            "invalid_response", "activity date is not a supported ISO timestamp: %s" % text
        )
    if value.tzinfo is None:
        value = value.replace(tzinfo=tz)
    return value.astimezone(timezone.utc)


def filter_rows(
    rows: Iterable[Dict[str, Any]], day: date, tz: ZoneInfo
) -> Tuple[List[Dict[str, Any]], Dict[str, Decimal]]:
    start, end = day_bounds(day, tz)
    selected: List[Dict[str, Any]] = []
    totals: Dict[str, Decimal] = {}
    for row in rows:
        if row.get("activityType") != "WITHDRAWAL":
            continue
        instant = row_instant(str(row.get("activityDate") or ""), tz)
        if instant is None:
            continue
        if instant.astimezone(tz).date() != day:
            continue
        if not (start <= instant < end):
            continue
        selected.append(row)
        amount = row.get("amount")
        if amount is not None:
            currency = str(row.get("currency") or "").strip() or "UNKNOWN"
            try:
                totals[currency] = totals.get(currency, Decimal("0")) + Decimal(str(amount))
            except InvalidOperation:
                raise ScriptError(
                    "invalid_response", "activity amount is not a decimal string"
                )
    return selected, totals


def totals_payload(totals: Dict[str, Decimal]) -> Dict[str, Any]:
    """Amounts are only comparable within one currency."""
    currencies = sorted(totals)
    payload: Dict[str, Any] = {
        "currencies": currencies,
        "totalsByCurrency": {key: format(value, "f") for key, value in sorted(totals.items())},
    }
    if len(currencies) == 1:
        payload["currency"] = currencies[0]
        payload["total"] = format(totals[currencies[0]], "f")
    else:
        payload["currency"] = None
        payload["total"] = None
        payload["mixedCurrencies"] = True
    return payload


def run_cli(args: List[str]) -> Dict[str, Any]:
    env = os.environ.copy()
    env.pop("WEALTHLOG_TOKEN", None)
    try:
        completed = subprocess.run(
            args,
            check=False,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            env=env,
            timeout=60,
        )
    except FileNotFoundError:
        raise ScriptError("cli_missing", "CLI executable not found", 5)
    except subprocess.TimeoutExpired:
        raise ScriptError("result_unknown", "CLI timed out; verify before retrying", 5)
    raw = completed.stdout.decode("utf-8", "replace").strip()
    try:
        payload = json.loads(raw) if raw else {}
    except json.JSONDecodeError:
        raise ScriptError(
            "invalid_response", "CLI did not print one JSON object", 5
        )
    if not isinstance(payload, dict):
        raise ScriptError("invalid_response", "CLI output must be a JSON object", 5)
    if completed.returncode != 0 or not payload.get("ok"):
        error = payload.get("error") or {}
        raise ScriptError(
            str(error.get("code") or "cli_error"),
            str(error.get("message") or "CLI call failed"),
            completed.returncode if completed.returncode else 5,
        )
    return payload


def base_cli(args: argparse.Namespace) -> List[str]:
    command = [args.cli, "--app-id", args.app_id]
    if args.token_file:
        command.extend(["--token-file", args.token_file])
    if args.ledger:
        command.extend(["--ledger", args.ledger])
    return command


def query_rows(args: argparse.Namespace) -> Dict[str, Any]:
    tz = ZoneInfo(args.timezone)
    received = parse_instant(args.received_at, tz)
    day = resolve_day(args.expr, received, tz)
    start, end = day_bounds(day, tz)
    query_from = (day - timedelta(days=1)).isoformat()
    query_to = (day + timedelta(days=1)).isoformat()

    rows: List[Dict[str, Any]] = []
    page = 0
    total_count = None
    while True:
        command = base_cli(args) + [
            "ordinary",
            "list",
            "--page",
            str(page),
            "--page-size",
            str(min(int(args.page_size), MAX_PAGE_SIZE)),
            "--date-from",
            query_from,
            "--date-to",
            query_to,
        ]
        for account_id in args.account_id or []:
            command.extend(["--account-id", account_id])
        payload = run_cli(command)
        page_data = (payload.get("data") or {}).get("page") or {}
        batch = page_data.get("rows") or []
        if total_count is None:
            total_count = int(page_data.get("totalCount") or 0)
        # The server clamps page size; trust the response, not the request.
        effective_page_size = int(page_data.get("pageSize") or 0) or min(
            int(args.page_size), MAX_PAGE_SIZE
        )
        rows.extend(batch)
        page += 1
        if not batch:
            break
        if total_count <= page * effective_page_size:
            break
        if page > 1000:
            raise ScriptError("invalid_response", "pagination did not terminate", 5)

    selected, totals = filter_rows(rows, day, tz)
    return {
        "ok": True,
        "day": day.isoformat(),
        "timezone": args.timezone,
        "startUtc": start.isoformat(),
        "endUtcExclusive": end.isoformat(),
        "queryFrom": query_from,
        "queryTo": query_to,
        "count": len(selected),
        **totals_payload(totals),
        "rows": selected,
    }


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description=__doc__)
    sub = parser.add_subparsers(dest="command", required=True)

    resolve = sub.add_parser("resolve", help="Resolve a relative date")
    resolve.add_argument("--received-at", required=True)
    resolve.add_argument("--timezone", required=True)
    resolve.add_argument("--expr", required=True)

    query = sub.add_parser("query", help="Query and filter ordinary spending")
    query.add_argument("--cli", required=True)
    query.add_argument("--app-id", required=True)
    query.add_argument("--ledger")
    query.add_argument("--token-file")
    query.add_argument("--timezone", required=True)
    query.add_argument("--received-at", required=True)
    query.add_argument("--expr", required=True)
    query.add_argument("--account-id", action="append")
    query.add_argument("--page-size", type=int, default=200)

    filtr = sub.add_parser("filter", help="Filter an existing rows payload")
    filtr.add_argument("--input", required=True)
    filtr.add_argument("--date", required=True)
    filtr.add_argument("--timezone", required=True)
    return parser


def main(argv: Optional[List[str]] = None) -> int:
    args = build_parser().parse_args(argv)
    try:
        if args.command == "resolve":
            tz = ZoneInfo(args.timezone)
            received = parse_instant(args.received_at, tz)
            day = resolve_day(args.expr, received, tz)
            start, end = day_bounds(day, tz)
            emit(
                {
                    "ok": True,
                    "day": day.isoformat(),
                    "timezone": args.timezone,
                    "startUtc": start.isoformat(),
                    "endUtcExclusive": end.isoformat(),
                    "queryFrom": (day - timedelta(days=1)).isoformat(),
                    "queryTo": (day + timedelta(days=1)).isoformat(),
                }
            )
        elif args.command == "query":
            emit(query_rows(args))
        else:
            tz = ZoneInfo(args.timezone)
            with open(args.input, "r", encoding="utf-8") as handle:
                payload = json.load(handle)
            raw_rows = payload.get("rows") if isinstance(payload, dict) else payload
            selected, totals = filter_rows(raw_rows or [], date.fromisoformat(args.date), tz)
            emit(
                {
                    "ok": True,
                    "day": args.date,
                    "count": len(selected),
                    **totals_payload(totals),
                    "rows": selected,
                }
            )
        return 0
    except ScriptError as error:
        emit({"ok": False, "error": {"code": error.code, "message": error.message}})
        return error.exit_code
    except ZoneInfoNotFoundError:
        emit({"ok": False, "error": {"code": "invalid_timezone", "message": "invalid timezone"}})
        return 2
    except (OSError, ValueError, json.JSONDecodeError) as error:
        emit({"ok": False, "error": {"code": "invalid_input", "message": str(error)}})
        return 2


if __name__ == "__main__":
    sys.exit(main())
