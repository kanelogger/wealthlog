#!/usr/bin/env python3
"""Regression tests for the Wealthlog Skill scripts (Python 3.9 stdlib only)."""

import json
import os
import stat
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[3]
DATE_SCRIPT = ROOT / "skills" / "wealthlog" / "scripts" / "wealthlog_date.py"
SNAPSHOT_SCRIPT = ROOT / "skills" / "wealthlog" / "scripts" / "wealthlog_snapshot.py"
PYTHON = sys.executable
sys.path.insert(0, str(SNAPSHOT_SCRIPT.parent))
import wealthlog_snapshot as snapshot_script


class SnapshotCredentialParityTest(unittest.TestCase):
    def test_normalized_bytes_and_file_checks_match_cli_contract(self):
        with tempfile.TemporaryDirectory() as directory:
            token = Path(directory) / "token"
            token.touch(mode=0o600)
            hashes = []
            for raw in (b"wfp_test", b"wfp_test\n", b"wfp_test\r\n"):
                token.write_bytes(raw)
                value = snapshot_script.read_token(token)
                hashes.append(snapshot_script.sha256_bytes(value.encode("utf-8")))
            self.assertEqual(len(set(hashes)), 1)
            for raw in (b"", b" wfp_test", b"wfp_test ", b"wfp\ttest", b"wfp_test\n\n",
                        b"wfp_test\r", b"x" * 4097, b"\xff"):
                token.write_bytes(raw)
                with self.subTest(raw=raw[:20]), self.assertRaises(snapshot_script.SnapshotError):
                    snapshot_script.read_token(token)
            token.write_bytes(b"wfp_test")
            link = Path(directory) / "link"
            link.symlink_to(token)
            with self.assertRaises(snapshot_script.SnapshotError):
                snapshot_script.read_token(link)
            fifo = Path(directory) / "fifo"
            os.mkfifo(fifo, 0o600)
            with self.assertRaises(snapshot_script.SnapshotError):
                snapshot_script.read_token(fifo)
            token.chmod(0o400)
            self.assertEqual(snapshot_script.read_token(token), "wfp_test")


def run(script, *args, stdin=None):
    cwd = None
    if script == SNAPSHOT_SCRIPT and "--state-dir" in args:
        cwd = str(Path(args[args.index("--state-dir") + 1]).parent)
    completed = subprocess.run(
        [PYTHON, str(script), *args],
        check=False,
        input=stdin,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
        cwd=cwd,
    )
    payload = json.loads(completed.stdout) if completed.stdout.strip() else {}
    return completed.returncode, payload


