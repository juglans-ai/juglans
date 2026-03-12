"""Minimal MCP mock server for testing std/mcps.jg"""
from http.server import HTTPServer, BaseHTTPRequestHandler
import json

TOOLS = [
    {
        "name": "echo",
        "description": "Echo the input message back",
        "inputSchema": {
            "type": "object",
            "properties": {
                "message": {"type": "string", "description": "Message to echo"}
            },
            "required": ["message"]
        }
    },
    {
        "name": "add",
        "description": "Add two numbers",
        "inputSchema": {
            "type": "object",
            "properties": {
                "a": {"type": "number"},
                "b": {"type": "number"}
            },
            "required": ["a", "b"]
        }
    }
]


class Handler(BaseHTTPRequestHandler):
    def do_POST(self):
        body = json.loads(self.rfile.read(int(self.headers["Content-Length"])))
        method = body.get("method")
        req_id = body.get("id", "1")

        if method == "tools/list":
            resp = {"jsonrpc": "2.0", "id": req_id, "result": {"tools": TOOLS}}
        elif method == "tools/call":
            name = body["params"]["name"]
            args = body["params"]["arguments"]
            if name == "echo":
                text = f"Echo: {args.get('message', '')}"
            elif name == "add":
                text = str(args.get("a", 0) + args.get("b", 0))
            else:
                text = f"Unknown tool: {name}"
            resp = {
                "jsonrpc": "2.0",
                "id": req_id,
                "result": {"content": [{"type": "text", "text": text}]}
            }
        else:
            resp = {"jsonrpc": "2.0", "id": req_id, "error": {"message": "Unknown method"}}

        out = json.dumps(resp).encode()
        self.send_response(200)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(out)))
        self.end_headers()
        self.wfile.write(out)

    def log_message(self, fmt, *args):
        print(f"[MCP Mock] {fmt % args}")


if __name__ == "__main__":
    port = 9876
    print(f"MCP mock server on http://localhost:{port}")
    HTTPServer(("127.0.0.1", port), Handler).serve_forever()
