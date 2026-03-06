# Built-in Web Server

Juglans includes a built-in web server that exposes workflows as HTTP APIs. It supports two modes:

1. **Built-in APIs** — Chat (SSE streaming), resource listing (agents, prompts, workflows)
2. **serve() HTTP Backend** — Turn any workflow into a custom HTTP API using `serve()` and `response()` builtins

## Quick Start

```bash
juglans web
```

Default: `http://127.0.0.1:8080`

### CLI Options

```bash
# Custom port
juglans web --port 3030

# Bind to all interfaces
juglans web --host 0.0.0.0

# Combined
juglans web --host 0.0.0.0 --port 8080
```

### Configuration

In `juglans.toml`:

```toml
[server]
host = "127.0.0.1"
port = 8080
```

**Priority:** CLI args > `juglans.toml` > defaults (`127.0.0.1:8080`)

---

## serve() and response() — HTTP Backend Builtins

The core new feature: define HTTP APIs entirely in `.jg` workflows.

### serve()

Marks a workflow node as the HTTP entry point. When the web server starts, it scans all `**/*.jg` files in the project. If a workflow contains a `serve()` node, it's registered as the **catch-all HTTP handler** for all unmatched routes.

**What it does at runtime:**

1. Reads pre-injected request data from `$input.*`
2. Computes `$input.route = "METHOD /path"` (e.g., `"GET /api/hello"`)
3. Returns a request summary for debugging

**Injected variables** (set by web server before workflow execution):

| Variable | Type | Description |
|----------|------|-------------|
| `$input.method` | string | HTTP method (`GET`, `POST`, etc.) |
| `$input.path` | string | Request path (`/api/hello`) |
| `$input.query` | object | Query parameters (`{key: "value"}`) |
| `$input.body` | any | Request body (parsed as JSON, or string fallback) |
| `$input.headers` | object | HTTP headers |
| `$input.route` | string | Auto-computed: `"METHOD /path"` for switch routing |

### response()

Sets the HTTP response status, body, and headers.

```
response(status=200, body={"message": "OK"}, headers={"X-Custom": "value"})
```

**Parameters:**

| Parameter | Type | Default | Description |
|-----------|------|---------|-------------|
| `status` | int | `200` | HTTP status code |
| `body` | any | — | Response body (JSON) |
| `headers` | object | — | Custom response headers |

Writes to `$response.status`, `$response.body`, `$response.headers` which the web server reads after workflow execution.

**Default behavior:** If `response()` is never called, the server returns status `200` with `$output` as the body.

### Example: HTTP API Workflow

```juglans
slug: "serve-test"
name: "HTTP Backend Test"
description: "Simple HTTP backend using serve() builtin"
entry: [request]

# Entry point — marks this workflow as an HTTP handler
[request]: serve()

# Handler nodes
[hello]: response(status=200, body={"message": "Hello from Juglans!"})
[echo]: response(status=200, body={"method": "GET", "query": $input.query, "path": $input.path})
[echo_post]: response(status=200, body={"method": "POST", "body": $input.body, "path": $input.path})
[not_found]: response(status=404, body={"error": "Not found", "path": $input.path, "method": $input.method})

# Routing — switch on "METHOD /path"
[request] -> switch $input.route {
  "GET /api/hello": [hello]
  "GET /api/echo": [echo]
  "POST /api/echo": [echo_post]
  default: [not_found]
}
```

Test it:

```bash
# Start server
juglans web

# Hit endpoints
curl http://localhost:8080/api/hello
# → {"message": "Hello from Juglans!"}

curl http://localhost:8080/api/echo?name=alice
# → {"method": "GET", "query": {"name": "alice"}, "path": "/api/echo"}

curl -X POST http://localhost:8080/api/echo \
  -H "Content-Type: application/json" \
  -d '{"data": "test"}'
# → {"method": "POST", "body": {"data": "test"}, "path": "/api/echo"}

curl http://localhost:8080/unknown
# → 404 {"error": "Not found", "path": "/unknown", "method": "GET"}
```

