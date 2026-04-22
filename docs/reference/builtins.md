# Builtin Tools Reference

Complete reference for all built-in tools available in Juglans workflows.

---

## AI Tools

### chat()

Conduct a conversation with an AI agent.

| Parameter | Type | Required | Default | Description |
|-----------|------|----------|---------|-------------|
| `agent` | string/ref | No | `"default"` | Agent node reference or slug |
| `message` | string | Yes | - | Message to send |
| `format` | string | No | `"text"` | Output format: `"text"` or `"json"` |
| `state` | string | No | `"context_visible"` | Message state control (see below) |
| `chat_id` | string | No | - | Conversation ID for reusing session context |
| `tools` | array/string | No | - | Tool definitions or slug references |
| `system_prompt` | string | No | - | Override agent's system prompt |
| `model` | string | No | - | Override agent's model |
| `temperature` | number | No | - | Override agent's temperature |
| `history` | string | No | - | History retrieval mode |
| `on_tool` | string | No | - | Route unresolved tool calls to a workflow node: `on_tool=[node]` |
| `on_tool_call` | string | No | - | Route unresolved tool calls to an external workflow file |
| `stream_tool_events` | boolean | No | `false` | Emit SSE events for tool call start/complete |

**`state` values:**

| state | Writes to Context | SSE Output | Description |
|-------|-------------------|------------|-------------|
| `context_visible` | Yes | Yes | Default. Normal message |
| `context_hidden` | Yes | No | AI-visible in subsequent turns, not streamed to user |
| `display_only` | No | Yes | Streamed to user, not visible to AI |
| `silent` | No | No | Neither stored nor streamed |

Composite syntax: `state="input_state:output_state"` controls input and output independently.

**`tools` resolution:**

- Inline JSON array: `tools=[{...}]`
- Single slug reference: `tools="@devtools"`
- Multiple slugs: `tools=["devtools", "web-tools"]`
- If omitted, falls back to the agent's configured `tools` field

**Agent parameter:** The `agent` parameter references an inline agent map node defined in the same workflow or imported via `libs:`:

```juglans
[assistant]: {
  "model": "gpt-4o-mini",
  "system_prompt": "You are a helpful assistant."
}

[ask]: chat(agent=assistant, message="Hello!")
[assistant] -> [ask]
```

For cross-workflow reuse with `libs:`:

```juglans
libs: ["./agents.jg"]

[ask]: chat(agent=agents.assistant, message="Hello!")
```

**More examples:**

```juglans
[classifier]: {
  "model": "gpt-4o-mini",
  "temperature": 0.0,
  "system_prompt": "Classify intent. Return JSON."
}

[classify]: chat(agent=classifier, message=input.text, format="json")
[classifier] -> [classify]
```

```juglans
[analyst]: {
  "model": "gpt-4o-mini",
  "system_prompt": "You are a data analyst."
}

[hidden]: chat(agent=analyst, message=input.data, state="context_hidden")
[analyst] -> [hidden]
```

---

### p()

Render a Prompt template (`.jgx` file). Template variables use `{{ name }}` syntax.

| Parameter | Type | Required | Default | Description |
|-----------|------|----------|---------|-------------|
| `file` | string | Conditional | - | Path to a `.jgx` template file (preferred) |
| `slug` | string | Conditional | - | Prompt slug (legacy; requires the file to be registered via `prompts:` glob) |
| `...` | any | No | - | Template variable key-value pairs |

Exactly one of `file` or `slug` must be provided. `file` is the preferred form and avoids the need for a `prompts:` header declaration.

**Example:**

```juglans
[prompt]: p(file="./prompts/greeting.jgx", name="Alice", language="Chinese")
```

---

## System Tools

### print()

Print a message to stdout. No prefix, no context modification. Suitable for debugging and simple output.

| Parameter | Type | Required | Default | Description |
|-----------|------|----------|---------|-------------|
| `message` | string | No | `""` | Text to print (also accepts `value` alias) |

**Example:**

```juglans
[hello]: print(message="Hello, world!")
```

---

### notify()

Send a status notification. Updates `reply.status` and displays in console/UI.

