#!/usr/bin/env python3
"""Drive `codex exec` rounds and keep their JSONL events.

The driver never uses `--last`, records the explicit session id, and can
inspect the saved events for reads of sentinel paths. It does not modify the
acceptance answer or the Skill.
"""

import argparse
import json
import os
import re
import signal
import subprocess
import sys
import tempfile
import time
from pathlib import Path
from typing import Any, Dict, List, Optional

from common import load_config, read_token


class DriverError(Exception):
    pass


def read_prompt(path: Optional[str], inline: Optional[str]) -> str:
    if path:
        return Path(path).read_text(encoding="utf-8")
    if inline:
        return inline
    raise DriverError("pass --prompt or --prompt-file")


def build_command(args: argparse.Namespace, prompt: str) -> List[str]:
    command = ["codex", "exec"]
    if args.session:
        command.extend(["resume", "--json", "--skip-git-repo-check"])
        command.extend(["-c", 'sandbox_mode="%s"' % args.sandbox])
    else:
        command.extend(
            [
                "--json",
                "--skip-git-repo-check",
                "-C",
                str(Path(args.workspace).resolve()),
                "--sandbox",
                args.sandbox,
            ]
        )
    for item in args.config or []:
        command.extend(["-c", item])
    command.extend(["-c", 'shell_environment_policy.filters={WEALTHLOG_MCP_TOKEN="exclude",WEALTHLOG_TOKEN="exclude"}'])
    if args.mcp_url:
        if not args.agent_config:
            raise DriverError("--mcp-url requires --agent-config")
        config = load_config(args.agent_config)
        command.extend(["-c", "features.shell_snapshot=false"])
        for key, value in {
            "url": args.mcp_url,
            "bearer_token_env_var": "WEALTHLOG_MCP_TOKEN",
            "required": True,
        }.items():
            command.extend(["-c", "mcp_servers.wealthlog.%s=%s" % (key, json.dumps(value))])
        command.extend(["-c", 'mcp_servers.wealthlog.http_headers={"X-WF-Profile-Id"=%s}'
                        % json.dumps(config["ledgerId"])])
        # Approval is scoped to the explicitly authorized acceptance surface.
        for tool in ("get_accounts", "query_ordinary_activities", "get_ordinary_activity",
                     "create_ordinary_activity", "update_ordinary_activity",
                     "list_pending_activities", "create_pending_activity"):
            command.extend(["-c", 'mcp_servers.wealthlog.tools.%s.approval_mode="approve"' % tool])
    if args.model:
        command.extend(["-m", args.model])
    if args.disable_memory:
        command.extend(["--disable", "memories"])
    if args.session:
        command.extend([args.session])
    command.append(prompt)
    return command


def run(args: argparse.Namespace) -> Dict[str, Any]:
    prompt = read_prompt(args.prompt_file, args.prompt)
    out_path = Path(args.out)
    out_path.parent.mkdir(parents=True, exist_ok=True)
    command = build_command(args, prompt)
    environment = os.environ.copy()
    if args.agent_config:
        environment["WEALTHLOG_AGENT_CONFIG"] = args.agent_config
        environment["WEALTHLOG_AGENT_STATE_DIR"] = str(load_config(args.agent_config)["stateDir"])
    if args.mcp_url:
        config = load_config(args.agent_config)
        environment["WEALTHLOG_MCP_TOKEN"] = read_token(Path(config["tokenFile"]))

    session_id: Optional[str] = None
    event_types: Dict[str, int] = {}
    failures: List[str] = []
    stopped_after_error = None
    started = time.time()
    with out_path.open("w", encoding="utf-8") as handle, tempfile.TemporaryFile(mode="w+") as errors:
        try:
            process = subprocess.Popen(
                command,
                cwd=str(Path(args.workspace).resolve()),
                env=environment,
                stdout=subprocess.PIPE,
                stderr=errors,
                text=True,
                bufsize=1,
                start_new_session=os.name == "posix",
            )
        except FileNotFoundError:
            raise DriverError("codex executable not found")
        assert process.stdout is not None
        for line in process.stdout:
            handle.write(line)
            handle.flush()
            text = line.strip()
            if not text:
                continue
            try:
                event = json.loads(text)
            except json.JSONDecodeError:
                continue
            if not isinstance(event, dict):
                continue
            kind = str(event.get("type") or "?")
            event_types[kind] = event_types.get(kind, 0) + 1
            if kind == "thread.started":
                session_id = str(event.get("thread_id") or "") or session_id
            if kind in ("turn.failed", "error"):
                message = event.get("message")
                if not message and isinstance(event.get("error"), dict):
                    message = event["error"].get("message")
                if message:
                    failures.append(str(message))
            item = event.get("item") or {}
            stop_code = getattr(args, "stop_after_error", None)
            if stop_code and kind == "item.completed" and item.get("type") == "command_execution":
                try:
                    result = json.loads(item.get("aggregated_output") or "")
                except ValueError:
                    result = {}
                if not isinstance(result, dict):
                    result = {}
                if (result.get("error") or {}).get("code") == stop_code:
                    stopped_after_error = stop_code
                    if os.name == "posix":
                        os.killpg(process.pid, signal.SIGTERM)
                    else:
                        process.terminate()
        code = process.wait()
        errors.seek(0)
        stderr = errors.read()
    elapsed = round(time.time() - started, 2)
    summary = {
        "ok": code == 0 and not failures,
        "exitCode": code,
        "sessionId": session_id or args.session,
        "eventsPath": str(out_path),
        "eventTypes": event_types,
        "failures": failures[:5],
        "stderrTail": stderr[-500:],
        "seconds": elapsed,
        "stoppedAfterError": stopped_after_error,
        "command": command[:-1] + ["<prompt>"],
    }
    Path(str(out_path) + ".summary.json").write_text(
        json.dumps(summary, ensure_ascii=False, indent=2) + "\n", encoding="utf-8"
    )
    return summary


