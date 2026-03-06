# Builtin Tools Reference

Juglans provides multiple builtin tools for various operations within workflows.

## AI Tools

### chat()

Conduct a conversation with an AI Agent.

**Parameters:**

| Parameter | Type | Required | Description |
|------|------|------|------|
| `agent` | string | Yes | Agent slug |
| `message` | string | Yes | Message to send |
| `format` | string | No | Output format ("text", "json") |
| `state` | string | No | Message state control (see table below) |
| `stateless` | string | No | Deprecated, use `state="silent"` instead |
| `chat_id` | string | No | Conversation ID for reusing session context |
| `tools` | array | No | Custom tool definitions (overrides Agent default configuration) |

**Examples:**

```juglans
# Basic conversation
[chat]: chat(agent="assistant", message="Hello!")

# Using variables
[chat]: chat(agent="assistant", message=$input.question)

# JSON output
[classify]: chat(
  agent="classifier",
  message=$input.text,
  format="json"
)

# Stateless call (deprecated, use state instead)
[analyze]: chat(
  agent="analyst",
  message=$input.data,
  stateless="true"
)

# Using the state parameter
[hidden]: chat(
  agent="analyst",
  message=$input.data,
  state="context_hidden"
)

# Reusing conversation context
[reply]: chat(
  agent="assistant",
  chat_id=$reply.chat_id,
  message=$input.followup
)

# With tools
[solver]: chat(
  agent="assistant",
  message=$input.question,
  tools=[
    {
      "type": "function",
      "function": {
        "name": "search_web",
        "description": "Search internet content",
        "parameters": {
          "type": "object",
          "properties": {
            "query": {"type": "string", "description": "Search keywords"}
          },
          "required": ["query"]
        }
      }
    }
  ]
)
```

**Output:**

Returns the AI response text. If `format="json"`, returns a parsed JSON object.

**`state` Parameter Description:**

Controls the visibility and persistence of `chat()` output:

| state | Writes to Context | SSE Output | Description |
|-------|-----------|---------|------|
| `context_visible` | Yes | Yes | Default value, normal message |
| `context_hidden` | Yes | No | Visible to AI in subsequent turns, not pushed to user |
| `display_only` | No | Yes | Pushed to user, not visible to AI in subsequent turns |
| `silent` | No | No | Neither |

- **Writes to Context**: Whether the result is stored in `$reply.output`, affecting whether subsequent nodes can read it
- **SSE Output**: Whether generated tokens are streamed to the frontend via SSE

```juglans
# Background analysis, not displayed to user, but results available to subsequent nodes
[bg_analyze]: chat(
  agent="analyst",
  message=$input.data,
  state="context_hidden"
)

# Displayed to user, but does not affect subsequent AI context
[greeting]: chat(
  agent="greeter",
  message="Welcome!",
  state="display_only"
)

# Completely silent
[silent_check]: chat(
  agent="validator",
  message=$input.data,
  state="silent"
)
```

**Tool Configuration Notes:**

- If `tools` parameter is specified in the workflow, the workflow configuration is used
- Otherwise, if the Agent configuration has a `tools` field, the Agent's default tools are used
- Tool configuration follows the OpenAI Function Calling format

---

### p()

Render a Prompt template.

**Parameters:**

| Parameter | Type | Required | Description |
|------|------|------|------|
| `slug` | string | Yes | Prompt slug |
| `...` | any | No | Template variables |

**Examples:**

```juglans
# Basic rendering
[prompt]: p(slug="greeting")

# Passing variables
[prompt]: p(slug="greeting", name="Alice", language="Chinese")

# Using input variables
[prompt]: p(
  slug="analysis",
  topic=$input.topic,
  data=$ctx.collected_data
)
```

**Output:**

Returns the rendered Prompt text.

---

### memory_search()

Search for relevant content in memory storage (RAG).

**Parameters:**

| Parameter | Type | Required | Description |
|------|------|------|------|
| `query` | string | Yes | Search query |
| `limit` | number | No | Result count limit |
| `threshold` | number | No | Similarity threshold |

**Examples:**

```juglans
[search]: memory_search(
  query=$input.question,
  limit=5,
  threshold=0.7
)
```

**Output:**

Returns an array of matching memory entries.

---

## System Tools

### notify()

Send a status notification.

**Parameters:**

| Parameter | Type | Required | Description |
|------|------|------|------|
| `status` | string | Yes | Notification message |

**Examples:**

```juglans
[start]: notify(status="Starting workflow...")
[progress]: notify(status="Processing item " + $ctx.index)
[done]: notify(status="Completed!")
```

**Output:**

No return value. The message is displayed in the console or UI.

---

### set_context()

Set context variables.

**Parameters:**

Any key-value pairs, supports nested paths.