| Parameter | Type | Required | Default | Description |
|-----------|------|----------|---------|-------------|
| `status` | string | No | - | Status text (updates `reply.status`) |
| `message` | string | No | `""` | Notification message |

**Output:**

```json
{"status": "sent", "content": "<message>"}
```

**Example:**

```juglans
[start]: notify(status="Starting workflow...")
```

---

### set_context() (internal)

Assignment syntax (`variable = value`) compiles to an internal `set_context` tool. Users should write assignments, not call `set_context` directly:

```juglans
[init]: count = 0, status = "ready"
```

---

### Assignment Syntax

Set one or more context variables using assignment syntax.

| Syntax | Description |
|--------|-------------|
| `[node]: key = value` | Set a single variable |
| `[node]: k1 = v1, k2 = v2` | Set multiple variables |

**Example:**

```juglans
[init]: count = 0, status = "ready"
```

```juglans
[inc]: count = count + 1
```

```juglans
[add]: results = append(results, output)
```

---

### timer()

Delay execution for a specified duration.

| Parameter | Type | Required | Default | Description |
|-----------|------|----------|---------|-------------|
| `ms` | number | No | `1000` | Delay in milliseconds |
| `seconds` | number | No | - | Delay in seconds (backward compatible) |

**Example:**

```juglans
[wait]: timer(ms=2000)
```

---

### reply()

Return a text message directly without calling an AI model. Supports state control and SSE streaming, identical to `chat()` state semantics.

| Parameter | Type | Required | Default | Description |
|-----------|------|----------|---------|-------------|
| `message` | string | No | `""` | Text to return |
| `state` | string | No | `"context_visible"` | Message state (same as `chat()` state) |

**Example:**

```juglans
[welcome]: reply(message="Welcome to the system!")
```

```juglans
[silent_reply]: reply(message="Internal note", state="silent")
```

---

### return()

Explicitly return a value as `output`. Designed for use inside function definitions.

| Parameter | Type | Required | Default | Description |
|-----------|------|----------|---------|-------------|
| `value` | any | No | `null` | Expression to evaluate and return |

**Example:**

```juglans
[add(a, b)]: {
  result = return(value=a + b)
}

[main]: add(a=1, b=2)
```

---

## HTTP Tools

### fetch()

Send an HTTP request. Recommended over `fetch_url()`.

| Parameter | Type | Required | Default | Description |
|-----------|------|----------|---------|-------------|
| `url` | string | Yes | - | Target URL |
| `method` | string | No | `"GET"` | HTTP method: GET, POST, PUT, DELETE, PATCH |
| `body` | object/string | No | - | Request body (auto-serialized as JSON) |
| `headers` | object | No | - | Custom request headers |

**Output:**

```json
{"status": 200, "ok": true, "data": {...}, "headers": {"content-type": "application/json"}}
```

**Example:**

```juglans
[get]: fetch(url="https://api.example.com/data")
```

```juglans
[post]: fetch(
  url="https://api.example.com/submit",
  method="POST",
  body=input.data
)
```

---

### fetch_url()

HTTP request tool (legacy). `fetch()` is recommended instead.

| Parameter | Type | Required | Default | Description |
|-----------|------|----------|---------|-------------|
| `url` | string | Yes | - | Target URL |
| `method` | string | No | `"GET"` | HTTP method |
| `headers` | object | No | - | Request headers |
| `body` | string | No | - | Request body |

**Output:**

```json
{"status": 200, "ok": true, "method": "GET", "url": "...", "content": {...}}
```

**Example:**

```juglans
[api]: fetch_url(url="https://api.example.com/data")
```

---

### serve()

Mark a workflow node as the HTTP entry point. When `juglans web` starts, it scans all `.jg` files and registers the workflow containing `serve()` as the catch-all HTTP handler. At runtime, `serve()` is a pass-through that reads pre-injected request data and computes `input.route`.

| Parameter | Type | Required | Default | Description |
|-----------|------|----------|---------|-------------|
| (none) | - | - | - | No parameters required |

**Injected variables** (set by web server before execution):