class DateScriptTest(unittest.TestCase):
    def test_resolve_yesterday_uses_received_instant(self):
        code, payload = run(
            DATE_SCRIPT,
            "resolve",
            "--received-at",
            "2026-10-21T00:30:00+08:00",
            "--timezone",
            "Asia/Shanghai",
            "--expr",
            "yesterday",
        )
        self.assertEqual(code, 0)
        self.assertTrue(payload["ok"])
        self.assertEqual(payload["day"], "2026-10-20")
        self.assertEqual(payload["startUtc"], "2026-10-19T16:00:00+00:00")
        self.assertEqual(payload["endUtcExclusive"], "2026-10-20T16:00:00+00:00")

    def test_filter_uses_local_day_and_withdrawals_only(self):
        rows = {
            "rows": [
                {"activityType": "WITHDRAWAL", "activityDate": "2026-10-20", "amount": "10"},
                {
                    "activityType": "WITHDRAWAL",
                    "activityDate": "2026-10-20T00:00:00+08:00",
                    "amount": "2.5",
                },
                {
                    "activityType": "WITHDRAWAL",
                    "activityDate": "2026-10-19T16:00:00Z",
                    "amount": "0.5",
                },
                {
                    "activityType": "WITHDRAWAL",
                    "activityDate": "2026-10-20T23:59:59+08:00",
                    "amount": "1",
                },
                {
                    "activityType": "WITHDRAWAL",
                    "activityDate": "2026-10-21T00:00:00+08:00",
                    "amount": "99",
                },
                {"activityType": "DEPOSIT", "activityDate": "2026-10-20", "amount": "99"},
            ]
        }
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "rows.json"
            path.write_text(json.dumps(rows), encoding="utf-8")
            code, payload = run(
                DATE_SCRIPT,
                "filter",
                "--input",
                str(path),
                "--date",
                "2026-10-20",
                "--timezone",
                "Asia/Shanghai",
            )
        self.assertEqual(code, 0)
        self.assertEqual(payload["count"], 4)
        self.assertEqual(payload["total"], "14.0")
        self.assertEqual(payload["currency"], "UNKNOWN")

    def test_filter_keeps_currencies_apart(self):
        rows = {
            "rows": [
                {
                    "activityType": "WITHDRAWAL",
                    "activityDate": "2026-10-20",
                    "amount": "10",
                    "currency": "CNY",
                },
                {
                    "activityType": "WITHDRAWAL",
                    "activityDate": "2026-10-20",
                    "amount": "3",
                    "currency": "USD",
                },
            ]
        }
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "rows.json"
            path.write_text(json.dumps(rows), encoding="utf-8")
            code, payload = run(
                DATE_SCRIPT,
                "filter",
                "--input",
                str(path),
                "--date",
                "2026-10-20",
                "--timezone",
                "Asia/Shanghai",
            )
        self.assertEqual(code, 0)
        self.assertEqual(payload["count"], 2)
        self.assertIsNone(payload["total"])
        self.assertTrue(payload["mixedCurrencies"])
        self.assertEqual(payload["totalsByCurrency"], {"CNY": "10", "USD": "3"})

    def test_filter_rejects_unparsable_dates(self):
        rows = {
            "rows": [
                {
                    "activityType": "WITHDRAWAL",
                    "activityDate": "2026-10-20T00:00:00.123456789+08:00",
                    "amount": "10",
                    "currency": "CNY",
                }
            ]
        }
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "rows.json"
            path.write_text(json.dumps(rows), encoding="utf-8")
            code, payload = run(
                DATE_SCRIPT,
                "filter",
                "--input",
                str(path),
                "--date",
                "2026-10-20",
                "--timezone",
                "Asia/Shanghai",
            )
        self.assertEqual(code, 2)
        self.assertFalse(payload["ok"])
        self.assertEqual(payload["error"]["code"], "invalid_response")

    def test_query_reads_all_pages_and_filters(self):
        with tempfile.TemporaryDirectory() as directory:
            fake = Path(directory) / "fake-wealthlog"
            fake.write_text(
                """#!/usr/bin/env python3
import json, sys
pages = {
  "0": [{"activityType":"WITHDRAWAL","activityDate":"2026-10-20","amount":"1"},
        {"activityType":"DEPOSIT","activityDate":"2026-10-20","amount":"50"}],
  "1": [{"activityType":"WITHDRAWAL","activityDate":"2026-10-20T12:00:00+08:00","amount":"2"}],
}
page = sys.argv[sys.argv.index("--page") + 1]
rows = pages.get(page, [])
print(json.dumps({"ok":True,"data":{"page":{"rows":rows,"totalCount":3,"page":int(page),"pageSize":2}}}))
""",
                encoding="utf-8",
            )
            fake.chmod(0o700)
            code, payload = run(
                DATE_SCRIPT,
                "query",
                "--cli",
                str(fake),
                "--app-id",
                "local.kanelogger.wealthlog.dev",
                "--ledger",
                "00000000-0000-0000-0000-000000000001",
                "--timezone",
                "Asia/Shanghai",
                "--received-at",
                "2026-10-21T09:00:00+08:00",
                "--expr",
                "yesterday",
                "--page-size",
                "2",
            )
        self.assertEqual(code, 0)
        self.assertEqual(payload["count"], 2)
        self.assertEqual(payload["total"], "3")

    def test_query_trusts_server_page_size_over_request(self):
        """A clamped page size must not end pagination after the first page."""
        with tempfile.TemporaryDirectory() as directory:
            fake = Path(directory) / "fake-wealthlog"
            fake.write_text(
                """#!/usr/bin/env python3
import json, sys
pages = {
  "0": [{"activityType":"WITHDRAWAL","activityDate":"2026-10-20","amount":"1","currency":"CNY"}],
  "1": [{"activityType":"WITHDRAWAL","activityDate":"2026-10-20","amount":"2","currency":"CNY"}],
  "2": [],
}
page = sys.argv[sys.argv.index("--page") + 1]
rows = pages.get(page, [])
print(json.dumps({"ok":True,"data":{"page":{"rows":rows,"totalCount":2,"page":int(page),"pageSize":1}}}))
""",
                encoding="utf-8",
            )
            fake.chmod(0o700)
            code, payload = run(
                DATE_SCRIPT,
                "query",
                "--cli",
                str(fake),
                "--app-id",
                "local.kanelogger.wealthlog.dev",
                "--ledger",
                "00000000-0000-0000-0000-000000000001",
                "--timezone",
                "Asia/Shanghai",
                "--received-at",
                "2026-10-21T09:00:00+08:00",
                "--expr",
                "yesterday",
                "--page-size",
                "500",
            )
        self.assertEqual(code, 0)
        self.assertEqual(payload["count"], 2)
        self.assertEqual(payload["total"], "3")


