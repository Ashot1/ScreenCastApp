# Test-only imitation of the stateless workspace-mcp server (ADR-022 «Testing Decisions», 24B).
# Started by a fake venv `bin/python` as `workspace_server.py <mode> -P -m main <args…>`; it
# listens on WORKSPACE_MCP_HOST:WORKSPACE_MCP_PORT, needs a Bearer token on every request and
# keeps no session. argv[1] selects the misbehaviour; nothing here is a product path.
import json
import os
import sys
import time
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

MODE = sys.argv[1] if len(sys.argv) > 1 else "ok"
# Every start leaves its pid, argv and environment in the private working folder (HOME), so
# tests count starts, check exits and see exactly what the recipe passed.
with open("starts", "a") as starts:
    starts.write(str(os.getpid()) + "\n")
with open("launch.json", "w") as launch:
    json.dump({"argv": sys.argv[2:], "env": dict(os.environ), "cwd": os.getcwd()}, launch)

if MODE == "exit-early":
    sys.stderr.write("workspace server failed to start\n")
    sys.exit(3)
if MODE == "slow-start":
    time.sleep(20)


def tool(name, read_only=True):
    return {
        "name": name,
        "description": name,
        "inputSchema": {"type": "object"},
        "annotations": {"readOnlyHint": read_only},
    }


TOOLS = [
    tool("search_drive_files"),
    tool("get_drive_file_content"),
    tool("search_docs"),
    tool("get_doc_as_markdown"),
    # Declared read-only but writes a local file: only the Core allowlist keeps it out.
    tool("get_drive_file_download_url"),
    tool("create_doc", read_only=False),
]


class Handler(BaseHTTPRequestHandler):
    def log_message(self, *args):
        pass

    def reply(self, status, body=None):
        data = b"" if body is None else json.dumps(body).encode()
        self.send_response(status)
        if body is not None:
            self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(data)))
        self.end_headers()
        self.wfile.write(data)

    def do_POST(self):
        length = int(self.headers.get("Content-Length", "0"))
        message = json.loads(self.rfile.read(length))
        auth = self.headers.get("Authorization", "")
        with open("requests", "a") as requests:
            requests.write(message.get("method", "") + " " + auth + "\n")
        if self.path != "/mcp" or not auth.startswith("Bearer ya29."):
            self.reply(401)
            return
        method = message.get("method")
        if "id" not in message:
            self.reply(202)
            return
        if method == "initialize":
            result = {"protocolVersion": "2025-06-18", "capabilities": {"tools": {}}}
        elif method == "tools/list":
            result = {"tools": TOOLS}
        elif method == "tools/call":
            query = json.dumps(message["params"].get("arguments", {}), sort_keys=True)
            text = "x" * (300 * 1024) if MODE == "big" else "found " + query
            result = {"content": [{"type": "text", "text": text}]}
        else:
            self.reply(200, {"jsonrpc": "2.0", "id": message["id"], "error": {"code": -32601}})
            return
        self.reply(200, {"jsonrpc": "2.0", "id": message["id"], "result": result})


host = os.environ.get("WORKSPACE_MCP_HOST", "")
port = int(os.environ.get("WORKSPACE_MCP_PORT", "0"))
try:
    server = ThreadingHTTPServer((host, port), Handler)
except OSError as e:
    sys.stderr.write("cannot listen on %s:%d: %s\n" % (host, port, e))
    sys.exit(4)
server.serve_forever()