| Variable | Type | Description |
|----------|------|-------------|
| `input.method` | string | HTTP method (`GET`, `POST`, etc.) |
| `input.path` | string | Request path |
| `input.query` | object | Query parameters |
| `input.body` | any | Request body |
| `input.headers` | object | HTTP headers |
| `input.route` | string | Auto-computed `"METHOD /path"` |

**Example:**

```juglans
[request]: serve()

[hello]: response(status=200, body={"message": "Hello!"})
[not_found]: response(status=404, body={"error": "Not found"})

[request] -> switch input.route {
  "GET /api/hello": [hello]
  default: [not_found]
}
```

---

### response()

Set the HTTP response for a `serve()` workflow. Writes to `response.*` which the web server reads after execution.

| Parameter | Type | Required | Default | Description |
|-----------|------|----------|---------|-------------|
| `status` | integer | No | `200` | HTTP status code |
| `body` | any | No | - | Response body (JSON) |
| `headers` | object | No | - | Custom response headers |
| `file` | string | No | - | File path to serve |

If `response()` is never called, the web server returns status `200` with `output` as body.

**Example:**

```juglans
[ok]: response(status=200, body={"message": "Success"})
```

```juglans
[cors]: response(status=200, body=output, headers={"X-Custom": "value"})
```

---

### http_request()

Full-featured HTTP client (httpx-style). Supports all HTTP methods, query params, JSON/form/multipart body, auth, timeout, cookies, and redirect control.

| Parameter | Type | Required | Default | Description |
|-----------|------|----------|---------|-------------|
| `url` | string | Yes | - | Request URL |
| `method` | string | No | `"GET"` | HTTP method: GET, POST, PUT, PATCH, DELETE, HEAD, OPTIONS |
| `params` | object | No | - | Query parameters (appended to URL) |
| `headers` | object | No | - | Custom request headers |
| `json` | object/string | No | - | JSON body (auto-sets `Content-Type: application/json`) |
| `data` | object | No | - | Form data (URL-encoded) |
| `files` | object | No | - | Multipart file upload: `{field: path}` |
| `content` | string | No | - | Raw body content |
| `timeout` | number | No | - | Request timeout in seconds |
| `auth` | string | No | - | `"Bearer token"` or `"user:pass"` |
| `follow_redirects` | boolean | No | `true` | Follow HTTP redirects |
| `cookies` | object | No | - | Cookies as `{name: value}` |

Body priority: `json` > `data` > `files` > `content`.

**Output:**

```json
{
  "status_code": 200,
  "headers": {"content-type": "application/json"},
  "json": {"id": 1, "name": "Alice"},
  "text": "{\"id\":1,\"name\":\"Alice\"}",
  "url": "https://api.example.com/users",
  "is_success": true,
  "elapsed": 0.234,
  "content_type": "application/json"
}
```

**Example:**

```juglans
[get]: http_request(url="https://httpbin.org/get", params='{"page": 1}')
```

```juglans
[post]: http_request(url="https://httpbin.org/post", method="POST", json='{"name": "Alice"}', timeout=30)
```

---

### http.jg Library

httpx-style convenience wrapper around `http_request()`. Import with `libs: ["http"]`.

**Available functions:** `http.get()`, `http.post()`, `http.put()`, `http.patch()`, `http.delete()`, `http.head()`, `http.options()`

Each function sets `method` automatically and passes all other parameters to `http_request()`.

**Example:**

```juglans
libs: ["http"]

[users]: http.get(url="https://api.example.com/users", params='{"page": 1}')
```

```juglans
libs: ["http"]

[create]: http.post(url="https://api.example.com/users", json='{"name": "Alice"}', auth="Bearer sk-xxx")
```

```juglans
libs: ["http"]

[upload]: http.post(url="https://example.com/upload", files='{"document": "/path/to/file.pdf"}')
```

---

## OAuth Tools

### oauth_token()

OAuth2 token exchange. Supports all standard grant types and built-in providers (GitHub, Google).