class SnapshotScriptTest(unittest.TestCase):
    def test_state_outside_workspace_or_different_from_environment_is_rejected(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            workspace = root / "workspace"
            workspace.mkdir()
            token = self.make_token(directory)
            source = root / "input.json"
            source.write_text("{}")
            for state, configured in ((root / "outside", None),
                                      (workspace / "state", workspace / "other")):
                environment = os.environ.copy()
                environment.pop("WEALTHLOG_AGENT_STATE_DIR", None)
                if configured:
                    environment["WEALTHLOG_AGENT_STATE_DIR"] = str(configured)
                completed = subprocess.run(
                    [PYTHON, str(SNAPSHOT_SCRIPT), "create", "--state-dir", str(state),
                     "--app-id", "local.kanelogger.wealthlog.dev", "--ledger", "ledger-a",
                     "--operation", "ordinary_create", "--credential-file", str(token),
                     "--received-at", "2026-10-22T08:00:00+08:00", "--input", str(source)],
                    cwd=str(workspace), env=environment, stdout=subprocess.PIPE,
                    stderr=subprocess.PIPE, text=True,
                )
                self.assertNotEqual(completed.returncode, 0)
                self.assertEqual(json.loads(completed.stdout)["error"]["code"], "state_unwritable")
                self.assertFalse(state.exists())

    def make_token(self, directory, mode=0o600):
        path = Path(directory) / "token"
        path.write_text("wfp_test_token\n", encoding="utf-8")
        path.chmod(mode)
        return path

    def test_create_verify_mark_and_list(self):
        with tempfile.TemporaryDirectory() as directory:
            token = self.make_token(directory)
            state = Path(directory) / "state"
            request_input = Path(directory) / "input.json"
            request_input.write_text(
                json.dumps({"activityType": "WITHDRAWAL", "amount": "1"}), encoding="utf-8"
            )
            code, created = run(
                SNAPSHOT_SCRIPT,
                "create",
                "--state-dir",
                str(state),
                "--app-id",
                "local.kanelogger.wealthlog.dev",
                "--ledger",
                "00000000-0000-0000-0000-000000000001",
                "--operation",
                "ordinary_create",
                "--credential-file",
                str(token),
                "--received-at",
                "2026-10-21T09:00:00+08:00",
                "--resolved-date",
                "2026-10-20",
                "--summary",
                "test",
                "--input",
                str(request_input),
            )
            self.assertEqual(code, 0, created)
            self.assertTrue(created["ok"])
            request_dir = Path(created["requestDir"])
            self.assertEqual(stat.S_IMODE(os.stat(request_dir).st_mode) & 0o777, 0o700)
            self.assertEqual(
                stat.S_IMODE(os.stat(request_dir / "request.json").st_mode) & 0o777,
                0o600,
            )
            self.assertNotIn("wfp_test_token", (request_dir / "context.json").read_text())

            code, verified = run(
                SNAPSHOT_SCRIPT, "verify", "--request-dir", str(request_dir)
            )
            self.assertEqual(code, 0)
            self.assertTrue(verified["ok"])
            self.assertEqual(verified["status"], "pending")

            alternate = Path(directory) / "other-token"
            alternate.write_text("wfp_other_subject")
            alternate.chmod(0o600)
            code, verified = run(SNAPSHOT_SCRIPT, "verify", "--request-dir", str(request_dir),
                                 "--credential-file", str(alternate))
            self.assertNotEqual(code, 0)
            self.assertIn("credentialSha256", verified["problems"])
            code, verified = run(SNAPSHOT_SCRIPT, "verify", "--request-dir", str(request_dir),
                                 "--ledger", "other-ledger", "--app-id", "other-app",
                                 "--operation", "ordinary_update")
            self.assertNotEqual(code, 0)
            self.assertTrue({"ledgerId", "appId", "operation"}.issubset(verified["problems"]))

            request_dir.joinpath("request.json").write_text(
                json.dumps(
                    {
                        "activityType": "WITHDRAWAL",
                        "amount": "2",
                        "requestKey": created["requestKey"],
                    }
                ),
                encoding="utf-8",
            )
            code, verified = run(
                SNAPSHOT_SCRIPT, "verify", "--request-dir", str(request_dir)
            )
            self.assertNotEqual(code, 0)
            self.assertFalse(verified["ok"])
            self.assertIn("requestSha256", verified["problems"])

            code, marked = run(
                SNAPSHOT_SCRIPT,
                "mark",
                "--request-dir",
                str(request_dir),
                "--status",
                "unknown",
            )
            self.assertEqual(code, 0)
            self.assertEqual(marked["status"], "unknown")

            code, listed = run(
                SNAPSHOT_SCRIPT, "list", "--state-dir", str(state)
            )
            self.assertEqual(code, 0)
            self.assertEqual(len(listed["items"]), 1)
            self.assertEqual(listed["items"][0]["status"], "unknown")

    def test_rejects_non_private_token(self):
        with tempfile.TemporaryDirectory() as directory:
            token = self.make_token(directory, mode=0o644)
            request_input = Path(directory) / "input.json"
            request_input.write_text("{}", encoding="utf-8")
            code, payload = run(
                SNAPSHOT_SCRIPT,
                "create",
                "--state-dir",
                str(Path(directory) / "state"),
                "--app-id",
                "local.kanelogger.wealthlog.dev",
                "--ledger",
                "00000000-0000-0000-0000-000000000001",
                "--operation",
                "pending_create",
                "--credential-file",
                str(token),
                "--received-at",
                "2026-10-21T09:00:00+08:00",
                "--input",
                str(request_input),
            )
            self.assertNotEqual(code, 0)
            self.assertFalse(payload["ok"])
            self.assertEqual(payload["error"]["code"], "invalid_credential")

    def test_rejects_input_with_request_key(self):
        with tempfile.TemporaryDirectory() as directory:
            token = self.make_token(directory)
            request_input = Path(directory) / "input.json"
            request_input.write_text(
                json.dumps({"requestKey": "fixed", "activityType": "WITHDRAWAL"}),
                encoding="utf-8",
            )
            code, payload = run(
                SNAPSHOT_SCRIPT,
                "create",
                "--state-dir",
                str(Path(directory) / "state"),
                "--app-id",
                "local.kanelogger.wealthlog.dev",
                "--ledger",
                "00000000-0000-0000-0000-000000000001",
                "--operation",
                "ordinary_create",
                "--credential-file",
                str(token),
                "--received-at",
                "2026-10-21T09:00:00+08:00",
                "--input",
                str(request_input),
            )
            self.assertNotEqual(code, 0)
            self.assertFalse(payload["ok"])
            self.assertEqual(payload["error"]["code"], "invalid_input")


if __name__ == "__main__":
    unittest.main()