def inspect(args: argparse.Namespace) -> Dict[str, Any]:
    path = Path(args.events)
    if not path.is_file():
        raise DriverError("events file not found: %s" % path)
    sentinels = [str(Path(item)) for item in args.sentinel or []]
    found = {sentinel: False for sentinel in sentinels}
    tool_events = 0
    turn_failed = False
    turn_completed = False
    pending_tools = set()
    incomplete_tools = 0
    malformed = 0
    with path.open("r", encoding="utf-8") as handle:
        for line in handle:
            try:
                event = json.loads(line)
            except ValueError:
                malformed += 1
                continue
            if not isinstance(event, dict):
                malformed += 1
                continue
            if event.get("type") == "turn.started":
                turn_completed = False
            if event.get("type") == "turn.completed":
                turn_completed = True
            if event.get("type") in ("turn.failed", "error"):
                turn_failed = True
            item = event.get("item") or {}
            if not isinstance(item, dict):
                malformed += 1
                continue
            if item.get("type") not in ("command_execution", "mcp_tool_call", "tool_call"):
                continue
            tool_events += 1
            item_id = item.get("id")
            if event.get("type") == "item.started":
                if item_id:
                    pending_tools.add(item_id)
                else:
                    incomplete_tools += 1
            elif event.get("type") == "item.completed":
                pending_tools.discard(item_id)
                if item.get("status") not in ("completed", "failed") or (
                    item.get("type") == "command_execution" and item.get("exit_code") is None
                ):
                    incomplete_tools += 1
            # Inspect invocation fields, not the model answer or tool output.
            invocation = json.dumps({key: item.get(key) for key in
                                     ("command", "arguments", "input")}, ensure_ascii=False)
            for sentinel in sentinels:
                # A sibling such as r5-config.json is not a read under r5/.
                if sentinel and re.search(re.escape(sentinel) + r"(?=$|[/\s\"'\\;|&()])", invocation):
                    found[sentinel] = True
    observed = [item for item, hit in found.items() if hit]
    if observed:
        verdict = "observed"
    elif (sentinels and tool_events and turn_completed and not turn_failed and not malformed
          and not pending_tools and not incomplete_tools):
        verdict = "not_observed"
    else:
        verdict = "indeterminate"
    return {
        "ok": verdict != "indeterminate",
        "events": str(path),
        "verdict": verdict,
        "observed": observed,
        "sentinels": sentinels,
        "toolEvents": tool_events,
        "turnFailed": turn_failed,
        "turnCompleted": turn_completed,
        "malformedLines": malformed,
        "incompleteTools": incomplete_tools + len(pending_tools),
        "note": "not_observed requires a completed event stream and covers only literal paths in saved tool invocations; indirect reads remain unproven",
    }


def main(argv: Optional[List[str]] = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    sub = parser.add_subparsers(dest="command", required=True)
    run_parser = sub.add_parser("run")
    run_parser.add_argument("--workspace", required=True)
    run_parser.add_argument("--prompt")
    run_parser.add_argument("--prompt-file")
    run_parser.add_argument("--out", required=True)
    run_parser.add_argument("--session")
    run_parser.add_argument("--config", action="append")
    run_parser.add_argument("--model")
    run_parser.add_argument("--sandbox", default="workspace-write")
    run_parser.add_argument("--disable-memory", action="store_true")
    run_parser.add_argument("--agent-config")
    run_parser.add_argument("--mcp-url", help="verified development or acceptance proxy URL")
    run_parser.add_argument("--stop-after-error", help="A13: stop after observing a specific CLI error")
    inspect_parser = sub.add_parser("inspect")
    inspect_parser.add_argument("--events", required=True)
    inspect_parser.add_argument("--sentinel", action="append")
    args = parser.parse_args(argv)
    try:
        result = run(args) if args.command == "run" else inspect(args)
        print(json.dumps(result, ensure_ascii=False, sort_keys=True))
        return 0 if result.get("ok") else 1
    except (DriverError, OSError) as error:
        print(
            json.dumps(
                {"ok": False, "error": {"code": "driver", "message": str(error)}},
                ensure_ascii=False,
            )
        )
        return 2


if __name__ == "__main__":
    sys.exit(main())