| Parameter | Type | Required | Default | Description |
|-----------|------|----------|---------|-------------|
| `grant_type` | string | Yes* | - | `client_credentials`, `password`, `refresh_token`, `authorization_code` |
| `token_url` | string | Yes* | - | Token endpoint URL |
| `provider` | string | No | - | Built-in provider: `github`, `google` (auto-sets token_url + headers) |
| `client_id` | string | No | - | Client ID |
| `client_secret` | string | No | - | Client secret |
| `scope` | string | No | - | Permission scope |
| `username` | string | No | - | Username (password grant) |
| `password` | string | No | - | Password (password grant) |
| `refresh_token` | string | No | - | Refresh token (refresh_token grant) |
| `code` | string | No | - | Authorization code (authorization_code grant) |
| `redirect_uri` | string | No | - | Redirect URI (authorization_code grant) |
| `extra_params` | object | No | - | Additional form params merged into request |

*When `provider` is set, `grant_type` defaults to `authorization_code` and `token_url` is auto-configured.

**Built-in Providers:**

| Provider | Token URL | Notes |
|----------|-----------|-------|
| `github` | `https://github.com/login/oauth/access_token` | Auto-adds `Accept: application/json` |
| `google` | `https://oauth2.googleapis.com/token` | Standard OAuth2 |

**Output:**

```json
{
  "access_token": "eyJhbG...",
  "token_type": "Bearer",
  "expires_in": 3600,
  "refresh_token": "dGhpcyB...",
  "scope": "read write",
  "raw": {}
}
```

**Example:**

```juglans
[token]: oauth_token(grant_type="client_credentials", token_url="https://auth.example.com/token", client_id=env("ID"), client_secret=env("SECRET"))
```

---

### oauth.jg Library

OAuth2 convenience wrapper. Import with `libs: ["oauth"]`.

**Generic functions:** `oauth.client_credentials()`, `oauth.password()`, `oauth.refresh()`, `oauth.authorization_code()`

**Provider functions:** `oauth.github()`, `oauth.github_refresh()`, `oauth.google()`, `oauth.google_refresh()`

**Example:**

```juglans
libs: ["oauth", "http"]

[token]: oauth.github(client_id=env("GH_ID"), client_secret=env("GH_SECRET"), code=input.code)
[repos]: http.get(url="https://api.github.com/user/repos", auth="Bearer " + token.access_token)
[token] -> [repos]
```

```juglans
libs: ["oauth", "http"]

[token]: oauth.client_credentials(token_url="https://auth.example.com/token", client_id=env("ID"), client_secret=env("SECRET"), scope="read")
[data]: http.get(url="https://api.example.com/data", auth="Bearer " + token.access_token)
[token] -> [data]
```

---

## Developer Tools (Devtools)

A Claude Code-style set of code operation tools. Can be called directly in `.jg` files or used by LLMs via `"tools": ["devtools"]` in an inline agent map node.

### read_file()

Read file contents, returned with line numbers (cat -n format).

| Parameter | Type | Required | Default | Description |
|-----------|------|----------|---------|-------------|
| `file_path` | string | Yes | - | Absolute or relative file path |
| `offset` | integer | No | `1` | Starting line number (1-based) |
| `limit` | integer | No | `2000` | Maximum lines to return |

**Output:**

```json
{"content": "     1\tline one\n     2\tline two", "total_lines": 150, "lines_returned": 100, "offset": 1}
```

**Example:**

```juglans
[read]: read_file(file_path="./src/main.rs")
```

```juglans
[read_range]: read_file(file_path="./src/main.rs", offset=50, limit=100)
```

---

### write_file()

Write content to a file (overwrite). Automatically creates parent directories.

| Parameter | Type | Required | Default | Description |
|-----------|------|----------|---------|-------------|
| `file_path` | string | Yes | - | File path |
| `content` | string | Yes | - | Content to write |

**Output:**

```json
{"status": "ok", "file_path": "...", "lines_written": 25, "bytes_written": 1024}
```

**Example:**

```juglans
[write]: write_file(file_path="./output/result.json", content=result)
```

---

### edit_file()

Exact string replacement in a file. `old_string` must be unique unless `replace_all` is set.

