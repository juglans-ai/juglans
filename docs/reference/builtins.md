# Builtin Tools Reference

Complete reference for all built-in tools available in Juglans workflows.

---

## AI Tools

### chat()

Conduct a conversation with an AI agent.

| Parameter | Type | Required | Default | Description |
|-----------|------|----------|---------|-------------|
| `agent` | string | No | `"default"` | Agent slug |
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

**Example:**

```juglans
[ask]: chat(agent="assistant", message="Hello!")
```

```juglans
[classify]: chat(
  agent="classifier",
  message=$input.text,
  format="json"
)
```

```juglans
[hidden]: chat(
  agent="analyst",
  message=$input.data,
  state="context_hidden"
)
```

```juglans
[reply]: chat(
  agent="assistant",
  chat_id=$reply.chat_id,
  message=$input.followup
)
```

---

### p()

Render a Prompt template (`.jgprompt` file). Template variables use `{{ name }}` syntax.

| Parameter | Type | Required | Default | Description |
|-----------|------|----------|---------|-------------|
| `slug` | string | Yes | - | Prompt slug (or `file` alias) |
| `...` | any | No | - | Template variable key-value pairs |

**Example:**

```juglans
[prompt]: p(slug="greeting", name="Alice", language="Chinese")
```

---

### memory_search()

Search for relevant content in memory storage (semantic/RAG search via Jug0).

| Parameter | Type | Required | Default | Description |
|-----------|------|----------|---------|-------------|
| `query` | string | Yes | - | Search query text |
| `limit` | number | No | `5` | Maximum number of results |

**Example:**

```juglans
[search]: memory_search(query=$input.question, limit=5)
```

---

### history()

Fetch chat history for a conversation session.

| Parameter | Type | Required | Default | Description |
|-----------|------|----------|---------|-------------|
| `chat_id` | string | Yes | - | Conversation session ID |
| `include_all` | boolean | No | `false` | Include hidden/silent messages |

**Example:**

```juglans
[msgs]: history(chat_id=$reply.chat_id, include_all="true")
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

Send a status notification. Updates `$reply.status` and displays in console/UI.

| Parameter | Type | Required | Default | Description |
|-----------|------|----------|---------|-------------|
| `status` | string | No | - | Status text (updates `$reply.status`) |
| `message` | string | No | `""` | Notification message |

**Example:**

```juglans
[start]: notify(status="Starting workflow...")
```

---

### set_context()

Set one or more context variables. Supports two modes.

**Multi-field mode** (recommended):

| Parameter | Type | Required | Default | Description |
|-----------|------|----------|---------|-------------|
| `<key>` | any | Yes | - | Any key-value pairs to set on `$ctx` |

**Legacy mode:**

| Parameter | Type | Required | Default | Description |
|-----------|------|----------|---------|-------------|
| `path` | string | Yes | - | Context key path |
| `value` | any | Yes | - | Value to assign |

**Example:**

```juglans
[init]: set_context(count=0, status="ready")
```

```juglans
[inc]: set_context(count=$ctx.count + 1)
```

```juglans
[add]: set_context(results=append($ctx.results, $output))
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

Explicitly return a value as `$output`. Designed for use inside function definitions.

| Parameter | Type | Required | Default | Description |
|-----------|------|----------|---------|-------------|
| `value` | any | No | `null` | Expression to evaluate and return |

**Example:**

```juglans
[add(a, b)]: {
  result = return(value=$ctx.a + $ctx.b)
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
{"status": 200, "ok": true, "data": {...}}
```

**Example:**

```juglans
[get]: fetch(url="https://api.example.com/data")
```

```juglans
[post]: fetch(
  url="https://api.example.com/submit",
  method="POST",
  body=$input.data
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

Mark a workflow node as the HTTP entry point. When `juglans web` starts, it scans all `.jg` files and registers the workflow containing `serve()` as the catch-all HTTP handler. At runtime, `serve()` is a pass-through that reads pre-injected request data and computes `$input.route`.

| Parameter | Type | Required | Default | Description |
|-----------|------|----------|---------|-------------|
| (none) | - | - | - | No parameters required |

**Injected variables** (set by web server before execution):

| Variable | Type | Description |
|----------|------|-------------|
| `$input.method` | string | HTTP method (`GET`, `POST`, etc.) |
| `$input.path` | string | Request path |
| `$input.query` | object | Query parameters |
| `$input.body` | any | Request body |
| `$input.headers` | object | HTTP headers |
| `$input.route` | string | Auto-computed `"METHOD /path"` |

**Example:**

```juglans
[request]: serve()

