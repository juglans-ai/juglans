# How to Build a Web API

Juglans can turn any workflow into an HTTP API using two builtins:

- **`serve()`** -- Marks a workflow as the HTTP entry point. Injects request data into `$input.*`.
- **`response()`** -- Sets the HTTP response status, body, and headers.

## Minimal HTTP API

```juglans
[request]: serve()
[hello]: response(status=200, body={"message": "Hello from Juglans!"})

[request] -> [hello]
```

Start the server:

```bash
juglans web
# Server listens on http://127.0.0.1:8080

curl http://localhost:8080/api/hello
# {"message": "Hello from Juglans!"}
```

## Routing

Use `switch $input.route` to dispatch requests. `$input.route` is auto-computed as `"METHOD /path"` (e.g., `"GET /api/users"`).

```juglans
[request]: serve()

[list_users]: response(status=200, body={"users": ["alice", "bob"]})
[create_user]: response(status=201, body={"created": $input.body.name})
[get_status]: response(status=200, body={"status": "ok"})
[not_found]: response(status=404, body={"error": "Not found"})

[request] -> switch $input.route {
  "GET /api/users": [list_users]
  "POST /api/users": [create_user]
  "GET /api/status": [get_status]
  default: [not_found]
}
```

## Request Data

The web server injects these variables before workflow execution:

| Variable | Type | Example |
|----------|------|---------|
| `$input.method` | string | `"GET"`, `"POST"` |
| `$input.path` | string | `"/api/users"` |
| `$input.query` | object | `{"page": "1"}` |
| `$input.body` | any | Parsed JSON or string |
| `$input.headers` | object | `{"content-type": "application/json"}` |
| `$input.route` | string | `"GET /api/users"` (auto-computed) |

## Start the Server

```bash
# Default: http://127.0.0.1:8080
juglans web

# Custom host and port
juglans web --host 0.0.0.0 --port 3030
```

Or configure in `juglans.toml`:

```toml
[server]
host = "127.0.0.1"
port = 8080
```

On startup, the server scans all `**/*.jg` files. If a workflow contains `serve()`, it is registered as the catch-all HTTP handler. Only one `serve()` workflow is supported.

## SSE Streaming

When a `chat()` node runs inside a `serve()` workflow, its output streams back to the client as Server-Sent Events. This is automatic -- no extra configuration needed.

```juglans
[request]: serve()
[ask]: chat(agent="assistant", message=$input.body.question)
[done]: response(status=200, body=$output)

[request] -> [ask] -> [done]
```

Call with SSE-capable client:

```bash
curl -N -X POST http://localhost:8080/api/chat \
  -H "Content-Type: application/json" \
  -d '{"question": "Explain recursion"}'
```

SSE events: `node_start`, `content` (streamed text), `node_complete`, `done`.

## Client Tool Bridge

When the LLM returns a tool call that is not a builtin or MCP tool, it is automatically forwarded to the client via an SSE `tool_call` event. The client executes the tool and POSTs the result back to `/api/chat/tool-result`.

Resolution priority:
1. Builtin registry (chat, notify, serve, response, etc.)
2. MCP tools
3. Client bridge (automatic fallback)

This lets frontends handle UI-specific operations (rendering charts, creating trade suggestions) while Juglans manages the orchestration.