| Parameter | Type | Required | Default | Description |
|-----------|------|----------|---------|-------------|
| `file_path` | string | Yes | - | File path |
| `old_string` | string | Yes | - | Text to find (must be unique) |
| `new_string` | string | Yes | - | Replacement text |
| `replace_all` | boolean | No | `false` | Replace all occurrences |

**Output:**

```json
{"status": "ok", "file_path": "...", "replacements": 1}
```

**Example:**

```juglans
[edit]: edit_file(
  file_path="./src/config.rs",
  old_string="version = \"1.0\"",
  new_string="version = \"2.0\""
)
```

---

### bash()

Execute a shell command with timeout control and output truncation.

| Parameter | Type | Required | Default | Description |
|-----------|------|----------|---------|-------------|
| `command` | string | Yes | - | Command to execute (also accepts `cmd` alias) |
| `timeout` | integer | No | `120000` | Timeout in milliseconds (max 600000) |
| `description` | string | No | - | Description for logging |

**Output:**

```json
{"stdout": "...", "stderr": "...", "exit_code": 0, "ok": true}
```

`stdout` is truncated beyond 30000 characters.

**Example:**

```juglans
[build]: bash(command="cargo build --release")
```

```juglans
[test]: bash(command="cargo test", timeout=300000)
```

---

### sh()

Alias for `bash()`. Maintained for backward compatibility. `sh(cmd="ls")` is equivalent to `bash(command="ls")`.

**Example:**

```juglans
[files]: sh(cmd="ls -la")
```

---

### glob()

File pattern matching. Returns a list of matching file paths.

| Parameter | Type | Required | Default | Description |
|-----------|------|----------|---------|-------------|
| `pattern` | string | Yes | - | Glob pattern (e.g., `**/*.rs`) |
| `path` | string | No | `.` | Base directory to search in |

**Output:**

```json
{"matches": ["./src/main.rs", "./src/lib.rs"], "count": 2, "pattern": "./**/*.rs"}
```

**Example:**

```juglans
[find]: glob(pattern="**/*.rs")
```

```juglans
[find_src]: glob(pattern="*.ts", path="./src")
```

---

### grep()

Regex search of file contents. Recursively searches files and returns matching lines.

| Parameter | Type | Required | Default | Description |
|-----------|------|----------|---------|-------------|
| `pattern` | string | Yes | - | Regular expression pattern |
| `path` | string | No | `.` | File or directory to search |
| `include` | string | No | - | File filter glob (e.g., `*.rs`) |
| `context_lines` | integer | No | `0` | Context lines before and after each match |
| `max_matches` | integer | No | `50` | Maximum number of matches |

**Output:**

```json
{"matches": [{"file": "./src/main.rs", "line": 10, "match": "fn main() {", "context": "..."}], "total_matches": 1, "files_searched": 15, "truncated": false}
```

**Example:**

```juglans
[todos]: grep(pattern="TODO|FIXME", path="./src")
```

```juglans
[search]: grep(pattern="fn main", include="*.rs", context_lines=2)
```

---

## Testing Tools

### mock()

Execute a workflow with injected node outputs. Nodes listed in `inject` are skipped during execution and their output is set to the injected value.

| Parameter | Type | Required | Default | Description |
|-----------|------|----------|---------|-------------|
| `workflow` | string | Yes | - | Workflow file path |
| `inject` | object | No | - | Map of `node_id` to injected output value |

**Example:**

```juglans
[test]: mock(
  workflow="main.jg",
  inject={"api_call": {"status": "ok", "data": "mocked"}}
)
```

---

### config()

Store test configuration. Returns all parameters as a JSON value for the test runner to read.

| Parameter | Type | Required | Default | Description |
|-----------|------|----------|---------|-------------|
| `...` | any | No | - | Key-value pairs (known keys: `agent`, `budget`, `mock`, `timeout`) |

**Example:**

```juglans
[setup]: config(agent="test-agent", timeout=30)
```

---

## Adapter Tools

### feishu_webhook()

Send a message via Feishu (Lark) webhook.

| Parameter | Type | Required | Default | Description |
|-----------|------|----------|---------|-------------|
| `message` | string | Yes | - | Message text to send |
| `webhook_url` | string | No | - | Webhook URL (falls back to `[bot.feishu]` config) |