[hello]: response(status=200, body={"message": "Hello!"})
[not_found]: response(status=404, body={"error": "Not found"})

[request] -> switch $input.route {
  "GET /api/hello": [hello]
  default: [not_found]
}
```

---

### response()

Set the HTTP response for a `serve()` workflow. Writes to `$response.*` which the web server reads after execution.

| Parameter | Type | Required | Default | Description |
|-----------|------|----------|---------|-------------|
| `status` | integer | No | `200` | HTTP status code |
| `body` | any | No | - | Response body (JSON) |
| `headers` | object | No | - | Custom response headers |
| `file` | string | No | - | File path to serve |

If `response()` is never called, the web server returns status `200` with `$output` as body.

**Example:**

```juglans
[ok]: response(status=200, body={"message": "Success"})
```

```juglans
[cors]: response(status=200, body=$output, headers={"X-Custom": "value"})
```

---

## Developer Tools (Devtools)

A Claude Code-style set of code operation tools. Can be called directly in `.jg` files or used by LLMs via `tools: ["devtools"]` in `.jgagent`.

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
[write]: write_file(file_path="./output/result.json", content=$ctx.result)
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

## Vector Tools

Vector storage and search tools for building RAG pipelines. Backed by Jug0 vector API.

### vector_create_space()

Create a new vector space.

| Parameter | Type | Required | Default | Description |
|-----------|------|----------|---------|-------------|
| `space` | string | Yes | - | Space name |
| `model` | string | No | - | Embedding model override |
| `public` | boolean | No | `false` | Whether the space is publicly accessible |

**Example:**

```juglans
[create]: vector_create_space(space="knowledge-base", public="true")
```

---

### vector_upsert()

Insert or update vector points in a space.

| Parameter | Type | Required | Default | Description |
|-----------|------|----------|---------|-------------|
| `space` | string | Yes | - | Space name |
| `points` | array | Yes | - | Array of point objects (JSON) |
| `model` | string | No | - | Embedding model override |

**Example:**

```juglans
[store]: vector_upsert(
  space="docs",
  points=[{"id": "doc1", "content": "Hello world"}]
)
```

---

### vector_search()

Search for similar vectors in a space.

| Parameter | Type | Required | Default | Description |
|-----------|------|----------|---------|-------------|
| `query` | string | Yes | - | Search query text |
| `space` | string | No | `"default"` | Space name |
| `limit` | number | No | `5` | Maximum number of results |
| `model` | string | No | - | Embedding model override |

**Example:**

```juglans
[results]: vector_search(query=$input.question, space="docs", limit=10)
```

---

### vector_list_spaces()

List all vector spaces. No parameters.

**Example:**

```juglans
[spaces]: vector_list_spaces()
```

---

### vector_delete_space()

Delete an entire vector space.

| Parameter | Type | Required | Default | Description |
|-----------|------|----------|---------|-------------|
| `space` | string | Yes | - | Space name to delete |

**Example:**

```juglans
[del]: vector_delete_space(space="old-space")
```

---

### vector_delete()

Delete specific vector points from a space.

| Parameter | Type | Required | Default | Description |
|-----------|------|----------|---------|-------------|
| `space` | string | Yes | - | Space name |
| `ids` | array/string | Yes | - | Point IDs (JSON array or comma-separated string) |

**Example:**

```juglans
[del]: vector_delete(space="docs", ids=["doc1", "doc2"])
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

## Utility Functions

Functions available in parameter expressions.

### Data Functions

