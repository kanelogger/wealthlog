#!/usr/bin/env python3
"""Regression tests for the acceptance harness programs."""

import json
import os
import stat
import subprocess
import sys
import tempfile
import time
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[3]
AGENT = ROOT / "e2e" / "agent"
PYTHON = sys.executable
sys.path.insert(0, str(AGENT))
import verify


def run(script, *args, stdin=None, env=None):
    completed = subprocess.run(
        [PYTHON, str(AGENT / script), *args],
        check=False,
        input=stdin,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
        env=env,
    )
    payload = json.loads(completed.stdout) if completed.stdout.strip() else {}
    return completed.returncode, payload


def write_executable(path, content):
    path.write_text(content, encoding="utf-8")
    path.chmod(0o700)


class CredentialStoreTest(unittest.TestCase):
    @unittest.skipUnless(sys.platform == "darwin", "macOS clipboard integration")
    def test_failed_clipboard_cleanup_does_not_report_success(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            target = root / "private" / "token"
            write_executable(root / "pbpaste", "#!/bin/sh\nprintf wfp_clipboard_fixture\n")
            write_executable(root / "pbcopy", "#!/bin/sh\nexit 1\n")
            environment = os.environ.copy()
            environment["PATH"] = str(root) + os.pathsep + environment["PATH"]
            code, payload = run("credential_store.py", "save", "--path", str(target),
                                "--from-clipboard", env=environment)
            self.assertEqual(code, 2)
            self.assertFalse(payload["ok"])
            self.assertEqual(target.read_text(), "wfp_clipboard_fixture")
            self.assertNotIn("wfp_clipboard_fixture", json.dumps(payload))

    def test_saves_private_file_and_refuses_overwrite(self):
        with tempfile.TemporaryDirectory() as directory:
            target = Path(directory) / "private" / "token"
            code, payload = run(
                "credential_store.py",
                "save",
                "--path",
                str(target),
                "--stdin",
                stdin="wfp_test_token\n",
            )
            self.assertEqual(code, 0, payload)
            self.assertEqual(stat.S_IMODE(target.stat().st_mode) & 0o777, 0o600)
            self.assertEqual(target.read_text(), "wfp_test_token")
            code, payload = run(
                "credential_store.py",
                "save",
                "--path",
                str(target),
                "--stdin",
                stdin="another\n",
            )
            self.assertNotEqual(code, 0)
            self.assertIn("overwrite", payload["error"]["message"])
            self.assertEqual(target.read_text(), "wfp_test_token")


class DeliverableScanTest(unittest.TestCase):
    def test_scans_build_named_directories_and_nested_json_strings(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory) / "deliverable"
            root.mkdir()
            secret = "wfp_nested_secret_123"
            token = Path(directory) / "token"
            token.write_text(secret)
            token.chmod(0o600)
            for name in ("dist", "target", "node_modules", ".git"):
                target = root / name
                target.mkdir()
                evidence = target / "evidence.json"
                evidence.write_text(json.dumps({"payload": json.dumps(secret).replace("w", "\\u0077")}))
                code, result = run("deliverable_scan.py", "--root", str(root),
                                   "--secret", "T1=" + str(token))
                self.assertEqual(code, 1)
                self.assertEqual(result["hits"], [{"label": "T1", "path": str(evidence.resolve()), "count": 1}])
                self.assertEqual(result["skipped"], 0)
                self.assertNotIn(secret, json.dumps(result))
                evidence.unlink()

    def test_rejects_unscannable_entries_instead_of_reporting_clean(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory) / "deliverable"
            root.mkdir()
            (root / "report.md").write_text("safe")
            token = Path(directory) / "token"
            token.write_text("wfp_test_credential_123")
            token.chmod(0o600)
            broken = root / "missing-evidence"
            broken.symlink_to(root / "absent")
            code, result = run("deliverable_scan.py", "--root", str(root),
                               "--secret", "T1=" + str(token))
            self.assertEqual(code, 2)
            self.assertFalse(result["ok"])
            self.assertEqual(result["error"]["code"], "scan")

    def test_only_registered_credential_files_can_be_exempted(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            token = root / "token"
            token.write_text("wfp_exemption_secret_123")
            token.chmod(0o600)
            evidence = root / "report.md"
            evidence.write_text(token.read_text())
            code, result = run("deliverable_scan.py", "--root", str(root),
                               "--secret", "T1=" + str(token), "--allow-file", str(evidence))
            self.assertEqual(code, 2)
            self.assertFalse(result["ok"])
            code, result = run("deliverable_scan.py", "--root", str(root),
                               "--secret", "T1=" + str(token), "--allow-file", str(token))
            self.assertEqual(code, 1)
            self.assertEqual(result["hits"][0]["path"], str(evidence.resolve()))

    def test_fails_closed_and_checks_escaped_json(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory) / "deliverable"
            root.mkdir()
            secret = "wfp_escaped_secret_123"
            token_file = Path(directory) / "token"
            token_file.write_text(secret)
            token_file.chmod(0o600)
            (root / "event.json").write_text('{"text":"\\u0077fp_escaped_secret_123"}')
            code, result = run("deliverable_scan.py", "--root", str(root),
                               "--secret", "T1=" + str(token_file))
            self.assertEqual(code, 1)
            self.assertEqual(result["hits"][0]["label"], "T1")
            code, _ = run("deliverable_scan.py", "--root", str(root / "absent"),
                          "--secret", "T1=" + str(token_file))
            self.assertNotEqual(code, 0)
            code, result = run("deliverable_scan.py", "--root", str(root))
            self.assertNotEqual(code, 0)

    def test_self_test_and_detects_secret(self):
        code, payload = run("deliverable_scan.py", "--self-test")
        self.assertEqual(code, 0, payload)
        self.assertTrue(payload["ok"])
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory) / "deliverable"
            root.mkdir()
            secret = "wfp_deliverable_secret_123"
            token_file = Path(directory) / "token"
            token_file.write_text(secret, encoding="utf-8")
            token_file.chmod(0o600)
            (root / "report.md").write_text("token=" + secret, encoding="utf-8")
            code, payload = run(
                "deliverable_scan.py",
                "--root",
                str(root),
                "--secret",
                "token=%s" % token_file,
            )
            self.assertNotEqual(code, 0)
            self.assertEqual(len(payload["hits"]), 1)
            self.assertEqual(payload["hits"][0]["label"], "token")
            self.assertNotIn(secret, json.dumps(payload))


class InventoryTest(unittest.TestCase):
    def test_init_add_and_close(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "inventory.json"
            code, payload = run(
                "resource_inventory.py",
                "init",
                "--path",
                str(path),
                "--run-id",
                "run-1",
                "--app-id",
                "local.kanelogger.wealthlog.dev",
                "--ledger-a",
                "00000000-0000-0000-0000-000000000001",
            )
            self.assertEqual(code, 0, payload)
            code, payload = run(
                "resource_inventory.py",
                "add",
                "--path",
                str(path),
                "--kind",
                "token",
                "--value",
                "T1",
                "--note",
                "read-only",
            )
            self.assertEqual(code, 0, payload)
            code, payload = run("resource_inventory.py", "close", "--path", str(path))
            self.assertEqual(code, 0, payload)
            value = json.loads(path.read_text(encoding="utf-8"))
            self.assertEqual(value["status"], "closed")
            self.assertEqual(len(value["items"]), 1)
            self.assertEqual(stat.S_IMODE(path.stat().st_mode) & 0o777, 0o600)


class FaultCliTest(unittest.TestCase):
    def test_pause_timeout_is_an_environment_failure_and_never_sends(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            marker = root / "called"
            real = root / "real"
            write_executable(real, "#!%s\nfrom pathlib import Path\nPath(%r).touch()\n" % (PYTHON, str(marker)))
            request = root / "request.json"
            request.write_text(json.dumps({"requestKey": "key-1"}))
            args = ["--real-cli", str(real), "--state", str(root / "state"), "--mode", "pause",
                    "--match-request-key", "key-1", "--timeout", "0.1", "--", "ordinary",
                    "update", "--input", str(request)]
            code, value = run("fault_cli.py", *args)
            self.assertEqual(code, 5)
            self.assertTrue(value["environmentFailure"])
            self.assertFalse(value["requestSent"])
            self.assertEqual(value["error"]["code"], "acceptance_pause_timeout")
            self.assertFalse(marker.exists())

    def fake_cli(self, directory):
        path = Path(directory) / "real-wealthlog"
        write_executable(
            path,
            """#!/usr/bin/env python3
import json, sys
input_path = sys.argv[sys.argv.index("--input") + 1]
payload = json.load(open(input_path))
key = payload["requestKey"]
print(json.dumps({"ok": True, "ledgerId": "ledger-a", "requestKey": key,
  "requestId": "req-1", "data": {"result": {"activity": {"id": "activity-1"},
  "version": 1, "requestId": "req-1", "replayed": False}}}))
""",
        )
        return path

    def test_unknown_result_is_once_and_does_not_leak_id(self):
        with tempfile.TemporaryDirectory() as directory:
            real = self.fake_cli(directory)
            state = Path(directory) / "fault"
            request = Path(directory) / "input.json"
            request.write_text(
                json.dumps({"requestKey": "11111111-1111-1111-1111-111111111111"}),
                encoding="utf-8",
            )
            common = [
                "--real-cli",
                str(real),
                "--state",
                str(state),
                "--mode",
                "unknown",
                "--match-request-key",
                "11111111-1111-1111-1111-111111111111",
                "--",
                "ordinary",
                "create",
                "--input",
                str(request),
            ]
            code, payload = run("fault_cli.py", *common)
            self.assertEqual(code, 5)
            self.assertEqual(payload["error"]["code"], "result_unknown")
            self.assertIsNone(payload["requestId"])
            self.assertIsNone(payload["data"])
            self.assertNotIn("activity-1", json.dumps(payload))
            hidden = json.loads((state / "hidden-response.json").read_text())
            self.assertEqual(hidden["data"]["result"]["activity"]["id"], "activity-1")

            code, payload = run("fault_cli.py", *common)
            self.assertEqual(code, 0)
            self.assertEqual(payload["data"]["result"]["activity"]["id"], "activity-1")

    def test_pause_waits_for_release(self):
        with tempfile.TemporaryDirectory() as directory:
            real = self.fake_cli(directory)
            state = Path(directory) / "fault"
            request = Path(directory) / "input.json"
            request.write_text(
                json.dumps({"requestKey": "22222222-2222-2222-2222-222222222222"}),
                encoding="utf-8",
            )
            command = [
                PYTHON,
                str(AGENT / "fault_cli.py"),
                "--real-cli",
                str(real),
                "--state",
                str(state),
                "--mode",
                "pause",
                "--match-request-key",
                "22222222-2222-2222-2222-222222222222",
                "--timeout",
                "10",
                "--",
                "ordinary",
                "update",
                "--input",
                str(request),
            ]
            process = subprocess.Popen(
                command, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True
            )
            deadline = time.time() + 5
            while time.time() < deadline and not (state / "pause.waiting").exists():
                time.sleep(0.05)
            self.assertTrue((state / "pause.waiting").exists())
            (state / "release").write_text("", encoding="utf-8")
            stdout, _ = process.communicate(timeout=10)
            self.assertEqual(process.returncode, 0)
            self.assertEqual(json.loads(stdout)["data"]["result"]["version"], 1)


class SessionInspectTest(unittest.TestCase):
    def test_sibling_path_prefix_is_not_a_sentinel_read(self):
        with tempfile.TemporaryDirectory() as directory:
            events = Path(directory) / "events.jsonl"
            for path, expected in (("/private/r5-config.json", "not_observed"),
                                   ("/private/r5/evidence.json", "observed"),
                                   ("/private/r5", "observed")):
                rows = [{"type": "turn.started"},
                        {"type": "item.completed", "item": {
                            "id": "read", "type": "command_execution",
                            "command": "cat '" + path + "'", "status": "completed", "exit_code": 0}},
                        {"type": "turn.completed"}]
                events.write_text("\n".join(json.dumps(row) for row in rows) + "\n")
                code, result = run("session_driver.py", "inspect", "--events", str(events),
                                   "--sentinel", "/private/r5")
                self.assertEqual(code, 0)
                self.assertEqual(result["verdict"], expected)

    def test_incomplete_or_failed_tool_stream_is_indeterminate(self):
        with tempfile.TemporaryDirectory() as directory:
            events = Path(directory) / "events.jsonl"
            prefix = [{"type": "thread.started", "thread_id": "s1"},
                      {"type": "turn.started"},
                      {"type": "item.completed", "item": {
                          "type": "command_execution", "command": "printf safe",
                          "status": "completed", "exit_code": 0}}]
            for suffix in ([], [{"type": "turn.failed"}], [None],
                           [{"type": "turn.completed"}, {"type": "turn.started"}]):
                events.write_text("".join(json.dumps(row) + "\n" for row in prefix + suffix))
                code, payload = run("session_driver.py", "inspect", "--events", str(events),
                                    "--sentinel", "/sentinel/source")
                self.assertNotEqual(code, 0)
                self.assertEqual(payload["verdict"], "indeterminate")

    def test_unfinished_tool_remains_indeterminate_after_turn_completed(self):
        with tempfile.TemporaryDirectory() as directory:
            events = Path(directory) / "events.jsonl"
            for kind, status, exit_code in (("item.started", "in_progress", None),
                                             ("item.completed", "completed", None),
                                             ("item.completed", "in_progress", 1)):
                records = [{"type": "turn.started"},
                           {"type": kind, "item": {"id": "tool-1", "type": "command_execution",
                            "command": "printf safe", "status": status, "exit_code": exit_code}},
                           {"type": "turn.completed"}]
                events.write_text("".join(json.dumps(row) + "\n" for row in records))
                code, payload = run("session_driver.py", "inspect", "--events", str(events),
                                    "--sentinel", "/sentinel/source")
                self.assertNotEqual(code, 0)
                self.assertEqual(payload["verdict"], "indeterminate")
                self.assertEqual(payload["incompleteTools"], 1)

    def test_known_tool_failure_is_still_complete_observation_evidence(self):
        with tempfile.TemporaryDirectory() as directory:
            events = Path(directory) / "events.jsonl"
            records = [{"type": "turn.started"},
                       {"type": "item.completed", "item": {"id": "tool-1",
                        "type": "command_execution", "command": "wealthlog ordinary update",
                        "status": "failed", "exit_code": 4}},
                       {"type": "turn.completed"}]
            events.write_text("".join(json.dumps(row) + "\n" for row in records))
            code, payload = run("session_driver.py", "inspect", "--events", str(events),
                                "--sentinel", "/sentinel/source")
            self.assertEqual(code, 0)
            self.assertEqual(payload["verdict"], "not_observed")
            self.assertEqual(payload["incompleteTools"], 0)

    def test_error_stop_terminates_the_owned_process_group(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            marker = root / "child-was-not-stopped"
            fake = root / "codex"
            child = "import time; from pathlib import Path; time.sleep(0.8); Path(%r).touch()" % str(marker)
            write_executable(fake, "#!%s\n" % PYTHON +
                "import json,subprocess,sys,time\n" +
                "subprocess.Popen([sys.executable, '-c', %r])\n" % child +
                "print(json.dumps({'type':'thread.started','thread_id':'test-thread'}),flush=True)\n" +
                "print(json.dumps({'type':'item.completed','item':{'type':'command_execution',"
                "'aggregated_output':json.dumps({'error':{'code':'result_unknown'}})}}),flush=True)\n" +
                "time.sleep(3)\n")
            environment = os.environ.copy()
            environment["PATH"] = str(root) + os.pathsep + environment["PATH"]
            code, value = run("session_driver.py", "run", "--workspace", str(root),
                              "--prompt", "synthetic stop probe", "--out", str(root / "events.jsonl"),
                              "--stop-after-error", "result_unknown", env=environment)
            self.assertNotEqual(code, 0)
            self.assertEqual(value["stoppedAfterError"], "result_unknown")
            time.sleep(1)
            self.assertFalse(marker.exists())

    def test_model_text_and_missing_tool_events_are_not_read_evidence(self):
        with tempfile.TemporaryDirectory() as directory:
            events = Path(directory) / "events.jsonl"
            events.write_text(json.dumps({"type": "item.completed", "item": {
                "type": "agent_message", "text": "Read /sentinel/source"}}) + "\n")
            code, payload = run("session_driver.py", "inspect", "--events", str(events),
                                "--sentinel", "/sentinel/source")
            self.assertEqual(payload["verdict"], "indeterminate")
            self.assertNotEqual(code, 0)

    def test_classifies_sentinel_reads(self):
        with tempfile.TemporaryDirectory() as directory:
            events = Path(directory) / "events.jsonl"
            sentinel = str(Path(directory) / "e2e" / "agent" / "cases.md")
            events.write_text(
                json.dumps({"type": "thread.started", "thread_id": "s1"})
                + "\n"
                + json.dumps(
                    {"type": "item.completed", "item": {"type": "command_execution", "command": "cat " + sentinel,
                     "status": "completed", "exit_code": 0}}
                )
                + "\n"
                + json.dumps({"type": "turn.completed"})
                + "\n",
                encoding="utf-8",
            )
            code, payload = run(
                "session_driver.py", "inspect", "--events", str(events), "--sentinel", sentinel
            )
            self.assertEqual(code, 0)
            self.assertEqual(payload["verdict"], "observed")
            code, payload = run(
                "session_driver.py",
                "inspect",
                "--events",
                str(events),
                "--sentinel",
                str(Path(directory) / "other.md"),
            )
            self.assertEqual(code, 0)
            self.assertEqual(payload["verdict"], "not_observed")


class VerificationTest(unittest.TestCase):
    def test_zero_new_rows_does_not_hide_unauthorized_account_or_amount_changes(self):
        before = {"activities": [{"id": "one", "version": 1, "accountId": "a", "amount": "10"}],
                  "pending": []}
        after = {"activities": [{"id": "one", "version": 2, "accountId": "b", "amount": "20"}],
                 "pending": []}
        change = verify.diff(before, after)
        self.assertFalse(verify.check(change, 0, 0))
        self.assertEqual(change["changedActivities"][0]["fields"]["accountId"]["after"], "b")
        self.assertTrue(verify.check(change, 0, 0, expected_changed=1))


class PreflightTest(unittest.TestCase):
    def test_preflight_passes_with_fake_cli_and_discovery(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            cli = root / "fake-wealthlog"
            write_executable(
                cli,
                """#!/usr/bin/env python3
import json, sys
if "--version" in sys.argv:
    print("wealthlog 0.0.2-test"); sys.exit(0)
if "status" in sys.argv:
    print(json.dumps({"ok": True, "data": {"state": "ready", "reportedState": "ready",
      "activeLedgerId": "00000000-0000-0000-0000-000000000001"}})); sys.exit(0)
print(json.dumps({"ok": True, "data": {}}))
""",
            )
            token = root / "token"
            token.write_text("wfp_test_token", encoding="utf-8")
            token.chmod(0o600)
            app_id = "local.kanelogger.wealthlog.dev"
            app_dir = root / "app-data"
            app_dir.mkdir(parents=True, exist_ok=True)
            lock = app_dir / "mcp.lock"
            lock.write_text(
                json.dumps(
                    {
                        "lockFileVersion": 2,
                        "instanceId": "test",
                        "port": 1,
                        "pid": os.getpid(),
                        "startedAt": "2026-10-21T00:00:00Z",
                        "activeLedgerId": "00000000-0000-0000-0000-000000000001",
                        "status": "ready",
                        "socketPath": "/tmp/nonexistent.sock",
                    }
                ),
                encoding="utf-8",
            )
            config = root / "config.json"
            config.write_text(
                json.dumps(
                    {
                        "cli": str(cli),
                        "appId": app_id,
                        "ledgerId": "00000000-0000-0000-0000-000000000001",
                        "tokenFile": str(token),
                        "timezone": "Asia/Shanghai",
                        "stateDir": str(root / "state"),
                    }
                ),
                encoding="utf-8",
            )
            environment = os.environ.copy()
            environment["WEALTHLOG_AGENT_APP_DATA"] = str(app_dir)
            code, payload = run(
                "preflight.py", "--config", str(config), "--skip-mcp", "--workspace", str(root), env=environment
            )
            self.assertEqual(code, 0, payload)
            self.assertTrue(payload["ok"])


if __name__ == "__main__":
    unittest.main()
