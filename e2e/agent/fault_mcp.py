#!/usr/bin/env python3
"""Loopback-only MCP acceptance proxy; drop one committed write response.

Forwards authentication, ledger binding and session headers unchanged. A
matching successful tools/call response is fully received before disconnecting
the downstream client. A private marker and hidden response allow independent
read-only persistence verification. Never logs request headers or bodies.
"""

import argparse
import http.client
import json
import socket
import subprocess
import sys
import time
import threading
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path
from urllib.parse import urlsplit

from common import HarnessError, ensure_private_dir, write_json
from mcp_client import parse_sse

HOP_HEADERS = {"connection", "keep-alive", "proxy-authenticate",
               "proxy-authorization", "te", "trailer", "transfer-encoding",
               "upgrade", "host", "content-length"}


def successful_tool_response(raw):
    try:
        payload = parse_sse(raw.decode("utf-8"))
    except (ValueError, UnicodeError, HarnessError):
        # Invalid or non-tool responses must remain visible to the real client.
        return False
    result = payload.get("result")
    if not isinstance(result, dict) or result.get("isError"):
        return False
    structured = result.get("structuredContent")
    if isinstance(structured, dict):
        return structured.get("ok") is not False and not structured.get("error")
    for block in result.get("content", []):
        if block.get("type") != "text":
            continue
        try:
            value = json.loads(block["text"])
        except (KeyError, ValueError):
            continue
        if isinstance(value, dict):
            return value.get("ok") is not False and not value.get("error")
    return False


def make_server(upstream, state, tool, request_key, port=0, verify_db=None, delay_read_seconds=0):
    target = urlsplit(upstream)
    if target.scheme != "http" or target.hostname not in ("127.0.0.1", "localhost", "::1"):
        raise ValueError("upstream must be an HTTP loopback URL")
    state = Path(state)
    ensure_private_dir(state)
    if not 0 <= delay_read_seconds <= 10:
        raise ValueError("read delay must be between zero and 10 seconds")
    lock = threading.Lock()

    class Handler(BaseHTTPRequestHandler):
        protocol_version = "HTTP/1.1"

        def log_message(self, *args):
            pass

        def forward(self):
            length = int(self.headers.get("Content-Length", "0"))
            if length > 8 * 1024 * 1024:
                self.send_error(413)
                return
            body = self.rfile.read(length) if length else None
            matched = False
            actual_key = None
            requested_tool = None
            if self.command == "POST" and body:
                try:
                    message = json.loads(body)
                    params = message.get("params") or {}
                    arguments = params.get("arguments") or {}
                    requested_tool = params.get("name") if message.get("method") == "tools/call" else None
                    actual_key = arguments.get("requestKey")
                    matched = (message.get("method") == "tools/call"
                               and params.get("name") == tool
                               and bool(actual_key)
                               and delay_read_seconds == 0
                               and (request_key is None or actual_key == request_key))
                except (ValueError, AttributeError):
                    pass
            connection = http.client.HTTPConnection(target.hostname, target.port, timeout=30)
            try:
                headers = {k: v for k, v in self.headers.items() if k.lower() not in HOP_HEADERS}
                connection.request(self.command, target.path or "/mcp", body=body, headers=headers)
                response = connection.getresponse()
                raw = response.read()
                if requested_tool == "get_accounts" and delay_read_seconds and response.status == 200:
                    delay = False
                    with lock:
                        if not (state / "delay.fired").exists():
                            write_json(state / "delay.fired", {"tool": requested_tool,
                                       "seconds": delay_read_seconds, "responseFullyReceived": True})
                            delay = True
                    if delay:
                        time.sleep(delay_read_seconds)
                if matched and response.status == 200 and successful_tool_response(raw):
                    with lock:
                        if not (state / "unknown.fired").exists():
                            if verify_db:
                                verification = subprocess.run(
                                    [sys.executable, str(Path(__file__).with_name("verify.py")),
                                     "committed", "--db", str(verify_db), "--request-key", actual_key],
                                    stdout=subprocess.PIPE, stderr=subprocess.PIPE, check=False,
                                )
                                if verification.returncode != 0:
                                    write_json(state / "unknown.not-injected", {
                                        "requestKey": actual_key, "reason": "persistence check failed"})
                                    matched = False
                            if matched:
                                self.hide_response(raw, actual_key)
                                return
                self.send_response(response.status)
                for name, value in response.getheaders():
                    if name.lower() not in HOP_HEADERS:
                        self.send_header(name, value)
                self.send_header("Content-Length", str(len(raw)))
                self.end_headers()
                self.wfile.write(raw)
            except (BrokenPipeError, ConnectionResetError):
                pass
            except (OSError, http.client.HTTPException):
                try:
                    self.send_error(502, "upstream unavailable")
                except (BrokenPipeError, ConnectionResetError):
                    pass
            finally:
                connection.close()

        def hide_response(self, raw, actual_key):
            # No response, ID or request metadata goes downstream.
            write_json(state / "hidden-response.json", parse_sse(raw.decode("utf-8")))
            write_json(state / "unknown.fired", {"requestKey": actual_key,
                      "responseFullyReceived": True, "persistenceVerified": bool(verify_db)})
            self.close_connection = True
            try:
                self.connection.shutdown(socket.SHUT_RDWR)
            except OSError:
                pass

        do_POST = forward
        do_GET = forward
        do_DELETE = forward

    return ThreadingHTTPServer(("127.0.0.1", port), Handler)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--upstream", required=True)
    parser.add_argument("--state", required=True)
    parser.add_argument("--tool", default="create_ordinary_activity")
    parser.add_argument("--match-request-key", help="omit only for a dedicated single-write session")
    parser.add_argument("--port", type=int, default=0)
    parser.add_argument("--verify-db", help="independent read-only commit verification")
    parser.add_argument("--delay-read-seconds", type=float, default=0,
                        help="read-only timeout preflight: delay get_accounts once; disables write response loss")
    args = parser.parse_args()
    server = make_server(args.upstream, args.state, args.tool, args.match_request_key,
                         args.port, args.verify_db, args.delay_read_seconds)
    print(json.dumps({"url": "http://127.0.0.1:%d/mcp" % server.server_port}), flush=True)
    try:
        server.serve_forever()
    except KeyboardInterrupt:
        pass
    finally:
        server.server_close()


if __name__ == "__main__":
    main()
