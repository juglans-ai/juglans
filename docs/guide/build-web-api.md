# How to Build a Web API

Juglans can turn any workflow into an HTTP API using two builtins:

- **`serve()`** -- Marks a workflow as the HTTP entry point. Injects request data into `input.*`.
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
# Server listens on http://127.0.0.1:3000 by default

curl http://localhost:3000/api/hello
# {"message": "Hello from Juglans!"}
```

> **Port precedence:** `--port` flag > `[server] port` in `juglans.toml` > built-in fallback `8080` (only used when no `juglans.toml` exists at all). Project scaffolds ship with `port = 3000`, which is why local dev typically shows `3000`. The official Docker image runs `juglans web --port 8080`, so when running in Docker publish `8080:8080`.

The node ID (`[hello]`) is **not** the URL path. There are two ways to declare routes — pick the one that fits your workflow:

1. **Decorator routing** (recommended for multi-endpoint APIs) — annotate function nodes with `@get` / `@post` / `@put` / `@delete` / `@patch`. The web server dispatches per-decorator.
2. **Single-handler with `switch input.route`** (recommended when the routing is a small data-driven map or when you want every request to flow through one pipeline).

## Decorator Routing

Place an HTTP-method decorator **immediately above** a function node. The web server registers each decorated function as a handler for that method + path; non-matching requests fall through to the bare `serve()` workflow (or 404 if there isn't one).

```juglans
[start]: serve()

@get("/api/users")
[list_users()]: response(status=200, body={"users": ["alice", "bob"]})

@post("/api/users")
[create_user()]: response(status=201, body={"created": input.body.name})

@get("/api/status")
[get_status()]: response(status=200, body={"status": "ok"})
```

Each decorated function can read:

- `input.body` — parsed request body (JSON when `Content-Type: application/json`, raw string otherwise)
- `input.query.*` — query parameters
- `input.headers.*` — request headers (lowercase keys)
- `input.path_parts` — array of path segments split on `/` (e.g. `GET /api/users/42` → `["api", "users", "42"]`)

Path matching is **exact-string** at the moment — `@get("/api/users")` matches only `/api/users`, not `/api/users/42`. There is no `:id` templating yet. For variable segments, register one decorator with a coarse path and split inside the handler using `input.path_parts`, or use a `serve()` workflow that switches on `input.route` (next section).

## Routing via `switch input.route`

When you'd rather keep a single `serve()` pipeline, use `switch input.route` to dispatch. `input.route` is auto-computed as `"METHOD /path"` (e.g., `"GET /api/users"`).

```juglans
[request]: serve()

[list_users]: response(status=200, body={"users": ["alice", "bob"]})
[create_user]: response(status=201, body={"created": input.body.name})
[get_status]: response(status=200, body={"status": "ok"})
[not_found]: response(status=404, body={"error": "Not found"})

[request] -> switch input.route {
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
| `input.method` | string | `"GET"`, `"POST"` |
| `input.path` | string | `"/api/users"` |
| `input.query` | object | `{"page": "1"}` |
| `input.body` | any | Parsed JSON or string |
| `input.headers` | object | `{"content-type": "application/json"}` |
| `input.route` | string | `"GET /api/users"` (auto-computed) |

## Start the Server

```bash
# Default: http://127.0.0.1:3000
juglans web

# Custom host and port
juglans web --host 0.0.0.0 --port 3030
```

Or configure in `juglans.toml`:

```toml
[server]
host = "127.0.0.1"
port = 3000
```

On startup, the server scans all `**/*.jg` files. If a workflow contains `serve()`, it is registered as the **Axum fallback handler**, meaning every request URL hits this single workflow. Only one `serve()` workflow is supported. Perform path-based dispatch inside the workflow using `input.route` or `input.path`.

## SSE Streaming

When a `chat()` node runs inside a `serve()` workflow, its output streams back to the client as Server-Sent Events. This is automatic -- no extra configuration needed.

```juglans
[request]: serve()
[ask]: chat(agent="assistant", message=input.body.question)
[done]: response(status=200, body=output)

[request] -> [ask] -> [done]
```

Call with SSE-capable client:

```bash
curl -N -X POST http://localhost:3000/api/chat \
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