### Auto-Discovery

On startup, the web server:

1. Scans `**/*.jg` under the project root
2. Parses each workflow and looks for nodes calling `serve()`
3. If found, registers the workflow as a catch-all fallback handler
4. Logs: `🌐 Discovered serve() workflow: <slug> (node: [<id>]) at <path>`

Only **one** serve() workflow is supported. The first one discovered is used.

### Data Flow

```
HTTP Request (any method, any path)
    ↓
handle_serve_request()
    ├── Parse HTTP: method, path, query, body, headers
    ├── Load & parse .jg from disk
    ├── Resolve flow imports
    ├── Build executor (prompts, agents, tools, MCP)
    └── Create context, inject $input.*
         ↓
Workflow Execution
    ├── serve() node reads $input.*, computes $input.route
    ├── switch routes to matching handler
    └── Handler calls response(status, body, headers)
         ↓
HTTP Response
    ├── Read $response.status (default: 200)
    ├── Read $response.body (default: $output)
    └── Return (StatusCode, JSON)
```

---

## Built-in API Endpoints

These are always available, regardless of whether a serve() workflow exists.

### GET /

HTML dashboard showing server status, loaded resources (agents, prompts, workflows), uptime, and configuration.

### GET /api/agents

List local `.jgagent` files.

```bash
curl http://localhost:8080/api/agents
```

Supports `?pattern=` query to filter by glob pattern (default: `**/*.jgagent`).

### GET /api/prompts

List local `.jgprompt` files.

```bash
curl http://localhost:8080/api/prompts
```

Supports `?pattern=` query to filter by glob pattern.

### GET /api/workflows

List local `.jg` files with validation info (node count, validity, errors/warnings).

```bash
curl http://localhost:8080/api/workflows
```

Supports `?pattern=` query to filter by glob pattern.

### POST /api/chat

SSE streaming chat endpoint. Jug0-compatible request format.

**Request:**

```bash
curl -X POST http://localhost:8080/api/chat \
  -H "Content-Type: application/json" \
  -d '{
    "chat_id": "@my-agent",
    "messages": [
      {"role": "user", "content": "Hello"}
    ]
  }'
```

**Request fields:**

| Field | Type | Description |
|-------|------|-------------|
| `chat_id` | UUID or `@handle` | Existing chat UUID, or `@agent-slug` to start with agent |
| `messages` | array | Message parts with `role` and `content` |
| `agent` | object | Agent config override (`slug`, `model`, `tools`, `system_prompt`) |
| `model` | string | Model override |
| `tools` | array | Custom tool definitions |
| `stream` | bool | Enable streaming |
| `memory` | bool | Enable memory |
| `variables` | object | Workflow variables (Juglans-specific) |
| `state` | string | Message state: `context_visible`, `context_hidden`, `display_only`, `silent` |

