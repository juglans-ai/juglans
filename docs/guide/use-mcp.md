# How to Use MCP Tools

MCP (Model Context Protocol) is an open protocol that lets Juglans call external tool servers — filesystem, GitHub, database, or your own custom services. Juglans speaks MCP over **HTTP / JSON-RPC** (stdio MCP is not currently exposed at the DSL level).

## Quick Start (recommended)

Pass an `mcp=` map to `chat()`. Each entry is a server name → URL (or full config). Juglans handles the `initialize` handshake, `tools/list` discovery, name-prefixing (so `read_file` becomes `fs.read_file`), and the dispatch back to the right server when the LLM calls a tool:

```juglans
[code_reviewer]: {
  "model": "gpt-4o-mini",
  "system_prompt": "You are a thorough code reviewer."
}

[ask]: chat(
  agent = code_reviewer,
  message = "Review PR #" + str(input.pr_number) + " in " + input.repo,
  mcp = {
    "fs": "http://localhost:3001/mcp/filesystem",
    "github": {
      "url": "http://localhost:3001/mcp/github",
      "token": input.github_token
    }
  }
)
```

That's the whole story for most use cases. Two server forms are supported in the map:

| Form | Example | When to use |
|---|---|---|
| Plain URL | `"fs": "http://..."` | No auth, no extras |
| Object | `"github": { "url": "...", "token": "..." }` | Bearer token, custom headers |

Tools surface to the LLM with the server name as a prefix — `fs.read_file`, `github.create_issue`, etc. When the LLM calls one, Juglans routes it back via JSON-RPC `tools/call` on the right server.

## How It Works

When `chat()` runs with an `mcp=` map, for each entry Juglans does the following before dispatching to the LLM:

1. Open the server URL (`POST /` with JSON-RPC body).
2. Send `initialize` with protocol version `2024-11-05`. If the server returns an `mcp-session-id` header, capture it and include it on subsequent requests.
3. Send `tools/list`; convert each tool's `inputSchema` to OpenAI function-calling format; rename to `<server>.<tool>`; merge into the request's `tools` array.
4. After the LLM finalizes its tool calls, dispatch each `<server>.<tool>` call via `tools/call` on the server, with the captured `mcp-session-id`.

You can verify this end-to-end by setting `RUST_LOG=juglans::builtins::ai=debug` — the `initialize`, `tools/list`, and `tools/call` requests log per-server.

## Build a Custom MCP Server

Implement an HTTP endpoint that handles JSON-RPC requests with three methods:

- `initialize` — return server capabilities + protocol version
- `tools/list` — return available tool definitions
- `tools/call` — execute a tool and return results

Example in Python (Flask):

```python
from flask import Flask, request, jsonify

app = Flask(__name__)

@app.route('/mcp', methods=['POST'])
def handle():
    req = request.json
    method = req.get("method")

    if method == "initialize":
        return jsonify({
            "jsonrpc": "2.0",
            "id": req["id"],
            "result": {
                "protocolVersion": "2024-11-05",
                "capabilities": {"tools": {}},
                "serverInfo": {"name": "my-server", "version": "0.1.0"}
            }
        })

    if method == "tools/list":
        return jsonify({
            "jsonrpc": "2.0",
            "id": req["id"],
            "result": {
                "tools": [{
                    "name": "my_tool",
                    "description": "My custom tool",
                    "inputSchema": {
                        "type": "object",
                        "properties": {"input": {"type": "string"}},
                        "required": ["input"]
                    }
                }]
            }
        })

    if method == "tools/call":
        args = req["params"]["arguments"]
        result = do_work(args)
        return jsonify({
            "jsonrpc": "2.0",
            "id": req["id"],
            "result": {"content": [{"type": "text", "text": result}]}
        })

app.run(port=5000)
```

Then point Juglans at it:

```juglans
[ask]: chat(
  message = input.query,
  mcp = { "my-tools": "http://localhost:5000/mcp" }
)
```

## Compatibility: legacy `std/mcps.jg`

Pre-0.2.10 workflows used a DSL-level wrapper:

```juglans
libs: ["std/mcps.jg"]

[github]: mcps.MCP(name = "github", url = "http://localhost:3001/mcp/github")
[ask]: chat(message = input.text, tools = mcp_tools, on_tool = [mcps.handle])

[github] -> [ask]
```

This still works for backward compatibility — the `mcps.MCP()` helper sends the same `initialize` + `tools/list` handshake, accumulates schemas into `mcp_tools`, and `mcps.handle` routes tool calls. **New workflows should prefer the native `mcp=` parameter** above; it's terser, requires no library import, and benefits from the same session/header handling.

## See also

- `chat()` parameter reference — [builtins.md → AI Tools](../reference/builtins.md#ai-tools)
- MCP specification — <https://modelcontextprotocol.io>