**Examples:**

```juglans
# Simple assignment
[init]: set_context(count=0)

# Multiple variables
[setup]: set_context(
  status="running",
  items=[],
  config={"timeout": 30}
)

# Multiple variables at once
[update]: set_context(name="Alice", score=100)

# Using expressions
[increment]: set_context(count=$ctx.count + 1)

# Append to array
[collect]: set_context(
  results=append($ctx.results, $output)
)
```

**Output:**

No return value. Variables are accessible via `$ctx.*`.

---

### timer()

Delay execution.

**Parameters:**

| Parameter | Type | Required | Description |
|------|------|------|------|
| `ms` | number | Yes | Delay in milliseconds |

**Examples:**

```juglans
# Wait 1 second
[wait]: timer(ms=1000)

# Dynamic delay
[delay]: timer(ms=$ctx.delay_time)
```

**Output:**

No return value. Execution pauses for the specified duration.

---

### sh()

> **Deprecated** — `sh()` is now an alias for `bash()`, maintained for backward compatibility. Use `bash()` instead. See [Developer Tools > bash()](#bash).

**Old syntax still works:**

```juglans
[files]: sh(cmd="ls -la")    # Equivalent to bash(command="ls -la")
```

---

## Developer Tools

A Claude Code-style set of code operation tools, registered under the `"devtools"` slug. Can be called directly in .jg files, or used automatically by LLMs via `tools: ["devtools"]` in .jgagent.

```juglans
# Use devtools in a workflow node
[run]: bash(command="echo hello")
```

In `.jgagent` files, enable devtools with:

```text
slug: "code-agent"
tools: ["devtools"]

# Can also be combined with other tool sets
tools: ["devtools", "web-tools"]
```

---

### read_file()

Read file contents, returned with line numbers.

**Parameters:**

| Parameter | Type | Required | Description |
|------|------|------|------|
| `file_path` | string | Yes | File path (absolute or relative) |
| `offset` | integer | No | Starting line number, 1-based (default 1) |
| `limit` | integer | No | Maximum number of lines to return (default 2000) |

**Examples:**

```juglans
# Read entire file
[read]: read_file(file_path="./src/main.rs")

# Read a specific range
[read]: read_file(file_path="./src/main.rs", offset=50, limit=100)
```

**Output:**

```json
{
  "content": "     1\tuse std::io;\n     2\tfn main() {...",
  "total_lines": 150,
  "lines_returned": 100,
  "offset": 50
}
```

| Field | Type | Description |
|------|------|------|
| `content` | string | File content with line numbers (cat -n format, max 2000 characters per line) |
| `total_lines` | number | Total number of lines in the file |
| `lines_returned` | number | Actual number of lines returned |
| `offset` | number | Starting line number |

---

### write_file()

Write to a file (overwrite), automatically creates parent directories.

**Parameters:**

| Parameter | Type | Required | Description |
|------|------|------|------|
| `file_path` | string | Yes | File path |
| `content` | string | Yes | File content |

**Examples:**

```juglans
[write]: write_file(file_path="./output/result.json", content=$ctx.result)
```

**Output:**

```json
{
  "status": "ok",
  "file_path": "./output/result.json",
  "lines_written": 25,
  "bytes_written": 1024
}
```

---

### edit_file()

Exact string replacement. `old_string` must be unique in the file, otherwise `replace_all=true` is required.

**Parameters:**

| Parameter | Type | Required | Description |
|------|------|------|------|
| `file_path` | string | Yes | File path |
| `old_string` | string | Yes | Text to replace (must be unique) |
| `new_string` | string | Yes | Replacement text |
| `replace_all` | boolean | No | Replace all matches (default false) |

**Examples:**

```juglans
# Exact replacement
[edit]: edit_file(
  file_path="./src/config.rs",
  old_string="version = \"1.0\"",
  new_string="version = \"2.0\""
)

# Global replacement
[rename]: edit_file(
  file_path="./src/main.rs",
  old_string="old_name",
  new_string="new_name",
  replace_all="true"
)
```

**Output:**

```json
{
  "status": "ok",
  "file_path": "./src/config.rs",
  "replacements": 1
}
```

**Error cases:**
- `old_string` not found -> error
- `old_string` appears multiple times and `replace_all=false` -> error (requires more context to make the match unique)

---

### glob()

File pattern matching, returns a list of matching paths.

**Parameters:**

| Parameter | Type | Required | Description |
|------|------|------|------|
| `pattern` | string | Yes | Glob pattern (e.g., `**/*.rs`, `src/**/*.json`) |
| `path` | string | No | Search directory (defaults to current directory) |

**Examples:**

```juglans
[find]: glob(pattern="**/*.rs")
[find_src]: glob(pattern="*.ts", path="./src")
```

**Output:**

```json
{
  "matches": ["./src/main.rs", "./src/lib.rs"],
  "count": 2,
  "pattern": "./**/*.rs"
}
```

---

### grep()

Regex search of file contents. Recursively searches files in a directory and returns matching lines with context.

**Parameters:**

| Parameter | Type | Required | Description |
|------|------|------|------|
| `pattern` | string | Yes | Regular expression |
| `path` | string | No | Search path (file or directory, defaults to current directory) |
| `include` | string | No | File filter glob (e.g., `*.rs`, `*.{ts,tsx}`) |
| `context_lines` | integer | No | Number of context lines before and after matches (default 0) |
| `max_matches` | integer | No | Maximum number of matches (default 50) |

**Examples:**

```juglans
# Search for TODOs
[todos]: grep(pattern="TODO|FIXME", path="./src")

# Search specific file types
[search]: grep(pattern="fn main", include="*.rs", context_lines=2)
```

**Output:**

```json
{
  "matches": [
    {
      "file": "./src/main.rs",
      "line": 10,
      "match": "fn main() {",
      "context": "     9\tuse std::io;\n    10\tfn main() {\n    11\t    println!(\"hello\");"
    }
  ],
  "total_matches": 1,
  "files_searched": 15,
  "truncated": false
}
```

---

### bash()

Execute a shell command with timeout control and output truncation. Replaces the old `sh()` tool.

**Parameters:**

| Parameter | Type | Required | Description |
|------|------|------|------|
| `command` | string | Yes | Command to execute (also accepts `cmd` parameter for backward compatibility) |
| `timeout` | integer | No | Timeout in milliseconds (default 120000, max 600000) |
| `description` | string | No | Command description (for logging) |

**Examples:**

```juglans
# Execute a command
[build]: bash(command="cargo build --release")

# With timeout
[test]: bash(command="cargo test", timeout=300000)

# Backward-compatible old syntax
[files]: bash(cmd="ls -la")
```

**Output:**

```json
{
  "stdout": "Command standard output...",
  "stderr": "Error output (if any)...",
  "exit_code": 0,
  "ok": true
}
```

| Field | Type | Description |
|------|------|------|
| `stdout` | string | Standard output (truncated beyond 30000 characters) |
| `stderr` | string | Standard error output |
| `exit_code` | number | Exit code (0 means success) |
| `ok` | boolean | Whether the command executed successfully |

**Security Note:**

Avoid directly executing user-provided commands to prevent command injection attacks:

```juglans
# Dangerous: do not do this
[bad]: bash(command=$input.user_command)

# Safe: use fixed commands with parameter validation
[safe]: bash(command="ls " + sanitize($input.directory))
```

> **Note**: `sh()` is an alias for `bash()`. `sh(cmd="ls")` is equivalent to `bash(command="ls")`.

---

## HTTP Backend Tools

### serve()

Marks a workflow node as the HTTP entry point. When `juglans web` starts, it scans all `.jg` files and registers the workflow containing a `serve()` node as the catch-all HTTP handler.

At runtime, `serve()` is a pass-through that reads pre-injected request data and computes `$input.route` for switch routing.

**Injected Variables** (set by web server before execution):

| Variable | Type | Description |
|----------|------|-------------|
| `$input.method` | string | HTTP method (`GET`, `POST`, etc.) |
| `$input.path` | string | Request path |
| `$input.query` | object | Query parameters |
| `$input.body` | any | Request body (JSON or string) |
| `$input.headers` | object | HTTP headers |
| `$input.route` | string | Auto-computed `"METHOD /path"` |

**Example:**

```juglans
slug: "my-api"
name: "HTTP API"
entry: [request]

[request]: serve()

[hello]: response(status=200, body={"message": "Hello!"})
[not_found]: response(status=404, body={"error": "Not found"})

[request] -> switch $input.route {
  "GET /api/hello": [hello]
  default: [not_found]
}
```

**Output:**

Returns a request summary: `{method, path, route, query, has_body}`.

See [Web Server Guide](../integrations/web-server.md) for full documentation.

---

### response()

Sets the HTTP response for a `serve()` workflow. Writes to `$response.*` which the web server reads after execution.

**Parameters:**

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `status` | int | No | HTTP status code (default: 200) |
| `body` | any | No | Response body (JSON) |
| `headers` | object | No | Custom response headers |

**Example:**

```juglans
# Simple response
[ok]: response(status=200, body={"message": "Success"})

# Echo request data
[echo]: response(status=200, body={"query": $input.query, "path": $input.path})

# Error response
[error]: response(status=500, body={"error": "Internal error"})

# Custom headers
[cors]: response(status=200, body=$output, headers={"X-Custom": "value"})
```

**Default behavior:** If `response()` is never called, the web server returns status `200` with `$output` as body.

---

## Network Tools

### fetch()

HTTP request tool (recommended).

**Parameters:**

| Parameter | Type | Required | Description |
|------|------|------|------|
| `url` | string | Yes | Target URL |
| `method` | string | No | HTTP method (default "GET") |
| `body` | object/string | No | Request body (automatically JSON-serialized) |
| `headers` | object | No | Custom request headers |

**Examples:**

```juglans
# GET request
[get]: fetch(url="https://api.example.com/data")

# POST request
[post]: fetch(
  url="https://api.example.com/submit",
  method="POST",
  body=$input.data
)

# With headers
[auth_get]: fetch(
  url="https://api.example.com/protected",
  headers={"Authorization": "Bearer " + $ctx.token}
)

# PUT request
[update]: fetch(
  url="https://api.example.com/items/1",
  method="PUT",
  body={"name": "updated", "value": $input.value}
)
```

**Output:**

```json
{
  "status": 200,
  "ok": true,
  "data": { ... }
}
```

| Field | Type | Description |
|------|------|------|
| `status` | number | HTTP status code |
| `ok` | boolean | True if status code is in the 200-299 range |
| `data` | any | Response content (automatically parsed as JSON, otherwise returned as string) |

**Error Handling:**

```juglans
[api]: fetch(url=$input.api_url)
[process]: notify(status="Processing response...")
[handle_error]: notify(status="API request failed")

[api] -> [process]
[api] on error -> [handle_error]
```

---

### fetch_url()

Fetch URL content (legacy compatible, `fetch()` is recommended).

**Parameters:**

| Parameter | Type | Required | Description |
|------|------|------|------|
| `url` | string | Yes | Target URL |
| `method` | string | No | HTTP method (default "GET") |
| `headers` | object | No | Request headers |
| `body` | string | No | Request body |

**Examples:**

```juglans
# GET request
[fetch]: fetch_url(url="https://api.example.com/data")

# POST request
[post]: fetch_url(
  url="https://api.example.com/submit",
  method="POST",
  headers={"Content-Type": "application/json"},
  body=json($input.data)
)

# With authentication
[api]: fetch_url(
  url="https://api.example.com/protected",
  headers={"Authorization": "Bearer " + $ctx.token}
)
```

**Output:**

Returns the response content. If it is JSON, it is automatically parsed into an object.

---

## Utility Functions

The following functions can be used in parameters:

### Data Transformation

```text
# JSON serialization
json($ctx.data)              # Object -> JSON string

# String concatenation
"Hello, " + $input.name      # Concatenate strings

# Array append
append($ctx.list, $item)     # Append element to array
```

### Arithmetic Operations

```text
$ctx.count + 1               # Addition
$ctx.total - $ctx.used       # Subtraction
$ctx.price * $ctx.quantity   # Multiplication
$ctx.total / $ctx.count      # Division
```

### Comparison Operations

```text
$ctx.score > 80              # Greater than
$ctx.count <= 10             # Less than or equal
$ctx.status == "done"        # Equal to
$ctx.value != null           # Not equal to
```

### Logical Operations

```text
$ctx.a && $ctx.b             # AND
$ctx.a || $ctx.b             # OR
!$ctx.flag                   # NOT
```

---

## Combining Tools in Workflows

### Complete Example

```juglans
name: "Data Processing"
version: "0.1.0"

prompts: ["./prompts/*.jgprompt"]
agents: ["./agents/*.jgagent"]

entry: [init]
exit: [done]

# Initialization
[init]: set_context(results=[], processed=0)
[start_notify]: notify(status="Starting data processing...")

# Fetch data
[fetch_data]: fetch_url(url=$input.data_url)

# Process each data item
[process]: foreach($item in $output.items) {
  # Render analysis Prompt
  [render]: p(slug="analyze-item", item=$item)

  # Call AI for analysis
  [analyze]: chat(
    agent="analyst",
    message=$output,
    format="json"
  )

  # Collect results
  [collect]: set_context(
    results=append($ctx.results, $output),
    processed=$ctx.processed + 1
  )

  # Progress notification
  [progress]: notify(status="Processed: " + $ctx.processed)

  [render] -> [analyze] -> [collect] -> [progress]
}

# Summarize
[summarize]: chat(
  agent="summarizer",
  message="Summarize: " + json($ctx.results)
)

# Complete
[done]: notify(status="Done! Processed " + $ctx.processed + " items")

# Execution flow
[init] -> [start_notify] -> [fetch_data] -> [process] -> [summarize] -> [done]
```

---

## Custom Tools

Custom tools can be added through MCP integration. See the [MCP Integration Guide](../integrations/mcp.md).