**Response:** Server-Sent Events stream (see [SSE Streaming](#sse-streaming) below).

### POST /api/chat/tool-result

Returns client tool execution results back to the server (see [Client Tool Bridge](#client-tool-bridge) below).

---

## Client Tool Bridge

When a workflow's LLM returns tool calls that don't match any builtin or MCP tool, they're automatically forwarded to the client via SSE for frontend execution.

### Tool Resolution Priority

```
1. Builtin Registry  →  chat(), p(), notify(), fetch_url(), serve(), response(), etc.
2. MCP Tools         →  Tools registered via MCP servers
3. Client Bridge     →  Auto-forwarded to frontend (SSE)
```

### SSE tool_call Event

When client-side execution is needed:

```
event: tool_call
data: {
  "call_id": "unique-call-id",
  "tools": [
    {
      "id": "call_abc123",
      "name": "create_trade_suggestion",
      "arguments": "{\"symbol\": \"AAPL\", \"action\": \"buy\"}"
    }
  ]
}
```

### Returning Tool Results

After the client executes the tool, POST results back:

```bash
curl -X POST http://localhost:8080/api/chat/tool-result \
  -H "Content-Type: application/json" \
  -d '{
    "call_id": "unique-call-id",
    "results": [
      {
        "tool_call_id": "call_abc123",
        "content": "{\"success\": true, \"executed_on_client\": true}"
      }
    ]
  }'
```

### Terminal vs Functional Tools

| Type | `executed_on_client` | LLM Loop | Example |
|------|---------------------|----------|---------|
| **Terminal** | `true` | Stops | `create_trade_suggestion` |
| **Functional** | `false` or absent | Continues | `get_market_data` |

- **Terminal tools**: Client executes and completes (e.g., rendering a UI component). Result includes `executed_on_client: true`.
- **Functional tools**: Client returns data for the LLM to continue processing (e.g., fetching live market data).

### Frontend Integration Example

```javascript
const eventSource = new EventSource('/api/chat');

eventSource.addEventListener('tool_call', async (event) => {
  const { call_id, tools } = JSON.parse(event.data);

  const results = [];
  for (const tool of tools) {
    const result = await executeClientTool(tool.name, JSON.parse(tool.arguments));
    results.push({
      tool_call_id: tool.id,
      content: JSON.stringify(result),
    });
  }

  await fetch('/api/chat/tool-result', {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ call_id, results }),
  });
});
```

---

## SSE Streaming

### Event Types

| Event | Description |
|-------|-------------|
| `node_start` | Node begins execution |
| `node_complete` | Node finished execution |
| `content` | Text content output (LLM-generated) |
| `tool_call` | Tool call forwarded to client |
| `error` | Error occurred |
| `done` | Execution complete |

### Client-Side Handling

```javascript
async function chat(messages) {
  const response = await fetch('/api/chat', {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ messages }),
  });

  const reader = response.body.getReader();
  const decoder = new TextDecoder();

  while (true) {
    const { done, value } = await reader.read();
    if (done) break;

    const text = decoder.decode(value);
    for (const line of text.split('\n')) {
      if (line.startsWith('data: ')) {
        const data = JSON.parse(line.slice(6));
        handleEvent(data);
      }
    }
  }
}
```

---

## Production Deployment

### systemd

```ini
# /etc/systemd/system/juglans.service
[Unit]
Description=Juglans Workflow Server
After=network.target

[Service]
Type=simple
User=juglans
WorkingDirectory=/opt/juglans
ExecStart=/usr/local/bin/juglans web --host 0.0.0.0 --port 8080
Restart=always
RestartSec=5

[Install]
WantedBy=multi-user.target
```

```bash
sudo systemctl enable juglans
sudo systemctl start juglans
```

### Docker

```dockerfile
FROM rust:1.75 as builder
WORKDIR /app
COPY . .
RUN cargo build --release

FROM debian:bookworm-slim
COPY --from=builder /app/target/release/juglans /usr/local/bin/
COPY . /app/
WORKDIR /app
EXPOSE 8080
CMD ["juglans", "web", "--host", "0.0.0.0", "--port", "8080"]
```

```bash
docker build -t juglans-server .
docker run -p 8080:8080 juglans-server
```

### Nginx Reverse Proxy

```nginx
upstream juglans {
    server 127.0.0.1:8080;
}

server {
    listen 443 ssl http2;
    server_name api.example.com;

    ssl_certificate /path/to/cert.pem;
    ssl_certificate_key /path/to/key.pem;

    location / {
        proxy_pass http://juglans;
        proxy_http_version 1.1;
        proxy_set_header Upgrade $http_upgrade;
        proxy_set_header Connection "upgrade";
        proxy_set_header Host $host;
        proxy_set_header X-Real-IP $remote_addr;
        proxy_buffering off;  # Required for SSE
    }
}
```