**Example:**

```juglans
[notify_feishu]: feishu_webhook(message="Deployment complete!")
```

---

### feishu_send()

Send a message (or image) to a Feishu chat using the `[bot.feishu]` `app_id` / `app_secret` credentials. Acquires a tenant access token and posts to the Feishu Open API.

| Parameter | Type | Required | Default | Description |
|-----------|------|----------|---------|-------------|
| `chat_id` | string | Yes | - | Target Feishu chat ID |
| `message` | string | Conditional | - | Text message body |
| `image` | string | Conditional | - | Image key or path |

At least one of `message` or `image` must be provided.

**Example:**

```juglans
[notify]: feishu_send(chat_id=input.chat_id, message="Deployment complete!")
```

---

## Workflow Composition Tools

### call()

Call a function defined in the current (or root) workflow by name. Any additional keyword parameters are passed as function arguments.

| Parameter | Type | Required | Default | Description |
|-----------|------|----------|---------|-------------|
| `fn` | string | Yes | - | Function name to invoke |
| `...` | any | No | - | Arguments forwarded to the function |

**Example:**

```juglans
[greet(name)]: print(message="Hello " + name)

[run]: call(fn="greet", name="Alice")
```

---

### execute_workflow()

Run another `.jg` workflow as a nested sub-execution. Returns the nested workflow's final `reply.output`.

| Parameter | Type | Required | Default | Description |
|-----------|------|----------|---------|-------------|
| `path` | string | Yes | - | Path to the `.jg` workflow file |
| `input` | object | No | - | Optional input override for the child workflow |

**Example:**

```juglans
[sub]: execute_workflow(path="./pipelines/process.jg", input={"query": input.query})
```

---

## Database ORM (`db.*`)

A 21-tool ORM layer for SQL databases. Tools are namespaced under `db.*` and operate on a named connection (set via `db.connect()`). See the source in `src/builtins/database.rs` for full parameter lists.

### Connection

| Tool | Description |
|------|-------------|
| `db.connect(name, url)` | Open a named connection (SQLite, Postgres, MySQL) |
| `db.disconnect(name)` | Close a connection |

### Query & Execute

| Tool | Description |
|------|-------------|
| `db.query(sql, params?, name?)` | Run a raw SELECT, returns rows as an array of objects |
| `db.exec(sql, params?, name?)` | Run a raw INSERT/UPDATE/DELETE statement |

### CRUD

| Tool | Description |
|------|-------------|
| `db.find(table, where?, order_by?, limit?, offset?)` | Find rows matching criteria |
| `db.find_one(table, where?)` | Find a single row |
| `db.create(table, data)` | Insert a row, returns the inserted record |
| `db.create_many(table, rows)` | Bulk insert |
| `db.upsert(table, data, conflict)` | Insert or update on conflict |
| `db.update(table, where, data)` | Update matching rows |
| `db.delete(table, where)` | Delete matching rows |
| `db.count(table, where?)` | Count matching rows |
| `db.aggregate(table, ops, where?, group_by?)` | Run aggregate functions (sum, avg, min, max) |

### Transactions

| Tool | Description |
|------|-------------|
| `db.begin(name?)` | Begin a transaction |
| `db.commit(name?)` | Commit the current transaction |
| `db.rollback(name?)` | Roll back the current transaction |

### Schema

| Tool | Description |
|------|-------------|
| `db.create_table(table, columns)` | Create a table |
| `db.drop_table(table)` | Drop a table |
| `db.alter_table(table, changes)` | Alter a table (add/drop/rename columns) |
| `db.tables(name?)` | List tables in the database |
| `db.columns(table, name?)` | List columns of a table |

**Example:**

```juglans
[open]: db.connect(name="main", url="sqlite://./app.db")
[user]: db.find_one(table="users", where={"id": input.user_id})
[count]: db.count(table="orders", where={"user_id": user.id})
```

---

## Conversation History (`history.*`)

Persistent chat history keyed by `chat_id`. Backed by the `[history]` config section (JSONL, SQLite, or in-memory). When enabled, `chat()` automatically loads recent messages and appends each turn — these primitives are for workflows that need to inspect or manipulate the store directly.

