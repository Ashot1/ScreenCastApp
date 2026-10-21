# Test-only stdio MCP server for local_host tests (ADR-022 «Testing Decisions»). One JSON-RPC
# message per line on stdin/stdout. argv[1] selects the misbehaviour; nothing here is a product path.
import json
import os
import signal
import sys
import time

MODE = sys.argv[1] if len(sys.argv) > 1 else "ok"
GRANDCHILD = None
# Every start leaves its pid in the private working folder, so tests count starts and check exits.
with open("starts", "a") as starts:
    starts.write(str(os.getpid()) + "\n")


def send(message):
    sys.stdout.write(json.dumps(message) + "\n")
    sys.stdout.flush()


def tool(name):
    return {
        "name": name,
        "description": name,
        "inputSchema": {"type": "object"},
        "annotations": {"readOnlyHint": True},
    }


def result(message_id, value):
    send({"jsonrpc": "2.0", "id": message_id, "result": value})


def text(message_id, value):
    result(message_id, {"content": [{"type": "text", "text": value}]})


def forever():
    while True:
        time.sleep(1)


if MODE == "exit-early":
    sys.stderr.write("exiting before reading anything\n")
    sys.exit(3)
if MODE == "no-read":
    forever()
if MODE == "ignore-term":
    signal.signal(signal.SIGTERM, signal.SIG_IGN)
if MODE == "grandchild":
    GRANDCHILD = os.fork()
    if GRANDCHILD == 0:
        # Same process group, outlives the leader and ignores SIGTERM: only the group poll ends it.
        signal.signal(signal.SIGTERM, signal.SIG_IGN)
        os.close(0)
        forever()
if MODE == "stderr-secret":
    sys.stderr.write("token=" + os.environ.get("TEST_SECRET", "") + " failed\n")
    sys.stderr.flush()
    sys.exit(4)

for line in sys.stdin:
    message = json.loads(line)
    method = message.get("method")
    message_id = message.get("id")
    if method == "initialize":
        if MODE == "silent":
            continue
        if MODE == "garbage":
            sys.stdout.write("this is not json\n")
            sys.stdout.flush()
            continue
        if MODE == "long-line":
            sys.stdout.write("x" * (300 * 1024))
            sys.stdout.flush()
            forever()
        if MODE == "stop-reading":
            # Answers initialize, then never reads stdin again.
            result(message_id, {"protocolVersion": "2025-06-18", "capabilities": {}})
            forever()
        if MODE == "close-stdin":
            # The next client write meets a closed pipe: EPIPE, never SIGPIPE in the host.
            os.close(0)
            result(message_id, {"protocolVersion": "2025-06-18", "capabilities": {}})
            time.sleep(1)
            sys.exit(0)
        result(message_id, {"protocolVersion": "2025-06-18", "capabilities": {"tools": {}}})
    elif method == "notifications/initialized":
        continue
    elif method == "tools/list":
        if MODE == "server-request":
            send({"jsonrpc": "2.0", "id": "s1", "method": "sampling/createMessage", "params": {}})
            continue
        send({"jsonrpc": "2.0", "method": "notifications/message", "params": {"level": "info"}})
        result(message_id, {"tools": [tool("echo"), tool("env"), tool("pid")]})
    elif method == "tools/call":
        name = message["params"]["name"]
        arguments = message["params"].get("arguments", {})
        if name == "echo":
            text(message_id, "echo:" + json.dumps(arguments, sort_keys=True))
        elif name == "env":
            value = {"env": dict(os.environ), "argv": sys.argv, "cwd": os.getcwd()}
            text(message_id, json.dumps(value, sort_keys=True))
        elif name == "pid":
            text(message_id, json.dumps({"leader": os.getpid(), "grandchild": GRANDCHILD}))
        else:
            send({"jsonrpc": "2.0", "id": message_id, "error": {"code": -32602, "message": "no"}})
if MODE in ("ignore-eof", "ignore-term", "grandchild"):
    # stdin EOF is the MCP shutdown request; these servers ignore it.
    forever()