| Function | Description | Example |
|----------|-------------|---------|
| `len(x)` | Length of string, array, or object | `len($ctx.items)` |
| `json(x)` | Serialize to JSON string | `json($ctx.data)` |
| `append(arr, item)` | Append element to array | `append($ctx.list, $output)` |
| `keys(obj)` | Get object keys as array | `keys($ctx.config)` |
| `values(obj)` | Get object values as array | `values($ctx.config)` |
| `flatten(arr)` | Flatten nested arrays | `flatten($ctx.nested)` |
| `unique(arr)` | Remove duplicate elements | `unique($ctx.tags)` |
| `sort(arr)` | Sort array | `sort($ctx.scores)` |
| `reverse(arr)` | Reverse array | `reverse($ctx.items)` |
| `slice(arr, start, end)` | Sub-array extraction | `slice($ctx.items, 0, 5)` |
| `sum(arr)` | Sum numeric array | `sum($ctx.values)` |
| `range(n)` | Generate `[0, 1, ..., n-1]` | `range(10)` |
| `default(val, fallback)` | Return `fallback` if `val` is null | `default($ctx.x, 0)` |

### String Functions

| Function | Description | Example |
|----------|-------------|---------|
| `str(x)` | Convert to string | `str($ctx.count)` |
| `upper(s)` | Uppercase | `upper($input.name)` |
| `lower(s)` | Lowercase | `lower($input.name)` |
| `trim(s)` | Strip whitespace | `trim($input.text)` |
| `split(s, delim)` | Split string into array | `split($input.csv, ",")` |
| `join(arr, delim)` | Join array into string | `join($ctx.tags, ", ")` |
| `replace(s, old, new)` | Replace substring | `replace($input.text, "foo", "bar")` |
| `contains(s, sub)` | Check if string/array contains value | `contains($ctx.list, "x")` |

### Numeric Functions

| Function | Description | Example |
|----------|-------------|---------|
| `int(x)` | Convert to integer | `int($input.count)` |
| `float(x)` | Convert to float | `float($input.price)` |
| `abs(x)` | Absolute value | `abs($ctx.delta)` |
| `round(x)` | Round to nearest integer | `round($ctx.score)` |
| `min(a, b)` / `min(arr)` | Minimum value | `min($ctx.a, $ctx.b)` |
| `max(a, b)` / `max(arr)` | Maximum value | `max($ctx.scores)` |

### Type & Logic Functions

| Function | Description | Example |
|----------|-------------|---------|
| `type(x)` | Type name as string | `type($ctx.value)` |
| `if(cond, a, b)` | Conditional expression | `if($ctx.ok, "yes", "no")` |

### Operators

```text
# Arithmetic
$ctx.a + $ctx.b        # Addition / String concatenation
$ctx.a - $ctx.b        # Subtraction
$ctx.a * $ctx.b        # Multiplication
$ctx.a / $ctx.b        # Division
$ctx.a % $ctx.b        # Modulo

# Comparison
$ctx.a == $ctx.b       # Equal
$ctx.a != $ctx.b       # Not equal
$ctx.a > $ctx.b        # Greater than
$ctx.a >= $ctx.b       # Greater than or equal
$ctx.a < $ctx.b        # Less than
$ctx.a <= $ctx.b       # Less than or equal

# Logical
$ctx.a and $ctx.b      # Logical AND (also &&)
$ctx.a or $ctx.b       # Logical OR (also ||)
not $ctx.flag          # Logical NOT (also !)

# Membership
$item in $ctx.list     # Membership test
$item not in $ctx.list # Negated membership
```

---

## Complete Workflow Example

```juglans
prompts: ["./prompts/*.jgprompt"]
agents: ["./agents/*.jgagent"]

[init]: set_context(results=[], processed=0)
[start_notify]: notify(status="Starting data processing...")

[fetch_data]: fetch(url=$input.data_url)

[process]: foreach($item in $output.items) {
  [render]: p(slug="analyze-item", item=$item)
  [analyze]: chat(agent="analyst", message=$output, format="json")
  [collect]: set_context(
    results=append($ctx.results, $output),
    processed=$ctx.processed + 1
  )
  [progress]: notify(status="Processed: " + str($ctx.processed))

  [render] -> [analyze] -> [collect] -> [progress]
}

[summarize]: chat(
  agent="summarizer",
  message="Summarize: " + json($ctx.results)
)

[done]: notify(status="Done! Processed " + str($ctx.processed) + " items")

[init] -> [start_notify] -> [fetch_data] -> [process] -> [summarize] -> [done]
```