| Tool | Parameters | Returns |
|------|------------|---------|
| `history.load` | `chat_id`, `limit?=20` | Array of `{role, content, created_at, tokens?, meta?}` |
| `history.append` | `chat_id`, `role`, `content`, `tokens?` | `{ok: true}` |
| `history.replace` | `chat_id`, `from`, `to`, `content`, `role?="system"` | `{ok: true}` — collapses `[from, to)` into one message |
| `history.trim` | `chat_id`, `keep_recent?=20` | `{ok: true}` |
| `history.clear` | `chat_id` | `{ok: true}` |
| `history.stats` | `chat_id` | `{chat_id, count, tokens, first_at, last_at}` |
| `history.list_chats` | — | `[chat_id, ...]` |

**Examples:**

```juglans
# Inspect
[s]: history.stats(chat_id = input.chat_id)

# Reset a thread
[reset]: history.clear(chat_id = input.chat_id)

# Collapse old messages into a summary (manual compaction)
[old]: history.load(chat_id = input.chat_id, limit = 999)
[sum]: chat(message = "Summarize:\n" + json(old), state = "silent")
[_]:   history.replace(
         chat_id = input.chat_id,
         from    = 0,
         to      = len(old) - 10,
         content = sum,
       )
```

If history is disabled (`[history].enabled = false`) or no `chat_id` is given, these tools return empty / no-op results without erroring.

---

## Device Control (feature-gated: `device`)

Available only when Juglans is built with the `device` Cargo feature enabled (not available on headless CI or the default Docker image). Uses `enigo` for cross-platform keyboard/mouse automation.

### Keyboard

| Tool | Description |
|------|-------------|
| `key_tap(key)` | Press and release a key (e.g. `enter`, `esc`, `a`) |
| `key_combo(keys)` | Press a chord (e.g. `"ctrl+shift+t"`) |
| `type_text(text)` | Type a string |
| `key_listen(duration?)` | Listen for key events for a duration |

### Mouse

| Tool | Description |
|------|-------------|
| `mouse_move(x, y)` | Move the mouse pointer to absolute coordinates |
| `mouse_click(button?)` | Click a mouse button (`left`, `right`, `middle`) |
| `mouse_scroll(x, y)` | Scroll horizontally/vertically |
| `mouse_position()` | Return the current pointer coordinates |
| `mouse_drag(from_x, from_y, to_x, to_y)` | Press, drag, and release |
| `mouse_listen(duration?)` | Listen for mouse events for a duration |

### Screen

| Tool | Description |
|------|-------------|
| `screen_size()` | Return `{width, height}` of the primary display |
| `screenshot(path)` | Capture the screen to a file |

---

## Utility Functions

For the full expression-language function catalog (string, numeric, collection, date/time, encoding, higher-order, etc.), see [expressions.md](./expressions.md).

---

## Complete Workflow Example

```juglans
prompts: ["./prompts/*.jgx"]

[analyst]: {
  "model": "gpt-4o-mini",
  "temperature": 0.3,
  "system_prompt": "You are a data analyst. Return structured JSON analysis."
}

[summarizer]: {
  "model": "gpt-4o-mini",
  "system_prompt": "You are a summarization assistant."
}

[init]: results = [], processed = 0
[start_notify]: notify(status="Starting data processing...")

[fetch_data]: fetch(url=input.data_url)

[process]: foreach(item in output.items) {
  [render]: p(slug="analyze-item", item=item)
  [analyze]: chat(agent=analyst, message=output, format="json")
  [collect]: results = append(results, output), processed = processed + 1
  [progress]: notify(status="Processed: " + str(processed))

  [render] -> [analyze] -> [collect] -> [progress]
}

[summarize]: chat(
  agent=summarizer,
  message="Summarize: " + json(results)
)

[done]: notify(status="Done! Processed " + str(processed) + " items")

[analyst] -> [init]
[summarizer] -> [init]
[init] -> [start_notify] -> [fetch_data] -> [process] -> [summarize] -> [done]
```
