# How to Use MCP Tools

MCP (Model Context Protocol) is an open protocol that lets Juglans call external tool servers -- filesystem, GitHub, database, or your own custom services.

Juglans handles MCP entirely in the DSL via the `std/mcps.jg` standard library. No engine-level configuration is needed.

## Quick Start

Import the MCP library, declare your servers, and use the auto-discovered tools in `chat()`:

```juglans
libs: ["std/mcps.jg"]

# Declare MCP servers — tools are auto-discovered via tools/list
[github]: mcps.MCP(name="github", url="http://localhost:3001/mcp/github")
[fs]: mcps.MCP(name="fs", url="http://localhost:3001/mcp/filesystem")

# mcp_tools contains all discovered tools in OpenAI function calling format
[ask]: chat(
  message=input.message,
  tools=mcp_tools,
  on_tool=[mcps.handle]
)

[github] -> [fs] -> [ask]
```

That's it. Each `mcps.MCP()` call fetches the server's `tools/list`, converts schemas to OpenAI format, and accumulates them into `mcp_tools`. The `mcps.handle` function routes tool calls to the correct server by name prefix.

## How It Works

### Tool Discovery

`mcps.MCP(name, url, token)` sends a JSON-RPC `tools/list` request to the server, then converts each tool's `inputSchema` to OpenAI function calling format. Tool names are prefixed with the server name:

- `read_file` → `fs.read_file`
- `create_issue` → `github.create_issue`

### Tool Routing

When the LLM calls a tool like `github.create_issue`, `mcps.handle` splits the name on `.` to find the server, then forwards the call via JSON-RPC `tools/call`.

### Authentication

Pass a `token` parameter for servers that require authentication:

```juglans
libs: ["std/mcps.jg"]

[github]: mcps.MCP(
  name="github",
  url="http://localhost:3001/mcp/github",
  token=input.github_token
)

[ask]: chat(
  message=input.message,
  tools=mcp_tools,
  on_tool=[mcps.handle]
)

[github] -> [ask]
```

## Complete Example: Code Review Workflow

```juglans
libs: ["std/mcps.jg"]

[code_reviewer]: {
  "model": "gpt-4o-mini",
  "system_prompt": "You are a thorough code reviewer."
}

# Connect to GitHub and filesystem MCP servers
[github]: mcps.MCP(name="github", url="http://localhost:3001/mcp/github", token=input.github_token)
[fs]: mcps.MCP(name="fs", url="http://localhost:3001/mcp/filesystem")

# AI agent with access to all MCP tools
[review]: chat(
  agent=code_reviewer,
  message="Review PR #" + str(input.pr_number) + " in " + input.repo,
  tools=mcp_tools,
  on_tool=[mcps.handle]
)

[notify]: print(message="Review completed: " + output)

[github] -> [fs] -> [review] -> [notify]
```

## Build a Custom MCP Server

Implement an HTTP server that handles JSON-RPC requests with two methods:

- `tools/list` -- returns available tool definitions
- `tools/call` -- executes a tool and returns results

Example in Python (Flask):

```python
from flask import Flask, request, jsonify

app = Flask(__name__)

@app.route('/', methods=['POST'])
def handle():
    req = request.json
    method = req.get("method")

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

    elif method == "tools/call":
        name = req["params"]["name"]
        args = req["params"]["arguments"]
        result = do_work(name, args)
        return jsonify({
            "jsonrpc": "2.0",
            "id": req["id"],
            "result": {"content": [{"type": "text", "text": result}]}
        })

app.run(port=5000)
```

Then use it:

```juglans
libs: ["std/mcps.jg"]

[my_server]: mcps.MCP(name="my-tools", url="http://localhost:5000")
[ask]: chat(message=input.query, tools=mcp_tools, on_tool=[mcps.handle])

[my_server] -> [ask]
```
