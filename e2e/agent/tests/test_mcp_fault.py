"""A response loss must follow successful upstream execution, once only."""

import http.client
import json
import sys
import tempfile
import threading
import socket
import unittest
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from fault_mcp import make_server


class McpFaultTest(unittest.TestCase):
    def test_read_timeout_probe_delays_only_the_first_read(self):
        class Upstream(BaseHTTPRequestHandler):
            def log_message(self, *args):
                pass

            def do_POST(self):
                self.rfile.read(int(self.headers["Content-Length"]))
                raw = b'{"jsonrpc":"2.0","id":1,"result":{"content":[]}}'
                self.send_response(200)
                self.send_header("Content-Length", str(len(raw)))
                self.end_headers()
                self.wfile.write(raw)

        with tempfile.TemporaryDirectory() as directory:
            upstream = ThreadingHTTPServer(("127.0.0.1", 0), Upstream)
            state = Path(directory) / "state"
            proxy = make_server("http://127.0.0.1:%d/mcp" % upstream.server_port,
                                state, "create_ordinary_activity", None, delay_read_seconds=0.5)
            threads = [threading.Thread(target=s.serve_forever) for s in (upstream, proxy)]
            for thread in threads:
                thread.start()
            body = json.dumps({"jsonrpc": "2.0", "id": 1, "method": "tools/call",
                               "params": {"name": "get_accounts", "arguments": {}}})
            try:
                first = http.client.HTTPConnection("127.0.0.1", proxy.server_port, timeout=0.1)
                first.request("POST", "/mcp", body)
                with self.assertRaises(socket.timeout):
                    first.getresponse()
                first.close()
                second = http.client.HTTPConnection("127.0.0.1", proxy.server_port, timeout=3)
                second.request("POST", "/mcp", body)
                self.assertEqual(json.loads(second.getresponse().read())["id"], 1)
                second.close()
                self.assertTrue((state / "delay.fired").exists())
                self.assertFalse((state / "unknown.fired").exists())
            finally:
                for server in (proxy, upstream):
                    server.shutdown()
                    server.server_close()
                for thread in threads:
                    thread.join()

    def test_only_target_success_is_hidden_once_and_headers_are_preserved(self):
        observed = []

        class Upstream(BaseHTTPRequestHandler):
            def log_message(self, *args):
                pass

            def do_POST(self):
                body = json.loads(self.rfile.read(int(self.headers["Content-Length"])))
                observed.append((dict(self.headers), body))
                failed = body["params"]["arguments"].get("fail", False)
                result = {"isError": failed, "structuredContent":
                          {"error": "denied"} if failed else {"activity": {"id": "hidden-id"}},
                          "content": []}
                raw = json.dumps({"jsonrpc": "2.0", "id": body["id"], "result": result}).encode()
                self.send_response(200)
                self.send_header("mcp-session-id", "session-1")
                self.send_header("Content-Length", str(len(raw)))
                self.end_headers()
                self.wfile.write(raw)

        with tempfile.TemporaryDirectory() as directory:
            upstream = ThreadingHTTPServer(("127.0.0.1", 0), Upstream)
            proxy = make_server("http://127.0.0.1:%d/mcp" % upstream.server_port,
                                Path(directory) / "state", "create_ordinary_activity", "key-1")
            threads = [threading.Thread(target=s.serve_forever) for s in (upstream, proxy)]
            for thread in threads:
                thread.start()

            def call(key, fail=False):
                connection = http.client.HTTPConnection("127.0.0.1", proxy.server_port, timeout=3)
                try:
                    body = {"jsonrpc": "2.0", "id": 2, "method": "tools/call", "params":
                            {"name": "create_ordinary_activity", "arguments":
                             {"requestKey": key, "fail": fail}}}
                    connection.request("POST", "/mcp", json.dumps(body),
                                       {"Authorization": "Bearer synthetic-sentinel",
                                        "X-WF-Profile-Id": "ledger-a", "mcp-session-id": "session-1"})
                    response = connection.getresponse()
                    return response.headers, json.loads(response.read())
                finally:
                    connection.close()

            try:
                self.assertTrue(call("key-1", True)[1]["result"]["isError"])
                self.assertFalse((Path(directory) / "state/unknown.fired").exists())
                self.assertFalse(call("other-key")[1]["result"]["isError"])
                with self.assertRaises(http.client.RemoteDisconnected):
                    call("key-1")
                marker = json.loads((Path(directory) / "state/unknown.fired").read_text())
                self.assertTrue(marker["responseFullyReceived"])
                self.assertEqual(len(observed), 3)
                self.assertEqual(observed[2][0]["Authorization"], "Bearer synthetic-sentinel")
                self.assertEqual(observed[2][0]["X-WF-Profile-Id"], "ledger-a")
                headers, response = call("key-1")
                self.assertEqual(headers["mcp-session-id"], "session-1")
                self.assertEqual(response["result"]["structuredContent"]["activity"]["id"], "hidden-id")
                self.assertEqual(len(observed), 4)
            finally:
                for server in (proxy, upstream):
                    server.shutdown()
                    server.server_close()
                for thread in threads:
                    thread.join()


if __name__ == "__main__":
    unittest.main()
